from dataclasses import dataclass
from enum import Enum
import numpy as np

class OrderType(Enum):
    LIMIT = "LIMIT"
    IOC = "IOC"
    MARKET = "MARKET"

class Side(Enum):
    BUY = "BUY"
    SELL = "SELL"

@dataclass
class ExecutionFriction:
    maker_fee_bps: float = 2.0
    taker_fee_bps: float = 5.0
    slippage_bps: float = 1.0
    tds_rate: float = 0.01
    latency_ms: float = 25.0

@dataclass
class TradeResult:
    entry_ts: int
    exit_ts: int
    side: Side
    size: float
    entry_price: float
    exit_price: float
    gross_pnl: float
    fees: float
    tds: float
    net_pnl: float
    slippage_cost: float

class SimulationEngine:
    def __init__(self, friction: ExecutionFriction):
        self.friction = friction

    def simulate_ioc_fill(
        self,
        side: Side,
        order_price: float,
        order_size: float,
        best_opposite_price: float,
        opposite_depth_qty: float
    ) -> tuple[bool, float, float]:
        if side == Side.BUY:
            if order_price < best_opposite_price:
                return False, 0.0, 0.0
            fill_price = best_opposite_price * (1.0 + (self.friction.slippage_bps / 10000.0))
        else:
            if order_price > best_opposite_price:
                return False, 0.0, 0.0
            fill_price = best_opposite_price * (1.0 - (self.friction.slippage_bps / 10000.0))

        fill_qty = min(order_size, opposite_depth_qty)
        return (fill_qty > 0.0), fill_price, fill_qty

    def calculate_trade_costs(self, notional: float, is_maker: bool) -> tuple[float, float]:
        fee_bps = self.friction.maker_fee_bps if is_maker else self.friction.taker_fee_bps
        fee = notional * (fee_bps / 10000.0)
        tds = notional * self.friction.tds_rate
        return fee, tds

    def execute_roundtrip(
        self,
        entry_ts: int,
        exit_ts: int,
        side: Side,
        size: float,
        entry_price: float,
        exit_price: float,
        entry_is_maker: bool = False,
        exit_is_maker: bool = False
    ) -> TradeResult:
        entry_notional = size * entry_price
        exit_notional = size * exit_price

        if side == Side.BUY:
            gross_pnl = (exit_price - entry_price) * size
        else:
            gross_pnl = (entry_price - exit_price) * size

        entry_fee, entry_tds = self.calculate_trade_costs(entry_notional, entry_is_maker)
        exit_fee, exit_tds = self.calculate_trade_costs(exit_notional, exit_is_maker)

        total_fees = entry_fee + exit_fee
        total_tds = entry_tds + exit_tds
        slippage_cost = (entry_notional + exit_notional) * (self.friction.slippage_bps / 10000.0)
        net_pnl = gross_pnl - total_fees - total_tds

        return TradeResult(
            entry_ts=entry_ts,
            exit_ts=exit_ts,
            side=side,
            size=size,
            entry_price=entry_price,
            exit_price=exit_price,
            gross_pnl=gross_pnl,
            fees=total_fees,
            tds=total_tds,
            net_pnl=net_pnl,
            slippage_cost=slippage_cost
        )

