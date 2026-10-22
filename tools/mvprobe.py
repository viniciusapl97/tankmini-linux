#!/usr/bin/env python3
"""Minimal SysEx transport for M-VAVE / JieLi pedals over ALSA rawmidi.

Frame format (from M-VAVE family prior art, see docs/protocol/):

    raw frame : 00 59 <cmd> <len u24 LE> <body> <cs>      cs = (~sum(body)) & 0xff
    on the wire: f0 <encode7(frame)> f7

No manufacturer id: the `00 59` sync packs to `00 32`, so every message
starts with `f0 00 32`.

The transport is a plain ALSA rawmidi character device (/dev/snd/midiC<card>D<dev>),
which keeps the tool dependency-free (no mido/rtmidi needed).
"""

from __future__ import annotations

import argparse
import os
import select
import sys
import time
from dataclasses import dataclass

SYNC = b"\x00\x59"
DEFAULT_DEV = "/dev/snd/midiC2D0"


# --- 8 <-> 7 bit packing ----------------------------------------------------

def encode7(data: bytes) -> bytes:
    """Pack arbitrary bytes into a 7-bit stream (LSB-first bit order).

    n raw bytes -> n + n // 7 + 1 packed bytes; the final flush byte is always
    emitted, even when the bit count lands on a multiple of 7.
    """
    out = bytearray()
    accum = 0
    nbits = 0
    for b in data:
        accum |= b << nbits
        nbits += 8
        while nbits >= 7:
            out.append(accum & 0x7F)
            accum >>= 7
            nbits -= 7
    out.append(accum & 0x7F)
    return bytes(out)


def decode7(data: bytes, nbytes: int | None = None) -> bytes:
    """Inverse of encode7. Stops after `nbytes` if given, else at input end."""
    out = bytearray()
    accum = 0
    nbits = 0
    for b in data:
        if b & 0x80:
            raise ValueError(f"non-7-bit byte 0x{b:02x} in payload")
        accum |= b << nbits
        nbits += 7
        while nbits >= 8:
            out.append(accum & 0xFF)
            accum >>= 8
            nbits -= 8
            if nbytes is not None and len(out) >= nbytes:
                return bytes(out)
    return bytes(out)


def checksum(body: bytes) -> int:
    return (~sum(body)) & 0xFF


def build_frame(cmd: int, body: bytes = b"") -> bytes:
    return SYNC + bytes([cmd]) + len(body).to_bytes(3, "little") + body + bytes([checksum(body)])


def wrap_sysex(frame: bytes) -> bytes:
    return b"\xf0" + encode7(frame) + b"\xf7"


# --- frame parsing ----------------------------------------------------------

@dataclass
class Frame:
    cmd: int
    body: bytes
    cs_ok: bool

    def hex(self) -> str:
        return f"cmd={self.cmd:02x} body[{len(self.body)}]={self.body.hex(' ')} cs={'ok' if self.cs_ok else 'BAD'}"


def parse_frames(raw: bytes) -> list[Frame]:
    """Split a byte stream into complete frames (tolerates leading junk)."""
    frames: list[Frame] = []
    i = 0
    while True:
        j = raw.find(SYNC, i)
        if j < 0 or len(raw) < j + 7:
            break
        cmd = raw[j + 2]
        length = int.from_bytes(raw[j + 3:j + 6], "little")
        end = j + 7 + length
        if end > len(raw):
            break
        body = raw[j + 6:j + 6 + length]
        cs = raw[end - 1]
        frames.append(Frame(cmd, body, checksum(body) == cs))
        i = end
    return frames


# --- transport --------------------------------------------------------------

class RawMidi:
    def __init__(self, dev: str = DEFAULT_DEV):
        self.fd = os.open(dev, os.O_RDWR | os.O_NONBLOCK)

    def close(self) -> None:
        os.close(self.fd)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def write(self, data: bytes) -> None:
        os.write(self.fd, data)

    def read(self, timeout: float = 1.0, flush_first: bool = True) -> bytes:
        if flush_first:
            while select.select([self.fd], [], [], 0)[0]:
                try:
                    if not os.read(self.fd, 65536):
                        break
                except BlockingIOError:
                    break
        buf = bytearray()
        deadline = time.monotonic() + timeout
        while True:
            remain = deadline - time.monotonic()
            if remain <= 0:
                break
            r, _, _ = select.select([self.fd], [], [], remain)
            if not r:
                continue
            try:
                chunk = os.read(self.fd, 65536)
            except BlockingIOError:
                continue
            if not chunk:
                break
            buf += chunk
        return bytes(buf)


