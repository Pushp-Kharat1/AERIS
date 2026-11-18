# ADVERSARIAL BLACK SWAN REPLAY: AUGUST 5, 2024 ETH CRASH

**Dataset:** Real 1-minute historical tick candles from Binance (`ETH/USDT`)  
**Time Window:** August 4, 18:00 UTC to August 5, 12:00 UTC (1,000 minutes)  
**Event:** Global Yen carry trade unwind & market-wide crypto liquidation cascade  
**Price Shock:** Dropped from **$\$2,915.35$** to a low of **$\$2,111.00$** (**$-27.59\%$ in hours**)  

---

## 1. EMPIRICAL LEVERAGE STRESS-TEST RESULTS

We ran our delta-neutral basis strategy through this exact crash under different leverage settings without assuming any manual intervention:

```
[Crash: ETH -27.59% in 1,000 minutes]
Leverage 2.0x -> Liquidated: NO  | Min Equity Remaining: $22,409.83 (Buffer: +$18,800 above maint margin)
Leverage 3.0x -> Liquidated: NO  | Min Equity Remaining:  $8,614.75 (Dangerously close: only $3,600 buffer)
Leverage 4.0x -> Liquidated: YES | Liquidated at $2,288.88 (-21.5% drop)
Leverage 5.0x -> Liquidated: YES | Liquidated at $2,453.01 (-15.8% drop)
```

---

## 2. THE MATHEMATICAL TRUTH ON LEVERAGE

> [!IMPORTANT]
> **The Hard Invariant for AERIS Risk Engine:**
> - If you run **$\ge 4.0\text{x}$ leverage** on a basis trade, a standard crypto market crash ($-22\%$) will **completely liquidate your long leg**, leaving your short leg naked and losing your capital.
> - At **$2.0\text{x}$ leverage**, the strategy survived the violent August 5 crash with **zero liquidations** and maintained a **$\$22,409$ equity buffer**.
> - **Net Market P&L at $2.0\text{x}$:** Exactly **$\$0.00$ delta drift** (Long loss of $-\$27,589$ was 100% perfectly offset by Short gain of $+\$27,589$).

---

## 3. THREE REQUIRED RISK ENGINE SAFEGUARDS

1. **Hard Leverage Cap:** The pre-trade risk gateway must permanently cap gross leverage at **$\le 2.5\text{x}$** for basis arbitrage. Anyone telling you to run 5x or 10x leverage on basis trades is guaranteeing liquidation in the next crash.
2. **Automated Cross-Venue Margin Rebalance:** When distance to liquidation on either leg drops below $35\%$, the system must automatically initiate a transfer from the winning wallet to the losing wallet.
3. **Emergency Circuit Breaker:** If an underlying asset moves $>15\%$ in less than 30 minutes, all new position entries are frozen until volatility stabilizes.
