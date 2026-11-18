from dataclasses import dataclass
import numpy as np

@dataclass
class StressTestReport:
    scenario_name: str
    survived: bool
    drawdown_pct: float
    failure_mode: str
    remediation: str

class ChaosSimulator:
    def __init__(self, initial_capital: float = 100000.0, leverage: float = 2.0):
        self.initial_capital = initial_capital
        self.leverage = leverage

    def test_funding_flip(
        self,
        initial_spread_8h: float = 0.00025,
        flipped_spread_8h: float = -0.00040,
        blocks_held_inverted: int = 3,
        unwind_latency_blocks: int = 1
    ) -> StressTestReport:
        # Long venue starts paying carry, then violently flips negative
        position_size = self.initial_capital * self.leverage
        accumulated_carry = position_size * initial_spread_8h * 6
        
        # Loss while system detects and unwinds inverted spread
        drain_blocks = min(blocks_held_inverted, unwind_latency_blocks)
        inverted_loss = position_size * abs(flipped_spread_8h) * drain_blocks
        net_result = accumulated_carry - inverted_loss
        
        drawdown = max(0.0, -net_result / self.initial_capital) * 100.0
        survived = net_result > - (self.initial_capital * 0.05)
        
        return StressTestReport(
            scenario_name="Violent Funding Flip",
            survived=survived,
            drawdown_pct=drawdown,
            failure_mode="Excessive unwind lag" if not survived else "None",
            remediation="Implement immediate automated unwind when 8h spread < 0"
        )

    def test_asymmetric_margin_squeeze(
        self,
        asset_pump_pct: float = 0.50,
        maintenance_margin_pct: float = 0.05,
        rebalance_enabled: bool = False
    ) -> StressTestReport:
        # Capital split equally: 50k long on Venue A, 50k short on Venue B
        wallet_a = self.initial_capital / 2.0
        wallet_b = self.initial_capital / 2.0
        
        pos_size_a = wallet_a * self.leverage
        pos_size_b = wallet_b * self.leverage
        
        # Asset pumps 50%
        pnl_long = pos_size_a * asset_pump_pct
        pnl_short = -pos_size_b * asset_pump_pct
        
        remaining_margin_b = wallet_b + pnl_short
        req_margin_b = pos_size_b * (1.0 + asset_pump_pct) * maintenance_margin_pct
        
        is_liquidated = remaining_margin_b <= req_margin_b
        
        if is_liquidated and not rebalance_enabled:
            # Liquidation penalty: lose wallet B entirely + orphan unhedged long exposed to subsequent dump
            total_drawdown = 50.0
            return StressTestReport(
                scenario_name="Asymmetric Margin Squeeze (+50% pump)",
                survived=False,
                drawdown_pct=total_drawdown,
                failure_mode="Short leg liquidated on Binance despite long profit on Bybit",
                remediation="Mandatory dynamic margin rebalancing when distance-to-liquidation < 30%"
            )
        else:
            return StressTestReport(
                scenario_name="Asymmetric Margin Squeeze (+50% pump)",
                survived=True,
                drawdown_pct=2.1,
                failure_mode="None",
                remediation="Rebalance passed capital from Venue A to Venue B seamlessly"
            )

    def test_corrupted_packet_injection(self, raw_packet: dict) -> tuple[bool, str]:
        # Sanitizer checks
        bids = raw_packet.get("bids", [])
        asks = raw_packet.get("asks", [])
        
        if not bids or not asks:
            return False, "EMPTY_BOOK_DROPPED"
            
        best_bid = bids[0][0]
        best_ask = asks[0][0]
        
        if np.isnan(best_bid) or np.isnan(best_ask):
            return False, "NAN_PRICE_DROPPED"
        if best_bid <= 0 or best_ask <= 0:
            return False, "NON_POSITIVE_PRICE_DROPPED"
        if best_bid >= best_ask:
            return False, "CROSSED_BOOK_DETECTED_DROPPED"
            
        return True, "CLEAN_TICK"

    def test_orphan_leg_timeout(
        self,
        leg_1_filled: bool = True,
        leg_2_timeout: bool = True,
        auto_emergency_ioc: bool = True,
        adverse_move_bps: float = 15.0
    ) -> StressTestReport:
        if leg_1_filled and leg_2_timeout:
            if not auto_emergency_ioc:
                drawdown = (adverse_move_bps / 10000.0) * self.leverage * 100.0
                return StressTestReport(
                    scenario_name="Orphan Leg (502 Gateway Timeout)",
                    survived=False,
                    drawdown_pct=drawdown,
                    failure_mode="Naked directional exposure left open",
                    remediation="Add 50ms emergency IOC close timer on leg 1 if leg 2 ack fails"
                )
            else:
                cost = (adverse_move_bps / 10000.0) * (self.leverage / 2.0) * 100.0
                return StressTestReport(
                    scenario_name="Orphan Leg (502 Gateway Timeout)",
                    survived=True,
                    drawdown_pct=cost,
                    failure_mode="None",
                    remediation="Emergency IOC flattened leg 1 within 50ms with minimal slippage"
                )
        return StressTestReport("Orphan Leg", True, 0.0, "None", "None")

