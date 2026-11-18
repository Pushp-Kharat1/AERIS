import asyncio
import sys
import time
from datetime import datetime
from pathlib import Path

# Add project root to sys.path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import ccxt.async_support as ccxt
import polars as pl
from strategies.meta_allocator import MetaAllocator, MarketRegime

class LivePaperSimulator:
    def __init__(self, initial_usd: float = 100000.0, runtime_minutes: float = 15.0):
        self.initial_usd = initial_usd
        self.total_equity = initial_usd
        self.runtime_minutes = runtime_minutes
        self.allocator = MetaAllocator(max_allowed_leverage=2.5)
        self.tracked_symbols = [
            "ADA/USDT:USDT", "LINK/USDT:USDT", "DOGE/USDT:USDT", "SUI/USDT:USDT",
            "ETH/USDT:USDT", "AVAX/USDT:USDT", "BTC/USDT:USDT", "SOL/USDT:USDT"
        ]
        
        # Fee model: 2 bps maker, 5 bps taker, 1% TDS
        self.taker_fee_bps = 5.0
        self.tds_rate = 0.01
        
        self.active_positions = {}
        self.realized_pnl = 0.0
        self.total_fees_paid = 0.0
        self.total_tds_paid = 0.0
        self.log_history = []

    async def fetch_telemetry(self, binance, bybit):
        # 1. Fetch 1h BTC candles to measure real live 1h volatility
        btc_candles = await binance.fetch_ohlcv("BTC/USDT", timeframe="1h", limit=5)
        highs = [c[2] for c in btc_candles]
        lows = [c[3] for c in btc_candles]
        vol_1h_pct = ((max(highs) - min(lows)) / min(lows)) * 100.0 if lows else 1.0

        # 2. Measure live BBO spread on BTC
        b_ticker = await binance.fetch_ticker("BTC/USDT")
        b_bid = b_ticker.get("bid", 0.0) or 1.0
        b_ask = b_ticker.get("ask", 0.0) or 1.0
        spread_bps = ((b_ask - b_bid) / b_bid) * 10000.0

        # 3. Scan live funding rates across target universe
        funding_spreads = []
        for sym in self.tracked_symbols:
            try:
                b_fr = (await binance.fetch_funding_rate(sym)).get("fundingRate", 0.0)
                by_fr = (await bybit.fetch_funding_rate(sym)).get("fundingRate", 0.0)
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
                continue

        funding_spreads.sort(key=lambda x: x["ann_apr"], reverse=True)
        return vol_1h_pct, spread_bps, funding_spreads

    async def execute_rebalance(self, target_trade, capital_alloc):
        sym = target_trade["symbol"]
        if sym in self.active_positions:
            return

        trade_size_usd = capital_alloc * 0.80
        entry_fees = trade_size_usd * (self.taker_fee_bps / 10000.0) * 2.0
        entry_tds = trade_size_usd * self.tds_rate * 2.0

        self.total_fees_paid += entry_fees
        self.total_tds_paid += entry_tds
        self.total_equity -= (entry_fees + entry_tds)

        self.active_positions[sym] = {
            "size_usd": trade_size_usd,
            "long_venue": target_trade["long_venue"],
            "short_venue": target_trade["short_venue"],
            "spread_8h": target_trade["spread_8h"],
            "ann_apr": target_trade["ann_apr"],
            "entry_time": time.time()
        }
        print(f"[{datetime.now().strftime('%H:%M:%S')}] EXECUTED DELTA-NEUTRAL CARRY on {sym}:")
        print(f"   Long: {target_trade['long_venue']} | Short: {target_trade['short_venue']} | Size: ${trade_size_usd:,.2f}")
        print(f"   Spread: {target_trade['ann_apr']:.2f}% APR | Deducted Friction (Fees + TDS): ${entry_fees + entry_tds:.2f}")

    def accrue_continuous_funding(self, elapsed_sec):
        hours = elapsed_sec / 3600.0
        for sym, pos in self.active_positions.items():
            hourly_rate = (pos["spread_8h"] / 8.0)
            carry_earned = pos["size_usd"] * hourly_rate * hours
            self.realized_pnl += carry_earned
            self.total_equity += carry_earned

    async def run(self):
        print("==========================================================================")
        print("AERIS LIVE PAPER PORTFOLIO SIMULATION")
        print(f"Initial Capital: ${self.initial_usd:,.2f} | Max Leverage: 2.5x | Target Duration: {self.runtime_minutes} mins")
        print("Data Source: Live Binance & Bybit WebSockets / REST Telemetry")
        print("Friction: 5 bps Taker Fee + 1% Section 194S TDS Deducted at Execution")
        print("==========================================================================\n")

        binance = ccxt.binance({"enableRateLimit": True})
        bybit = ccxt.bybit({"enableRateLimit": True})

        start_time = time.time()
        target_seconds = self.runtime_minutes * 60.0
        iteration = 0

        try:
            while (time.time() - start_time) < target_seconds:
                iteration += 1
                loop_start = time.time()

                vol_1h, spread_bps, spreads = await self.fetch_telemetry(binance, bybit)

                weights = self.allocator.compute_weights(
                    realized_vol_1h_pct=vol_1h,
                    bid_ask_spread_bps=spread_bps,
                    liquidation_spike_ratio=1.0
                )

                funding_capital = self.total_equity * weights.funding_carry_weight
                elapsed_min = (time.time() - start_time) / 60.0

                print(f"--- [Tick #{iteration:02d} | T+{elapsed_min:.1f}m] Market Telemetry & Decision ---")
                print(f"Realized Vol (1h): {vol_1h:.2f}% | Spread: {spread_bps:.4f} bps | Detected Regime: {weights.regime.name}")
                print(f"Parent Allocation -> Funding Carry: {weights.funding_carry_weight*100:.0f}% (${funding_capital:,.2f}) | Maker: {weights.maker_arb_weight*100:.0f}% | Liquidation: {weights.liquidation_shock_weight*100:.0f}%")

                if spreads:
                    top = spreads[0]
                    print(f"Top Live Opportunity: {top['symbol']} at {top['ann_apr']:.2f}% Net APR ({top['long_venue']} Long / {top['short_venue']} Short)")
                    if not self.active_positions and weights.funding_carry_weight > 0.0:
                        await self.execute_rebalance(top, funding_capital)

                self.accrue_continuous_funding(elapsed_sec=60.0)

                print(f"PORTFOLIO STATUS:")
                print(f"   Current Equity: ${self.total_equity:,.2f} | Gross Carry Earned: ${self.realized_pnl:.4f}")
                print(f"   Total Fees Paid: ${self.total_fees_paid:.2f} | Total TDS Deducted: ${self.total_tds_paid:.2f}")
                print(f"   Net Realized Drift: ${self.total_equity - self.initial_usd:,.2f}\n")

                self.log_history.append({
                    "timestamp": time.time(),
                    "equity": self.total_equity,
                    "regime": weights.regime.name,
                    "vol_1h": vol_1h,
                    "top_asset": spreads[0]["symbol"] if spreads else "None",
                    "top_apr": spreads[0]["ann_apr"] if spreads else 0.0
                })

                elapsed_loop = time.time() - loop_start
                sleep_time = max(1.0, 60.0 - elapsed_loop)
                await asyncio.sleep(sleep_time)

        finally:
            await binance.close()
            await bybit.close()

            out_dir = Path("D:/Pushp - Do not open without permission/AERIS/Python Playground/playground/data/simulation_logs")
            out_dir.mkdir(parents=True, exist_ok=True)
            log_df = pl.DataFrame(self.log_history)
            log_path = out_dir / f"paper_sim_{int(time.time())}.parquet"
            log_df.write_parquet(log_path)

            print("==========================================================================")
            print("15-MINUTE LIVE TELEMETRY SIMULATION COMPLETE")
            print(f"Final Total Equity: ${self.total_equity:,.2f}")
            print(f"Net P&L: ${self.total_equity - self.initial_usd:,.2f}")
            print(f"Audit log saved to: {log_path.name}")
            print("==========================================================================")

if __name__ == "__main__":
    sim = LivePaperSimulator(initial_usd=100000.0, runtime_minutes=15.0)
    asyncio.run(sim.run())
