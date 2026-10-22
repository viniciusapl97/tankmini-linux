# 0002 — Plug-and-play for beginners, full control for pros

Status: implemented (2026-10-22), pending: ear check of live edit, IR upload (needs go-ahead), BLE
Stack: Rust + GTK4 + libadwaita 1.7 API (system has 1.10), no WebView.
Supersedes the "out of scope" list of 0001 where noted.

## Goal

Two audiences, one app:

- **Beginner**: plugs a guitar, violão or bass into the pedal and wants to play.
  Never needs to know what a "preset", "chain" or "IR" is.
- **Advanced**: knows signal chains and wants every byte the pedal exposes,
  plus a library, setlists, undo and A/B.

## Design language

Dark-first, follows the system scheme when asked (Preferências → Aparência).
Solid near-black zinc surfaces, hairline translucent borders instead of
shadows, Adwaita Sans (Inter derivative) with slight negative tracking on
titles, pill buttons, **one** solid primary action per screen. Effect families
keep a colour, but only as a small accent (knob ring, dot), never as large
saturated areas. Body text is never coloured. Product/effect names (Chorus,
Delay, Hall, Gain, IR…) stay in English; prose is pt-BR.

## Information architecture

```
Header:   [◀ rail toggle]  TANK MINI  · status dot ·   [Tocar | Editar]  [⋯]
Rail:     search · Na pedaleira (21, grouped by bank) · Biblioteca · Setlists
Tocar:    big preset name + footswitch label, instrument chip, 4 macro knobs
          (Volume, Drive, Brilho, Ambiente), "Sons prontos" gallery,
          prev/next preset buttons.
Editar:   chain strip (drag to reorder), one card per block (on/off, variant,
          all knobs, copy/paste block), A/B toggle, undo/redo, bytes panel.
Bottom:   live indicator · dirty state · [Desfazer] · [Salvar na pedaleira]
```

## Features and acceptance

### Device layer

- D1 All USB I/O runs on a worker thread that owns the `Pedal`; the UI never
  blocks. Commands: Load, SelectSlot, Preview(slot, 92 B), Save(slot, 92 B),
  WriteTable, Commit. Consecutive Preview commands are coalesced (only the
  newest per slot is sent).
- D2 Hot-plug: the worker probes for the pedal every 1.5 s when disconnected,
  and polls the active slot every 1 s when connected. Unplug → UI shows
  "desconectada" without losing local edits; replug → reload, and local
  unsaved edits are kept (a banner offers to send them again).
- D3 Footswitch changes on the pedal show up in the app within ~1 s.
- D4 Permission diagnosis: when the rawmidi node exists but cannot be opened,
  the app says so and shows the exact fix (`sudo usermod -aG audio $USER`,
  re-login), instead of a generic error.

### Live edit (B1)

- Every edit of the **playing** preset is mirrored into the pedal's edit
  buffer (t4 `0x10000000`), field by field like the official app (VOL, type +
  its parameter block after a 100 ms settle, on/off, chain, single words),
  ~30 ms after the change. The stored table is never touched, nothing wears.
  Clicking a preset makes the pedal play it (and re-sends its unsaved edits).
- "Salvar na pedaleira" = backup → write → read-back → commit (unchanged from
  0001). Switching presets on the pedal discards RAM edits, so leaving the
  preset with unsaved edits asks first.
- Toggle in Preferências → Edição ao vivo (default on).
- Verified on the unit: buffer writes ACK, read back, and leave the table
  untouched (`examples/live_probe.rs`). **Open (needs ears)**: confirmation
  that the change is heard immediately.

### Beginner

- B2 First-run assistant (also from ⋯ → Assistente): instrument (guitarra /
  violão / baixo), how to connect (USB-C, phones/amp out, power), permission
  check, then lands on Tocar with a suggested sound.
- B3 Tocar mode: four macro knobs that map onto real parameters:
  Volume → preset VOL, Drive → amp Gain, Brilho → amp Treble,
  Ambiente → reverb Mix + delay Mix (and turns those blocks on/off).
