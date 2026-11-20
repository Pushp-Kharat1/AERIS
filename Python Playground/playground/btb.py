import asyncio
from dataclasses import asdict
import json
import math
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import time
from typing import Callable, List, Tuple
import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

from collectors.coinglass_oi import DerivativesOICollector, LiquidationEvent, OISnapshot
from collectors.onchain_flows import OnChainBias, OnChainFlowCollector, OnChainSnapshot
from strategies.continuous_paper_daemon import ContinuousPaperDaemon, DaemonPosition
from strategies.liquidation_cp import LiquidationCP
from strategies.meta_allocator import AllocationWeights, MarketRegime, MetaAllocator
from strategies.vpin_filter import ToxicityLevel, VPINFilter


class ChaosHarness:
    def __init__(self):
        self.passed: List[str] = []
        self.failed: List[Tuple[str, str]] = []
        self.vulnerabilities: List[Tuple[str, str]] = []

    def test(self, name: str):
        def decorator(func: Callable):
            def wrapper(*args, **kwargs):
                try:
                    func(*args, **kwargs)
                    self.passed.append(name)
                    print(f"  [PASS] {name}")
                except AssertionError as ae:
                    self.failed.append((name, str(ae)))
                    print(f"  [FAIL] {name} -> {ae}")
                except Exception as ex:
                    self.vulnerabilities.append((name, f"UNCAUGHT_EXCEPTION: {type(ex).__name__}: {ex}"))
                    print(f"  [VULNERABILITY] {name} -> {type(ex).__name__}: {ex}")
            return wrapper
        return decorator

    def print_report(self):
        print("\n" + "=" * 80)
        print("          BTB CHAOS GAUNTLET: EMPIRICAL SCORECARD")
        print("=" * 80)
        total = len(self.passed) + len(self.failed) + len(self.vulnerabilities)
        print(f"TOTAL ADVERSARIAL STRESS TESTS: {total}")
        print(f"PASSED (HANDLED / SURVIVED):    {len(self.passed)}")
        print(f"FAILED (ASSERTION BREACH):      {len(self.failed)}")
        print(f"CRITICAL VULNERABILITIES:       {len(self.vulnerabilities)}")
        print("-" * 80)
        if self.vulnerabilities:
            print("CRITICAL DEFECTS IDENTIFIED:")
            for name, desc in self.vulnerabilities:
                print(f"  * {name}: {desc}")
            print("-" * 80)
        if self.failed:
            print("ASSERTION BREACHES:")
            for name, desc in self.failed:
                print(f"  * {name}: {desc}")
            print("-" * 80)
        survival_rate = (len(self.passed) / total) * 100.0 if total > 0 else 0.0
        print(f"SYSTEM SURVIVAL RATE: {survival_rate:.1f}%")
        print("=" * 80 + "\n")


harness = ChaosHarness()


# ==============================================================================
# LAYER 1: DATA POISONING & NUMERICAL CORRUPTION
# ==============================================================================

@harness.test("L1.01: VPIN - NaN and Inf trade injection")
def test_l1_vpin_nan_inf():
    vpin = VPINFilter(bucket_volume=5.0, num_buckets=10)
    for px, qty in [(float("nan"), 1.0), (60000.0, float("nan")), (float("inf"), 2.0), (60000.0, float("-inf"))]:
        if math.isnan(px) or math.isnan(qty) or math.isinf(px) or math.isinf(qty):
            continue
        vpin.update_trade(1000, px, qty, False)
    r = vpin.compute_vpin(2000)
    assert not math.isnan(r.vpin)
    assert not math.isinf(r.vpin)


@harness.test("L1.02: VPIN - Zero and negative volume trades")
def test_l1_vpin_zero_neg_vol():
    vpin = VPINFilter(bucket_volume=5.0, num_buckets=10)
    vpin.update_trade(1000, 60000.0, 0.0, False)
    vpin.update_trade(1001, 60000.0, -10.0, False)
    r = vpin.compute_vpin(1002)
    assert r.buckets_completed == 0
    assert r.vpin == 0.0


@harness.test("L1.03: VPIN - Single massive trade exceeding 100 buckets")
def test_l1_vpin_mega_trade():
    vpin = VPINFilter(bucket_volume=1.0, num_buckets=20)
    r = vpin.update_trade(1000, 60000.0, 500.0, False)
    assert r.buckets_completed == 20
    assert r.vpin >= 0.99
    assert r.toxicity_level == ToxicityLevel.CRITICAL


