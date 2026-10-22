#!/usr/bin/env python3
"""Backup / inspect the TANK MINI preset slots over USB-MIDI.

For every slot i: write the select command (type 4, 0xE0000000 + i, data [i]),
read the live preset block (type 4, 0x0000, 1942 bytes), store it, then restore
the original slot. Read-only with respect to flash.

Usage:
    python3 tools/slots.py backup captures/presets [--slots 0-20]
"""

from __future__ import annotations

import argparse
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from mvprobe import DEFAULT_DEV, RawMidi, build_frame, checksum, parse_frames, send_frame  # noqa: E402

PRESET_LEN = 1942
TYPE_DEV = 4


def active_slot(midi: RawMidi) -> int:
    body = bytes([TYPE_DEV]) + (0x20000000).to_bytes(4, "little") + (1).to_bytes(3, "little")
    frames = parse_frames(send_frame(midi, build_frame(0x23, body), 0.6))
    for f in frames:
        if f.cmd == 0x23 and len(f.body) >= 9:
            return f.body[8]
    raise RuntimeError("no active-slot reply")


def select_slot(midi: RawMidi, slot: int) -> bool:
    body = bytes([TYPE_DEV]) + (0xE0000000 + slot).to_bytes(4, "little") + (1).to_bytes(3, "little") + bytes([slot])
    frames = parse_frames(send_frame(midi, build_frame(0x22, body), 1.0))
    return any(f.cmd == 0x00 and f.body[:1] == b"\x00" for f in frames)


def read_preset(midi: RawMidi) -> bytes:
    body = bytes([TYPE_DEV]) + (0).to_bytes(4, "little") + PRESET_LEN.to_bytes(3, "little")
    frames = parse_frames(send_frame(midi, build_frame(0x23, body), 1.5))
    for f in frames:
        if f.cmd == 0x23 and len(f.body) >= 8 + PRESET_LEN:
            return f.body[8:8 + PRESET_LEN]
    raise RuntimeError("short or missing preset reply")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("outdir")
    ap.add_argument("--slots", default="0-20")
    ap.add_argument("--gap", type=float, default=0.15)
    ap.add_argument("--dev", default=DEFAULT_DEV)
    a = ap.parse_args()

    lo, _, hi = a.slots.partition("-")
    slots = range(int(lo), int(hi or lo) + 1)
    os.makedirs(a.outdir, exist_ok=True)

    with RawMidi(a.dev) as midi:
        start = active_slot(midi)
        print(f"active slot at start: {start}")
        for s in slots:
            ok = select_slot(midi, s)
            time.sleep(a.gap)
            data = read_preset(midi)
            path = os.path.join(a.outdir, f"slot_{s:02d}.bin")
            with open(path, "wb") as fh:
                fh.write(data)
            nz = sum(1 for b in data if b)
            marker = "" if data[1932:1932 + 8] == b"PATCHEND" else " !! no PATCHEND"
            print(f"slot {s:02d}: ack={ok} bytes={len(data)} nonzero={nz}{marker} -> {path}")
        select_slot(midi, start)
        time.sleep(a.gap)
        print(f"restored slot {start}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