- B4 "Sons prontos": built-in starting points per instrument (Limpo, Limpo
  com chorus, Crunch, Rock, High gain, Solo, Ambiente, Violão, Baixo limpo,
  Baixo com drive). Applying one writes it to the selected slot (live) and
  marks the slot dirty.
- B5 Plain-language tooltip/description for every block and parameter.
- B6 "Como usar a pedaleira" page with the footswitch map (bank × A/B/C).
- B7 Prev/next preset (also ←/→ keys) and a clear "tocando agora" state.

### Advanced

- A1 Undo/redo for every edit (Ctrl+Z / Ctrl+Shift+Z), per session.
- A2 A/B: hold-to-compare / toggle between the edited and the saved version
  of the preset (live when connected).
- A3 Copy/paste a single block between presets (variant + on/off + 6 params).
- A4 Drag-and-drop chain reordering (arrows stay for keyboard users).
- A5 "Mostrar todos os parâmetros": show the 6 raw slots of every block, even
  unnamed ones, with the byte offset in the tooltip.
- A6 Bytes panel: hexdump of the selected preset with the changed bytes
  highlighted against the saved version.
- A7 Local library (unlimited presets on the computer): save current preset
  with name, instrument, tags and notes; search; favourites; send to any slot.
  Stored as JSON under `~/.local/share/tankmini/library.json`.
- A8 Setlists: an ordered list of library presets written to consecutive
  slots starting at a chosen bank, with one backup + one commit.
- A9 Backup browser: list backups with date, note and slot, restore any of
  them (not only the newest), delete old ones.
- A10 Keyboard shortcuts window (Ctrl+?).
- A11 CLI `--json` output for `show`, `patch`, `backups`, `names` and a
  `library` subcommand (list / export / apply).

### Pedalboard view (added 2026-10-22, owner's request)

- P1 In Editar, each block is drawn as a stompbox in signal order: enclosure
  in the family colour, corner screws, status LED, black knobs over a printed
  scale (label + value silkscreened below), the model name as the logo
  (click → model list) and a metal footswitch that toggles the block.
  Off blocks are desaturated with the LED dark.
- P2 Order, move buttons and the block menu sit above each pedal.
- P3 Preferências → Aparência → "Visual dos blocos": Pedais realistas
  (default), Pedais minimalistas (pedal shape in the app style: surface card,
  family-coloured knobs, a switch) or Cartões (compact). Setting `edit_style`.

### App customisation (Preferências)

Theme (sistema / escuro / claro), live edit on/off, knob sensitivity, show
advanced panels, show all parameters, confirm before switching with unsaved
edits, default instrument.

Stored in `~/.config/tankmini/settings.json`.

### Globals (from `app-findings.md`)

- G1 Global bypass toggle (Ctrl+Shift+B), follows the pedal (polled).
- G2 USB loopback switch in Preferências → Avançado (write-only on the pedal).

## Not in this iteration (blocked or unsafe)

- **No such feature on this pedal** (per the app): master volume, input gain,
  global EQ, tuner, tap tempo, drums, looper. The per-preset VOL is the only
  level control.
- **Amp/IR upload**: the flow is described in `app-findings.md` §3 (type 5,
  preview at `0x0000`/`0x2000`, activate `0xA0000000`, save `0xF0000000+s`),
  but it writes flash and the cab sample encoding is not fully understood.
  Needs the owner's go-ahead before the first test.
- **BLE** (service AE40, chars AE41/AE42): a second transport over BlueZ;
  later.
- Firmware update: out — no recovery path is known.
- Real units: only cab Low Cut / High Cut have a documented scale (shown in
  Hz/kHz); everything else stays 0–100.

## Tests

- core: history (undo/redo bounds), library round-trip and search, setlist
  slot planning, macros (round-trip + clamping + which bytes they touch),
  templates (valid patches, chain is a permutation, units in range), block
  copy/paste, settings round-trip, backup metadata parsing, worker coalescing.
- manual on the unit: live edit audible, hot-plug, footswitch follow, save
  survives a power cycle (already verified in 0001), setlist write.
