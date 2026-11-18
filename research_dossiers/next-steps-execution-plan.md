# AERIS NEXT STEPS: THE 3-TRACK EXECUTION PLAN

**Entity:** PKBoost AI Labs &times; AEGIS Quant  
**State:** Alpha Verified &bull; Chaos Tests Passed &bull; Parent Meta-Model Active  
**Goal:** Transition from research sandbox to an automated paper-trading system  

---

## 1. THE 3 CONCRETE TRACKS TO HIT NEXT

```mermaid
flowchart TD
    T1["TRACK 1: TELEMETRY & ALERT DAEMON\n• Connect real-time Binance + Bybit feeds to Parent Model\n• Automated 15-minute regime updates\n• Discord / Telegram / Console live alerts"]
    
    T2["TRACK 2: CONTINUOUS PAPER-TRADING SIMULATOR\n• Simulate live portfolio P&L over 72 hours\n• Parent Model dynamically shifting real paper capital\n• Track Sharpe, max drawdown & net carry after TDS"]
    
    T3["TRACK 3: THE RUST HOT-PATH SCAFFOLD\n• Port the Non-Bypassable Risk Gateway to Rust\n• Implement the atomic 50ms two-legged router\n• Zero-allocation order book ladder"]

    T1 --> T2 --> T3

    style T1 fill:#0f172a,stroke:#38bdf8,color:#fff
    style T2 fill:#1e1b4b,stroke:#818cf8,color:#fff
    style T3 fill:#0d2818,stroke:#50fa7b,color:#fff
```

---

## 2. DETAIL ON EACH TRACK

### Track 1: Live Telemetry Daemon (`telemetry_daemon.py`)
- **What it does:** Runs continuously in the background. Every 5 minutes, it pulls:
  - Realized 1h volatility on BTC & ETH.
  - Bid-ask spread widths across venues.
  - Live 8-hour funding rates for top 15 assets.
- **The Parent Model Output:** Evaluates current market state (`QUIET_CARRY`, `NORMAL_VOLATILITY`, `LIQUIDATION_CRASH`) and prints the optimal capital weights $\vec{w}$ with the current top 3 carry trades.

### Track 2: The 72-Hour Paper Portfolio Simulator (`paper_runner.py`)
- **The True Reality Check:** Let the entire multi-strategy system run live on real market data for 3 to 7 days in "paper" mode:
  - Starts with simulated $\$100,000$ virtual balance.
  - Parent Model dynamically shifts capital into the top carry assets (e.g. ADA Bybit vs. Binance).
  - Automatically simulates 8-hour funding payouts and rebalancing transactions.
  - Deducts exchange fees and 1% TDS on every simulated transfer.
  - Proves whether our net Sharpe ratio stays $> 2.0$ over multiple market days.

### Track 3: The Rust Hot-Path Core (`aeris-risk` & `aeris-execution`)
- While the paper portfolio runs in Python, we begin scaffolding the production Rust engine in `crates/aeris-risk`:
  - Enforce the **$\le 2.5\text{x}$ leverage limit** we derived from the August 5 crash test.
  - Implement the **50ms emergency IOC cancel timer** for orphan legs.
  - Build the multi-threaded WebSocket order router.

---

## 3. MY RECOMMENDATION: WHAT WE SHOULD DO TONIGHT

I recommend we build **Track 1 & Track 2 right now in `playground/`**:
1. Wire up `playground/strategies/telemetry_daemon.py` to stream live market stats into our `MetaAllocator`.
2. Spin up a live **Paper Portfolio Tracker** so we can watch our virtual \$100k balance collect real 8-hour funding yields starting tonight.
