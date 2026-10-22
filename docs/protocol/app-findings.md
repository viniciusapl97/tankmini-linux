# TANK MINI: what the official app does

Static reverse engineering of the official M-EFCS Flutter app (Blutter output in
`captures/blutter_out/`, raw `libapp.so` in `captures/apk/lib/arm64-v8a/`).
Nothing in this file was tested on the pedal. It records what the app sends and
expects, so every item still needs a device check before it is marked
**verified** in `tank-mini-usb-sysex.md` or `preset-map.md`.

Markers: **from app** (read directly out of the code or constants),
**inferred** (my reading of code that is only partly decoded, or a guess from
data). File references are relative to `captures/blutter_out/asm/m_efcs/`.

## 0. A trap when reading the asm: Smi tagging

Dart AOT passes small ints as tagged Smis, so the stored value is twice the real
one. Values in `r2 = 8`, `r16 = 48` or `mov x2, #8`, when they are passed as
Dart objects (call arguments, list elements, record fields), are **value × 2**.
In the code paths below:

- `r2 = 8` before `writeData`/`readData` means **type 4**, and `r2 = 10` means
  **type 5**.
- `r3 = 536870912` (0x20000000) passed into `readData` means address
  **0x10000000**. The same constant stored straight into an unboxed field (as
  `asyncReadData` does) means 0x20000000.
- `r16 = 48` in `waitForSave` means **24 bytes**, and `r16 = 2` means 1.

Constants loaded from the object pool as Mints (for example
`[pp+0x37330] 0xf0000000`) are real values. Every address below was checked
against this rule.

## 1. Memory map as the app sees it

