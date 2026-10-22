# TANK MINI — USB-MIDI protocol

Device: M-VAVE TANK MINI, firmware `TANK MINI_004`, USB `4c4a:c755` (JieLi),
USB audio + USB-MIDI composite. All facts below were observed on the physical
unit through `/dev/snd/midiC2D0` unless marked otherwise.

Markers: **verified** (observed on this unit), **from app** (read out of the
official M-EFCS binaries), **inferred**.

## 1. Transport

ALSA rawmidi, one port: `hw:2,0,0` / `/dev/snd/midiC2D0`.

Messages are SysEx-wrapped raw frames, no manufacturer ID:

```
f0 <encode7(frame)> f7
```

`encode7` is the same LSB-first 7-bit packing used by the rest of the M-VAVE
family (see `../_tools/ref/irbox/docs/usb.md`): stream bits `7k..7k+6` go into
output byte `k`, and the final flush byte is always emitted, so `n` raw bytes
become `n + n//7 + 1` packed bytes. Because the sync word `00 59` packs to
`00 32`, every message starts with `f0 00 32`. **Verified** for queries, reads
and ACKs on this unit.

## 2. Frame format

```
off  0   1   2     3..5           6 .. 6+len-1   6+len
     00  59  cmd   len (u24 LE)   body           cs
```

`cs = (~sum(body)) & 0xff`, i.e. empty body → `ff`. **Verified** on every reply.

## 3. Commands

| Cmd | Meaning | Behaviour on this unit |
|---|---|---|
| `00` | ACK | body = 1 status byte: `00` accepted, `01` error (**verified**) |
| `11` | name + version | 27-byte body: `TANK MINI_004\0` + 11 zero bytes (**verified**) |
| `12` | other info | 6 zero bytes (**verified**) |
| `13`, `17`, `1b`, `20` | other queries | no reply (**verified**) |
| `22` | WRITE | `type + addr u32 + len u24 + data`, always ACKed (**verified** for select) |
| `23` | READ | `type + addr u32 + len u24` → echoes header + `data[len]` (**verified**) |
| `21` | ERASE | exists in the app; **never send** |

Queries with empty body are `00 59 <cmd> 00 00 00 ff`.

## 4. Region types

| Type | Name | Behaviour here |
|---|---|---|
| 0–3, 6, 7 | NOR/NAND/SD/EFF/RSV | error ACK, status `01` (**verified**) |
| 4 | DEV | serves data, holds the live preset (**verified**) |
| 5 | USR | serves data; mostly empty on this unit (**verified**) |

## 5. Verified addresses (type 4)

| Address | Len | Meaning |
|---|---|---|
| `0x0000` | 1942 | live preset block for the active slot, ends with ASCII `PATCHEND\0\0` at `0x078c` (**verified**) |
| `0x20000000` | 1 | active slot index, 0-based (**verified**; returned `01`, then `00` after select) |
| `0xE0000000 + i` | 1 (write `[i]`) | select slot `i`; ACKed and the active-slot byte follows (**verified**) |

Reads past the preset window return stale RAM, not zeroes: the same parameter
fragments reappear at unrelated addresses (e.g. `0x4000` echoes bytes that also
occur near `0x01e0`). Do not interpret out-of-window reads as data. **Verified**
by comparing a 256 KB dump against the live preset.

## 6. Preset block layout (type 4, `0x0000`, 1942 bytes)

The block is 21 records of **92 bytes** (`21 × 92 = 1932`) followed by
`"PATCHEND" 00 00`. **Verified**: the on-device read is byte-identical to the
first records of `tankMini_am3_preset.bin` shipped inside the official app, and
the record stride reproduces every block boundary seen in the dump.

Per-record first 0x20 bytes (offsets relative to the record):

| Off | Size | Observed | Notes |
|---|---|---|---|
| `0x00` | 2 | `0x3a`–`0x61` | per-record id, constant per record across presets (**inferred**) |
| `0x02` | 2 | always `00 01` | version/marker (**inferred**) |
| `0x04` | 4 | `02 03 04 05` | fixed index list on every record (**inferred**) |
| `0x08` | 12 | mixed `00/01/02…` | flags per sub-block (**inferred**) |
| `0x0f` | 1 | `03, 03, 05, 01, 0d, 0b, …` | effect/block type id, repeats across presets (**inferred**) |
| `0x10` | 16 | mixed | flags/ids (**inferred**) |
| `0x20` | 60 | u16 LE values, mostly 0–100 | 30 parameters per record (**inferred**) |

Field semantics, parameter ranges and the meaning of the id at `0x00` are still
open — the next step is the Dart AOT analysis of `libapp.so`
(classes `EffectParam`, `EffectParamDef`, `EffectViewParam`, `Patch*`).

## 7. Type 5 addresses

| Address | Len | Result |
|---|---|---|
| `0x80000000` | 280 | 20 entries × 14 bytes, NUL-padded ASCII cab/amp names (**verified**): `EAGLProV30s`, `Sperimental`, `JUICE4x12V30`, `Mess Bog`, `FdChamp`, `FdPrJunir`, `Mar960BV30`, `DizzlV30`, `Electrovoice`, `MessDRectV30`, `TwinJensenC`, `TwedDlx1X12`, `FendShowman`, `J120Rolnd`, `UK-C30Silvers`, `BassAgula25`, `BassJensen10`, `BassFdBassman`, `BassAmpg410`, `BassEDN300` |

Entry stride is **14**, not the 17 published for the IR Box; reading 280 bytes
ends exactly after the 20th name (the manual documents 20 IR CAB slots). A
544-byte read (the IR Box length) returns the same table plus the first bytes of
whatever follows, which is what produced the earlier "32 × 17" assumption —
that was wrong for this unit (**verified**).

The rest of type 5 (`0x0000`–`0x10000`) reads as zeroes: on this unit the
"working copy" region of the older family is not populated. The IR upload path
still has to be tested.

## 7b. Slot behaviour observed so far (**verified**, but surprising)

1. `select slot i` is ACKed and the active-slot byte at `0x20000000` follows
   immediately (0 → 1 after the pedal's own footswitch B, 1 → 2 after C).
2. Reading the block at `0x0000` after switching to 1B, 1C, 5, 12 and 20 returns
   **byte-identical** data to 1A. Either every stored preset on this unit is
   currently identical, or `0x0000` exposes a single working buffer that is not
   per-slot. Open question, listed below.
3. Writing a modified block to `0x0000` is reflected on read-back, and a
   subsequent `select` of any slot **discards** the edit (so select really does
   reload from flash). Conclusion: writes to `0x0000` are RAM-only for the
   lifetime of the active selection; persistence needs a save command that has
   not been found yet.
4. All 21 records are untouched by switching slots, which is what rules out
   "the block is the whole 21-preset library".

## 8. Open questions

1. Record semantics: what is the id at record `0x00`, and how the 21 records map
   to the 6 effect modules of the pedal.
2. Where the 21 preset names live.
3. Save sequence for a slot (write + ACK, flash timing).
4. IR / cab upload path and format (the app ships `tankMini_am{3,4}_ampCab.bin`,
   245 760 bytes each).
5. Whether the pedal streams state changes when knobs are turned (nothing seen
   while idling — **verified** for a 2 s idle listen).
6. BLE GATT transport (the Android app controls the pedal wirelessly; service
   UUIDs to be pulled from the APK).
