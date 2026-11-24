"""
AERIS MPM LIVE DAEMON
Continuously collects live Binance & Bybit derivatives data (Open Interest, Liquidations, Spread, Volatility)
Classifies the regime via MetaAllocator & VPIN, and streams MacroPayload into aeris_shm.dat over zero-copy IPC.
"""

import asyncio
import os
from pathlib import Path
import sys
import time
from typing import List

# Ensure Python Playground is in path
sys.path.insert(0, str(Path(__file__).resolve().parent))

from collectors.coinglass_oi import DerivativesOICollector
from ipc_bridge import SharedMemoryBridgePy
from strategies.meta_allocator import MarketRegime, MetaAllocator
from strategies.vpin_filter import VPINFilter
import httpx


async def run_mpm_daemon():
    print("\n" + "=" * 80)
    print("       AERIS MACRO PARENT MODEL (MPM) LIVE TELEMETRY DAEMON")
    print("=" * 80)

    shm_path = Path(__file__).resolve().parent / "data" / "aeris_shm.dat"
    shm_path.parent.mkdir(parents=True, exist_ok=True)
    bridge = SharedMemoryBridgePy(shm_path)

    allocator = MetaAllocator(max_allowed_leverage=2.5)
    vpin = VPINFilter(bucket_volume=2.0, num_buckets=30)
    oi_collector = DerivativesOICollector(symbol="BTCUSDT")

    universe = ["BTCUSDT", "ETHUSDT"]
    print(f"[MPM] Initialized SharedMemoryBridge at: {shm_path}")
    print("[MPM] Streaming live OI & Liquidation feeds from Binance Futures...\n")

    # Start background liquidation stream if supported
    asyncio.create_task(oi_collector.stream_liquidations())

    seq = 0
    client = httpx.AsyncClient(timeout=4.0)

    try:
        while True:
            t0 = time.time()
            now_ms = int(t0 * 1000)

            # 1. Fetch live Open Interest
            oi_snap = await oi_collector.fetch_binance_oi()

            # 2. Fetch live Ticker & Price metrics for BTC
            try:
                res_ticker = await client.get("https://fapi.binance.com/fapi/v1/ticker/bookTicker?symbol=BTCUSDT")
                if res_ticker.status_code == 200:
                    t_data = res_ticker.json()
                    bid = float(t_data.get("bidPrice", 60000.0))
                    ask = float(t_data.get("askPrice", 60001.0))
                    spread_bps = ((ask - bid) / bid) * 10000.0
                else:
                    spread_bps = 1.0
            except Exception:
                spread_bps = 1.0

            # 3. Fetch recent trades for VPIN toxicity estimation
            try:
                res_trades = await client.get("https://fapi.binance.com/fapi/v1/trades?symbol=BTCUSDT&limit=25")
                if res_trades.status_code == 200:
                    trades = res_trades.json()
                    for tr in trades:
                        vpin.update_trade(
                            timestamp_ms=int(tr.get("time", now_ms)),
                            price=float(tr.get("price", 60000.0)),
                            qty=float(tr.get("qty", 0.1)),
                            is_buyer_maker=bool(tr.get("isBuyerMaker", False)),
                        )
            except Exception:
                pass

            # 4. Evaluate Derivatives & Microstructure Signals
            oi_sig = oi_collector.evaluate_metrics(now_ms)
            vpin_r = vpin.compute_vpin(now_ms)
            vpin_val = vpin_r.vpin if vpin_r else 0.35
            vpin_z = vpin_r.vpin_zscore if vpin_r else 0.0

            # Estimate 1h volatility (default 1.5% if cold)
            vol_1h = 1.5

            weights = allocator.compute_weights(
                realized_vol_1h_pct=vol_1h,
                bid_ask_spread_bps=spread_bps,
                liquidation_spike_ratio=oi_sig.liquidation_spike_ratio,
                vpin=vpin_val,
                vpin_zscore=vpin_z,
                oi_drop_5m_pct=oi_sig.oi_change_5m_pct,
            )

            # 4. Determine Macro Bias:
            # 1 = SUPPLY_SQUEEZE (Bullish - rejects shorts)
            # 2 = DISTRIBUTION (Bearish - rejects longs)
            # 0 = NEUTRAL / CHOP (Allows disciplined two-way probes)
            if oi_sig.is_short_squeeze or (oi_sig.oi_change_5m_pct > 1.0 and oi_snap and oi_snap.price > 0):
                macro_bias = 1
                bias_str = "SUPPLY_SQUEEZE (Bullish)"
            elif oi_sig.is_long_flush or (oi_sig.oi_change_5m_pct < -2.0) or weights.regime == MarketRegime.LIQUIDATION_CRASH:
                macro_bias = 2
                bias_str = "DISTRIBUTION (Bearish)"
            else:
                macro_bias = 0
                bias_str = "NEUTRAL / CHOP"

            # 5. Stage and Flip into Shared Memory (Atomic Release)
            seq = bridge.stage_and_flip(
                macro_bias=macro_bias,
                leverage_cap=weights.total_leverage_cap,
                universe=universe,
            )

            print(
                f"[MPM DAEMON] 🧠 Seq #{seq:4d} | Regime: {weights.regime.name:<20} | "
                f"Bias: {bias_str:<23} | OI Δ5m: {oi_sig.oi_change_5m_pct:+.2f}% | "
                f"Spread: {spread_bps:4.1f} bps | Lev Cap: {weights.total_leverage_cap:.2f}x",
                flush=True,
            )

            elapsed = time.time() - t0
            sleep_time = max(0.5, 3.0 - elapsed)
            await asyncio.sleep(sleep_time)

    except (KeyboardInterrupt, asyncio.CancelledError):
        print("\n[MPM] Shutting down live daemon...")
    finally:
        await client.aclose()
        oi_collector.stop()
        bridge.close()
        print("[MPM] Shared memory unmapped cleanly.")


if __name__ == "__main__":
    asyncio.run(run_mpm_daemon())
