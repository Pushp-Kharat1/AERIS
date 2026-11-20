# AERIS

A multi-strategy quantitative trading platform and parent meta-allocation engine for liquid cryptocurrency derivatives.

The project follows a two-tier architecture:
1. **Python Research Playground (`Python Playground/`):** Strategy prototyping, real exchange WebSocket telemetry collection, adversarial chaos testing, and portfolio regime allocation.
2. **Rust Production Core (`crates/` - planned):** Sub-microsecond pre-trade risk gateway, deterministic order routing, and exchange WebSocket/REST execution hot paths.

---

## Design Principles

- **Absolute Empirical Truth:** No paper returns without full real-world frictions. Every backtest and simulation strictly accounts for VIP taker/maker tiers, market impact slippage, and statutory Indian Section 194S 1% crypto TDS on gross transfers.
- **Alpha First, Rust Second:** We don't write low-latency C++ or Rust until a strategy's statistical edge has survived empirical fee hurdles and adversarial market shocks in Python.
- **Deterministic Risk Clamps:** Machine learning and RL models propose candidate allocations, but deterministic risk rules dispose them. Hard gross leverage ceilings ($\le 2.5\times$), toxic-flow maker order cancellation, and mandatory cash reserves cannot be overridden by statistical models.

---

## Strategy Sleeves

### 1. Delta-Neutral Funding Carry (`strategies/funding_dispersion.py`)
Exploits structural funding rate divergence across exchanges (e.g. Bybit Long vs. Binance Short). Holds delta-neutral spot/perp positions on high-dispersion pairs (ADA, LINK, DOGE) to harvest annualized carry while immunizing the portfolio against market direction.

### 2. Liquidation Counter-Punch (`strategies/liquidation_cp.py`)
Rather than chasing breakout spikes (which face high adverse selection and cascade risk), this sleeve monitors real-time Open Interest collapses ($> 2.5\%$) and orderbook absorption wicks. Enters mean-reversion bounces with tight $-35\text{ bps}$ stop-losses and $+140\text{ bps}$ targets ($4:1$ payoff ratio).

### 3. Order Flow Imbalance (OFI) & Lead-Lag (`strategies/ofi_directional.py`, `strategies/lead_lag_arb.py`)
Tracks multi-level book updates (L1–L5) and cross-exchange price propagation to measure short-horizon queue dynamics and lead-lag latency between major venues.

### 4. Breakout Momentum (`strategies/aggressive_tm.py`)
Volume-spike breakout detection across rolling volatility windows with volatility-adjusted profit targets.

---

## Master Parent Model (MPM)

Managing multiple strategies simultaneously requires continuous risk-based capital allocation. The repository implements two allocators:

1. **Heuristic Rule-Based MPM (`strategies/meta_allocator.py`):**
   Classifies market telemetry into four discrete regimes (`QUIET_CARRY`, `NORMAL_VOLATILITY`, `VOLATILITY_EXPANSION`, `LIQUIDATION_CRASH`). During volatility expansions, maker strategies are instantaneously pulled to zero to prevent adverse selection, while dry powder is routed into cash reserves and counter-punch limiters.

2. **Gymnasium RL Meta-Environment & PPO Agent (`strategies/rl_meta_env.py`, `strategies/train_rl_mpm.py`):**
   A Farama `gymnasium` environment wrapping 10-dimensional market telemetry. Uses a PyTorch Actor-Critic network trained via PPO with differential Sortino rewards and quadratic drawdown penalties to learn smooth portfolio transitions across regime shifts.

---

## Repository Structure

```
AERIS/
├── Python Playground/
│   └── playground/
│       ├── collectors/         # Live WebSocket feeds (Binance, Bybit, Hyperliquid)
│       ├── backtest/
│       │   ├── sim_engine.py   # Fee, slippage, and Section 194S TDS friction model
│       │   ├── metrics.py      # Sharpe, Sortino, Calmar, Drawdown analytics
│       │   └── chaos_engine.py # Adversarial stress tests (orphan legs, poison ticks)
│       ├── strategies/
│       │   ├── funding_dispersion.py
│       │   ├── liquidation_cp.py
│       │   ├── aggressive_tm.py
│       │   ├── ofi_directional.py
│       │   ├── lead_lag_arb.py
│       │   ├── meta_allocator.py
│       │   ├── rl_meta_env.py
│       │   ├── train_rl_mpm.py
│       │   └── live_paper_simulator.py
│       ├── notebooks/          # Exploratory research notebooks
│       └── requirements.txt    # Pinned Python dependencies
├── .hgignore                   # Mercurial ignore definitions
├── .gitignore                  # Git mirror ignore definitions
└── README.md
```

---

## Getting Started

### 1. Prerequisites
- Python 3.12+
- Mercurial (`hg`) with `hg-git` extension enabled for GitHub sync

### 2. Environment Setup
```bash
cd "Python Playground/playground"
python -m venv .venv

# On Windows:
.venv\Scripts\activate
# On Linux/macOS:
source .venv/bin/activate

pip install -r requirements.txt
```

### 3. Running Live Paper Simulation
Runs paper trading on live telemetry feeds from Binance and Bybit:
```bash
python strategies/live_paper_simulator.py --duration-mins 15
```

### 4. Training the RL Meta-Allocator & Running Benchmarks
Trains the PPO agent on multi-regime market scenarios and benchmarks performance against pure carry and the heuristic MPM:
```bash
python strategies/train_rl_mpm.py
```

### 5. Running the Chaos Stress Engine
Replays historical black swan data (e.g. August 5, 2024 ETH -27.6% drop) with synthetic poison ticks and asymmetric leg dropouts:
```bash
python backtest/chaos_engine.py
```

---

## License

Proprietary research and software. All rights reserved.
