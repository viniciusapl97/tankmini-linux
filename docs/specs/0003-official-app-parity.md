# 0003 — What the official app does that we did not

Source: a walk through M-EFCS 3.9.2314 on a phone connected to the pedal
(2026-10-22) and the static notes in `docs/protocol/app-findings.md` §11–12.

## Done in this iteration

- **Pedal tuner** (type 4 `0x70100000`): the Tune screen switches it on while
  it is shown, sends the A4 reference (430–450 Hz) and a "mute while tuning"
  switch; it falls back to the pedal's note when there is no USB audio. CLI:
  `tankmini tune --pedal`. Note mapping verified with a guitar.
- **Official preset files**: `.tkm` (one preset, 92 bytes) and `.atkm` (all
  21, 1942 bytes). Export current, export all, import either.
- **Swap presets** (menu "Trocar de lugar com…").
- **IR import from WAV** (menu "Importar IR (cab)…"; CLI `tankmini cab
  audition` / `cab save --yes`): converts like the app, auditions through the
  type 5 buffer, saves to a slot after a confirmation and a backup of that slot
  in `~/.local/share/tankmini/cab-backups/`.

- **AMP model import** (`.am4data`, menu "Importar amp (.am4data)…"; CLI
  `tankmini amp audition` / `amp save --yes`): the LMT block is computed by
  our port of the app's GRU (bit exact); audition through the type 5 buffer
  (verified on the device: identical to the app's upload); save to an AMP slot
  after a confirmation and a backup in `~/.local/share/tankmini/amp-backups/`.

- **Community browser** (menu "Comunidade M-VAVE…"): search the vendor's
  public site, sorted by downloads or date, only importable posts by default;
  "Abrir" downloads the file to `~/.local/share/tankmini/community/<post>/`
  and opens the amp or IR import dialog. `core::community` behind the
  `community` feature (ureq), so the CLI stays offline.

## Waiting on the owner

- Ear test of the audition, IR and AMP (does the pedal play the buffer at
  once? does an AM4 model sound right on firmware 004, whose own slots hold
  AM2?).
  If not, drop the "Ouvir" button and look for the activation step.
- First real slot save (flash). Backup first; restore path = save the backup
  file back (`cab save` accepts only WAV today — add `cab restore` if needed).

## Backlog: UI ideas from the official app

| Idea | Where it would go | Notes |
|---|---|---|
| Signal chain as a strip of small tiles with arrows; click to jump to that block's card | Top of Edit | **Done**: the existing signal-path chips now scroll to the block and outline it (all three edit styles) |
| VOL as a wide bar with − / + | Play | **Done**: Volume left the knob row for a full-width bar (±5 per click); Drive, Brilho, Ambiente stay knobs |
| Rare actions in one "More" menu (share/import, swap, factory restore, tuner) | Header menu | We have the menu; keep the main screen clean |
| Swap dialog with two position pickers and an arrow between them | Dialogs | Ours picks one target from the current preset; fine for now |
| Search + Edit over an imported IR/AMP list | IR import | Worth it once people keep several IRs; we currently import one at a time |
| Tuner with a clear flat/sharp needle and ♭/♯ 50 scale | Tune | **Done**: drawn ±50-cent meter with a needle and an in-tune zone, ♭/♯ beside the note lighting up on the side it is off, "Abaixo do tom · aperte a corda" / "Acima do tom · afrouxe a corda", the six open strings with the current one lit, smoothed needle. Waiting on the owner's opinion |

Not worth porting: Audio separator (stem separation, unrelated to the pedal,
needs 6 GB RAM), Community (vendor forum behind a login).

## Not done

- AMP models: AM2/AM3 files are not supported (only AM4). The community has
  many `.am2data`, which is the format this pedal's own slots hold — the next
  thing to support (the app computes their LMT with `calc_bias_gru`, weights
  at +0x43c).
- NAM import: the official app converts on the vendor's server; not
  reproducible offline.
