import asyncio
import json
import time
from pathlib import Path
import pyarrow as pa
import pyarrow.parquet as pq
import websockets

class BinanceDataCollector:
    def __init__(self, symbol: str, output_dir: Path):
        self.symbol = symbol.lower()
        self.output_dir = output_dir
        self.output_dir.mkdir(parents=True, exist_ok=True)
        self.running = False
        self.depth_buffer = []
        self.trade_buffer = []
        self.flush_threshold = 1000

        self.depth_schema = pa.schema([
            ("timestamp_ms", pa.int64()),
            ("local_ts_ns", pa.int64()),
            ("first_update_id", pa.int64()),
            ("final_update_id", pa.int64()),
            ("bids", pa.list_(pa.struct([("price", pa.float64()), ("qty", pa.float64())]))),
            ("asks", pa.list_(pa.struct([("price", pa.float64()), ("qty", pa.float64())])))
        ])

        self.trade_schema = pa.schema([
            ("timestamp_ms", pa.int64()),
            ("local_ts_ns", pa.int64()),
            ("trade_id", pa.int64()),
            ("price", pa.float64()),
            ("qty", pa.float64()),
            ("is_buyer_maker", pa.bool_())
        ])

    def flush_depth(self):
        if not self.depth_buffer:
            return
        table = pa.Table.from_pylist(self.depth_buffer, schema=self.depth_schema)
        filename = self.output_dir / f"binance_{self.symbol}_depth_{int(time.time()*1000)}.parquet"
        pq.write_table(table, filename, compression="zstd")
        self.depth_buffer.clear()

    def flush_trades(self):
        if not self.trade_buffer:
            return
        table = pa.Table.from_pylist(self.trade_buffer, schema=self.trade_schema)
        filename = self.output_dir / f"binance_{self.symbol}_trades_{int(time.time()*1000)}.parquet"
        pq.write_table(table, filename, compression="zstd")
        self.trade_buffer.clear()

    async def run(self):
        self.running = True
        url = f"wss://stream.binance.com:9443/stream?streams={self.symbol}@depth20@100ms/{self.symbol}@trade"
        
        while self.running:
            try:
                async with websockets.connect(url, ping_interval=20, ping_timeout=10) as ws:
                    async for message in ws:
                        local_ts = time.time_ns()
                        payload = json.loads(message)
                        stream = payload.get("stream", "")
                        data = payload.get("data", {})

                        if "depth" in stream:
                            self.depth_buffer.append({
                                "timestamp_ms": data.get("E", 0),
                                "local_ts_ns": local_ts,
                                "first_update_id": data.get("U", 0),
                                "final_update_id": data.get("u", 0),
                                "bids": [{"price": float(p), "qty": float(q)} for p, q in data.get("bids", [])],
                                "asks": [{"price": float(p), "qty": float(q)} for p, q in data.get("asks", [])]
                            })
                            if len(self.depth_buffer) >= self.flush_threshold:
                                self.flush_depth()

                        elif "trade" in stream:
                            self.trade_buffer.append({
                                "timestamp_ms": data.get("E", 0),
                                "local_ts_ns": local_ts,
                                "trade_id": data.get("t", 0),
                                "price": float(data.get("p", 0.0)),
                                "qty": float(data.get("q", 0.0)),
                                "is_buyer_maker": bool(data.get("m", False))
                            })
                            if len(self.trade_buffer) >= self.flush_threshold:
                                self.flush_trades()

            except Exception:
                await asyncio.sleep(2)
            finally:
                self.flush_depth()
                self.flush_trades()

if __name__ == "__main__":
    collector = BinanceDataCollector("btcusdt", Path("./data/binance"))
    try:
        asyncio.run(collector.run())
    except KeyboardInterrupt:
        collector.flush_depth()
        collector.flush_trades()

