import time
from pathlib import Path
import numpy as np
import polars as pl
from sklearn.linear_model import Ridge
import lightgbm as lgb
from strategies.ofi_directional import OrderFlowImbalanceStrategy
from backtest.sim_engine import SimulationEngine, ExecutionFriction, Side
from backtest.metrics import compute_metrics, PerformanceMetrics

def extract_features(depth_df: pl.DataFrame, k_levels: int = 5) -> pl.DataFrame:
    strategy = OrderFlowImbalanceStrategy(depth_levels=k_levels)
    rows = depth_df.to_dicts()
    
    features = []
    prev_row = None
    
    for row in rows:
        if prev_row is None:
            prev_row = row
            continue
            
        prev_bids = np.array([[b["price"], b["qty"]] for b in prev_row["bids"][:k_levels]])
        curr_bids = np.array([[b["price"], b["qty"]] for b in row["bids"][:k_levels]])
        prev_asks = np.array([[a["price"], a["qty"]] for a in prev_row["asks"][:k_levels]])
        curr_asks = np.array([[a["price"], a["qty"]] for a in row["asks"][:k_levels]])
        
        ofi = strategy.compute_multilevel_ofi(prev_bids, curr_bids, prev_asks, curr_asks)
        
        b1_px, b1_sz = curr_bids[0, 0], curr_bids[0, 1]
        a1_px, a1_sz = curr_asks[0, 0], curr_asks[0, 1]
        mid_price = (b1_px + a1_px) / 2.0
        spread_bps = ((a1_px - b1_px) / mid_price) * 10000.0
        imbalance_l1 = (b1_sz - a1_sz) / (b1_sz + a1_sz + 1e-9)
        
        features.append({
            "timestamp_ms": row["timestamp_ms"],
            "local_ts_ns": row["local_ts_ns"],
            "mid_price": mid_price,
            "spread_bps": spread_bps,
            "imbalance_l1": imbalance_l1,
            "multilevel_ofi": ofi
        })
        prev_row = row
        
    feat_df = pl.DataFrame(features)
    
    # 5-tick forward price change in basis points
    feat_df = feat_df.with_columns([
        ((pl.col("mid_price").shift(-5) - pl.col("mid_price")) / pl.col("mid_price") * 10000.0).alias("target_return_5_bps")
    ]).drop_nulls()
    
    return feat_df

def run_pipeline(data_file: Path) -> dict[str, PerformanceMetrics]:
    depth_df = pl.read_parquet(data_file)
    dataset = extract_features(depth_df)
    
    split_idx = int(len(dataset) * 0.7)
    train = dataset[:split_idx]
    test = dataset[split_idx:]
    
    feature_cols = ["spread_bps", "imbalance_l1", "multilevel_ofi"]
    X_train = train.select(feature_cols).to_numpy()
    y_train = train.select("target_return_5_bps").to_numpy().ravel()
    
    X_test = test.select(feature_cols).to_numpy()
    y_test = test.select("target_return_5_bps").to_numpy().ravel()
    test_rows = test.to_dicts()
    
    # 1. Baseline: Ridge
    ridge = Ridge(alpha=100.0)
    ridge.fit(X_train, y_train)
    preds_ridge = ridge.predict(X_test)
    
    # 2. Primary: LightGBM
    lgbm = lgb.LGBMRegressor(
        n_estimators=100,
        learning_rate=0.05,
        max_depth=4,
        num_leaves=15,
        verbosity=-1,
        random_state=42
    )
    lgbm.fit(X_train, y_train)
    preds_lgbm = lgbm.predict(X_test)
    
    friction = ExecutionFriction(maker_fee_bps=2.0, taker_fee_bps=5.0, slippage_bps=1.0, tds_rate=0.01)
    engine = SimulationEngine(friction=friction)
    
    results = {}
    for name, preds in [("Ridge_Baseline", preds_ridge), ("LightGBM_Primary", preds_lgbm)]:
        gross_pnls, fees, tds, returns = [], [], [], []
        
        for i in range(len(preds) - 5):
            pred_bps = preds[i]
            # Hurdle: prediction must clear 2x taker fee + slippage + TDS
            hurdle_bps = 2.0 * friction.taker_fee_bps + friction.slippage_bps + (friction.tds_rate * 10000.0)
            
            if abs(pred_bps) > hurdle_bps:
                side = Side.BUY if pred_bps > 0 else Side.SELL
                entry_px = test_rows[i]["mid_price"]
                exit_px = test_rows[i + 5]["mid_price"]
                trade = engine.execute_roundtrip(
                    entry_ts=test_rows[i]["local_ts_ns"],
                    exit_ts=test_rows[i + 5]["local_ts_ns"],
                    side=side,
                    size=1.0,
                    entry_price=entry_px,
                    exit_price=exit_px,
                    entry_is_maker=False,
                    exit_is_maker=False
                )
                gross_pnls.append(trade.gross_pnl)
                fees.append(trade.fees)
                tds.append(trade.tds)
                trade_ret = (trade.net_pnl / (1.0 * entry_px))
                returns.append(trade_ret)
                
        metrics = compute_metrics(
            returns=np.array(returns),
            gross_pnl_series=np.array(gross_pnls),
            fees_series=np.array(fees),
            tds_series=np.array(tds)
        )
        results[name] = metrics
        
    return results