| Type | Address | Len | R/W | Meaning | Evidence |
|---|---|---|---|---|---|
| 4 | `0x10000000` | 92 | R/W | **Edit buffer**: the active patch (same 92-byte layout as one preset record). Live edits go here. | **from app**: `ProductDataBase.writePatchField` passes base `0x10000000` (`data/product_data_base.dart:1049`). `_loadPresetData` reads it with the patch size (`:348`, Smi-tagged `0x20000000`). |
| 4 | `0x20000000` | 4 | R/W | **Glob** (`TGlob`, 4 bytes): `[0]` = active preset index, `[2]` = global bypass. `[1]`, `[3]` unknown. | **from app**: `GlobWrapper` callocs 4 bytes (`data/products/tank_mini/structs.dart:671`); `getGlobPatchIndex` = `ldrb [x,#0]`; `globalBypassed` = `ldrb [x,#2]` (`data/products/tank_mini/data.dart:609`); `_reloadGlobal` reads `0x20000000` with glob size (`product_data_base.dart:6196`). |
| 4 | `0x0000 + i*92` | 92 | R/W | Stored preset `i` (0..20) | **from app** `PresetStore` / `PresetLayout.addressOf` = base + i×stride; already verified |
| 4 | `0xE0000000 + i` | 1 | W | Select preset `i`. App data = **`[0x01]`**, not `[i, i]`. | **from app** `DeviceConnection.switchPreset` (`protocol/device_connection.dart:1364-1375`: one-element list, Smi 2 = 1) |
| 4 | `0xF0000000` | 0 | W | Commit the working table to flash | **from app** `PresetStore._commitAndSelect` (`protocol/preset_store.dart:659`); verified |
| 4 | `0xD0000000` | 1 | W | **USB loopback** on/off `[1]`/`[0]` | **from app**: disassembly of `LoopbackState.setLoopbackEnabled` closure at `0x5ad528` (writeData type 4, addr = pool Mint `[pp+0x1c1f8] 0xd0000000`) |
| 4 | `0x20000002` | 1 | W | **Global bypass** `[1]` = bypassed / `[0]` | **from app**: disassembly of `setGlobalBypassEnabled` closure at `0x5afb98` (`mov x3,#4; movk x3,#0x4000,lsl#16` = Smi 0x40000004 → 0x20000002) |
| 4 | `0x70200000` | 4 | W | Swap two presets: `u16le(a) u16le(b)` | **from app** `DeviceConnection.swapPreset` (`device_connection.dart:1503`); not tried, probably writes flash |
| 4 | `0x70300000` | 1 | W | Part of factory restore (writes `[0x00]`, then re-reads glob) | **from app** `_runFactoryRestore` closure (`product_data_base.dart:9859`); **destructive**, do not send |
| 4 | `0x70600000` | 10 | R | "AmpDs module-type" descriptor: 9 bytes + checksum. The app only acts on it for newer firmware. | **from app**: `ProductConfig.off_74`, used in `_loadAmpDsModuleTypeInfo` (`product_data_base.dart:3505`, length Smi 20 = 10 at `:3563`) |
| 5 | `0x80000000` | 20×14 | R | **CAB** slot names | **from app**: `ProductConfig.off_5c` = `(20, 14, 0x80000000)` → "CAB" request (`product_data_base.dart:5217`); verified on device |
| 5 | `0x90000000` | 20×14 | R | **AMP** slot names. **Not read on the device yet.** | **from app**: `ProductConfig.off_60` = `(20, 14, 0x90000000)` → "AMP" request (`:5221`) |
| 5 | `0xB0000000` | 24 | R | Model-preview capability (TANK MINI only; other products use `0xD0000000` len 32) | **from app** `_readPreviewCapability` (`device_connection.dart:2778`, type 5 at `:2803`) |
| 5 | `0x00000000` | 8192 | W | AMP preview buffer | **from app** `ModelPreview.upload` / `pack` |
| 5 | `0x00002000` | 4096 | W | CAB preview buffer | **from app** `ModelPreview.upload` (`protocol/model_preview.dart:594`) |
| 5 | `0x20000000 + off` | n | W | Live tweak of a byte inside the preview header (cab importer writes `0x20000013`) | **from app** `import_cab.dart:1408`; **inferred** meaning |
| 5 | `0xA0000000` | 1 | W | Activate the preview: `[1]` = amp, `[2]` = cab | **from app** `model_preview.dart:844,886` |
| 5 | `0xF0000000 + s` (s<20) | 1 | W | Save the AMP preview into slot `s` (data `[0x00]`) | **from app** `ModelPreview.writeControl` range check (`model_preview.dart:192`) + `ResourceSaveController.save` |
| 5 | `0xF0000100 + s` (s<20) | 1 | W | Save the CAB preview into slot `s` (data `[0x00]`) | **from app** `import_cab.dart:1802` (`r7 = 0xF0000100`) + `resource_save_controller.dart:98-134` |
| 5 | `0xF0000000` | 1 | R | Save status: `0xFF` = done/idle | **from app** `TankMiniModelProtocol.waitForSave` (`protocol/tank_mini_model_protocol.dart:75-125`) |
| 5 | `0xD0000000` | 24 | R | Save result: 6 × u32 LE, success iff `[0]==1, [1]==0, [2]==0` | **from app** same function (`:141-175`) |
| 5 | `0x70000000` | 245760 | erase+W | Factory AMP+CAB blob (`tankMini_am{3,4}_ampCab.bin`) | **from app** factory restore (`product_data_base.dart:9681`). **Destructive.** |

`ProductConfig` for TANK MINI (`objs.txt:2912`, **from app**): 6 modules,
6 params per module, 21 presets, patch base `0x10000000`, glob base
`0x20000000`, preset-name address/length 0 (preset names are generated
locally as `1A..7C`), AmpDs info `0x70600000`, effect list from
`buildKe1EffectParams`, index formatter `i → "{i/3+1}{'A'+i%3}"`. The values
`off_40=9`, `off_48=0x28` and `off_50=0xF0` are unknown.

Important for the implementation: **the app never reads or writes `0x0000+i*92`
for live editing. Everything goes through the 92-byte edit buffer at
`0x10000000`.** `READ type 4 0x10000000 len 92` should return the active
preset. This is the first thing to verify.

## 2. Patch layout corrections (92 bytes, **from app**)

`PatchFields` (`data/products/tank_mini/structs.dart`) defines vol, chain,
effon, effunit and effpara:

| Off | Size | Field | Evidence |
|---|---|---|---|
| `0x00` | int16 LE | **vol**, the preset volume 0..100. This is the field the docs called "patch id" (observed 0x3a–0x61 = 58–97). | `strh` at offset 0 (`structs.dart:474`); the UI has a "VOL" `ParamSlider` 0..100 (`ui/products/tank_mini_view.dart:193-198`) |
| `0x02` | 6×u8 | chain (module order) | `getChainAt` = byte `2+i` (`data.dart:113`) |
| `0x08` | 6×u8 | on/off | `+8` (`structs.dart:446`) |
| `0x0e` | 6×u8 | unit / type | `+0xe` (`structs.dart:420`) |
| `0x14` | 36×int16 LE | params `[module][6]` | `0x14 + m*12 + p*2`, read with `ldrsh` (signed) (`data.dart:1382,1411`) |

