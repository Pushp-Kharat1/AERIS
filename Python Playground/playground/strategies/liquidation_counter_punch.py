from dataclasses import dataclass
import numpy as np

@dataclass
class LiquidationWickSignal:
    timestamp_ms: int
    symbol: str
    direction: int
    confidence: float
    trigger_type: str
    entry_price: float
    stop_loss_px: float
    take_profit_px: float
    time_limit_sec: int
    oi_drop_pct: float

class LiquidationCounterPunch:
    def __init__(
        self,
        cascade_drop_bps: float = 180.0,
        min_oi_drop_pct: float = 2.5,
        absorption_wick_threshold: float = 0.35,
        stop_loss_bps: float = 35.0,
        take_profit_bps: float = 140.0,
        time_limit_sec: int = 720
    ):
        self.cascade_drop_bps = cascade_drop_bps
        self.min_oi_drop_pct = min_oi_drop_pct
        self.absorption_wick_threshold = absorption_wick_threshold
        self.stop_loss_bps = stop_loss_bps
        self.take_profit_bps = take_profit_bps
        self.time_limit_sec = time_limit_sec

    def evaluate_wick(
        self,
        timestamp_ms: int,
        symbol: str,
        open_px: float,
        high_px: float,
        low_px: float,
        close_px: float,
        volume: float,
        prior_oi: float,
        curr_oi: float,
        rolling_mean_volume: float
    ) -> LiquidationWickSignal:
        candle_range = high_px - low_px
        if candle_range <= 0.0 or prior_oi <= 0.0:
            return LiquidationWickSignal(timestamp_ms, symbol, 0, 0.0, "NO_RANGE", close_px, 0.0, 0.0, 0, 0.0)

        oi_delta_pct = ((curr_oi - prior_oi) / prior_oi) * 100.0
        candle_return_bps = ((close_px - open_px) / open_px) * 10000.0
        wick_drop_bps = ((open_px - low_px) / open_px) * 10000.0
        wick_pump_bps = ((high_px - open_px) / open_px) * 10000.0
        volume_ratio = volume / (rolling_mean_volume + 1e-9)

        lower_wick_ratio = (min(open_px, close_px) - low_px) / candle_range
        upper_wick_ratio = (high_px - max(open_px, close_px)) / candle_range

        # Long Flush Counter-Punch:
        # 1. Price dumped violently during candle (low plummeted)
        # 2. Open Interest collapsed (forced long liquidations)
        # 3. Aggressive buying absorbed selling: close finished well off lows (lower wick dominant)
        if (
            wick_drop_bps >= self.cascade_drop_bps
            and oi_delta_pct <= -self.min_oi_drop_pct
            and lower_wick_ratio >= self.absorption_wick_threshold
            and volume_ratio >= 2.0
        ):
            stop_px = low_px * (1.0 - (self.stop_loss_bps / 10000.0))
            tp_px = close_px * (1.0 + (self.take_profit_bps / 10000.0))
            confidence = min(1.0, (lower_wick_ratio / 0.5) * (abs(oi_delta_pct) / self.min_oi_drop_pct))
            return LiquidationWickSignal(
                timestamp_ms=timestamp_ms,
                symbol=symbol,
                direction=1,
                confidence=confidence,
                trigger_type="LONG_LIQUIDATION_ABSORPTION_WICK",
                entry_price=close_px,
                stop_loss_px=stop_px,
                take_profit_px=tp_px,
                time_limit_sec=self.time_limit_sec,
                oi_drop_pct=oi_delta_pct
            )

        # Short Squeeze Counter-Punch:
        # 1. Price spiked violently (forced short stops)
        # 2. Open Interest collapsed
        # 3. Heavy seller absorption rejected highs (upper wick dominant)
        if (
            wick_pump_bps >= self.cascade_drop_bps
            and oi_delta_pct <= -self.min_oi_drop_pct
            and upper_wick_ratio >= self.absorption_wick_threshold
            and volume_ratio >= 2.0
        ):
            stop_px = high_px * (1.0 + (self.stop_loss_bps / 10000.0))
            tp_px = close_px * (1.0 - (self.take_profit_bps / 10000.0))
            confidence = min(1.0, (upper_wick_ratio / 0.5) * (abs(oi_delta_pct) / self.min_oi_drop_pct))
            return LiquidationWickSignal(
                timestamp_ms=timestamp_ms,
                symbol=symbol,
                direction=-1,
                confidence=confidence,
                trigger_type="SHORT_SQUEEZE_ABSORPTION_WICK",
                entry_price=close_px,
                stop_loss_px=stop_px,
                take_profit_px=tp_px,
                time_limit_sec=self.time_limit_sec,
                oi_drop_pct=oi_delta_pct
            )

        return LiquidationWickSignal(timestamp_ms, symbol, 0, 0.0, "NO_SIGNAL", close_px, 0.0, 0.0, 0, oi_delta_pct)
