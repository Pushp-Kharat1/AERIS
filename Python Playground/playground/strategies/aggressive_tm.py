from dataclasses import dataclass
import numpy as np

@dataclass
class TakerSignal:
    timestamp_ms: int
    symbol: str
    direction: int # +1 Long, -1 Short, 0 None
    confidence: float
    trigger_type: str
    entry_price: float
    stop_loss_px: float
    take_profit_px: float

class AggressiveTM:
    def __init__(
        self,
        volume_spike_threshold_sigma: float = 3.5,
        min_breakout_bps: float = 120.0,
        stop_loss_bps: float = 60.0,
        take_profit_bps: float = 280.0
    ):
        self.volume_spike_threshold_sigma = volume_spike_threshold_sigma
        self.min_breakout_bps = min_breakout_bps
        self.stop_loss_bps = stop_loss_bps
        self.take_profit_bps = take_profit_bps

    def evaluate_breakout(
        self,
        timestamp_ms: int,
        symbol: str,
        recent_closes: np.ndarray,
        recent_volumes: np.ndarray,
        curr_price: float,
        curr_volume: float
    ) -> TakerSignal:
        if len(recent_closes) < 20 or len(recent_volumes) < 20:
            return TakerSignal(timestamp_ms, symbol, 0, 0.0, "INSUFFICIENT_DATA", curr_price, 0.0, 0.0)

        # Volume spike detection
        vol_mean = np.mean(recent_volumes)
        vol_std = np.std(recent_volumes) + 1e-9
        vol_zscore = (curr_volume - vol_mean) / vol_std

        # Price momentum detection
        ref_price = recent_closes[-1]
        price_change_bps = ((curr_price - ref_price) / ref_price) * 10000.0

        direction = 0
        trigger = "NONE"

        # Explosive upward breakout
        if vol_zscore >= self.volume_spike_threshold_sigma and price_change_bps >= self.min_breakout_bps:
            direction = 1
            trigger = "VOL_EXPANSION_BULLISH_BREAKOUT"
            stop_px = curr_price * (1.0 - (self.stop_loss_bps / 10000.0))
            tp_px = curr_price * (1.0 + (self.take_profit_bps / 10000.0))

        # Explosive downward liquidation cascade
        elif vol_zscore >= self.volume_spike_threshold_sigma and price_change_bps <= -self.min_breakout_bps:
            direction = -1
            trigger = "LIQUIDATION_CASCADE_BEARISH_BREAKOUT"
            stop_px = curr_price * (1.0 + (self.stop_loss_bps / 10000.0))
            tp_px = curr_price * (1.0 - (self.take_profit_bps / 10000.0))

        else:
            stop_px = 0.0
            tp_px = 0.0

        confidence = min(1.0, abs(vol_zscore) / 5.0)

        return TakerSignal(
            timestamp_ms=timestamp_ms,
            symbol=symbol,
            direction=direction,
            confidence=confidence, 
            trigger_type=trigger,
            entry_price=curr_price,
            stop_loss_px=stop_px,
            take_profit_px=tp_px
        )

AggressiveTakerMomentum = AggressiveTM


