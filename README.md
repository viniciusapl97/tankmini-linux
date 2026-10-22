# tankmini-linux

Native Linux control app for the **M-VAVE TANK MINI** guitar/bass multi-effects pedal,
built on a reverse-engineered USB-MIDI protocol instead of the official M-EFCS app.

## Status

| Area | State |
|---|---|
| USB-MIDI transport (SysEx framing, checksum) | verified on device |
| Identity / version query | verified (`TANK MINI_004`) |
| Read the preset block (1942 B) | verified |
| Active slot query + slot select | verified |
| Cab/amp name table (20 × 14 B) | verified |
| Block write (173-byte chunks, ACK, read-back verify) | verified (RAM only) |
| Save to flash (`WRITE 0xF0000000`, empty body) | verified across a power cycle; `tankmini save` + GUI |
| Preset table structure (21 × 92 B, chain, 36 params) | verified, see `docs/protocol/preset-map.md` |
| Module roles, effect variants, parameter names | recovered from the app; see `docs/protocol/preset-map.md` |
| Local preset names (sidecar `names.tsv`) | working; editable in the GUI and via `tankmini name` |
| Export / import a single preset (`.tankmini`, 92 B) | working in GUI and CLI, with backup + commit |
| Copy a preset to another slot | working (`tankmini copy`, GUI menu) |
| Factory reset (embedded factory table) | working (`tankmini factory`, GUI menu, with confirmation) |
| CLI (`tankmini`) | devices, identify, dump, show, patch, slot, apply, restore, backups, names, save, name, export, import, copy, factory, raw |
| GTK4 app (`tankmini-gui`) | running: preset list, 6-chain editor, 36 sliders per preset, backup/restore, hex tab |

## Hardware

- Pedal: M-VAVE TANK MINI, USB `4c4a:c755` (JieLi), serial `1120041A00030918`
- Presents as a USB audio interface (44.1 kHz) **and** a USB-MIDI device
- On this machine: ALSA card 2 → `/dev/snd/midiC2D0`

## Layout

```
docs/protocol/   protocol notes, one file per topic (evidence-marked)
docs/specs/      specs, written before implementation
assets/          factory-table.bin (byte-exact factory dump of this firmware)
tools/           probing / capture tooling (Python, stdlib only)
captures/        raw dumps and extracted app assets (git-ignored)
```

## Tools

```bash
# identity
python3 tools/mvprobe.py query 11

# read the active preset (1942 bytes, ends with "PATCHEND")
python3 tools/mvprobe.py read 4 0 1942

# dump a larger region to a file
python3 tools/mvprobe.py dump 4 0 262144 -o captures/type4.bin
```

## Reference material

Prior art for the same M-VAVE protocol family (different devices, same framing):

- `marco-jardim/mvave-irbox-tools` — frame format, 8→7 packing, checksum, region map
- `pferreir/cuvave-midi` — Cube Baby memory map
- `Pinudo/cubebaby-tools`, `aroum/fm1-custom-fw`

Local copies live in `../_tools/ref/` (not part of this repo).
REA (`../_tools/rea`, `rea` CLI) is used for the static analysis of the app.

## Legal

Reverse engineering is limited to interoperability with hardware we own: the pedal
on this desk and the official app binaries. No firmware redistribution, no
cracked tools.
