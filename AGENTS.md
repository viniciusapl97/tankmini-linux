# AGENTS.md

Project: a native Linux control app for the **M-VAVE TANK MINI** guitar/bass
multi-effects pedal, built on a reverse-engineered USB-MIDI protocol instead of
the official M-EFCS app.

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
  `type 4`: `0x0000`–`0x0795` (table), `0x10000000`–`0x1000005b` (edit buffer),
  `0x20000000`–`0x20000003` (glob; byte 2 = bypass, writable), `0xE0000000+i`
  (select), `0xF0000000` (commit); `type 5`: `0x80000000`–`0x80000117` (cab
  names), `0x90000000`–`0x90000117` (amp names).
- Live edits go to the edit buffer (`0x10000000`), never to the stored table:
  that is what the official app does and it wears nothing.
- Amp/IR upload (type 5, `0xA0000000`, `0xF0000000+s`) writes flash and is
  only described in `docs/protocol/app-findings.md`; do not send it without
  the owner's go-ahead. Flash erase/factory restore paths (`0x70xxxxxx`, CMD
  `0x21`) are destructive: never.
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
cargo run -p tankmini-gui                              # GTK4 + libadwaita app
TANKMINI_DEMO=0 cargo run -p tankmini-gui              # no USB, factory table (UI work)
TANKMINI_INSTANCE=dev cargo run -p tankmini-gui        # second instance, own app id
cargo run -p tankmini-core --example live_probe        # hardware check of live edit
python3 tools/mvprobe.py read 4 0 1942                 # raw probe, no deps
python3 tools/mvprobe.py read 4 92 92                  # single preset (slot 1)
python3 tools/slots.py captures/presets                # all 21 presets to files
```

## Layout

```
crates/tankmini-core   protocol, transport, preset table, discovery, backups,
                       worker (USB thread), history, library, macros, templates
crates/tankmini-cli    headless control; `--json` for scripts
crates/tankmini-gui    GTK4 + libadwaita app: app.rs (model, worker events),
                       window.rs (shell), play.rs (Tocar), edit.rs + pedal.rs
                       (Editar: cards / pedals), rail.rs, dialogs.rs,
                       knob.rs (Cairo knobs), theme.rs + style.css (tokens)
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

Transport, identity, table read/write, single-preset read/write, slot select,
save to flash, both name tables, the edit buffer (live edit) and global bypass
are verified. Parameter names and order come from the app
(`docs/protocol/app-findings.md`). Still open: amp/IR upload on this unit,
glob bytes 1 and 3, whether the edit buffer survives a preset switch (the app
re-sends edits anyway).

## GUI rules

- UI strings in pt-BR; effect/product names stay as the pedal prints them
  (Gain, Chorus, dTape…).
- Visual language: near-black zinc surfaces, hairline borders, pill buttons,
  one solid CTA, family colours only as accents. Colours are `@tm_*` tokens
  defined per scheme in `theme.rs`; never hard-code UI colours in views
  (the stompbox enclosures are the one deliberate exception).
- USB only through `tankmini_core::worker` (never block the GTK thread).
  Live edits go through `App::edit`, which records undo, schedules the
  edit-buffer preview and defers view rebuilds to idle — never rebuild a
  container inside the handler of one of its widgets.
- The rawmidi port is exclusive: with the GUI open, the CLI cannot connect.
- Check UI changes visually: screenshots under Xvfb
  (`GDK_BACKEND=x11`, `TANKMINI_DEMO`, `TANKMINI_INSTANCE`) so a running copy
  on the owner's desktop is not activated.
