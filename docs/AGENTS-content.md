# AGENTS.md

Project: a native Linux control app for the **M-VAVE TANK MINI** guitar/bass
multi-effects pedal, built on a reverse-engineered USB-MIDI protocol instead of
the official M-EFCS app.

> This file lives at `docs/AGENTS-content.md` because the Hermes desktop client
> cannot answer the approval prompt for protected agent-instruction files. To
> enable it, run from the repo root: `mv docs/AGENTS-content.md AGENTS.md`

## Language

- Code, comments, commit messages, protocol docs, specs: **English**.
- Conversation with the owner (Vini): **Brazilian Portuguese**.
- User-facing UI strings: **pt-BR**, kept in one place so a translation layer
  stays possible.

## Process

1. Protocol claims are marked **verified** (observed on the unit), **from app**
   (recovered from the official M-EFCS binaries) or **inferred** in
   `docs/protocol/`. Never present an inference as verified.
2. Spec before implementation: `docs/specs/NNNN-*.md`.
3. Tests before behavior: a failing test, then the implementation. `cargo test
   --workspace` must stay green.
4. Update the protocol doc in the same change that verifies or refutes a claim.

## Hardware safety

- Port: `/dev/snd/midiC2D0` (ALSA card 2), USB-MIDI with 7-bit SysEx packing.
- **Never send command `0x21` (ERASE).** It exists in the official app, it is
  never needed, and there is no confirmed recovery path.
- Probe unknown addresses read-only first. The verified window is
  `type 4, 0x0000`–`0x0795` plus `type 5, 0x80000000`–`0x80000117`.
- Before any write that changes what the pedal plays, dump the table to
  `~/.local/share/tankmini/backups/` (the CLI and the GUI both do this
  automatically).
- A wrong checksum makes the pedal ignore the frame silently: "no reply" is
  almost always a malformed frame, not a dead device.

## Commands

```bash
PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace   # mise shim has no toolchain
cargo build --workspace
cargo run -p tankmini-cli -- identify                  # firmware string
cargo run -p tankmini-gui                              # GTK4 app
python3 tools/mvprobe.py read 4 0 1942                 # raw probe, no deps
python3 tools/mvprobe.py read 4 92 92                  # single preset (slot 1)
python3 tools/slots.py captures/presets                # all 21 presets to files
```

## Layout

```
crates/tankmini-core   protocol, transport, preset table, discovery, backups
crates/tankmini-cli    headless control (identify/dump/show/apply/restore/raw)
crates/tankmini-gui    GTK4 desktop app
docs/protocol          evidence-marked protocol notes + preset map
docs/specs             specs, written before implementation
tools                  Python probes and analysis helpers (stdlib only)
captures               raw dumps, APK, Blutter output (git-ignored)
```

## Reference material (not vendored)

- `../_tools/ref/irbox` — closest documented sibling device (same framing)
- `../_tools/blutter` — Dart AOT analysis; run
  `build/manual5/blutter_dartvm* -i captures/apk/lib/arm64-v8a/libapp.so -o captures/blutter_out`
  (needs capstone 5 in `~/opt/capstone5`)
- `../_tools/rea` — REA toolkit source (see the `mvave-pedal-usb-midi` skill)

## App under analysis

`M-EFCS` (official), package `com.digAmp.m_efcs`, Flutter (Dart AOT in
`lib/arm64-v8a/libapp.so`). Official APK, no store needed:
`https://resource.m-vave.com/software/app/M-EFCS.apk`. Recovered app structure
lives in `captures/blutter_out/asm/m_efcs/`: `protocol/` (frames, preset store,
flash writer) and `data/products/tank_mini/`.

## State of the reverse engineering

Transport, identity, table read/write, single-preset read/write, slot select and
the cab name table are verified. The per-parameter map (which byte is Gain, Mix,
Decay…) and the save-to-flash command are still open:
`docs/protocol/preset-map.md` lists exactly what is known and what is not.
