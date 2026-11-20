from collections import deque
from dataclasses import dataclass
from enum import Enum
import numpy as np


class ToxicityLevel(Enum):
    LOW = 0
    NORMAL = 1
    ELEVATED = 2
    CRITICAL = 3


@dataclass
class VolumeBucket:
    bucket_index: int
    start_ts_ms: int
    end_ts_ms: int
    volume: float
    buy_volume: float
    sell_volume: float
    vwap: float

    @property
    def imbalance(self) -> float:
        return abs(self.buy_volume - self.sell_volume)


@dataclass
class VPINReading:
    timestamp_ms: int
    vpin: float
    vpin_zscore: float
    toxicity_level: ToxicityLevel
    buckets_completed: int
    buy_ratio: float
    sell_ratio: float


class VPINFilter:
    def __init__(
        self,
        bucket_volume: float = 10.0,
        num_buckets: int = 50,
        elevated_threshold: float = 0.55,
        critical_threshold: float = 0.75,
        zscore_critical: float = 2.0
    ):
        self.bucket_volume = bucket_volume
        self.num_buckets = num_buckets
        self.elevated_threshold = elevated_threshold
        self.critical_threshold = critical_threshold
        self.zscore_critical = zscore_critical

        self.buckets = deque(maxlen=num_buckets)
        self.history_vpin = deque(maxlen=500)

        self.current_bucket_idx = 0
        self.accumulated_vol = 0.0
        self.accumulated_buy_vol = 0.0
        self.accumulated_sell_vol = 0.0
        self.accumulated_dollar_vol = 0.0
        self.current_start_ts = 0

    def update_trade(
        self,
        timestamp_ms: int,
        price: float,
        qty: float,
        is_buyer_maker: bool
    ) -> VPINReading:
        if self.current_start_ts == 0:
            self.current_start_ts = timestamp_ms

        trade_buy_vol = 0.0 if is_buyer_maker else qty
        trade_sell_vol = qty if is_buyer_maker else 0.0

        remaining_trade_vol = qty
        remaining_buy_vol = trade_buy_vol
        remaining_sell_vol = trade_sell_vol

        while remaining_trade_vol > 0.0:
            vol_needed = self.bucket_volume - self.accumulated_vol
            fill_vol = min(remaining_trade_vol, vol_needed)
            fraction = fill_vol / remaining_trade_vol

            fraction_buy = remaining_buy_vol * fraction
            fraction_sell = remaining_sell_vol * fraction

            self.accumulated_vol += fill_vol
            self.accumulated_buy_vol += fraction_buy
            self.accumulated_sell_vol += fraction_sell
            self.accumulated_dollar_vol += fill_vol * price

            remaining_trade_vol -= fill_vol
            remaining_buy_vol -= fraction_buy
            remaining_sell_vol -= fraction_sell

            if self.accumulated_vol >= self.bucket_volume - 1e-9:
                vwap = self.accumulated_dollar_vol / self.accumulated_vol if self.accumulated_vol > 0 else price
                bucket = VolumeBucket(
                    bucket_index=self.current_bucket_idx,
                    start_ts_ms=self.current_start_ts,
                    end_ts_ms=timestamp_ms,
                    volume=self.accumulated_vol,
                    buy_volume=self.accumulated_buy_vol,
                    sell_volume=self.accumulated_sell_vol,
                    vwap=vwap
                )
                self.buckets.append(bucket)
                self.current_bucket_idx += 1

                self.accumulated_vol = 0.0
                self.accumulated_buy_vol = 0.0
                self.accumulated_sell_vol = 0.0
                self.accumulated_dollar_vol = 0.0
                self.current_start_ts = timestamp_ms

        return self.compute_vpin(timestamp_ms)

    def compute_vpin(self, timestamp_ms: int) -> VPINReading:
        if len(self.buckets) == 0:
            return VPINReading(
                timestamp_ms=timestamp_ms,
                vpin=0.0,
                vpin_zscore=0.0,
                toxicity_level=ToxicityLevel.LOW,
                buckets_completed=0,
                buy_ratio=0.5,
                sell_ratio=0.5
            )

        total_imbalance = sum(b.imbalance for b in self.buckets)
        total_vol = sum(b.volume for b in self.buckets)
        total_buy = sum(b.buy_volume for b in self.buckets)
        total_sell = sum(b.sell_volume for b in self.buckets)

        vpin = total_imbalance / total_vol if total_vol > 0 else 0.0
        self.history_vpin.append(vpin)

        if len(self.history_vpin) >= 20:
            arr = np.array(self.history_vpin)
            mean_vpin = np.mean(arr)
            std_vpin = np.std(arr) + 1e-8
            zscore = float((vpin - mean_vpin) / std_vpin)
        else:
            zscore = 0.0

        if vpin >= self.critical_threshold or zscore >= self.zscore_critical:
            level = ToxicityLevel.CRITICAL
        elif vpin >= self.elevated_threshold or zscore >= 1.0:
            level = ToxicityLevel.ELEVATED
        elif vpin >= 0.35:
            level = ToxicityLevel.NORMAL
        else:
            level = ToxicityLevel.LOW

        buy_ratio = total_buy / total_vol if total_vol > 0 else 0.5
        sell_ratio = total_sell / total_vol if total_vol > 0 else 0.5

        return VPINReading(
            timestamp_ms=timestamp_ms,
            vpin=float(vpin),
            vpin_zscore=zscore,
            toxicity_level=level,
            buckets_completed=len(self.buckets),
            buy_ratio=float(buy_ratio),
            sell_ratio=float(sell_ratio)
        )