Arrays at 0x08, 0x0e and 0x14 are indexed by **module id**, not by chain
position. Module ids are fixed (`buildKe1EffectParams`,
`data/products/ke1/config.dart:110`):

| id | name | units (unit byte = index) | params p0..pN (wire order) |
|---|---|---|---|
| 0 | FX (dynamics) | 0 Noise Gate, 1 Boost, 2 Compress | Noise Gate: `Gate`; Boost: `Gate, Gain`; Compress: `Gate, Sustain, Attack, Level` |
| 1 | AMP | 0..19 = AMP slot (names at type 5 `0x90000000`) | `Gain, Level, Bass, Mid, Treble` (5 used, 0..100) |
| 2 | MOD | Chorus, Phaser, Tremolo, Flanger, Vibrato, Univibe, Autofilter | as in `compact_effect_params.json` |
| 3 | DLY | **0 Analog, 1 Duck, 2 dTape, 3 Dual, 4 Lofi** | Analog `Time,Fb,Mix,Phaser,Pitch`; Duck `Time,Fb,Mix,Unpack,Speed,Depth`; **dTape `Time,Fb,Mix,Grit,Speed,Depth`**; Dual `Time,Fb,Mix,`**`T-Mode`**`,Speed,Depth`; Lofi `Time,Fb,Mix,Grit,Speed,Depth` |
| 4 | REV | Room, Hall, Swell, Spring, Shimmer, Cloud | as in json |
| 5 | CAB | 0..19 = CAB slot (names at type 5 `0x80000000`) | **`Level, Low Cut, High Cut`** |

Evidence and notes:

- **AMP params** come from `BlackboxData._initIrAmpDs`, which TankMini calls
  (`data/products/blackbox/data.dart:507-520`), using list `[pp+0x17da0]`:
  EffectParamDef "Gain", "Level", "Bass", "Mid", "Treble", all 0..100. If the
  pedal reports an AmpDs module type of NAM or AM5 (via `0x70600000`), the app
  relabels them `Input, Output, Bass, Middle, Treble` (`[pp+0x181c0]`,
  `TankMiniData.onAmpDsModuleTypeLoaded`). **from app**
- **CAB params** come from `kCabParams` (`data/effect_param_def.dart:534-560`).
  The list order is `[Level, Low Cut, High Cut]`:
  - Low Cut 0..100 is shown as `20 + v*280/100` Hz (20–300 Hz).
  - High Cut 0..100 is flagged `reverse`, shown as `(50 + v*130/100)/10` kHz
    (5.0–18.0 kHz). **from app**
- **The delay has 5 variants.** The json missed the 3rd because its name is a
  named argument, `tapeName = "dTape"`, and Dual's 4th param is
  `dualFourthParam = "T-Mode"` (`data/products/shared/compact_effect_params.dart:560`;
  list order `[Analog, Duck, dTape, Dual, Lofi]` at `:895-910`). **from app**
- **FX** child lists are built inline in `buildKe1EffectParams`; all ranges
  default to 0..100. **from app**
- **Unit byte → child.** For AMP and CAB, `initResourceChildTypes` sets each
  child's protocol unit = its index. For the compact lists, unit = list index.
  **inferred** (from `childIndexForProtocolUnit` usage)

## 3. Live edit: exactly what is sent (question 1)

Every live edit is a **per-field WRITE to the edit buffer**:
`00 59 22 … type=4, addr = 0x10000000 + field offset, data = the field bytes`.
No select, no "apply", and **no `0xF0000000`** follows a live edit.

