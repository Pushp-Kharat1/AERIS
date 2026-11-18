# AERIS ALPHA PLAYGROUND — SPRINT 1 PROPOSAL

**Document:** Sprint 1 Research & Alpha Discovery Proposal  
**Target Directory:** `D:\Pushp - Do not open without permission\AERIS\Python Playground\playground\`  
**Governing Standard:** Empirical Truth-First, Zero Fluff, Strict Friction Modeling  
**Status:** PROPOSAL — Awaiting Formal Approval Before Code Execution  

---

## 1. OBJECTIVE & MANDATE

Before allocating capital or building low-level execution engines in Rust, the playground exists to answer one question with mathematical precision:

> **Does the proposed strategy generate statistically significant, positive net P&L after accounting for all real-world market frictions (taker/maker fees, realistic fill queues, adverse selection, slippage, and Indian 1% Section 194S TDS)?**

If the empirical return after frictions is zero or negative, the result is documented as a failure and the hypothesis is discarded immediately. No flattering metrics, no hidden drawdowns, no survivorship bias.

---

## 2. GOVERNING RULES & CODE STANDARDS

1. **Zero Unnecessary Comments:** Code will be strictly self-documenting through precise type hints, explicit naming, and mathematical formulas. Comments are restricted exclusively to non-obvious mathematical assertions or exchange protocol quirks.
2. **Workspace Cleanliness:** Zero dead code, zero temporary dump files, zero duplicated scripts. Everything has a singular purpose and a defined lifecycle.
3. **Absolute Truth in Metrics:**
   - No lookahead bias (strict point-in-time timestamping).
   - Zero-latency simulation is prohibited; execution models will enforce deterministic latency penalties.
   - P&L models must account for:
     - Venue maker fee (0.02%) vs. taker fee (0.05%–0.075%).
     - Adverse selection penalty on passive limit fills.
     - Section 194S 1% TDS on every gross transfer/trade where applicable.
   - Any failure or loss is recorded exactly as measured.

---

## 3. SCOPE OF DIRECTORY TREE & ARTIFACTS

The proposed structure to be initialized inside `Python Playground\playground\`:

```
D:\Pushp - Do not open without permission\AERIS\Python Playground\
└── playground/
    ├── requirements.txt            # Polars, NumPy, SciPy, WebSockets, Httpx, PyArrow, Matplotlib, Ipykernel
    │
    ├── notebooks/                  # Interactive exploratory notebooks
    │   ├── 01_orderbook_ofi_research.ipynb    # Multi-level Order Flow Imbalance & tick direction
    │   ├── 02_cross_venue_funding_arb.ipynb   # Delta-neutral basis & funding dispersion
    │   └── 03_lead_lag_latency.ipynb          # Leader-follower latency & quote updates
    │
    ├── collectors/                 # Async WebSocket streaming collectors (Parquet output)
    │   ├── binance_ws.py           # Depth-20 snapshots + trade flow
    │   ├── bybit_ws.py             # Bybit v5 orderbook.50 + public trade stream
    │   └── hyperliquid_ws.py       # Hyperliquid L1 book & liquidation feed
    │
    ├── backtest/                   # Vectorized backtesting & realistic accounting
    │   ├── sim_engine.py           # Queue position, adverse selection, fee & TDS model
    │   └── metrics.py              # Sharpe, Sortino, Calmar, Max Drawdown, Net P&L
    │
    └── strategies/                 # Pure strategy formulations
        ├── ofi_directional.py      # Microstructure queue pressure signal
        ├── funding_dispersion.py   # Cash-and-carry / cross-venue perpetual funding engine
        └── lead_lag_arb.py         # Cross-venue tick anticipation engine
```

---

## 4. THE 3 ALPHA TRACKS: SPECIFICATION & METRICS

### Track 1: Multi-Level Order Flow Imbalance (OFI)
- **Hypothesis:** Net volume changes across top $K$ order book levels ($L_1 \dots L_k$) contain predictive power over mid-price returns $\Delta P_{t+\tau}$ over horizon $\tau \in [100\text{ms}, 5\text{s}]$.
- **Math:**
  $$OFI_t^{(k)} = \Delta B_t^{(k)} - \Delta A_t^{(k)}$$
- **Target:** Out-of-sample directional accuracy $> 54\%$ after taker fee deduction.

### Track 2: Cross-Venue Funding Rate & Basis Dispersion (Delta-Neutral)
- **Hypothesis:** Funding rate differentials between Binance, Bybit, and Hyperliquid create persistent carry spreads without directional market exposure.
- **Math:**
  $$\text{Net Yield} = \sum (F_{\text{long venue}} - F_{\text{short venue}}) - \text{Rebalance Frictions} - \text{TDS}$$
- **Target:** Annualized Net Sharpe $> 2.5$, Max Drawdown $< 5\%$.

### Track 3: Lead-Lag Cross-Exchange Latency Arbitrage
- **Hypothesis:** Binance perpetual volume shocks propagate to Bybit and spot books with measurable millisecond delays.
- **Math:** Detection of aggressive market orders exceeding book depth percentile $P_{95}$ triggering immediate limit orders on lagging book.
- **Target:** Positive expectancy under simulated $25\text{ms}$ network transit latency.

---

## 5. TESTING & VERIFICATION PLAN

1. **Step 1: Environment Provisioning**
   - Pin Python 3.12 dependencies into `requirements.txt`.
   - Verify isolated environment without global package pollution.
2. **Step 2: Microstructure Data Ingestion (Live Capture)**
   - Capture continuous raw tick and depth data in compressed Parquet format across Binance, Bybit, and Hyperliquid.
3. **Step 3: Empirical Validation & Reporting**
   - Execute vectorized backtests using identical fee matrices and Indian tax deductions.
   - Output unambiguous metrics tables (Gross P&L, Net P&L, Sharpe, Max Drawdown, Win Rate, Trade Count).
4. **Step 4: Milestone Sign-off**
   - Present final findings report with complete truth. If zero alpha is found, document why and pivot. If edge is verified, prepare mathematical specification for the Rust hot-path.

---

*This proposal is presented for your review. No files will be created until this proposal is formally approved.*
