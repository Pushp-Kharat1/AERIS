from dataclasses import dataclass
from enum import Enum
from typing import Optional
import numpy as np


class MarketRegime(Enum):
    QUIET_CARRY = 0
    NORMAL_VOLATILITY = 1
    VOLATILITY_EXPANSION = 2
    LIQUIDATION_CRASH = 3


@dataclass
class AllocationWeights:
    regime: MarketRegime
    funding_carry_weight: float
    maker_arb_weight: float
    liquidation_shock_weight: float
    stat_arb_weight: float
    total_leverage_cap: float
    vpin_shield_active: bool = False
    oi_flush_detected: bool = False


class MetaAllocator:
    def __init__(self, max_allowed_leverage: float = 2.5):
        self.max_allowed_leverage = max_allowed_leverage

    def classify_regime(
        self,
        realized_vol_1h_pct: float,
        bid_ask_spread_bps: float,
        liquidation_spike_ratio: float,
        vpin: float = 0.35,
        vpin_zscore: float = 0.0,
        oi_drop_5m_pct: float = 0.0
    ) -> MarketRegime:
        is_crash = (
            realized_vol_1h_pct > 6.0
            or liquidation_spike_ratio > 4.0
            or bid_ask_spread_bps > 10.0
            or (oi_drop_5m_pct <= -2.5 and liquidation_spike_ratio >= 2.0)
        )
        if is_crash:
            return MarketRegime.LIQUIDATION_CRASH

        is_expansion_or_toxic = (
            realized_vol_1h_pct > 3.5
            or bid_ask_spread_bps > 3.0
            or liquidation_spike_ratio > 2.0
            or vpin >= 0.70
            or vpin_zscore >= 2.0
        )
        if is_expansion_or_toxic:
            return MarketRegime.VOLATILITY_EXPANSION

        if realized_vol_1h_pct > 1.0 or bid_ask_spread_bps > 1.2 or vpin >= 0.50:
            return MarketRegime.NORMAL_VOLATILITY

        return MarketRegime.QUIET_CARRY

    def compute_weights(
        self,
        realized_vol_1h_pct: float,
        bid_ask_spread_bps: float,
        liquidation_spike_ratio: float,
        vpin: float = 0.35,
        vpin_zscore: float = 0.0,
        oi_drop_5m_pct: float = 0.0,
        onchain_bias: int = 0
    ) -> AllocationWeights:
        regime = self.classify_regime(
            realized_vol_1h_pct=realized_vol_1h_pct,
            bid_ask_spread_bps=bid_ask_spread_bps,
            liquidation_spike_ratio=liquidation_spike_ratio,
            vpin=vpin,
            vpin_zscore=vpin_zscore,
            oi_drop_5m_pct=oi_drop_5m_pct
        )

        vpin_shield = vpin >= 0.70 or vpin_zscore >= 2.0
        oi_flush = oi_drop_5m_pct <= -2.5 and liquidation_spike_ratio >= 2.0

        if regime == MarketRegime.QUIET_CARRY:
            carry_w = 0.75 if onchain_bias == 1 else 0.70
            maker_w = 0.15 if onchain_bias == 1 else 0.20
            return AllocationWeights(
                regime=regime,
                funding_carry_weight=carry_w,
                maker_arb_weight=maker_w,
                liquidation_shock_weight=0.00,
                stat_arb_weight=0.10,
                total_leverage_cap=min(2.0, self.max_allowed_leverage),
                vpin_shield_active=vpin_shield,
                oi_flush_detected=oi_flush
            )

        elif regime == MarketRegime.NORMAL_VOLATILITY:
            maker_w = 0.00 if vpin_shield else 0.35
            carry_w = 0.70 if vpin_shield else 0.35
            return AllocationWeights(
                regime=regime,
                funding_carry_weight=carry_w,
                maker_arb_weight=maker_w,
                liquidation_shock_weight=0.05,
                stat_arb_weight=0.25,
                total_leverage_cap=min(2.2, self.max_allowed_leverage),
                vpin_shield_active=vpin_shield,
                oi_flush_detected=oi_flush
            )

        elif regime == MarketRegime.VOLATILITY_EXPANSION:
            return AllocationWeights(
                regime=regime,
                funding_carry_weight=0.50,
                maker_arb_weight=0.00,
                liquidation_shock_weight=0.30,
                stat_arb_weight=0.20,
                total_leverage_cap=min(1.8, self.max_allowed_leverage),
                vpin_shield_active=vpin_shield,
                oi_flush_detected=oi_flush
            )

        else:
            return AllocationWeights(
                regime=regime,
                funding_carry_weight=0.20,
                maker_arb_weight=0.00,
                liquidation_shock_weight=0.80,
                stat_arb_weight=0.00,
                total_leverage_cap=min(1.5, self.max_allowed_leverage),
                vpin_shield_active=vpin_shield,
                oi_flush_detected=oi_flush
            )
