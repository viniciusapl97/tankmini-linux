#!/usr/bin/env python3
"""Measure what the pedal does to a test signal, to check that a written preset
actually reaches the audio path.

Plays a deterministic noise burst into the pedal's USB audio sink and records the
pedal's USB audio source at the same time (PipeWire), then reports level and
band energy. Pure stdlib: wav module + array, no numpy.

    python3 tools/audio_probe.py measure 1
    python3 tools/audio_probe.py probe          # baseline, mangle, restore

The pedal exposes itself as `alsa_output.usb-Jieli_...` (playback) and
`alsa_input.usb-Jieli_...` (capture).
"""

from __future__ import annotations

import array
import math
import random
import subprocess
import sys
import tempfile
import wave
from pathlib import Path

SINK_MATCH = "usb-Jieli"
RATE = 44100
SECONDS = 2.0


def find_targets() -> tuple[str, str]:
    out = subprocess.run(["pactl", "list", "short", "sinks"], capture_output=True, text=True).stdout
    sink = next((line.split("\t")[1] for line in out.splitlines() if SINK_MATCH in line), None)
    inp = subprocess.run(["pactl", "list", "short", "sources"], capture_output=True, text=True).stdout
    source = next(
        (
            line.split("\t")[1]
            for line in inp.splitlines()
            if SINK_MATCH in line and "monitor" not in line
        ),
        None,
    )
    if not sink or not source:
        raise SystemExit("pedal audio device not found in PipeWire (is it plugged in?)")
    return sink, source


def write_noise(path: Path, seconds: float = SECONDS) -> None:
    rng = random.Random(1234)
    n = int(RATE * seconds)
    samples = array.array("h")
    for i in range(n):
        # Pink-ish noise: white noise with a slow random walk to keep spectrum busy.
        v = rng.gauss(0, 0.12)
        samples.append(max(-32767, min(32767, int(v * 32767))))
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(samples.tobytes())


def play_and_record(sink: str, source: str, out: Path) -> None:
    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp:
        stim = Path(tmp.name)
    write_noise(stim)
    rec = subprocess.Popen(
        [
            "pw-record",
            "--target",
            source,
            "--format",
            "s16",
            "--rate",
            str(RATE),
            "--channels",
            "2",
            str(out),
        ]
    )
    play = subprocess.run(["pw-cat", "--playback", "--target", sink, str(stim)],
                          capture_output=True, text=True)
    rec.wait(timeout=SECONDS + 5)
    if play.returncode != 0:
        print("playback error:", play.stderr.strip()[:300], file=sys.stderr)


def analyse(path: Path) -> dict[str, float]:
    with wave.open(str(path), "rb") as w:
        ch = w.getnchannels()
        width = w.getsampwidth()
        frames = w.readframes(w.getnframes())
    assert width == 2, f"unexpected sample width {width}"
    data = array.array("h")
    data.frombytes(frames)
    mono = data[::ch]
    n = len(mono) or 1
    rms = math.sqrt(sum(s * s for s in mono) / n) / 32768.0
    peak = max((abs(s) for s in mono), default=0) / 32768.0
    # Crude band energies via zero-crossing rate as a brightness proxy.
    zc = sum(1 for i in range(1, n) if (mono[i - 1] < 0) != (mono[i] < 0)) / (n / RATE)
    return {"rms": rms, "peak": peak, "zcr_hz": zc, "frames": float(n)}


def main() -> int:
    sink, source = find_targets()
    print(f"sink  : {sink}\nsource: {source}")
    tmpdir = Path(tempfile.mkdtemp(prefix="tankmini-audio-"))
    out = tmpdir / "rec.wav"
    play_and_record(sink, source, out)
    print("recording:", out)
    stats = analyse(out)
    print({k: round(v, 4) for k, v in stats.items()})
    return 0


if __name__ == "__main__":
    sys.exit(main())