def send_frame(midi: RawMidi, frame: bytes, timeout: float = 1.0) -> bytes:
    midi.read(timeout=0.05)  # drop stale bytes
    midi.write(wrap_sysex(frame))
    raw = midi.read(timeout=timeout, flush_first=False)
    # unwrap sysex -> frames
    out = bytearray()
    for seg in raw.split(b"\xf0"):
        if b"\xf7" not in seg:
            continue
        payload = seg[: seg.index(b"\xf7")]
        try:
            out += decode7(payload)
        except ValueError:
            pass
    return bytes(out)


def query(cmd: int, body: bytes = b"", dev: str = DEFAULT_DEV, timeout: float = 1.0) -> list[Frame]:
    frame = build_frame(cmd, body)
    with RawMidi(dev) as midi:
        return parse_frames(send_frame(midi, frame, timeout))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dev", default=DEFAULT_DEV)
    ap.add_argument("--timeout", type=float, default=1.0)
    sub = ap.add_subparsers(dest="action", required=True)

    q = sub.add_parser("query", help="send a command with an empty body")
    q.add_argument("opcode", type=lambda s: int(s, 16))
    q.add_argument("--body", default="", help="hex body")
    q.add_argument("--raw", action="store_true", help="print raw frame bytes too")

    r = sub.add_parser("read", help="read a memory region (cmd 23)")
    r.add_argument("type", type=lambda s: int(s, 16))
    r.add_argument("addr", type=lambda s: int(s, 16))
    r.add_argument("length", type=int)

    li = sub.add_parser("listen", help="dump incoming bytes for N seconds")
    li.add_argument("seconds", type=float)

    d = sub.add_parser("dump", help="dump a memory region to a file in chunks")
    d.add_argument("type", type=lambda s: int(s, 16))
    d.add_argument("addr", type=lambda s: int(s, 16))
    d.add_argument("length", type=int)
    d.add_argument("-o", "--out", required=True)
    d.add_argument("--chunk", type=int, default=1000)
    d.add_argument("--gap", type=float, default=0.05)

    w = sub.add_parser("write", help="write to a memory region (cmd 22)")
    w.add_argument("type", type=lambda s: int(s, 16))
    w.add_argument("addr", type=lambda s: int(s, 16))
    w.add_argument("data", help="hex data")

    a = ap.parse_args()
    if a.action == "write":
        payload = bytes.fromhex(a.data)
        body = bytes([a.type]) + a.addr.to_bytes(4, "little") + len(payload).to_bytes(3, "little") + payload
        for f in query(0x22, body, a.dev, a.timeout):
            print(f.hex())
        return 0
    if a.action == "query":
        body = bytes.fromhex(a.body) if a.body else b""
        for f in query(a.opcode, body, a.dev, a.timeout):
            print(f.hex())
            if a.raw:
                print("   frame:", build_frame(f.cmd, f.body).hex(" "))
    elif a.action == "read":
        body = bytes([a.type]) + a.addr.to_bytes(4, "little") + a.length.to_bytes(3, "little")
        frames = query(0x23, body, a.dev, a.timeout)
        if not frames:
            print("(no reply)")
            return 1
        for f in frames:
            print(f.hex())
            if f.cmd == 0x23 and len(f.body) >= 8:
                data = f.body[8:]
                print(f"   data[{len(data)}]: {data.hex(' ')}")
                print(f"   ascii: {''.join(chr(c) if 32 <= c < 127 else '.' for c in data)}")
    elif a.action == "dump":
        import time as _t
        data = bytearray()
        off = 0
        with RawMidi(a.dev) as midi:
            while off < a.length:
                n = min(a.chunk, a.length - off)
                body = bytes([a.type]) + (a.addr + off).to_bytes(4, "little") + n.to_bytes(3, "little")
                frames = parse_frames(send_frame(midi, build_frame(0x23, body), a.timeout))
                got = None
                for f in frames:
                    if f.cmd == 0x23 and len(f.body) >= 8:
                        got = f.body[8:]
                    elif f.cmd == 0x00:
                        print(f"   off 0x{off:04x}: ACK status {f.body.hex(' ') if f.body else '?'}", file=sys.stderr)
                if got is None:
                    print(f"   off 0x{off:04x}: no reply, padding zeros", file=sys.stderr)
                    got = bytes(n)
                data += got[:n]
                off += n
                _t.sleep(a.gap)
        with open(a.out, "wb") as fh:
            fh.write(data)
        print(f"wrote {len(data)} bytes to {a.out}")
        print(f"non-zero bytes: {sum(1 for b in data if b)}")
    else:
        with RawMidi(a.dev) as m:
            data = m.read(timeout=a.seconds, flush_first=False)
        print(data.hex(" "))
    return 0


if __name__ == "__main__":
    sys.exit(main())