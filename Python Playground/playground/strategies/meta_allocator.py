from dataclasses import dataclass
from enum import Enum
import numpy as np

class MarketRegime(Enum):
    QUIET_CARRY = 0          # Low volatility, steady funding spreads
    NORMAL_VOLATILITY = 1    # Healthy two-way flow, sector divergence
    VOLATILITY_EXPANSION = 2 # Manipulative wicks, fast fakeouts, book thinning
    LIQUIDATION_CRASH = 3    # Violent systemic cascade, massive forced liquidations

@dataclass
class AllocationWeights:
    regime: MarketRegime
    funding_carry_weight: float
    maker_arb_weight: float
    liquidation_shock_weight: float
    stat_arb_weight: float
    total_leverage_cap: float

class MetaAllocator:
    def __init__(self, max_allowed_leverage: float = 2.5):
        self.max_allowed_leverage = max_allowed_leverage

    def classify_regime(
        self,
        realized_vol_1h_pct: float,
        bid_ask_spread_bps: float,
        liquidation_spike_ratio: float
    ) -> MarketRegime:
        # 1. Full-blown crash: systemic cascade
        if realized_vol_1h_pct > 6.0 or liquidation_spike_ratio > 4.0 or bid_ask_spread_bps > 10.0:
            return MarketRegime.LIQUIDATION_CRASH
            
        # 2. Volatility Expansion / Fakeout Trap: fast pump/dump wicks with widening spreads
        elif realized_vol_1h_pct > 3.5 or bid_ask_spread_bps > 3.0 or liquidation_spike_ratio > 2.0:
            return MarketRegime.VOLATILITY_EXPANSION
            
        # 3. Normal trading conditions
        elif realized_vol_1h_pct > 1.0 or bid_ask_spread_bps > 1.2:
            return MarketRegime.NORMAL_VOLATILITY
            
        # 4. Quiet conditions
        else:
            return MarketRegime.QUIET_CARRY

    def compute_weights(
        self,
        realized_vol_1h_pct: float,
        bid_ask_spread_bps: float,
        liquidation_spike_ratio: float
    ) -> AllocationWeights:
        regime = self.classify_regime(
            realized_vol_1h_pct,
            bid_ask_spread_bps,
            liquidation_spike_ratio
        )

        if regime == MarketRegime.QUIET_CARRY:
            # Shift idle capital to safe 25% carry
            return AllocationWeights(
                regime=regime,
                funding_carry_weight=0.70,
                maker_arb_weight=0.20,
                liquidation_shock_weight=0.00,
                stat_arb_weight=0.10,
                total_leverage_cap=2.0
            )

        elif regime == MarketRegime.NORMAL_VOLATILITY:
            # Balanced deployment across active spreads and pairs
            return AllocationWeights(
                regime=regime,
                funding_carry_weight=0.35,
                maker_arb_weight=0.35,
                liquidation_shock_weight=0.05,
                stat_arb_weight=0.25,
                total_leverage_cap=2.2
            )

        elif regime == MarketRegime.VOLATILITY_EXPANSION:
            # DEFENSIVE GUARD: Pull resting maker orders to avoid getting run over by fakeouts
            return AllocationWeights(
                regime=regime,
                funding_carry_weight=0.50, # Lean on hedged carry
                maker_arb_weight=0.00,     # ZERO maker orders (avoids toxic adverse selection)
                liquidation_shock_weight=0.30, # Ready for counter-punch if it cascades
                stat_arb_weight=0.20,     # Tight pairs tracking
                total_leverage_cap=1.8     # De-risk leverage
            )

        else: # LIQUIDATION_CRASH
            # FULL COUNTER-PUNCH: Scoop panicked liquidations at 5-10% discount
            return AllocationWeights(
                regime=regime,
                funding_carry_weight=0.20, # Maintain hedged baseline
                maker_arb_weight=0.00,     # Shut down
                liquidation_shock_weight=0.80, # Scoop the panic wick
                stat_arb_weight=0.00,     # Shut down decoupled pairs
                total_leverage_cap=1.5     # Strict capital preservation
            )