| UI action | Wire | Evidence |
|---|---|---|
| Knob, param `p` of module `m` | WRITE type 4 `0x10000014 + m*12 + p*2`, 2 bytes int16 LE | `sendEffectParam` builds `FieldRef(paraOff + (m*6+p)*2, 2)` (`product_data_base.dart:16334`) → `writePatchField` (`:16417`) → `writeData(type 4, 0x10000000+start, rawView.sublist(start,end))` (`:7137`) |
| Module on/off | WRITE type 4 `0x10000008 + m`, 1 byte (0/1) | `sendEffectEnable`, `FieldRef(effOnOff+m, 1)`, key `enable:m` (`:7976-8014`) |
| Change variant (unit) | WRITE type 4 `0x1000000E + m`, 1 byte; then **wait 100 ms**; then **READ type 4 `0x10000014 + m*12`, 12 bytes** to pick up the defaults the pedal loaded for that unit | `loadUpdateParamData` closure: `sendRange(0x10000000, off, 1)` (`:15753`), `Duration 0x186a0 µs` (`:15864`), `readData(type 4, 0x10000000 + paraOff + m*6*2, 6*2)` (`:15962`) |
| Reorder chain | WRITE type 4 `0x10000002`, 6 bytes | `sendChainOrder` → `FieldRef(chainOff=2, 6)`, key `chain` (`:15084`) |
| Preset VOL slider | WRITE type 4 `0x10000000`, 2 bytes int16 | `TankMiniView.build` closure → `sendPatchField(vol)` (`tank_mini_view.dart` closure `0x5ad898`) |

How sends are paced (**from app**, partly **inferred**):

- There is no time-based throttle.
- All device-field writes are serialised through one `MutationCoordinator` lane
  (`"device-fields"`). Each write waits for its ACK before the next one goes out.
- Each edit carries an operation key (`param:m:p`, `enable:m`, `unit:m`,
  `chain`) and a generation number (`_beginEffectOperation`). Before a queued
  write is sent, its `latest` closure (`0x567fb4`) checks that no newer edit
  with the same key has superseded it. Stale ones are dropped, so only the most
  recent pending value per parameter goes out.
- If a write fails, the app restores the previous bytes of the local range.
- Knobs (`ParamKnob._updateValue`, `ui/widgets/knob_widget.dart:5884`) fire
  `onChanged` on every integer change while dragging.
- The VOL `ParamSlider` only calls back on drag end or step buttons
  (`ui/widgets/param_slider.dart`, the DragUpdate handler only calls
  `setState`).

Save (only on an explicit "save"), from `savePresetToDevice` → `PresetStore.save`
→ `_commitAndSelect` (`protocol/preset_store.dart:426-750`), **from app**:

