#!/usr/bin/env python3
"""Write a preset block back to the pedal (type 4, addr 0x0000) and verify it.

Writes in small chunks (default 173 bytes, the chunk size the official family
apps use), waiting for the ACK of each chunk, then reads the block back and
compares it with the input file.

    python3 tools/preset_write.py captures/presets/slot_00.bin --verify-only
    python3 tools/preset_write.py captures/presets/slot_00.bin
"""

from __future__ import annotations

import argparse
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from mvprobe import DEFAULT_DEV, RawMidi, build_frame, parse_frames, send_frame  # noqa: E402

TYPE_DEV = 4
BASE = 0x0


def read_block(midi: RawMidi, length: int) -> bytes:
    body = bytes([TYPE_DEV]) + BASE.to_bytes(4, "little") + length.to_bytes(3, "little")
    for f in parse_frames(send_frame(midi, build_frame(0x23, body), 1.5)):
        if f.cmd == 0x23 and len(f.body) >= 8 + length:
            return f.body[8:8 + length]
    raise RuntimeError("no preset reply")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("file")
    ap.add_argument("--chunk", type=int, default=173)
    ap.add_argument("--verify-only", action="store_true")
    ap.add_argument("--dev", default=DEFAULT_DEV)
    a = ap.parse_args()

    data = open(a.file, "rb").read()
    with RawMidi(a.dev) as midi:
        before = read_block(midi, len(data))
        print(f"current block: {len(before)} bytes, equal to file: {before == data}")
        if a.verify_only:
            return 0
        ok = 0
        for off in range(0, len(data), a.chunk):
            part = data[off:off + a.chunk]
            body = bytes([TYPE_DEV]) + (BASE + off).to_bytes(4, "little") + len(part).to_bytes(3, "little") + part
            frames = parse_frames(send_frame(midi, build_frame(0x22, body), 1.0))
            acked = [f for f in frames if f.cmd == 0x00]
            status = acked[0].body[:1] if acked else b"?"
            print(f"  chunk @0x{off:04x} len={len(part):3d} status={status.hex() or 'none'}")
            ok += 1 if status == b"\x00" else 0
        after = read_block(midi, len(data))
        print(f"chunks acked: {ok}/{(len(data) + a.chunk - 1) // a.chunk}")
        print(f"read-back matches file: {after == data}")
        if after != data:
            diffs = [i for i in range(len(data)) if after[i] != data[i]]
            print(f"differing bytes: {len(diffs)} first at 0x{diffs[0]:04x}" if diffs else "no diffs")
    return 0


if __name__ == "__main__":
    sys.exit(main())
