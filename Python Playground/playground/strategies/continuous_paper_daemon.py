import asyncio
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
import json
from pathlib import Path
import sqlite3
import sys
import time
from typing import Dict, List, Optional
import ccxt.async_support as ccxt
import polars as pl

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from collectors.coinglass_oi import DerivativesOICollector
from collectors.onchain_flows import OnChainFlowCollector
from strategies.meta_allocator import AllocationWeights, MarketRegime, MetaAllocator
from strategies.vpin_filter import ToxicityLevel, VPINFilter


@dataclass
class DaemonPosition:
    symbol: str
    size_usd: float
    long_venue: str
    short_venue: str
    spread_8h: float
    ann_apr: float
    entry_ts_ms: int
    last_funding_accrual_ts: int
    accrued_funding_usd: float
    unrealized_pnl: float


@dataclass
class DaemonTelemetry:
    timestamp_ms: int
    total_equity: float
    cash_usd: float
    deployed_usd: float
    leverage: float
    regime: str
    vpin: float
    vpin_zscore: float
    oi_change_5m_pct: float
    active_positions_count: int
    accrued_funding_total: float
    total_fees_paid: float
    total_tds_paid: float
    net_pnl: float


class ContinuousPaperDaemon:
    def __init__(
        self,
        initial_capital: float = 100000.0,
        db_path: Optional[Path] = None,
        log_dir: Optional[Path] = None,
        poll_interval_sec: float = 15.0
    ):
        self.initial_capital = initial_capital
        self.total_equity = initial_capital
        self.cash_usd = initial_capital
        self.poll_interval_sec = poll_interval_sec

        base_dir = Path(__file__).resolve().parent.parent / "data"
        self.db_path = db_path or (base_dir / "daemon_state.db")
        self.log_dir = log_dir or (base_dir / "daemon_logs")
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self.log_dir.mkdir(parents=True, exist_ok=True)

        self.allocator = MetaAllocator(max_allowed_leverage=2.5)
        self.vpin = VPINFilter(bucket_volume=2.0, num_buckets=30)
        self.oi_collector = DerivativesOICollector(symbol="BTCUSDT")
        self.onchain = OnChainFlowCollector(asset="BTC")

        self.universe = [
            "ADA/USDT:USDT", "LINK/USDT:USDT", "DOGE/USDT:USDT", "SUI/USDT:USDT",
            "ETH/USDT:USDT", "AVAX/USDT:USDT", "BTC/USDT:USDT", "SOL/USDT:USDT"
        ]

        self.taker_fee_bps = 5.0
        self.tds_rate = 0.01

        self.active_positions: Dict[str, DaemonPosition] = {}
        self.total_fees_paid = 0.0
        self.total_tds_paid = 0.0
        self.accrued_funding_total = 0.0
        self.running = False
        self.telemetry_history: List[DaemonTelemetry] = []

        self._init_db()

    def _init_db(self):
        with sqlite3.connect(self.db_path) as conn:
            cursor = conn.cursor()
            cursor.execute("""
                CREATE TABLE IF NOT EXISTS daemon_state (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                )
            """)
            cursor.execute("""
                CREATE TABLE IF NOT EXISTS telemetry_ticks (
                    timestamp_ms INTEGER PRIMARY KEY,
                    equity REAL,
                    cash REAL,
                    deployed REAL,
                    leverage REAL,
                    regime TEXT,
                    vpin REAL,
                    net_pnl REAL
                )
            """)
            conn.commit()

    def persist_state(self):
        with sqlite3.connect(self.db_path) as conn:
            cursor = conn.cursor()
            state = {
                "total_equity": self.total_equity,
                "cash_usd": self.cash_usd,
                "total_fees_paid": self.total_fees_paid,
                "total_tds_paid": self.total_tds_paid,
                "accrued_funding_total": self.accrued_funding_total,
                "positions": {k: asdict(v) for k, v in self.active_positions.items()}
            }
            cursor.execute("INSERT OR REPLACE INTO daemon_state (key, value) VALUES ('live_state', ?)", (json.dumps(state),))
            conn.commit()

    def recover_state(self) -> bool:
        with sqlite3.connect(self.db_path) as conn:
            cursor = conn.cursor()
            cursor.execute("SELECT value FROM daemon_state WHERE key = 'live_state'")
            row = cursor.fetchone()
            if row:
                state = json.loads(row[0])
                self.total_equity = state["total_equity"]
                self.cash_usd = state["cash_usd"]
                self.total_fees_paid = state["total_fees_paid"]
                self.total_tds_paid = state["total_tds_paid"]
                self.accrued_funding_total = state["accrued_funding_total"]
                self.active_positions = {
                    k: DaemonPosition(**v) for k, v in state.get("positions", {}).items()
                }
                return True
        return False

    async def fetch_market_state(self, binance, bybit):
        btc_candles = await binance.fetch_ohlcv("BTC/USDT", timeframe="1h", limit=5)
        highs = [c[2] for c in btc_candles]
        lows = [c[3] for c in btc_candles]
        vol_1h = ((max(highs) - min(lows)) / min(lows)) * 100.0 if lows else 1.0

        b_ticker = await binance.fetch_ticker("BTC/USDT")
        bid = b_ticker.get("bid", 0.0) or 1.0
        ask = b_ticker.get("ask", 0.0) or 1.0
        spread_bps = ((ask - bid) / bid) * 10000.0

        oi_snap = await self.oi_collector.fetch_binance_oi()
        oi_sig = self.oi_collector.evaluate_metrics()

        b_trades = await binance.fetch_trades("BTC/USDT", limit=15)
        last_vpin_r = None
        for t in b_trades:
            last_vpin_r = self.vpin.update_trade(
                timestamp_ms=int(t["timestamp"]),
                price=float(t["price"]),
                qty=float(t["amount"]),
                is_buyer_maker=(t["side"] == "sell")
            )

        funding_spreads = []
        try:
            b_rates_task = binance.fetch_funding_rates(self.universe)
            by_rates_task = bybit.fetch_funding_rates(self.universe)
            b_rates, by_rates = await asyncio.gather(b_rates_task, by_rates_task, return_exceptions=True)
            
            b_dict = b_rates if isinstance(b_rates, dict) else {}
            by_dict = by_rates if isinstance(by_rates, dict) else {}
            
            for sym in self.universe:
                b_fr = b_dict.get(sym, {}).get("fundingRate", 0.0) if isinstance(b_dict.get(sym), dict) else 0.0
                by_fr = by_dict.get(sym, {}).get("fundingRate", 0.0) if isinstance(by_dict.get(sym), dict) else 0.0
                if b_fr is None: b_fr = 0.0
                if by_fr is None: by_fr = 0.0
                spread_8h = abs(b_fr - by_fr)
                ann_apr = spread_8h * 3.0 * 365.0 * 100.0
                funding_spreads.append({
                    "symbol": sym,
                    "binance_8h": b_fr,
                    "bybit_8h": by_fr,
                    "spread_8h": spread_8h,
                    "ann_apr": ann_apr,
                    "long_venue": "Bybit" if by_fr < b_fr else "Binance",
                    "short_venue": "Binance" if by_fr < b_fr else "Bybit"
                })
        except Exception:
            pass

        funding_spreads.sort(key=lambda x: x["ann_apr"], reverse=True)
        return vol_1h, spread_bps, oi_sig, last_vpin_r, funding_spreads

    def accrue_funding(self, now_ms: int):
        for sym, pos in self.active_positions.items():
            elapsed_sec = (now_ms - pos.last_funding_accrual_ts) / 1000.0
            if elapsed_sec >= 60.0:
                fraction_of_8h = elapsed_sec / (8.0 * 3600.0)
                payout = pos.size_usd * pos.spread_8h * fraction_of_8h
                pos.accrued_funding_usd += payout
                pos.last_funding_accrual_ts = now_ms
                self.accrued_funding_total += payout
                self.cash_usd += payout
                self.total_equity += payout

    def rebalance(self, weights: AllocationWeights, top_spreads: List[Dict], now_ms: int):
        deployed_usd = sum(p.size_usd for p in self.active_positions.values())
        max_deployable = self.total_equity * weights.total_leverage_cap

        if weights.regime in [MarketRegime.VOLATILITY_EXPANSION, MarketRegime.LIQUIDATION_CRASH] and weights.vpin_shield_active:
            if deployed_usd > max_deployable:
                excess = deployed_usd - max_deployable
                for sym in list(self.active_positions.keys()):
                    pos = self.active_positions.pop(sym)
                    self.cash_usd += pos.size_usd
                    deployed_usd -= pos.size_usd
                    if deployed_usd <= max_deployable:
                        break

        target_carry_deploy = max_deployable * weights.funding_carry_weight
        if deployed_usd < target_carry_deploy and top_spreads:
            for candidate in top_spreads[:3]:
                sym = candidate["symbol"]
                if sym in self.active_positions or candidate["ann_apr"] < 12.0:
                    continue

                slot_size = min(self.cash_usd * 0.5, (target_carry_deploy - deployed_usd) / 2.0)
                if slot_size < 1000.0:
                    break

                fees = slot_size * (self.taker_fee_bps / 10000.0) * 2.0
                tds = slot_size * self.tds_rate * 2.0
                total_drag = fees + tds

                self.cash_usd -= (slot_size + total_drag)
                self.total_equity -= total_drag
                self.total_fees_paid += fees
                self.total_tds_paid += tds

                self.active_positions[sym] = DaemonPosition(
                    symbol=sym,
                    size_usd=slot_size,
                    long_venue=candidate["long_venue"],
                    short_venue=candidate["short_venue"],
                    spread_8h=candidate["spread_8h"],
                    ann_apr=candidate["ann_apr"],
                    entry_ts_ms=now_ms,
                    last_funding_accrual_ts=now_ms,
                    accrued_funding_usd=0.0,
                    unrealized_pnl=0.0
                )
                deployed_usd += slot_size

    async def run_step(self, binance, bybit):
        now_ms = int(time.time() * 1000)
        vol_1h, spread_bps, oi_sig, vpin_r, spreads = await self.fetch_market_state(binance, bybit)

        vpin_val = vpin_r.vpin if vpin_r else 0.35
        vpin_z = vpin_r.vpin_zscore if vpin_r else 0.0

        weights = self.allocator.compute_weights(
            realized_vol_1h_pct=vol_1h,
            bid_ask_spread_bps=spread_bps,
            liquidation_spike_ratio=oi_sig.liquidation_spike_ratio,
            vpin=vpin_val,
            vpin_zscore=vpin_z,
            oi_drop_5m_pct=oi_sig.oi_change_5m_pct
        )

        self.accrue_funding(now_ms)
        self.rebalance(weights, spreads, now_ms)

        deployed = sum(p.size_usd for p in self.active_positions.values())
        leverage = deployed / self.total_equity if self.total_equity > 0 else 0.0
        net_pnl = self.total_equity - self.initial_capital

        telem = DaemonTelemetry(
            timestamp_ms=now_ms,
            total_equity=self.total_equity,
            cash_usd=self.cash_usd,
            deployed_usd=deployed,
            leverage=leverage,
            regime=weights.regime.name,
            vpin=vpin_val,
            vpin_zscore=vpin_z,
            oi_change_5m_pct=oi_sig.oi_change_5m_pct,
            active_positions_count=len(self.active_positions),
            accrued_funding_total=self.accrued_funding_total,
            total_fees_paid=self.total_fees_paid,
            total_tds_paid=self.total_tds_paid,
            net_pnl=net_pnl
        )
        self.telemetry_history.append(telem)

        with sqlite3.connect(self.db_path) as conn:
            cursor = conn.cursor()
            cursor.execute(
                "INSERT OR REPLACE INTO telemetry_ticks VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (now_ms, self.total_equity, self.cash_usd, deployed, leverage, weights.regime.name, vpin_val, net_pnl)
            )
            conn.commit()

        self.persist_state()
        return telem

    def dump_history_parquet(self) -> Path:
        dicts = [asdict(t) for t in self.telemetry_history]
        df = pl.DataFrame(dicts)
        out_file = self.log_dir / f"daemon_telemetry_{int(time.time())}.parquet"
        df.write_parquet(out_file, compression="zstd")
        return out_file
