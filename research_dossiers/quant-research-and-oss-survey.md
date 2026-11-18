# STATE-OF-THE-ART QUANTITATIVE RESEARCH & OSS SURVEY (2025–2026)

**Focus:** Order Flow Imbalance (OFI), Perpetual Basis Dispersion, Hyperliquid & High-Frequency Crypto Microstructure  
**Entity:** PKBoost AI Labs &times; AEGIS Quant Research Division  

---

## 1. KEY RESEARCH PAPER DISCOVERIES (ARXIV & RECENT LITERATURE)

### A. Non-Linear & Hybrid Order Flow Imbalance (OFI)
- **Paper:** *"Hybrid Vector Auto Regression and Neural Network Model for Order Flow Imbalance Prediction in High Frequency Trading"* (Rahman & Upadhye)
  - **Finding:** Linear regression on OFI ($L_1 \dots L_5$) degrades during market regime shifts (liquidation cascades). A hybrid model combining VAR (capturing linear cross-level auto-correlation) with a shallow Feedforward Neural Network (FNN) modeling the residual error captures non-linear price impacts with **~58% out-of-sample tick directional accuracy on Binance USDT perpetuals**.
  - **Relevance to AERIS:** Standard linear OFI in `ofi_directional.py` will serve as our baseline; we can benchmark an FNN residual layer as our first ML enhancement.

### B. High-Frequency Calendar Bunching & Return Predictability
- **Paper:** *"The Quarter-Hour Effect: Periodic Algorithmic Trading and Return Predictability in Cryptocurrency Futures"* (Kim & Hansen)
  - **Finding:** Execution algorithms across crypto markets synchronize tightly around calendar boundaries (1m, 5m, 15m candle closes). This algorithmic "bunching" produces systematic drops in trade-size roundness and creates predictable 500ms to 2-second liquidity dry-ups.
  - **Alpha Angle:** Avoid executing passive limit orders right at the 00-second mark (high adverse selection). Instead, quote aggressively 2–3 seconds *after* calendar bursts to capture mean-reverting spread widening.

### C. Feature Stability Across Altcoin Perpetuals
- **Paper:** *"Explainable Patterns in Cryptocurrency Microstructure"* (Bieganowski & Ślepaczuk)
  - **Finding:** Evaluated thousands of microstructural features across multi-year tick databases on Binance perpetuals using gradient boosting. 
  - **Crucial Takeaway:** SHAP value importance showed that **Signed Taker Volume Ratio** and **Top-5 Depth Ratio** have high cross-asset portability. An alpha model trained on BTC/ETH features generalizes with minimal decay to SOL, AVAX, and high-beta alts.

---

## 2. PRODUCTION OPEN-SOURCE FRAMEWORKS & REPOSITORIES

The institutional landscape has consolidated around specific architectures:

| Project | Core Language | Strategy Layer | Venues Supported | Key Strengths |
|---|---|---|---|---|
| **NautilusTrader** (`nautechsystems/nautilus_trader`) | **Rust** (Core matching, Tokio IO) | **Python** (High-level event handler) | Binance, Bybit, OKX, Hyperliquid | Production-grade, zero-alloc queue simulation, native Hyperliquid adapter. |
| **HFTBacktest** (`nkaz001/hftbacktest`) | **Python** (Numba JIT) | **Python** | Binance, Bybit (L2 tick data) | Accurate limit-order queue position modeling, cancellation latency, and adverse selection simulation. |
| **Barter-rs** (`barter-rs/barter-rs`) | **Rust** | **Rust** | Binance, Coinbase, Kraken | Pure Rust modular live trading framework. Strong typed WebSocket normalizers. |
| **Hyperliquid Official SDKs** (`hyperliquid-dex/hyperliquid-rust-sdk`) | **Rust & Python** | Both | Hyperliquid L1 (HyperCore) | Direct EIP-712 transaction signing, sub-500ms L1 block updates, nonces, and `l2Book` streaming. |

---

## 3. IMPLICATIONS FOR SPRINT 1 PLAYGROUND

1. **Queue Simulation:** We will incorporate HFTBacktest's queue position decay formula into `sim_engine.py` to ensure our passive limit order fill rates are not overly optimistic.
2. **Hyperliquid Connector:** Utilizing Hyperliquid's official WebSocket protocol (`l2Book` + `trades`) provides us with raw, low-latency access directly into on-chain CLOB dynamics alongside Binance/Bybit.
3. **Dual Metric Target:** Any candidate strategy must demonstrate **both** positive out-of-sample expectancy ($> 1.5$ bps net per trade after fees) and a Sharpe ratio $> 2.0$ over rolling 7-day walk-forward intervals.