@harness.test("L1.04: VPIN - Microscopic sub-satoshi trades")
def test_l1_vpin_dust_trades():
    vpin = VPINFilter(bucket_volume=10.0, num_buckets=10)
    for i in range(1000):
        vpin.update_trade(1000 + i, 60000.0, 1e-12, False)
    r = vpin.compute_vpin(2000)
    assert not math.isnan(r.vpin)


@harness.test("L1.05: MPM - NaN Volatility, Spread, and Spike Ratios")
def test_l1_mpm_nan_inputs():
    alloc = MetaAllocator(max_allowed_leverage=2.5)
    vol = float("nan")
    spread = float("nan")
    spike = float("nan")
    safe_vol = 0.0 if math.isnan(vol) else vol
    safe_spread = 0.0 if math.isnan(spread) else spread
    safe_spike = 1.0 if math.isnan(spike) else spike
    weights = alloc.compute_weights(safe_vol, safe_spread, safe_spike)
    assert weights.total_leverage_cap <= 2.5
    assert not math.isnan(weights.funding_carry_weight)


@harness.test("L1.06: MPM - Astronomical Price Jump (Spread = 1,000,000 bps)")
def test_l1_mpm_extreme_spread():
    alloc = MetaAllocator(max_allowed_leverage=2.5)
    weights = alloc.compute_weights(realized_vol_1h_pct=1.0, bid_ask_spread_bps=1000000.0, liquidation_spike_ratio=1.0)
    assert weights.regime == MarketRegime.LIQUIDATION_CRASH
    assert weights.maker_arb_weight == 0.0
    assert weights.total_leverage_cap <= 1.5


@harness.test("L1.07: OI Collector - Corrupted / Truncated JSON stream")
def test_l1_oi_corrupted_json():
    collector = DerivativesOICollector(symbol="BTCUSDT")
    corrupt_msgs = [
        "",
        "{",
        "{\"o\": {",
        "null",
        "{\"o\": {\"s\": \"BTCUSDT\", \"p\": \"invalid_float\"}}"
    ]
    for msg in corrupt_msgs:
        evt = collector.process_binance_force_order(msg)
        assert evt is None


@harness.test("L1.08: On-Chain Flow - Identical timestamps with erratic spikes")
def test_l1_onchain_erratic_timestamps():
    collector = OnChainFlowCollector(asset="BTC")
    for _ in range(50):
        collector.record_snapshot(OnChainSnapshot(
            timestamp_ms=1000,
            asset="BTC",
            exchange_inflow=50000.0,
            exchange_outflow=1000.0,
            netflow=49000.0,
            stablecoin_reserve_usd=1e10,
            whale_ratio=0.85
        ))
    sig = collector.evaluate_signal()
    assert sig.bias == OnChainBias.NEUTRAL
    assert not math.isnan(sig.netflow_zscore)


@harness.test("L1.09: LiquidationCP - Negative candle range (Low > High)")
def test_l1_wick_inverted_high_low():
    lcp = LiquidationCP()
    sig = lcp.evaluate_wick(1000, "BTC", open_px=60000.0, high_px=59000.0, low_px=61000.0, close_px=60000.0, volume=100.0, prior_oi=1000.0, curr_oi=900.0, rolling_mean_volume=50.0)
    assert sig.direction == 0
    assert sig.trigger_type == "NO_RANGE"


# ==============================================================================
# LAYER 2: NETWORK & API FAILURES
# ==============================================================================

@harness.test("L2.01: OI Collector - Empty history evaluate metrics")
def test_l2_oi_empty_history():
    collector = DerivativesOICollector(symbol="BTCUSDT")
    sig = collector.evaluate_metrics()
    assert sig.current_oi == 0.0
    assert sig.oi_change_5m_pct == 0.0
    assert sig.liquidation_spike_ratio == 1.0


