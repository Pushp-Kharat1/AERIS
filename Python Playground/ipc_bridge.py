from dataclasses import dataclass
import mmap
import os
from pathlib import Path
import struct
import time
from typing import List, Optional, Tuple

MAGIC = 0x41455249  # 'AERI'
SCALE = 100_000_000
PAYLOAD_SIZE = 160
LAYOUT_SIZE = 336


@dataclass
class MacroPayloadPy:
    updated_at_ns: int
    macro_bias: int
    leverage_cap: float
    universe: List[str]


class SharedMemoryBridgePy:
    def __init__(self, shm_path: Optional[Path] = None):
        if shm_path is None:
            shm_path = Path(__file__).resolve().parent / "data" / "aeris_shm.dat"
        self.shm_path = shm_path
        self.shm_path.parent.mkdir(parents=True, exist_ok=True)
        self._init_mmap()

    def _init_mmap(self):
        if not self.shm_path.exists() or self.shm_path.stat().st_size < 4096:
            with open(self.shm_path, "wb") as f:
                f.write(b"\x00" * 4096)

        self.file_obj = open(self.shm_path, "r+b")
        self.mm = mmap.mmap(self.file_obj.fileno(), 4096, access=mmap.ACCESS_WRITE)

        magic, active_idx, seq = struct.unpack_from("<IIQ", self.mm, 0)
        if magic != MAGIC:
            struct.pack_into("<IIQ", self.mm, 0, MAGIC, 0, 0)
            self.mm.flush()

    def stage_and_flip(self, macro_bias: int, leverage_cap: float, universe: List[str]) -> int:
        now_ns = int(time.time() * 1_000_000_000)
        _, active_idx, seq = struct.unpack_from("<IIQ", self.mm, 0)
        next_idx = 1 - (active_idx & 1)

        offset = 16 + (next_idx * PAYLOAD_SIZE)

        u_bytes = bytearray(128)
        u_len = min(len(universe), 8)
        for i in range(u_len):
            sym_raw = universe[i].encode("utf-8")[:16]
            u_bytes[i * 16 : i * 16 + len(sym_raw)] = sym_raw

        leverage_raw = int(round(leverage_cap * SCALE))

        struct.pack_into(
            "<QB7xqB128s7x",
            self.mm,
            offset,
            now_ns,
            macro_bias,
            leverage_raw,
            u_len,
            bytes(u_bytes),
        )

        next_seq = seq + 1
        struct.pack_into("<IQ", self.mm, 4, next_idx, next_seq)
        self.mm.flush()
        return next_seq

    def read_active(self) -> Tuple[int, MacroPayloadPy]:
        _, active_idx, seq = struct.unpack_from("<IIQ", self.mm, 0)
        offset = 16 + ((active_idx & 1) * PAYLOAD_SIZE)

        now_ns, bias, lev_raw, u_len, u_bytes = struct.unpack_from(
            "<QB7xqB128s7x", self.mm, offset
        )

        universe = []
        for i in range(u_len):
            sym_slice = u_bytes[i * 16 : (i + 1) * 16]
            sym_str = sym_slice.split(b"\x00")[0].decode("utf-8", errors="ignore")
            universe.append(sym_str)

        payload = MacroPayloadPy(
            updated_at_ns=now_ns,
            macro_bias=bias,
            leverage_cap=lev_raw / SCALE,
            universe=universe,
        )
        return seq, payload

    def close(self):
        self.mm.close()
        self.file_obj.close()
