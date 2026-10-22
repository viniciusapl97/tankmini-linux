# Preset map

What the pedal actually stores, byte by byte. Every line carries its evidence
level: **verified** (observed on this unit), **from app** (recovered from the
official M-EFCS binaries via Blutter) or **inferred**.

## 1. The table

`READ type 4 addr 0x0000 len 1942` returns:

```
0x0000 .. 0x078b   21 presets × 92 bytes   (0x078c = 1932 bytes)
0x078c .. 0x0793   "PATCHEND"
0x0794 .. 0x0795   00 00
```

The 21 entries are the pedal's 21 presets (7 banks × A/B/C), **not** 21 effect
blocks inside one preset: reading `addr = i*92 len 92` returns exactly the same
bytes as the corresponding slice of the whole-table read (**verified** for
i = 0, 1, 2, 5, 20), so a single preset can be read and written on its own.

Reading the table is independent of the active slot — switching presets on the
pedal does not change it (**verified**). That is why an earlier "every slot reads
the same" result was a red herring: we were reading the whole library, not the
active preset.

## 2. One preset (92 bytes)

| Off | Size | Field | Evidence |
|---|---|---|---|
| `0x00` | 2 | patch id (observed `0x3a`–`0x61`); changes with content | verified raw, meaning unknown |
| `0x02` | 6 | **chain**: signal order of the 6 modules, always a permutation of `0..5` | verified (all 21 presets are permutations) |
| `0x08` | 6 | **on/off per module** | **verified** (`getEffOnAt` = byte `0x08+i`) |
| `0x0e` | 6 | **module type ("unit") per module** | **verified** (`getEffUnitAt` = byte `0x0e+i`) |
| `0x14` | 72 | **36 parameters**, u16 LE, `0..100` | **verified** (`getEffParaAt` = `0x14 + i*12 + p*2`) |

The accessor formulas come from the app's own `TankMiniData`
(`getEffOnAt`, `getEffUnitAt`, `getEffParaAt`, recovered with Blutter) and match
the device dump exactly: offset `0x14 + 5*12 + 5*2 + 2 = 0x5c` is the end of the
preset.

### Module roles (inferred from unit ranges)

The six type bytes hold, in every factory preset, values whose maxima are exactly
the counts the manual documents, which pins each slot to a role:

| Slot | Type byte | Values seen | Role | Count in manual |
|---|---|---|---|---|
| 0 | `0x0e` | 0–2 | dinâmica (gate / boost / comp) | noise gate, boost, compressor |
| 1 | `0x0f` | 0–19 | **amp** (preamp model) | 20 preamps |
| 2 | `0x10` | 0–6 | **modulação** | 7 modulations |
| 3 | `0x11` | 0–4 | **delay** | 5 delays |
| 4 | `0x12` | 0–5 | **reverb** | 6 reverbs |
| 5 | `0x13` | 0–19 | **cabinet / IR** | 20 IR CAB slots |

Amp and cabinet type bytes index the 20-entry name table of §4 — that is where
`Mess Bog`, `FGLProV30s`, … come from in the UI.

### Parameter names

| Module | Where the names come from |
|---|---|
| modulação, delay, reverb | the app's `compact_effect_params.dart` lists, per variant, in wire order (**from app**) |
| amp | `Gain, Bass, Mid, Treble, Pres, Level` (**from app**: `EffectParamDef` names; order inferred) |
| dinâmica | `Threshold, Release, Level` / `Level, Tone` / `Sustain, Attack, Level` (**inferred**) |
| cabinet | only `Level` is named; remaining slots are unnamed until confirmed (**inferred**) |

`0x14 + 36*2 = 0x5c`, i.e. the parameter block fills the rest of the 92 bytes
exactly (**verified**).

### Parameters

36 values = 6 modules × 6 slots. The first six always read `0` in the factory
presets, so the first module of the chain position carries no parameter bytes of
its own; modules further down the chain carry the real values (e.g. preset 1B:
`75, 75, 49, 58, 70, 0`).

The app's own parameter vocabulary for this device family (**from app**, extracted
from `compact_effect_params.dart`):