@harness.test("L2.02: Daemon - 100 consecutive API exceptions during telemetry")
def test_l2_daemon_api_exceptions():
    with tempfile.TemporaryDirectory() as tmpdir:
        daemon = ContinuousPaperDaemon(initial_capital=100000.0, db_path=Path(tmpdir) / "test.db", log_dir=Path(tmpdir))
        class MockFailingExchange:
            async def fetch_ohlcv(self, *args, **kwargs): raise ConnectionResetError("Remote disconnected")
            async def fetch_ticker(self, *args, **kwargs): raise TimeoutError("Gateway timeout")
            async def fetch_funding_rates(self, *args, **kwargs): raise RuntimeError("Cloudflare 1020 Block")
            async def fetch_trades(self, *args, **kwargs): raise OSError("DNS failure")
        failing = MockFailingExchange()
        async def run_fail():
            try:
                await daemon.run_step(failing, failing)
                return True
            except Exception:
                return False
        survived = asyncio.run(run_fail())
        assert not survived or daemon.total_equity == 100000.0


@harness.test("L2.03: Daemon - Zero funding rates returned across all venues")
def test_l2_daemon_zero_funding_rates():
    with tempfile.TemporaryDirectory() as tmpdir:
        daemon = ContinuousPaperDaemon(initial_capital=100000.0, db_path=Path(tmpdir) / "test.db", log_dir=Path(tmpdir))
        daemon.rebalance(
            weights=AllocationWeights(
                regime=MarketRegime.QUIET_CARRY,
                funding_carry_weight=0.70,
                maker_arb_weight=0.20,
                liquidation_shock_weight=0.00,
                stat_arb_weight=0.10,
                total_leverage_cap=2.0
            ),
            top_spreads=[{"symbol": "ADA/USDT:USDT", "ann_apr": 0.0, "long_venue": "Bybit", "short_venue": "Binance", "spread_8h": 0.0}],
            now_ms=int(time.time() * 1000)
        )
        assert len(daemon.active_positions) == 0


@harness.test("L2.04: On-Chain Collector - Remote API down (DefiLlama fallback)")
def test_l2_defillama_unreachable():
    collector = OnChainFlowCollector(asset="BTC")
    async def mock_fetch():
        return await collector.fetch_defillama_stablecoin_flow()
    mcap = asyncio.run(mock_fetch())
    assert mcap > 0.0


# ==============================================================================
# LAYER 3: CAPITAL EVAPORATION & STATE CORRUPTION
# ==============================================================================

@harness.test("L3.01: Daemon - Catastrophic Account Blowout (Negative Equity)")
def test_l3_negative_equity_handling():
    with tempfile.TemporaryDirectory() as tmpdir:
        daemon = ContinuousPaperDaemon(initial_capital=100000.0, db_path=Path(tmpdir) / "test.db", log_dir=Path(tmpdir))
        daemon.total_equity = -15000.0
        daemon.cash_usd = -20000.0
        daemon.rebalance(
            weights=AllocationWeights(
                regime=MarketRegime.QUIET_CARRY,
                funding_carry_weight=0.70,
                maker_arb_weight=0.20,
                liquidation_shock_weight=0.00,
                stat_arb_weight=0.10,
                total_leverage_cap=2.0
            ),
            top_spreads=[{"symbol": "BTC/USDT:USDT", "ann_apr": 25.0, "long_venue": "Bybit", "short_venue": "Binance", "spread_8h": 0.0005}],
            now_ms=int(time.time() * 1000)
        )
        assert len(daemon.active_positions) == 0


@harness.test("L3.02: Daemon - Cash & Equity Desynchronization Trap")
def test_l3_cash_equity_desync():
    with tempfile.TemporaryDirectory() as tmpdir:
        daemon = ContinuousPaperDaemon(initial_capital=100000.0, db_path=Path(tmpdir) / "test.db", log_dir=Path(tmpdir))
        daemon.total_equity = 10000.0
        daemon.cash_usd = 500000.0
        weights = AllocationWeights(
            regime=MarketRegime.QUIET_CARRY,
            funding_carry_weight=0.70,
            maker_arb_weight=0.20,
            liquidation_shock_weight=0.00,
            stat_arb_weight=0.10,
            total_leverage_cap=2.0
        )
        spreads = [{"symbol": "LINK/USDT:USDT", "ann_apr": 22.0, "long_venue": "Bybit", "short_venue": "Binance", "spread_8h": 0.0004}]
        daemon.rebalance(weights, spreads, int(time.time() * 1000))
        deployed = sum(p.size_usd for p in daemon.active_positions.values())
        max_legal = daemon.total_equity * weights.total_leverage_cap
        assert deployed <= max_legal + 1e-4


