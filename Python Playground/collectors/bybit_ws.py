import asyncio
import json
import time
from pathlib import Path
import pyarrow as pa
import pyarrow.parquet as pq
import websockets

class BybitDataCollector:
    def __init__(self, symbol: str, output_dir: Path):
        self.symbol = symbol.upper()
        self.output_dir = output_dir
        self.output_dir.mkdir(parents=True, exist_ok=True)
        self.running = False
        self.depth_buffer = []
        self.trade_buffer = []
        self.flush_threshold = 1000

        self.depth_schema = pa.schema([
            ("timestamp_ms", pa.int64()),
            ("local_ts_ns", pa.int64()),
            ("sequence", pa.int64()),
            ("bids", pa.list_(pa.struct([("price", pa.float64()), ("qty", pa.float64())]))),
            ("asks", pa.list_(pa.struct([("price", pa.float64()), ("qty", pa.float64())])))
        ])

        self.trade_schema = pa.schema([
            ("timestamp_ms", pa.int64()),
            ("local_ts_ns", pa.int64()),
            ("trade_id", pa.string()),
            ("price", pa.float64()),
            ("qty", pa.float64()),
            ("side", pa.string())
        ])

    def flush_depth(self):
        if not self.depth_buffer:
            return
        table = pa.Table.from_pylist(self.depth_buffer, schema=self.depth_schema)
        filename = self.output_dir / f"bybit_{self.symbol.lower()}_depth_{int(time.time()*1000)}.parquet"
        pq.write_table(table, filename, compression="zstd")
        self.depth_buffer.clear()

    def flush_trades(self):
        if not self.trade_buffer:
            return
        table = pa.Table.from_pylist(self.trade_buffer, schema=self.trade_schema)
        filename = self.output_dir / f"bybit_{self.symbol.lower()}_trades_{int(time.time()*1000)}.parquet"
        pq.write_table(table, filename, compression="zstd")
        self.trade_buffer.clear()

    async def run(self):
        self.running = True
        url = "wss://stream.bybit.com/v5/public/linear"
        sub_msg = {
            "op": "subscribe",
            "args": [
                f"orderbook.50.{self.symbol}",
                f"publicTrade.{self.symbol}"
            ]
        }

        while self.running:
            try:
                async with websockets.connect(url, ping_interval=20, ping_timeout=10) as ws:
                    await ws.send(json.dumps(sub_msg))
                    async for message in ws:
                        local_ts = time.time_ns()
                        payload = json.loads(message)
                        topic = payload.get("topic", "")
                        data = payload.get("data", {})

                        if "orderbook" in topic:
                            bids = [{"price": float(item[0]), "qty": float(item[1])} for item in data.get("b", [])]
                            asks = [{"price": float(item[0]), "qty": float(item[1])} for item in data.get("a", [])]
                            self.depth_buffer.append({
                                "timestamp_ms": payload.get("ts", 0),
                                "local_ts_ns": local_ts,
                                "sequence": data.get("seq", 0),
                                "bids": bids,
                                "asks": asks
                            })
                            if len(self.depth_buffer) >= self.flush_threshold:
                                self.flush_depth()

                        elif "publicTrade" in topic:
                            for trade in (data if isinstance(data, list) else [data]):
                                self.trade_buffer.append({
                                    "timestamp_ms": trade.get("T", 0),
                                    "local_ts_ns": local_ts,
                                    "trade_id": str(trade.get("i", "")),
                                    "price": float(trade.get("p", 0.0)),
                                    "qty": float(trade.get("v", 0.0)),
                                    "side": trade.get("S", "")
                                })
                            if len(self.trade_buffer) >= self.flush_threshold:
                                self.flush_trades()

            except Exception:
                await asyncio.sleep(2)
            finally:
                self.flush_depth()
                self.flush_trades()

if __name__ == "__main__":
    collector = BybitDataCollector("BTCUSDT", Path("./data/bybit"))
    try:
        asyncio.run(collector.run())
    except KeyboardInterrupt:
        collector.flush_depth()
        collector.flush_trades()

