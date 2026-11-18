from dataclasses import dataclass

@dataclass
class LeadLagSignal:
    timestamp_ns: int
    leader_symbol: str
    laggard_symbol: str
    direction: int
    leader_price_change_bps: float
    laggard_spread_bps: float
    expected_profit_bps: float
    is_valid: bool

class LeadLagArbitrageStrategy:
    def __init__(
        self,
        min_leader_move_bps: float = 6.0,
        max_laggard_reaction_ms: float = 150.0,
        total_roundtrip_cost_bps: float = 8.0
    ):
        self.min_leader_move_bps = min_leader_move_bps
        self.max_laggard_reaction_ms = max_laggard_reaction_ms
        self.total_roundtrip_cost_bps = total_roundtrip_cost_bps

    def detect_signal(
        self,
        timestamp_ns: int,
        leader_prev_px: float,
        leader_curr_px: float,
        laggard_bid: float,
        laggard_ask: float
    ) -> LeadLagSignal:
        leader_return_bps = ((leader_curr_px - leader_prev_px) / leader_prev_px) * 10000.0
        laggard_mid = (laggard_bid + laggard_ask) / 2.0
        laggard_spread_bps = ((laggard_ask - laggard_bid) / laggard_mid) * 10000.0

        direction = 0
        expected_profit = 0.0

        if leader_return_bps >= self.min_leader_move_bps:
            direction = 1
            expected_profit = leader_return_bps - laggard_spread_bps - self.total_roundtrip_cost_bps
        elif leader_return_bps <= -self.min_leader_move_bps:
            direction = -1
            expected_profit = abs(leader_return_bps) - laggard_spread_bps - self.total_roundtrip_cost_bps

        is_valid = (direction != 0) and (expected_profit > 0.0)

        return LeadLagSignal(
            timestamp_ns=timestamp_ns,
            leader_symbol="LEADER",
            laggard_symbol="LAGGARD",
            direction=direction,
            leader_price_change_bps=leader_return_bps,
            laggard_spread_bps=laggard_spread_bps,
            expected_profit_bps=expected_profit,
            is_valid=is_valid
        )