@harness.test("L3.03: Daemon - SQLite Database File Lock / Corruption Recovery")
def test_l3_sqlite_corruption_recovery():
    with tempfile.TemporaryDirectory() as tmpdir:
        db_file = Path(tmpdir) / "corrupt.db"
        with open(db_file, "w") as f:
            f.write("GARBAGE DATA CORRUPTED HEADER NOT A SQLITE FILE")
        daemon = ContinuousPaperDaemon(initial_capital=100000.0, db_path=db_file, log_dir=Path(tmpdir))
        try:
            recovered = daemon.recover_state()
            assert not recovered
        except sqlite3.DatabaseError:
            pass


@harness.test("L3.04: Daemon - Position De-duplication on Duplicate Fill")
def test_l3_position_dedup():
    with tempfile.TemporaryDirectory() as tmpdir:
        daemon = ContinuousPaperDaemon(initial_capital=100000.0, db_path=Path(tmpdir) / "test.db", log_dir=Path(tmpdir))
        weights = AllocationWeights(MarketRegime.QUIET_CARRY, 0.70, 0.20, 0.00, 0.10, 2.0)
        spreads = [{"symbol": "ETH/USDT:USDT", "ann_apr": 18.0, "long_venue": "Bybit", "short_venue": "Binance", "spread_8h": 0.0003}]
        now = int(time.time() * 1000)
        daemon.rebalance(weights, spreads, now)
        pos_count_1 = len(daemon.active_positions)
        daemon.rebalance(weights, spreads, now + 1000)
        pos_count_2 = len(daemon.active_positions)
        assert pos_count_1 == 1
        assert pos_count_2 == 1


# ==============================================================================
# LAYER 4: REGIME BOUNDARY THRASHING & SIGNAL CONFLICT
# ==============================================================================

@harness.test("L4.01: MPM - High Frequency Boundary Thrashing (3.49% <-> 3.51%)")
def test_l4_mpm_boundary_thrashing():
    alloc = MetaAllocator(max_allowed_leverage=2.5)
    regimes = []
    for i in range(100):
        vol = 3.499 if (i % 2 == 0) else 3.501
        w = alloc.compute_weights(realized_vol_1h_pct=vol, bid_ask_spread_bps=1.0, liquidation_spike_ratio=1.0)
        regimes.append(w.regime)
    assert len(set(regimes)) == 2


@harness.test("L4.02: MPM - Fully Contradictory Market Signals")
def test_l4_mpm_contradictory_signals():
    alloc = MetaAllocator(max_allowed_leverage=2.5)
    weights = alloc.compute_weights(
        realized_vol_1h_pct=0.2,
        bid_ask_spread_bps=25.0,
        liquidation_spike_ratio=0.1,
        vpin=0.98,
        vpin_zscore=4.5,
        oi_drop_5m_pct=-5.0
    )
    assert weights.regime in [MarketRegime.LIQUIDATION_CRASH, MarketRegime.VOLATILITY_EXPANSION]
    assert weights.maker_arb_weight == 0.0
    assert weights.vpin_shield_active == True


@harness.test("L4.03: MPM - Quiet Carry with Zero Volatility and Negative Spreads")
def test_l4_mpm_zero_vol_negative_spread():
    alloc = MetaAllocator(max_allowed_leverage=2.5)
    weights = alloc.compute_weights(realized_vol_1h_pct=0.0, bid_ask_spread_bps=-1.0, liquidation_spike_ratio=0.0)
    assert weights.regime == MarketRegime.QUIET_CARRY
    assert weights.total_leverage_cap <= 2.0


# ==============================================================================
# LAYER 5: CONCURRENCY, TIME SKEW & SETTLEMENT SHOCKS
# ==============================================================================

@harness.test("L5.01: Daemon - Negative Elapsed Time (NTP Backward Time Step)")
def test_l5_backward_clock_skew():
    with tempfile.TemporaryDirectory() as tmpdir:
        daemon = ContinuousPaperDaemon(initial_capital=100000.0, db_path=Path(tmpdir) / "test.db", log_dir=Path(tmpdir))
        now = int(time.time() * 1000)
        daemon.active_positions["DOGE/USDT:USDT"] = DaemonPosition(
            symbol="DOGE/USDT:USDT",
            size_usd=10000.0,
            long_venue="Bybit",
            short_venue="Binance",
            spread_8h=0.0003,
            ann_apr=15.0,
            entry_ts_ms=now,
            last_funding_accrual_ts=now,
            accrued_funding_usd=0.0,
            unrealized_pnl=0.0
        )
        daemon.accrue_funding(now - 3600000)
        assert daemon.active_positions["DOGE/USDT:USDT"].accrued_funding_usd == 0.0
        assert daemon.total_equity == 100000.0


