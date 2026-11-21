import asyncio
import json
import time
from pathlib import Path
import pyarrow as pa
import pyarrow.parquet as pq
import websockets

class HyperliquidDataCollector:
    def __init__(self, coin: str, output_dir: Path):
        self.coin = coin.upper()
        self.output_dir = output_dir
        self.output_dir.mkdir(parents=True, exist_ok=True)
        self.running = False
        self.book_buffer = []
        self.trade_buffer = []
        self.flush_threshold = 1000

        self.book_schema = pa.schema([
            ("timestamp_ms", pa.int64()),
            ("local_ts_ns", pa.int64()),
            ("coin", pa.string()),
            ("bids", pa.list_(pa.struct([("price", pa.float64()), ("qty", pa.float64())]))),
            ("asks", pa.list_(pa.struct([("price", pa.float64()), ("qty", pa.float64())])))
        ])

        self.trade_schema = pa.schema([
            ("timestamp_ms", pa.int64()),
            ("local_ts_ns", pa.int64()),
            ("coin", pa.string()),
            ("side", pa.string()),
            ("price", pa.float64()),
            ("qty", pa.float64()),
            ("hash", pa.string())
        ])

    def flush_books(self):
        if not self.book_buffer:
            return
        table = pa.Table.from_pylist(self.book_buffer, schema=self.book_schema)
        filename = self.output_dir / f"hl_{self.coin.lower()}_l2_{int(time.time()*1000)}.parquet"
        pq.write_table(table, filename, compression="zstd")
        self.book_buffer.clear()

    def flush_trades(self):
        if not self.trade_buffer:
            return
        table = pa.Table.from_pylist(self.trade_buffer, schema=self.trade_schema)
        filename = self.output_dir / f"hl_{self.coin.lower()}_trades_{int(time.time()*1000)}.parquet"
        pq.write_table(table, filename, compression="zstd")
        self.trade_buffer.clear()

    async def run(self):
        self.running = True
        url = "wss://api.hyperliquid.xyz/ws"
        sub_l2 = {"method": "subscribe", "subscription": {"type": "l2Book", "coin": self.coin}}
        sub_trades = {"method": "subscribe", "subscription": {"type": "trades", "coin": self.coin}}

        while self.running:
            try:
                async with websockets.connect(url, ping_interval=20, ping_timeout=10) as ws:
                    await ws.send(json.dumps(sub_l2))
                    await ws.send(json.dumps(sub_trades))
                    async for message in ws:
                        local_ts = time.time_ns()
                        payload = json.loads(message)
                        channel = payload.get("channel", "")
                        data = payload.get("data", {})

                        if channel == "l2Book":
                            levels = data.get("levels", [[], []])
                            bids = [{"price": float(item.get("px", 0.0)), "qty": float(item.get("sz", 0.0))} for item in levels[0]]
                            asks = [{"price": float(item.get("px", 0.0)), "qty": float(item.get("sz", 0.0))} for item in levels[1]]
                            self.book_buffer.append({
                                "timestamp_ms": data.get("time", 0),
                                "local_ts_ns": local_ts,
                                "coin": data.get("coin", self.coin),
                                "bids": bids,
                                "asks": asks
                            })
                            if len(self.book_buffer) >= self.flush_threshold:
                                self.flush_books()

                        elif channel == "trades":
                            for item in (data if isinstance(data, list) else [data]):
                                self.trade_buffer.append({
                                    "timestamp_ms": item.get("time", 0),
                                    "local_ts_ns": local_ts,
                                    "coin": item.get("coin", self.coin),
                                    "side": item.get("side", ""),
                                    "price": float(item.get("px", 0.0)),
                                    "qty": float(item.get("sz", 0.0)),
                                    "hash": str(item.get("hash", ""))
                                })
                            if len(self.trade_buffer) >= self.flush_threshold:
                                self.flush_trades()

            except Exception:
                await asyncio.sleep(2)
            finally:
                self.flush_books()
                self.flush_trades()

if __name__ == "__main__":
    collector = HyperliquidDataCollector("BTC", Path("./data/hyperliquid"))
    try:
        asyncio.run(collector.run())
    except KeyboardInterrupt:
        collector.flush_books()
        collector.flush_trades()