| Group | Variants | Parameters per variant |
|---|---|---|
| Reverb | Room, Hall, Swell, Spring, Shimmer, Cloud | `Decay, Mix, HPass, LPass, Depth` / `…, RiseT` / `…, Combs` / `Tone, Pitch, Amt` / `…, Diff` |
| Delay | Analog, Duck, Dual, Lofi | `Time, Fb, Mix, Phaser, Pitch` / `…, Unpack, Speed, Depth` / `Time, Fb, Mix, Speed, Depth` / `…, Grit` |
| Modulation | Chorus, Phaser, Tremolo, Flanger, Vibrato, Univibe, Autofilter | `Speed, Depth, Mix` / `Speed, ParamA, ParamB, Fb` / `Speed, Depth, Level` / `Speed, Depth, Fb, Mix` / `Speed, Depth` / `Speed, Depth, Mix` / `Speed, Min, Max, Mix, Fb` |

The manual lists exactly 7 modulations, 6 reverbs, 5 delays, 20 preamps, 20 IR
cab slots and a 3-band EQ for this pedal, which matches those lists.

**What is still missing** to label each of the 36 slots: which module index in
the chain maps to AMP / CAB / MOD / DLY / RVB, and which variant byte selects the
reverb/delay/modulation child. Both live in the app's runtime-built tables
(`EffectChildType`, `EffectViewParam`) that are not materialised in the AOT
snapshot; recovering them needs either the app running (BLE/USB oracle) or
disassembly of the builders plus the FFI struct layout.

## 3. Controls

| Control | Wire | Evidence |
|---|---|---|
| Active slot | `READ type 4 0x20000000 len 1` | verified |
| Select slot `i` | `WRITE type 4 0xE0000000+i`, data `[i, i]` (the app sends two ints) | verified |
| Read preset `i` | `READ type 4 0x0000+i*92 len 92` | verified |
| Write preset `i` | `WRITE type 4 0x0000+i*92` + 92 bytes | verified (read-back), RAM only |
| Write whole table | 173-byte chunks from `0x0000`, ACK per chunk | verified |

### Save to flash (**verified**)

`WRITE type 4 0xF0000000` with an **empty payload** commits the working table to
flash — ACKed with status `0x00` on this unit. This is exactly what the official
app does (`PresetStore._write(0xF0000000, const [])`, recovered from
`preset_store.dart`), and it is what `tankmini save` and the GUI's
"Salvar na pedaleira" send after writing the bytes.

Sequence: write the preset (92 B at `0x0000 + i*92`) → read back and compare →
`WRITE type 4 0xF0000000` (empty) → ACK.

**Survives a power cycle (verified).** Preset 02 has parameter `0x2c` set from
`0x03` to `0x2a`, committed, and the pedal was switched off and on again: reading
the whole table afterwards showed exactly one byte different from the factory
dump (that one). Writing `0x03` back and committing restored a table byte-identical
to the factory capture, which also proves the commit is what makes the change
permanent rather than a stale RAM buffer.

Not separated yet: whether a plain write *without* the commit also persists on
this firmware — the control run was inconclusive because the pedal had already
been committed. The app always sends the commit, so this does not affect use.

## 4. Name table (type 5)

20 entries × 14 bytes at `0x80000000`, NUL padded, ends exactly at 280 bytes
(**verified**): `EAGLProV30s`, `Sperimental`, `JUICE4x12V30`, `Mess Bog`,
`FdChamp`, `FdPrJunir`, `Mar960BV30`, `DizzlV30`, `Electrovoice`,
`MessDRectV30`, `TwinJensenC`, `TwedDlx1X12`, `FendShowman`, `J120Rolnd`,
`UK-C30Silvers`, `BassAgula25`, `BassJensen10`, `BassFdBassman`, `BassAmpg410`,
`BassEDN300`.

These are the 20 IR CAB slots of the pedal. The IR payload itself is not in the
table; the app ships `tankMini_am{3,4}_ampCab.bin` (245 760 B, `AM2\0` magic +
name + float header + samples) and uploads it through an address still unknown.

## 5. How this was recovered

1. USB probing with `tools/mvprobe.py` (identity, region sweep, chunked reads).
2. The official APK's own preset template matched the device dump byte for byte,
   confirming the block layout before any decoder was written.
3. Blutter (`../_tools/blutter`) against `libapp.so`, giving the app's module
   structure (`protocol/preset_store.dart`, `preset_layout.dart`,
   `data/products/tank_mini/*`, `data/products/shared/compact_effect_params.dart`).
4. Differential analysis of the 21 factory presets to separate headers from the
   parameter block and to bound every value range.