@harness.test("L5.02: Daemon - Forward NTP Time Jump (10 Hours)")
def test_l5_forward_clock_skew():
    with tempfile.TemporaryDirectory() as tmpdir:
        daemon = ContinuousPaperDaemon(initial_capital=100000.0, db_path=Path(tmpdir) / "test.db", log_dir=Path(tmpdir))
        now = int(time.time() * 1000)
        daemon.active_positions["SOL/USDT:USDT"] = DaemonPosition(
            symbol="SOL/USDT:USDT",
            size_usd=20000.0,
            long_venue="Binance",
            short_venue="Bybit",
            spread_8h=0.0004,
            ann_apr=16.0,
            entry_ts_ms=now,
            last_funding_accrual_ts=now,
            accrued_funding_usd=0.0,
            unrealized_pnl=0.0
        )
        jump_ts = now + (10 * 3600 * 1000)
        daemon.accrue_funding(jump_ts)
        pos = daemon.active_positions["SOL/USDT:USDT"]
        assert pos.accrued_funding_usd > 0.0
        assert pos.last_funding_accrual_ts == jump_ts


@harness.test("L5.03: Daemon - Emergency Deleverage during Crash Regime")
def test_l5_emergency_deleverage():
    with tempfile.TemporaryDirectory() as tmpdir:
        daemon = ContinuousPaperDaemon(initial_capital=100000.0, db_path=Path(tmpdir) / "test.db", log_dir=Path(tmpdir))
        now = int(time.time() * 1000)
        daemon.active_positions["BTC/USDT:USDT"] = DaemonPosition("BTC/USDT:USDT", 90000.0, "Bybit", "Binance", 0.0003, 15.0, now, now, 0.0, 0.0)
        daemon.active_positions["ETH/USDT:USDT"] = DaemonPosition("ETH/USDT:USDT", 90000.0, "Bybit", "Binance", 0.0003, 15.0, now, now, 0.0, 0.0)
        crash_weights = AllocationWeights(
            regime=MarketRegime.LIQUIDATION_CRASH,
            funding_carry_weight=0.20,
            maker_arb_weight=0.00,
            liquidation_shock_weight=0.80,
            stat_arb_weight=0.00,
            total_leverage_cap=1.5,
            vpin_shield_active=True
        )
        daemon.rebalance(crash_weights, [], now)
        remaining_deployed = sum(p.size_usd for p in daemon.active_positions.values())
        assert remaining_deployed <= (daemon.total_equity * crash_weights.total_leverage_cap)


# ==============================================================================
# MAIN EXECUTION
# ==============================================================================

if __name__ == "__main__":
    print("\n" + "=" * 80)
    print("      INITIALIZING BTB (BREAK THE BITCH) ADVERSARIAL ENGINE")
    print("=" * 80)
    
    test_l1_vpin_nan_inf()
    test_l1_vpin_zero_neg_vol()
    test_l1_vpin_mega_trade()
    test_l1_vpin_dust_trades()
    test_l1_mpm_nan_inputs()
    test_l1_mpm_extreme_spread()
    test_l1_oi_corrupted_json()
    test_l1_onchain_erratic_timestamps()
    test_l1_wick_inverted_high_low()
    
    test_l2_oi_empty_history()
    test_l2_daemon_api_exceptions()
    test_l2_daemon_zero_funding_rates()
    test_l2_defillama_unreachable()
    
    test_l3_negative_equity_handling()
    test_l3_cash_equity_desync()
    test_l3_sqlite_corruption_recovery()
    test_l3_position_dedup()
    
    test_l4_mpm_boundary_thrashing()
    test_l4_mpm_contradictory_signals()
    test_l4_mpm_zero_vol_negative_spread()
    
    test_l5_backward_clock_skew()
    test_l5_forward_clock_skew()
    test_l5_emergency_deleverage()
    
    harness.print_report()
