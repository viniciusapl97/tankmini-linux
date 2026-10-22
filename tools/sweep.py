#!/usr/bin/env python3
"""Read-only address sweep: dump 32 bytes at a list of (type, address) pairs.

Prints hex + printable ASCII so tables of names are obvious.
"""

from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from mvprobe import DEFAULT_DEV, RawMidi, build_frame, parse_frames, send_frame  # noqa: E402

ADDRESSES = [
    0x0, 0x40, 0x80, 0x100, 0x200, 0x220, 0x400, 0x800, 0x1000, 0x2000,
    0x4000, 0x8000, 0x10000, 0x20000, 0x40000, 0x80000, 0x100000,
    0x200000, 0x400000, 0x800000, 0x1000000, 0x2000000, 0x4000000,
    0x8000000, 0x10000000, 0x20000000, 0x40000000, 0x80000000,
    0xA0000000, 0xC0000000, 0xE0000000, 0xF0000000,
]


def read(midi: RawMidi, rtype: int, addr: int, length: int) -> bytes | None:
    body = bytes([rtype]) + addr.to_bytes(4, "little") + length.to_bytes(3, "little")
    for f in parse_frames(send_frame(midi, build_frame(0x23, body), 0.5)):
        if f.cmd == 0x23 and len(f.body) >= 8 + length:
            return f.body[8:8 + length]
        if f.cmd == 0x00:
            return None
    return None


def main() -> int:
    dev = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_DEV
    with RawMidi(dev) as midi:
        for rtype in (4, 5):
            print(f"===== type {rtype}")
            for a in ADDRESSES:
                d = read(midi, rtype, a, 32)
                if d is None:
                    print(f"0x{a:08x}  -- error ACK")
                    continue
                nz = sum(1 for b in d if b)
                asc = "".join(chr(b) if 32 <= b < 127 else "." for b in d)
                print(f"0x{a:08x} nz={nz:2d}  {d.hex(' ')}  |{asc}|")
    return 0


if __name__ == "__main__":
    sys.exit(main())