1. Write 92 bytes (the edit buffer's bytes) to `0x0000 + i*92`. The app's
   chunk limit is 1015 bytes per WRITE frame (`protocol/command_queue.dart:3048`),
   so it fits in one frame.
2. Write `0xF0000000` with an empty payload.
3. Wait 100 ms.
4. `switchPreset(i)`: WRITE `0xE0000000+i` `[0x01]`, then wait 100 ms.

Bank import (`PresetStore.writeBank`) writes the records, commits with
`0xF0000000`, then also writes the active record into `0x10000000`
(`preset_store.dart:242`).

## 4. Global settings (question 2)

For TANK MINI the global block is **4 bytes at type 4 `0x20000000`** (`TGlob`,
one declared field, `patch`):

- byte 0 = active preset index (read).
- byte 2 = global bypass (read via `globalBypassed`, written with
  `WRITE 0x20000002 [0/1]`).
- bytes 1 and 3 are unknown; the app never touches them.

The only other device-level control on the TANK MINI page is **USB loopback**,
written as `WRITE type 4 0xD0000000 [0/1]`. The app never reads it back: the
state is a local bool, false on connect.

The TANK MINI page (`TankMiniView`) has a preset bar, an action row (loopback,
global bypass), the VOL slider (per preset, patch 0x00) and the effect chain.
The app has **no** master volume, input gain, global EQ, output mode, tuner,
tap tempo, BPM, drum or looper for this product. Those exist only for other
products (KPT/MK300/Tank-G use `writeGlobField` with larger globs). **from app**

## 5. Model (AMP / IR) preview and upload (question 3)

`tank_mini_model_protocol.dart`:

- `matchesDevice` returns true when the device name, with spaces and `-`
  removed, equals `TANKMINI`.
- `isCapability(bytes)` is true when 24 bytes decode as u32 LE
  `[1, 0x1001c, 0x2000, 0x14, 0x28000, 0x1000]` (`:451`). Reading of the words:
  - `0x2000` = amp slot size (8 KiB)
  - `0x14` = 20 slots
  - `0x28000` = offset of the cab area (20 × 8 KiB)
  - `0x1000` = cab slot size (4 KiB)
  - `0x1001c` is unknown.

  This matches the factory blob exactly (§5.3). **from app / inferred meaning**
- `waitForSave(isActive, read)` (`:10-175`):
  1. Loop for up to 5 s (`5000000` µs).
  2. Each pass: READ type 5 `0xF0000000` len 1. If the result is `[0xFF]`, go
     to step 3; otherwise wait 50 ms (`Duration 0xc350`) and repeat.
  3. READ type 5 `0xD0000000` **len 24** (Smi 48). Success iff u32[0] == 1,
     u32[1] == 0 and u32[2] == 0. **from app**
  4. The `read` callback is the `DeviceConnection` closure that calls
     `readData(type 5, …)` (`device_connection.dart:5402`).

`0xd0000000` therefore means two different things:

- type 4: USB loopback (write).
- type 5: save result (24-byte read).

The "len 48" is the Smi-tagged form of 24.

### 5.1 End-to-end flow for a CAB (IR) slot, from `ImportCabState`

1. Probe: READ type 5 `0xB0000000` len 24. Expect the capability above. If
   the probe fails, the app uses the older non-preview path, which is not
   relevant here.
2. Load the user WAV with `WaveUtils` (pure Dart; **no ffmpeg** in this path).
   Convert to mono, resample with a Kaiser-windowed sinc (β = 8.6) to the
   `WaveUtils` target format, **44.1 kHz, 24-bit** (`utils/wave_utils.dart:2907`).
   Truncate to `field_33` bytes:
   - 3072 bytes = **1024 samples × 3 bytes** for TANK MINI (`import_cab.dart:969`,
     Smi 0x1800).
   - 6144 bytes for other products.
3. Build the CAB record (`_cabHeader`, `import_cab.dart:2219`), 20-byte header
   + samples:
   ```
   "CAB" 00 | name[14] NUL-padded | 01 | b19 | 24-bit LE mono samples…
   ```
   On the first preview `b19` comes from a widget field; `_cabHeader(name, n)`
   writes `n`. In the factory blob `b19` is 0x64/0x78/0x7f, so it is most
   likely the cab level byte (**inferred**). The live "parameter" write goes to
   type 5 `0x20000013` = header offset 19 (**inferred** link).
4. `ModelPreview.upload(cab, record, activate)`:
   - `pack` zero-pads the record to 4096 bytes and rejects anything larger
     (`modelTooLarge`).
   - WRITE type 5 `0x00002000` with the 4096 bytes. Writes are sequential, with
     up to 3 attempts and 150 ms × attempt backoff.
   - Then WRITE type 5 `0xA0000000` `[0x02]` to make the pedal play the
     preview.
5. Save into CAB slot `s` (`_executeSave` → `ResourceSaveController.save`):
   - `_prepareSave` re-uploads the record, now with the chosen name in the
     header.
   - WRITE type 5 `0xF0000100 + s` `[0x00]`.
   - `writeControl` sees an address in `[0xF0000100, 0xF0000114)` and runs
     `waitForSave` (§5).
   - Then the name list is updated locally (`rename`). Whether the pedal
     rewrites `0x80000000` by itself is **inferred** (yes, since the name is in
     the record).

### 5.2 AMP slot

Same mechanism (**from app**; the AMP record format is only **inferred**):

- Kind amp → preview buffer type 5 `0x00000000`, up to 8192 bytes.
- Activate with `0xA0000000 [0x01]`.
- Save with `0xF0000000 + s` `[0x00]` + `waitForSave`.

The importer that produces an AMP record for TANK MINI is the generic
`import_amp_ds` / NAM path (`utils/nam_to_amx.dart`,
`utils/nam_a2_lite_exporter.dart`). I did not trace it; see §9.

### 5.3 Factory blob format (`tankMini_am{3,4}_ampCab.bin`, 245 760 B)

Checked by reading the asset:

- **20 AMP records × 0x2000** at `0x00000..0x27FFF`. Each record is:
  - `"AM2\0"`
  - name[14] (`UWE-Twins`, `UK-C30Normal`, …, `BassMarkT501`)
  - `0x64` at +0x12
  - 3 unknown bytes at +0x13..0x15
  - LE float32 triplets from +0x18 that look like tone-stack band definitions
    (e.g. `400,-9,4`, `600,-12,12`, `2500,…`; **inferred**)
  - model data up to +0x183b
- **20 CAB records × 0x1000** at `0x28000..0x3BFFF`. Each record is
  `"CAB\0" name[14] <format> <level>` + **1024 samples of plain 24-bit LE
  PCM** (3072 bytes, rest zero). Format is `01` in the am3 blob, `02` in am4.
  All factory IRs are peak-normalized to about full scale. (An earlier note
  here said the data did not decode as PCM; the `a b a` byte triplets are just
  the shape of those IRs. Corrected 2026-10-22, see §12.)

Factory restore of this blob is `FlashWriter.run(type 5, 0x70000000, blob)`
(§6). So the slot storage is presumably:

- amp `s` at `0x70000000 + s*0x2000`
- cab `s` at `0x70028000 + s*0x1000`

(**inferred**)

## 6. `flash_writer.dart` (question 4): destructive

`FlashWriter.run(type, addr, data)` (`protocol/flash_writer.dart:10-270`):

1. Reject an empty payload or `addr + len > 2^32`.
2. Erase 4 KiB sectors from `floor(addr/4096)*4096`, covering
   `ceil((addr%4096 + len)/4096)` sectors. Each erase is CMD `0x21`
   (`makeErasePacket` emits `00 59 21 …`).
3. Write the data with `writeDataWithProgress`.

Its only caller is `DeviceConnection.eraseAndWrite`, used only by **factory
restore** (`_runFactoryRestore`, `product_data_base.dart:9644-9760`):

| Kind | Type | Address |
|---|---|---|
| ampCab | 5 | `0x70000000` |
| drum | 6 | `0x20000000` |
| midiDrum | 6 | `0` |

There is **no firmware-update path** in the app; it only shows "visit the
website". Never call ERASE or these addresses from the Linux app. The other
factory-restore writes are also destructive: `0x70300000 [0]` (global) and the
preset bank rewrite + commit.

## 7. Notifications, tuner, tap (question 5)

- The app does **not** consume unsolicited messages from the pedal. Instead,
  `_startGlobPolling` runs a 1 s periodic timer (`Duration 0xf4240 µs`,
  `product_data_base.dart:2679`). While the command queue is idle, the timer
  sends `asyncReadData(type 4, 0x20000000, globSize=4)`
  (`device_connection.dart:764`).
- If the glob bytes changed (`listEquals`), for example because a footswitch
  changed the preset, the app runs `loadPreset`: READ `0x10000000` len 92, then
  re-reads the AmpDs info at `0x70600000`. **from app**
- Footswitch preset changes are therefore detected by **polling
  `0x20000000` once per second**. Do the same.
- There is no tuner, tap tempo or BPM for TANK MINI in the app (§4).

## 8. BLE (question 6)

From `transport/ble_channel.dart:1487-1670`, **from app**:

- Service `0000ae40-0000-1000-8000-00805f9b34fb` ("AE40").
- Characteristics `0000ae41-…` and `0000ae42-…`.
- Notifications are enabled through CCCD `0x2902`.
- The same `00 59` frames (without SysEx 7-bit wrapping, **inferred**) ride on
  these characteristics.

Which characteristic is write and which is notify was not separated in the
asm. By the JieLi convention, AE41 is write and AE42 is notify. **inferred**

## 9. What I could NOT determine

- Glob bytes 1 and 3. Also whether `READ 0x20000000 len 4` differs from the
  verified len-1 read beyond byte 0.
- Whether writes to `0x10000000` survive a preset switch. The app assumes not:
  only "save" persists.
- The meaning of `0x1001c` in the capability, and of `ProductConfig`
  `off_40/48/50` (9, 40, 240).
- The stored CAB sample encoding in the factory blob, and the AM2 body format.
  The importer that turns a NAM or other user amp model into an AM2 record for
  TANK MINI (`import_amp_ds`, `nam_to_amx`, `nam_a2_lite_exporter`) was not
  traced.
- The exact live-tweak address map inside the preview header for AMP
  (`writeModelPreviewParameter` with `addr = field_2f + 0x14`).
- The 10-byte layout of `0x70600000`. Only the checksum on the last byte and
  "FX/AM5/NAM/GFW" module types are visible.
- Which BLE characteristic is write and which is notify (§8).
- The FX module param meaning beyond the labels (for example, whether `Gate` in
  Boost/Compress is a shared noise-gate threshold).
- Factory presets keep module 0 params at 0. That is consistent with FX params
  defaulting to 0; not confirmed.

## 10. Corrections to the existing docs

- `preset-map.md` §2: bytes `0x00-0x01` are **VOL** (int16, 0..100), not an id.
- `preset-map.md` §3: the app's select payload is **`[0x01]`**, not `[i, i]`.
- `preset-map.md` "Parameter names":
  - AMP order is **Gain, Level, Bass, Mid, Treble**.
  - CAB is **Level, Low Cut, High Cut**.
  - DLY has 5 variants, including **dTape**.
- `tank-mini-usb-sysex.md` §7: `0x80000000` is the **CAB** name table; the
  **AMP** names are at type 5 `0x90000000`.
- Live editing should target the **edit buffer at type 4 `0x10000000`**, not
  the preset table at `0x0000`.

## 11. Second pass (2026-10-22): app v3.9.2314 on a real phone + pedal over BLE

The installed app (S24, `com.digAmp.m_efcs` 3.9.2314, split APK) was driven
over adb while connected to the pedal by BLE. Screens were observed;
everything below about bytes is still **from app** (static) unless marked.

### 11.1 Tuner (corrects §7: there IS a tuner for TANK MINI)

`data/tuner_controller.dart`, `TunerController`. Block at **type 4
`0x70100000`**, single-byte writes to `0x70100000 + offset`:

| Offset | Dir | Meaning | Evidence |
|---|---|---|---|
| 0 | R/W | Bypass volume while tuning, 0..100 | `setBypassVolume` clamps Smi 0..200 → 0..100, `_writeByte(0, v)` |
| 1 | R/W | Reference pitch as signed offset from 440 Hz (430..450) | `setReferenceHz` clamps 430..450, writes `v - 440` (`sub #0x1b8`); `_poll` decodes `(b & 0x7f) - (b & 0x80) + 440` |
| 2 | W | Tuner on = `1` (written by `start`); `dispose` writes `0x70100002` again (off, **inferred** `0`) | `start`: `_writeByte(2, 1)` |
| 3 | R | `1` while the tuner runs | **verified** |
| 4 | R | Note: 1 = G#, 2 = A … 12 = G; `0` = signal, no stable note; `0xFF` = silent | **verified** (open A → 2, open D → 7) |
| 5 | R | Deviation, 127 = centre; app's in-tune window 118..=136 | **verified** direction (flat < 127); unit not calibrated |

`_poll`: READ type 4 `0x70100000` len 6 on a periodic timer, only while
the tuner dialog is open. Enable (`2 ← 1`), disable (`2 ← 0`) and the idle read `64 00 ff 00 ff 00` are **verified** on the device (2026-10-22), as is the note mapping above with a guitar. UI: needle, ±50 cent scale, "NO SIGNAL", Reference
440 Hz and Bypass Vol steppers.

### 11.2 Preset files (Share / Import current, Share / Import all)

- Product entry: extension **`.tkm`** (`register_products.dart:4495`).
  "All presets" uses `".a" + ext` → **`.atkm`** (`allPresetsSuffix`).
- Bank file = exactly the factory asset layout, **1942 bytes**: 21 × 92-byte
  patches + `"PATCHEND\0\0"` trailer. Checked: the shipped
  `tankMini_am4_preset.bin` and the 21 dumps in `captures/presets/` all end
  in `50 41 54 43 48 45 4E 44 00 00`.
- Import all (`_importPresetFile`): `PresetFiles.readBank` (length must match
  `_bankLength(factorySize)`) → `PresetStore.writeBank` =
  `writeDataWithProgress(type 4, addr 0, bytes)` → `PresetStore.save`
  (commit `0xF0000000`) → reload names and the current preset.
- Import current: `PresetFiles.readSingle` → one patch (92 bytes,
  **inferred**: size check against the layout record) written into the
  current slot, then save. Share current = `getCurrentPresetBytes`
  (`toBytes` of the patch struct) named `<preset>_<name>.tkm`.
- Swap preset: dialog with two positions → swaps two records in the table
  and commits.
- **No flash-erase involved**: these are the same table writes + commit our
  app already does. Safe to port.

### 11.3 AMP model files (Import → AMP tab, Community)

- Accepted names match `\.am(\d+)(data)?$`, e.g. `.am4data`, `.am3data`
  (`utils/amx_file_utils.dart amVersionOfName`). Community posts are tagged
  `AM4DATA` / `AM3DATA`; the number is the amp-engine generation and must
  match the pedal (factory blobs `am3` vs `am4`).
- `AmpHead.fromBytes` / `fromAlignedBytes` (`ui/modules/import_amp_ds.dart`):
  4-byte magic, 14-byte name, bytes 18..21 read individually, header 22 B
  (aligned variant 24 B). Bytes 18..21 are most likely the default
  Level/Gain range shown in the Import screen ("Level 100", "Gain 30–60")
  (**inferred**).
- The model body is a GRU network: `libamx_parser_lib.so` exports
  `calc_bias_gru` / `calc_bias_gru_am4`, called before upload.
- **NAM import is server-side**: `utils/nam_to_amx.dart` uploads the `.nam`
  to `https://community.m-vave.com/bbs/api` (`uploadnam`, `namhash`) and
  downloads a trained AM record. Not portable offline.
- Upload path is §5.2 (preview buffer type 5 `0x0`, activate `0xA0000000
  [1]`, save `0xF0000000 + s`). Writes flash slots → owner go-ahead needed.

### 11.4 Screen inventory (for UI ideas)

Param tab: preset picker (1A..7C), Sync, Save, Save to, BYPASS, Loopback,
More (share/import current, swap, share/import all, factory restore,
tuner), VOL bar, signal chain as 6 tiles in two rows with arrows, one
expanded block card below (model dropdown + on switch + knobs).
Import tab: AMP / CAB segmented; "Load AMX/CAB File to List", "Save to
Device", Level (and Gain range for AMP) bars, search + Edit over a local
file list. Audio separator tab (stem separation of WAV/MP3/MP4; ≥6 GB RAM
warning). Community tab: web forum with AM4DATA/AM3DATA amp captures,
likes/downloads, Login/Publish.

## 12. CAB (IR) import on firmware `TANK MINI_004` (2026-10-22, on the device)

Captured by letting the official app (3.9.2314, BLE) audition a WAV, then
reading the pedal over USB. **verified** unless marked.

- **No preview capability.** READ type 5 `0xB0000000` (and `0xD0000000`,
  `0xA0000000`) returns junk left in the reply buffer (fragments of the
  firmware string and cab names), never the 24-byte capability of §5. So the
  app takes its legacy path (`ModelPreview.probe` → null).
- **Audition = one write of the record to type 5 `0x00000000`.** After the
  app's Audition the buffer held `"CAB\0"`, 14 zero bytes (no name yet), `01`,
  `32` (level 50 = the app's "CAB Level"), then the WAV's samples. No
  activation write was seen in the static path (`0xA0000000` is preview-only).
  Whether the pedal *plays* the buffer right away is **not confirmed by ear**.
- The app's conversion (`WaveUtils.conversionMono24Bit`): average channels,
  Kaiser-sinc resample to 44.1 kHz (β 8.6), scale floats to 24-bit and clamp,
  **no normalization**. `tankmini-core::ir` reproduces it: for a 24-bit
  44.1 kHz mono file our record is byte-identical to the app's (3092 bytes).
- Legacy sample budget in `sendCabFileData` is 6144 bytes when the probe fails,
  but a slot is 4096 bytes; we send 1024 samples (3072 bytes), what the slots
  and the app's upload both hold.
- **Save** (`_executeSave` → `ResourceSaveController.save`, base
  `0xF0000100`): rewrite the 20-byte header at type 5 `0x0` with the final
  name, then WRITE type 5 `0xF0000100 + slot` `[0]`. **from app; not run yet.**
  Our `Pedal::cab_save` verifies by reading the slot back from
  `0x70028000 + slot*0x1000` (reads **verified** to match the factory blob).
- **Transfer limits.** One READ of 3092 bytes from type 5 `0x0` hung the
  pedal's USB side (`rawmidi drain error`, then `device descriptor read/64,
  error -110` on replug) until a full power cycle. 128-byte writes and
  512-byte reads work. The RAM buffer is cleared by a power cycle.
