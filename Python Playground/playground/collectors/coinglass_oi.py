import asyncio
from collections import deque
from dataclasses import dataclass
from datetime import datetime, timezone
import json
import os
import time
from typing import Callable, Optional
import httpx
import numpy as np
import websockets


@dataclass
class OISnapshot:
    timestamp_ms: int
    symbol: str
    open_interest: float
    open_interest_usd: float
    price: float


@dataclass
class LiquidationEvent:
    timestamp_ms: int
    symbol: str
    side: str
    price: float
    qty: float
    usd_value: float


@dataclass
class DerivativesSignal:
    timestamp_ms: int
    symbol: str
    current_oi: float
    oi_change_5m_pct: float
    oi_change_1h_pct: float
    recent_liquidation_usd: float
    liquidation_spike_ratio: float
    is_long_flush: bool
    is_short_squeeze: bool


class DerivativesOICollector:
    def __init__(
        self,
        symbol: str = "BTCUSDT",
        history_window_sec: int = 7200,
        coinglass_api_key: Optional[str] = None
    ):
        self.symbol = symbol.upper()
        self.history_window_sec = history_window_sec
        self.coinglass_api_key = coinglass_api_key or os.getenv("COINGLASS_API_KEY")
        
        self.oi_history = deque(maxlen=2880)
        self.liquidations = deque(maxlen=5000)
        self.running = False

    async def fetch_binance_oi(self) -> Optional[OISnapshot]:
        url = f"https://fapi.binance.com/fapi/v1/openInterest?symbol={self.symbol}"
        px_url = f"https://fapi.binance.com/fapi/v1/ticker/price?symbol={self.symbol}"
        async with httpx.AsyncClient(timeout=5.0) as client:
            try:
                res_oi, res_px = await asyncio.gather(client.get(url), client.get(px_url))
                if res_oi.status_code == 200 and res_px.status_code == 200:
                    data_oi = res_oi.json()
                    data_px = res_px.json()
                    oi = float(data_oi["openInterest"])
                    px = float(data_px["price"])
                    ts = int(data_oi.get("time", time.time() * 1000))
                    snapshot = OISnapshot(
                        timestamp_ms=ts,
                        symbol=self.symbol,
                        open_interest=oi,
                        open_interest_usd=oi * px,
                        price=px
                    )
                    self.oi_history.append(snapshot)
                    return snapshot
            except Exception:
                pass
        return None

    def record_liquidation(self, event: LiquidationEvent):
        self.liquidations.append(event)

    def process_binance_force_order(self, msg_str: str) -> Optional[LiquidationEvent]:
        try:
            data = json.loads(msg_str)
            o = data.get("o", {})
            sym = o.get("s", "")
            if sym != self.symbol and self.symbol != "ALL":
                return None
            side = o.get("S", "")
            px = float(o.get("p", 0.0))
            qty = float(o.get("q", 0.0))
            ts = int(o.get("T", time.time() * 1000))
            event = LiquidationEvent(
                timestamp_ms=ts,
                symbol=sym,
                side=side,
                price=px,
                qty=qty,
                usd_value=px * qty
            )
            self.record_liquidation(event)
            return event
        except Exception:
            return None

    def evaluate_metrics(self, current_ts_ms: Optional[int] = None) -> DerivativesSignal:
        if current_ts_ms is None:
            current_ts_ms = int(time.time() * 1000)

        if not self.oi_history:
            return DerivativesSignal(
                timestamp_ms=current_ts_ms,
                symbol=self.symbol,
                current_oi=0.0,
                oi_change_5m_pct=0.0,
                oi_change_1h_pct=0.0,
                recent_liquidation_usd=0.0,
                liquidation_spike_ratio=1.0,
                is_long_flush=False,
                is_short_squeeze=False
            )

        curr = self.oi_history[-1]
        ts_5m_ago = current_ts_ms - (5 * 60 * 1000)
        ts_1h_ago = current_ts_ms - (60 * 60 * 1000)

        oi_5m_ago = None
        oi_1h_ago = None

        for snap in reversed(self.oi_history):
            if oi_5m_ago is None and snap.timestamp_ms <= ts_5m_ago:
                oi_5m_ago = snap.open_interest
            if oi_1h_ago is None and snap.timestamp_ms <= ts_1h_ago:
                oi_1h_ago = snap.open_interest
                break

        if oi_5m_ago is None and len(self.oi_history) > 1:
            oi_5m_ago = self.oi_history[0].open_interest
        if oi_1h_ago is None and len(self.oi_history) > 1:
            oi_1h_ago = self.oi_history[0].open_interest

        delta_5m = ((curr.open_interest - oi_5m_ago) / oi_5m_ago * 100.0) if (oi_5m_ago and oi_5m_ago > 0) else 0.0
        delta_1h = ((curr.open_interest - oi_1h_ago) / oi_1h_ago * 100.0) if (oi_1h_ago and oi_1h_ago > 0) else 0.0

        ts_15m_ago = current_ts_ms - (15 * 60 * 1000)
        recent_liqs = [liq for liq in self.liquidations if liq.timestamp_ms >= ts_15m_ago]
        recent_usd = sum(liq.usd_value for liq in recent_liqs)

        older_liqs = [liq for liq in self.liquidations if ts_1h_ago <= liq.timestamp_ms < ts_15m_ago]
        baseline_rate_15m = (sum(liq.usd_value for liq in older_liqs) / 3.0) if older_liqs else 10000.0
        spike_ratio = recent_usd / max(baseline_rate_15m, 1000.0)

        long_liqs_usd = sum(liq.usd_value for liq in recent_liqs if liq.side == "SELL")
        short_liqs_usd = sum(liq.usd_value for liq in recent_liqs if liq.side == "BUY")

        is_long_flush = delta_5m <= -2.0 and long_liqs_usd > short_liqs_usd * 2.0 and spike_ratio >= 2.0
        is_short_squeeze = delta_5m <= -2.0 and short_liqs_usd > long_liqs_usd * 2.0 and spike_ratio >= 2.0

        return DerivativesSignal(
            timestamp_ms=current_ts_ms,
            symbol=self.symbol,
            current_oi=curr.open_interest,
            oi_change_5m_pct=float(delta_5m),
            oi_change_1h_pct=float(delta_1h),
            recent_liquidation_usd=float(recent_usd),
            liquidation_spike_ratio=float(spike_ratio),
            is_long_flush=bool(is_long_flush),
            is_short_squeeze=bool(is_short_squeeze)
        )

    async def stream_liquidations(self, on_event: Optional[Callable[[LiquidationEvent], None]] = None):
        stream_name = f"{self.symbol.lower()}@forceOrder" if self.symbol != "ALL" else "!forceOrder@arr"
        uri = f"wss://fstream.binance.com/ws/{stream_name}"
        self.running = True
        while self.running:
            try:
                async with websockets.connect(uri, ping_interval=20, ping_timeout=20) as ws:
                    while self.running:
                        msg = await ws.recv()
                        evt = self.process_binance_force_order(msg)
                        if evt and on_event:
                            on_event(evt)
            except asyncio.CancelledError:
                break
            except Exception:
                await asyncio.sleep(2.0)

    def stop(self):
        self.running = False
