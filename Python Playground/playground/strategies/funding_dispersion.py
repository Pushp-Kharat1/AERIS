from dataclasses import dataclass
import numpy as np

@dataclass
class FundingOpportunity:
    long_venue: str
    short_venue: str
    spread_rate_8h: float
    annualized_spread_pct: float
    estimated_friction_bps: float
    net_annualized_yield_pct: float
    is_executable: bool

class FundingDispersionStrategy:
    def __init__(
        self,
        min_annualized_spread_pct: float = 15.0,
        rebalance_cost_bps: float = 12.0,
        tds_rate: float = 0.01
    ):
        self.min_annualized_spread_pct = min_annualized_spread_pct
        self.rebalance_cost_bps = rebalance_cost_bps
        self.tds_rate = tds_rate

    def evaluate_spread(
        self,
        rates_8h: dict[str, float]
    ) -> list[FundingOpportunity]:
        opportunities = []
        venues = list(rates_8h.keys())

        for i in range(len(venues)):
            for j in range(len(venues)):
                if i == j:
                    continue
                v_long = venues[i]
                v_short = venues[j]

                r_long = rates_8h[v_long]
                r_short = rates_8h[v_short]

                spread_8h = r_short - r_long
                annualized_spread = spread_8h * 3.0 * 365.0 * 100.0

                friction_bps = self.rebalance_cost_bps + (self.tds_rate * 10000.0)
                net_yield = annualized_spread - (friction_bps / 100.0)
                is_executable = net_yield >= self.min_annualized_spread_pct

                opportunities.append(FundingOpportunity(
                    long_venue=v_long,
                    short_venue=v_short,
                    spread_rate_8h=spread_8h,
                    annualized_spread_pct=annualized_spread,
                    estimated_friction_bps=friction_bps,
                    net_annualized_yield_pct=net_yield,
                    is_executable=is_executable
                ))

        opportunities.sort(key=lambda x: x.net_annualized_yield_pct, reverse=True)
        return opportunities

