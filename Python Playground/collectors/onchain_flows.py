from collections import deque
from dataclasses import dataclass
from enum import Enum
import os
import time
from typing import Dict, List, Optional
import httpx
import numpy as np


class OnChainBias(Enum):
    NEUTRAL = 0
    SUPPLY_SQUEEZE_BULLISH = 1
    DISTRIBUTION_BEARISH = 2


@dataclass
class OnChainSnapshot:
    timestamp_ms: int
    asset: str
    exchange_inflow: float
    exchange_outflow: float
    netflow: float
    stablecoin_reserve_usd: float
    whale_ratio: float


@dataclass
class OnChainSignal:
    timestamp_ms: int
    bias: OnChainBias
    netflow_zscore: float
    stablecoin_flow_zscore: float
    whale_ratio_zscore: float
    confidence: float


class OnChainFlowCollector:
    def __init__(
        self,
        asset: str = "BTC",
        cryptoquant_key: Optional[str] = None,
        glassnode_key: Optional[str] = None
    ):
        self.asset = asset.upper()
        self.cryptoquant_key = cryptoquant_key or os.getenv("CRYPTOQUANT_API_KEY")
        self.glassnode_key = glassnode_key or os.getenv("GLASSNODE_API_KEY")
        self.history = deque(maxlen=720)

    async def fetch_defillama_stablecoin_flow(self) -> float:
        url = "https://stablecoins.llama.fi/stablecoins?includePrices=false"
        try:
            async with httpx.AsyncClient(timeout=6.0) as client:
                res = await client.get(url)
                if res.status_code == 200:
                    data = res.json().get("peggedAssets", [])
                    total_mcap = sum(
                        float(a.get("circulating", {}).get("peggedUSD", 0.0))
                        for a in data[:5]
                    )
                    return total_mcap
        except Exception:
            pass
        return 160_000_000_000.0

    def record_snapshot(self, snapshot: OnChainSnapshot):
        self.history.append(snapshot)

    def evaluate_signal(self, current_ts_ms: Optional[int] = None) -> OnChainSignal:
        if current_ts_ms is None:
            current_ts_ms = int(time.time() * 1000)

        if len(self.history) < 5:
            return OnChainSignal(
                timestamp_ms=current_ts_ms,
                bias=OnChainBias.NEUTRAL,
                netflow_zscore=0.0,
                stablecoin_flow_zscore=0.0,
                whale_ratio_zscore=0.0,
                confidence=0.0
            )

        netflows = np.array([s.netflow for s in self.history])
        whale_ratios = np.array([s.whale_ratio for s in self.history])
        stables = np.array([s.stablecoin_reserve_usd for s in self.history])

        latest = self.history[-1]

        nf_mean = np.mean(netflows)
        nf_std = np.std(netflows) + 1e-6
        nf_z = float((latest.netflow - nf_mean) / nf_std)

        wr_mean = np.mean(whale_ratios)
        wr_std = np.std(whale_ratios) + 1e-6
        wr_z = float((latest.whale_ratio - wr_mean) / wr_std)

        st_diff = np.diff(stables) if len(stables) > 1 else np.array([0.0])
        st_mean = np.mean(st_diff)
        st_std = np.std(st_diff) + 1e-6
        st_z = float((st_diff[-1] - st_mean) / st_std) if len(st_diff) > 0 else 0.0

        if nf_z <= -1.5 and st_z >= 0.5:
            bias = OnChainBias.SUPPLY_SQUEEZE_BULLISH
            confidence = min(1.0, (abs(nf_z) + st_z) / 4.0)
        elif nf_z >= 1.5 and wr_z >= 1.5:
            bias = OnChainBias.DISTRIBUTION_BEARISH
            confidence = min(1.0, (nf_z + wr_z) / 4.0)
        else:
            bias = OnChainBias.NEUTRAL
            confidence = 0.0

        return OnChainSignal(
            timestamp_ms=current_ts_ms,
            bias=bias,
            netflow_zscore=nf_z,
            stablecoin_flow_zscore=st_z,
            whale_ratio_zscore=wr_z,
            confidence=confidence
        )
