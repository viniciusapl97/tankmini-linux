//! Background owner of the USB connection.
//!
//! The GUI must never block on USB: every exchange can take up to a second
//! when the pedal is slow to ACK. A [`Worker`] thread owns the [`Pedal`],
//! executes [`Command`]s in order, watches for hot-plug and for preset changes
//! made with the footswitches, and reports back through [`Event`]s.
//!
//! Knob drags produce a burst of `Preview` commands; [`coalesce`] keeps only
//! the newest one per slot so the pedal follows the knob instead of lagging
//! behind a queue. A preview is sent the way the official app does it: only
//! the fields that changed, written into the pedal's edit buffer
//! ([`crate::ADDR_EDIT`]), never into the stored table ([`edit_writes`]).

use std::io;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::backup::BackupStore;
use crate::discovery::find_pedal;
use crate::patch::{PresetTable, PATCH_LEN};
use crate::Pedal;

const PROBE_EVERY: Duration = Duration::from_millis(1500);
const POLL_EVERY: Duration = Duration::from_millis(1000);
const TUNER_EVERY: Duration = Duration::from_millis(80);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Re-read identity, active slot, table and cab names.
    Reload,
    /// Make the pedal play `slot`. Discards RAM edits on the pedal.
    Select(u8),
    /// Make the playing preset sound like `bytes` (edit buffer only: nothing
    /// is stored, no flash wear). Ignored when `slot` is not the playing one.
    Preview { slot: u8, bytes: Vec<u8> },
    /// Global bypass (all effects off) on/off.
    SetBypass(bool),
    /// USB loopback on/off.
    SetLoopback(bool),
    /// Backup → write one preset → verify → commit to flash.
    Save { slot: u8, bytes: Vec<u8> },
    /// Backup → write the whole table → verify → optional commit.
    WriteTable { table: PresetTable, commit: bool, note: String },
    /// The pedal's own tuner (RAM controls only).
    Tuner(TunerControl),
    /// Put a CAB record ([`crate::ir::record`]) in the audition buffer (RAM).
    CabAudition(Vec<u8>),
    /// Back up CAB slot `slot`, then store `record` there (**flash**).
    CabSave { slot: u8, record: Vec<u8> },
    /// Put an AMP record ([`crate::amp::Model::record`]) in the buffer (RAM).
    AmpAudition(Vec<u8>),
    /// Back up AMP slot `slot`, then store `record` there (**flash**).
    AmpSave { slot: u8, record: Vec<u8> },
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunerControl {
    /// Start, and report [`Event::Tuner`] readings until `Off`.
    On,
    Off,
    /// Reference A4 in Hz (430..450).
    Reference(u16),
    /// Output volume while tuning, 0..100.
    BypassVolume(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disconnect {
    /// No pedal on USB.
    Absent,
    /// The rawmidi node exists but this user cannot open it.
    Permission(String),
    /// Another program holds the MIDI port (rawmidi is exclusive): usually a
    /// second copy of this app, or the CLI mid-command.
    Busy(String),
    /// The node opened but the pedal did not answer.
    NoReply(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Connected {
        firmware: String,
        node: String,
        table: PresetTable,
        active: Option<u8>,
        amp_names: Vec<String>,
        cab_names: Vec<String>,
        bypass: bool,
    },
    Disconnected(Disconnect),
    /// Global bypass changed (from us or from the pedal).
    Bypass(bool),
    /// The pedal now plays this slot (footswitch or our own select).
    ActiveSlot(u8),
    Previewed { slot: u8, ok: bool },
    /// `bytes` is what was written, so the caller never has to remember it.
    Saved { slot: u8, ok: bool, message: String, bytes: Vec<u8> },
    TableWritten { table: PresetTable, ok: bool, message: String },
    /// A USB exchange started (true) or finished (false).
    Busy(bool),
    /// A reading from the pedal's tuner, while it is on.
    Tuner(crate::tuner::Reading),
    /// Result of `CabAudition` (`slot` = None) or `CabSave`.
    Cab { slot: Option<u8>, ok: bool, message: String },
    /// The cab names after a slot was rewritten.
    CabNames(Vec<String>),
    /// Result of `AmpAudition` (`slot` = None) or `AmpSave`.
    Amp { slot: Option<u8>, ok: bool, message: String },
    /// The amp names after a slot was rewritten.
    AmpNames(Vec<String>),
}

/// One write into the edit buffer. `settle` asks for a pause afterwards: the
/// pedal loads the new variant's defaults when a module type changes, and our
/// own values must land after that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditWrite {
    pub offset: usize,
    pub data: Vec<u8>,
    pub settle: bool,
}

/// Field-by-field writes that turn the edit buffer `from` into `to`, in the
/// order the official app uses: VOL, module types (+ their whole parameter
/// block), on/off, chain, then single parameter words.
pub fn edit_writes(from: &[u8], to: &[u8]) -> Vec<EditWrite> {
    use crate::patch::{MODULE_COUNT, ON_OFFSET, PARAMS_PER_MODULE, PARAM_OFFSET, UNIT_OFFSET};
    let mut out = Vec::new();
    if from.len() != PATCH_LEN || to.len() != PATCH_LEN {
        return out;
    }
    let w = |offset: usize, len: usize, settle: bool| EditWrite { offset, data: to[offset..offset + len].to_vec(), settle };
    if from[0..2] != to[0..2] {
        out.push(w(0, 2, false));
    }
    let mut retyped = [false; MODULE_COUNT];
    for m in 0..MODULE_COUNT {
        if from[UNIT_OFFSET + m] != to[UNIT_OFFSET + m] {
            retyped[m] = true;
            out.push(w(UNIT_OFFSET + m, 1, true));
            let block = PARAM_OFFSET + m * PARAMS_PER_MODULE * 2;
            out.push(w(block, PARAMS_PER_MODULE * 2, false));
        }
    }
    for m in 0..MODULE_COUNT {
        if from[ON_OFFSET + m] != to[ON_OFFSET + m] {
            out.push(w(ON_OFFSET + m, 1, false));
        }
    }
    if from[2..8] != to[2..8] {
        out.push(w(2, 6, false));
    }
    for m in (0..MODULE_COUNT).filter(|m| !retyped[*m]) {
        for k in 0..PARAMS_PER_MODULE {
            let off = PARAM_OFFSET + (m * PARAMS_PER_MODULE + k) * 2;
            if from[off..off + 2] != to[off..off + 2] {
                out.push(w(off, 2, false));
            }
        }
    }
    out
}

/// Drop superseded commands: a `Preview` is useless when a later `Preview`
/// for the same slot or a whole-table write is queued, and a `Select` when a
/// later `Select` follows (holding Ctrl+→ should land on the last preset, not
/// walk through every one). A `Save` does not supersede a preview: whether a
/// table write reaches the edit buffer is not known. Order is preserved.
pub fn coalesce(cmds: Vec<Command>) -> Vec<Command> {
    let mut out = Vec::with_capacity(cmds.len());
    for (i, cmd) in cmds.iter().enumerate() {
        let later = &cmds[i + 1..];
        let superseded = match cmd {
            Command::Preview { slot, .. } => later.iter().any(|l| match l {
                Command::Preview { slot: s, .. } => s == slot,
                Command::WriteTable { .. } => true,
                _ => false,
            }),
            Command::Select(_) => later.iter().any(|l| matches!(l, Command::Select(_))),
            _ => false,
        };
        if !superseded {
            out.push(cmd.clone());
        }
    }
    out
}

pub struct Worker {
    tx: Sender<Command>,
    handle: Option<thread::JoinHandle<()>>,
}

/// How stale the polled active slot may be before a live edit re-checks it,
/// so a footswitch press never receives a diff meant for another preset.
const FRESH: Duration = Duration::from_millis(150);

impl Worker {
    /// Start the thread. `emit` is called from the worker thread.
    pub fn spawn(emit: impl Fn(Event) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let handle = thread::Builder::new()
            .name("tankmini-usb".into())
            .spawn(move || Loop::new(Box::new(emit)).run(rx))
            .expect("spawn worker thread");
        Self { tx, handle: Some(handle) }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

struct Loop {
    emit: Box<dyn Fn(Event) + Send>,
    pedal: Option<Pedal>,
    active: Option<u8>,
    bypass: Option<bool>,
    /// What the pedal's edit buffer holds, when known.
    shadow: Option<Vec<u8>>,
    last_probe: Option<Instant>,
    last_poll: Instant,
    last_disconnect: Option<Disconnect>,
    /// The tuner was asked on; it is (re)started after a reconnect.
    tuner: bool,
    last_tuner: Instant,
}

impl Loop {
    fn new(emit: Box<dyn Fn(Event) + Send>) -> Self {
        Self {
            emit,
            pedal: None,
            active: None,
            bypass: None,
            shadow: None,
            last_probe: None,
            last_poll: Instant::now(),
            last_disconnect: None,
            tuner: false,
            last_tuner: Instant::now(),
        }
    }

    fn run(mut self, rx: Receiver<Command>) {
        self.connect();
        loop {
            let wait = if self.tuner { TUNER_EVERY } else { Duration::from_millis(250) };
            let first = match rx.recv_timeout(wait) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => {
                    self.stop_tuner();
                    return;
                }
            };
            if let Some(first) = first {
                let mut batch = vec![first];
                batch.extend(rx.try_iter());
                for cmd in coalesce(batch) {
                    if cmd == Command::Shutdown {
                        self.stop_tuner();
                        return;
                    }
                    (self.emit)(Event::Busy(true));
                    let result = self.execute(cmd);
                    (self.emit)(Event::Busy(false));
                    if let Err(e) = result {
                        self.drop_connection(e);
                    }
                }
            }
            self.tick();
        }
    }

    fn tick(&mut self) {
        if self.pedal.is_none() {
            let due = self.last_probe.is_none_or(|t| t.elapsed() >= PROBE_EVERY);
            if due {
                self.connect();
            }
            return;
        }
        if self.tuner && self.last_tuner.elapsed() >= TUNER_EVERY {
            self.last_tuner = Instant::now();
            match self.pedal.as_mut().map(|p| p.tuner()) {
                Some(Ok(Some(r))) => (self.emit)(Event::Tuner(r)),
                Some(Err(e)) => {
                    self.drop_connection(e);
                    return;
                }
                _ => {}
            }
        }
        if self.last_poll.elapsed() >= POLL_EVERY {
            self.last_poll = Instant::now();
            let polled = self.pedal.as_mut().map(|p| p.glob());
            match polled {
                Some(Ok(Some(glob))) => self.take_glob(glob),
                Some(Ok(None)) => {}
                Some(Err(e)) => self.drop_connection(e),
                None => {}
            }
        }
    }

    /// Leave the pedal's tuner off, so the pedal never stays muted after we quit.
    fn stop_tuner(&mut self) {
        if self.tuner {
            self.tuner = false;
            if let Some(p) = self.pedal.as_mut() {
                let _ = p.tuner_write(crate::tuner::OFF_ENABLE, 0);
            }
        }
    }

    /// Apply a fresh glob read: active slot and bypass, announcing changes.
    fn take_glob(&mut self, glob: [u8; 4]) {
        let slot = glob[0];
        if (slot as usize) < crate::patch::PATCH_COUNT && self.active != Some(slot) {
            self.active = Some(slot);
            self.shadow = None;
            (self.emit)(Event::ActiveSlot(slot));
        }
        let bypass = glob[2] != 0;
        if self.bypass != Some(bypass) {
            self.bypass = Some(bypass);
            (self.emit)(Event::Bypass(bypass));
        }
    }

    fn drop_connection(&mut self, err: io::Error) {
        self.pedal = None;
        self.shadow = None;
        self.active = None;
        self.bypass = None;
        self.last_probe = Some(Instant::now());
        let reason = if find_pedal().is_some() {
            Disconnect::NoReply(err.to_string())
        } else {
            Disconnect::Absent
        };
        self.report_disconnect(reason);
    }

    fn report_disconnect(&mut self, reason: Disconnect) {
        if self.last_disconnect.as_ref() != Some(&reason) {
            self.last_disconnect = Some(reason.clone());
            (self.emit)(Event::Disconnected(reason));
        }
    }

    fn connect(&mut self) {
        self.last_probe = Some(Instant::now());
        let Some(device) = find_pedal() else {
            self.report_disconnect(Disconnect::Absent);
            return;
        };
        let node = device.midi_path.display().to_string();
        let mut pedal = match Pedal::open(&device.midi_path) {
            Ok(p) => p,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                self.report_disconnect(Disconnect::Permission(node));
                return;
            }
            Err(e) if e.raw_os_error() == Some(libc::EBUSY) => {
                self.report_disconnect(Disconnect::Busy(node));
                return;
            }
            Err(e) => {
                self.report_disconnect(Disconnect::NoReply(e.to_string()));
                return;
            }
        };
        match Self::read_all(&mut pedal) {
            Ok((firmware, active, table, amp_names, cab_names)) => {
                let active = active.filter(|a| (*a as usize) < crate::patch::PATCH_COUNT);
                let bypass = pedal.glob().ok().flatten().is_some_and(|g| g[2] != 0);
                self.bypass = Some(bypass);
                self.active = active;
                self.shadow = None;
                self.last_disconnect = None;
                self.last_poll = Instant::now();
                if self.tuner {
                    let _ = pedal.tuner_write(crate::tuner::OFF_ENABLE, 1);
                }
                self.pedal = Some(pedal);
                (self.emit)(Event::Connected { firmware, node, table, active, amp_names, cab_names, bypass });
            }
            Err(e) => self.report_disconnect(Disconnect::NoReply(e)),
        }
    }

    #[allow(clippy::type_complexity)]
    fn read_all(pedal: &mut Pedal) -> Result<(String, Option<u8>, PresetTable, Vec<String>, Vec<String>), String> {
        let firmware = pedal
            .identify()
            .map_err(|e| e.to_string())?
            .map(|i| i.firmware)
            .ok_or("a pedaleira não respondeu à identificação")?;
        let active = pedal.active_slot().map_err(|e| e.to_string())?;
        let table = pedal
            .read_table()
            .map_err(|e| e.to_string())?
            .ok_or("a pedaleira não devolveu a tabela de presets")?;
        let amps = pedal.amp_names().unwrap_or_default();
        let cabs = pedal.cab_names().unwrap_or_default();
        Ok((firmware, active, table, amps, cabs))
    }

    fn execute(&mut self, cmd: Command) -> io::Result<()> {
        if self.pedal.is_none() {
            let offline = "pedaleira desconectada".to_string();
            match cmd {
                Command::Reload => self.connect(),
                Command::Save { slot, bytes } => (self.emit)(Event::Saved { slot, ok: false, message: offline, bytes }),
                Command::WriteTable { table, .. } => {
                    (self.emit)(Event::TableWritten { table, ok: false, message: offline })
                }
                // Remembered, so the tuner starts as soon as the pedal is back.
                Command::Tuner(TunerControl::On) => self.tuner = true,
                Command::Tuner(TunerControl::Off) => self.tuner = false,
                Command::CabAudition(_) => {
                    (self.emit)(Event::Cab { slot: None, ok: false, message: offline })
                }
                Command::CabSave { slot, .. } => {
                    (self.emit)(Event::Cab { slot: Some(slot), ok: false, message: offline })
                }
                Command::AmpAudition(_) => {
                    (self.emit)(Event::Amp { slot: None, ok: false, message: offline })
                }
                Command::AmpSave { slot, .. } => {
                    (self.emit)(Event::Amp { slot: Some(slot), ok: false, message: offline })
                }
                _ => {}
            }
            return Ok(());
        }
        match cmd {
            Command::Reload => {
                self.pedal = None;
                self.connect();
            }
            Command::Select(slot) => {
                let pedal = self.pedal.as_mut().expect("checked");
                if pedal.select_slot(slot)? {
                    thread::sleep(Duration::from_millis(60));
                    self.active = Some(slot);
                    self.shadow = None;
                    (self.emit)(Event::ActiveSlot(slot));
                }
            }
            Command::SetBypass(on) => {
                let pedal = self.pedal.as_mut().expect("checked");
                if pedal.set_bypass(on)? {
                    self.bypass = Some(on);
                }
                // Always report the real state so an optimistic UI snaps back
                // when the pedal refuses.
                (self.emit)(Event::Bypass(self.bypass.unwrap_or(false)));
            }
            Command::SetLoopback(on) => {
                let pedal = self.pedal.as_mut().expect("checked");
                pedal.set_loopback(on)?;
            }
            Command::Preview { slot, bytes } => {
                if self.last_poll.elapsed() > FRESH {
                    self.last_poll = Instant::now();
                    let glob = self.pedal.as_mut().expect("checked").glob()?;
                    if let Some(glob) = glob {
                        self.take_glob(glob);
                    }
                }
                if self.active != Some(slot) || bytes.len() != PATCH_LEN {
                    return Ok(());
                }
                let pedal = self.pedal.as_mut().expect("checked");
                let current = match self.shadow.take() {
                    Some(s) => Some(s),
                    None => pedal.read_edit()?,
                };
                let Some(current) = current else {
                    (self.emit)(Event::Previewed { slot, ok: false });
                    return Ok(());
                };
                let mut ok = true;
                for w in edit_writes(&current, &bytes) {
                    if pedal.write_edit(w.offset, &w.data)? != 0 {
                        ok = false;
                        break;
                    }
                    if w.settle {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
                // On a refused write the buffer is in an unknown state: re-read
                // it before the next preview.
                self.shadow = ok.then_some(bytes);
                (self.emit)(Event::Previewed { slot, ok });
            }
            Command::Save { slot, bytes } => {
                let pedal = self.pedal.as_mut().expect("checked");
                let result = save_patch(pedal, slot, &bytes);
                self.shadow = None;
                let (ok, message) = match result {
                    Ok(r) => r,
                    Err(e) => {
                        (self.emit)(Event::Saved { slot, ok: false, message: e.to_string(), bytes });
                        return Err(e);
                    }
                };
                (self.emit)(Event::Saved { slot, ok, message, bytes });
            }
            Command::WriteTable { table, commit, note } => {
                let pedal = self.pedal.as_mut().expect("checked");
                let result = write_table(pedal, &table, commit, &note);
                self.shadow = None;
                let (ok, message) = match result {
                    Ok(r) => r,
                    Err(e) => {
                        (self.emit)(Event::TableWritten { table, ok: false, message: e.to_string() });
                        return Err(e);
                    }
                };
                (self.emit)(Event::TableWritten { table, ok, message });
            }
            Command::Tuner(control) => {
                use crate::tuner::{reference_byte, OFF_BYPASS_VOLUME, OFF_ENABLE, OFF_REFERENCE};
                let pedal = self.pedal.as_mut().expect("checked");
                match control {
                    TunerControl::On => {
                        pedal.tuner_write(OFF_ENABLE, 1)?;
                        self.tuner = true;
                    }
                    TunerControl::Off => {
                        pedal.tuner_write(OFF_ENABLE, 0)?;
                        self.tuner = false;
                    }
                    TunerControl::Reference(hz) => {
                        pedal.tuner_write(OFF_REFERENCE, reference_byte(hz))?;
                    }
                    TunerControl::BypassVolume(v) => {
                        pedal.tuner_write(OFF_BYPASS_VOLUME, v.min(100))?;
                    }
                }
            }
            Command::CabAudition(record) => {
                let ok = self.pedal.as_mut().expect("checked").cab_upload(&record)?;
                let message = if ok { "IR enviado para a pedaleira" } else { "a pedaleira não confirmou o IR" };
                (self.emit)(Event::Cab { slot: None, ok, message: message.into() });
            }
            Command::CabSave { slot, record } => {
                let pedal = self.pedal.as_mut().expect("checked");
                let (ok, message) = match save_cab(pedal, slot, &record)? {
                    Ok(path) => (true, format!("IR gravado no cab {} (backup em {})", slot + 1, path.display())),
                    Err(e) => (false, e),
                };
                (self.emit)(Event::Cab { slot: Some(slot), ok, message });
                if ok {
                    let names = self.pedal.as_mut().expect("checked").cab_names()?;
                    (self.emit)(Event::CabNames(names));
                }
            }
            Command::AmpAudition(record) => {
                let ok = self.pedal.as_mut().expect("checked").amp_upload(&record)?;
                let message = if ok { "amp enviado para a pedaleira" } else { "a pedaleira não confirmou o amp" };
                (self.emit)(Event::Amp { slot: None, ok, message: message.into() });
            }
            Command::AmpSave { slot, record } => {
                let pedal = self.pedal.as_mut().expect("checked");
                let (ok, message) = match save_amp(pedal, slot, &record)? {
                    Ok(path) => (true, format!("amp gravado no lugar {} (backup em {})", slot + 1, path.display())),
                    Err(e) => (false, e),
                };
                (self.emit)(Event::Amp { slot: Some(slot), ok, message });
                if ok {
                    let names = self.pedal.as_mut().expect("checked").amp_names()?;
                    (self.emit)(Event::AmpNames(names));
                }
            }
            Command::Shutdown => {}
        }
        Ok(())
    }
}

/// Where CAB slot backups go (one 4096-byte file per save).
pub fn cab_backup_dir() -> std::path::PathBuf {
    crate::paths::data_dir().join("cab-backups")
}

/// Where AMP slot backups go (one 8192-byte file per save).
pub fn amp_backup_dir() -> std::path::PathBuf {
    crate::paths::data_dir().join("amp-backups")
}

fn stamp() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn save_amp(pedal: &mut Pedal, slot: u8, record: &[u8]) -> io::Result<Result<std::path::PathBuf, String>> {
    let Some(previous) = pedal.amp_slot(slot)? else {
        return Ok(Err("não consegui ler o amp atual para o backup; nada foi gravado".into()));
    };
    let dir = amp_backup_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("amp{slot:02}-{}.bin", stamp()));
    std::fs::write(&path, previous)?;
    if pedal.amp_save(slot, record)? {
        Ok(Ok(path))
    } else {
        Ok(Err(format!("a gravação não conferiu (backup em {})", path.display())))
    }
}

/// Keep a copy of the slot, then save. The inner error is for the user.
fn save_cab(pedal: &mut Pedal, slot: u8, record: &[u8]) -> io::Result<Result<std::path::PathBuf, String>> {
    let Some(previous) = pedal.cab_slot(slot)? else {
        return Ok(Err("não consegui ler o cab atual para o backup; nada foi gravado".into()));
    };
    let dir = cab_backup_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("cab{slot:02}-{}.bin", stamp()));
    std::fs::write(&path, previous)?;
    if pedal.cab_save(slot, record)? {
        Ok(Ok(path))
    } else {
        Ok(Err(format!("a gravação não conferiu (backup em {})", path.display())))
    }
}

fn backup_current(pedal: &mut Pedal, slot: Option<u8>, note: &str) -> io::Result<()> {
    if let Some(current) = pedal.read_table()? {
        BackupStore::new(BackupStore::default_root()).save(&current.to_preset(), slot, note)?;
    }
    Ok(())
}

fn save_patch(pedal: &mut Pedal, slot: u8, bytes: &[u8]) -> io::Result<(bool, String)> {
    if bytes.len() != PATCH_LEN {
        return Ok((false, "preset com tamanho inválido".into()));
    }
    backup_current(pedal, Some(slot), "antes de salvar")?;
    let ack = pedal.write_patch(slot, bytes)?;
    thread::sleep(Duration::from_millis(120));
    let verified = pedal.read_patch(slot)?.as_deref() == Some(bytes);
    if ack != 0 || !verified {
        return Ok((false, format!("a pedaleira não confirmou a gravação (ack 0x{ack:02x})")));
    }
    match pedal.commit()? {
        Some(0) => Ok((true, "salvo na memória da pedaleira".into())),
        other => Ok((false, format!("gravado, mas o commit na memória falhou ({other:?})"))),
    }
}

fn write_table(pedal: &mut Pedal, table: &PresetTable, commit: bool, note: &str) -> io::Result<(bool, String)> {
    backup_current(pedal, None, note)?;
    let report = pedal.write_table(table)?;
    if !report.verified {
        return Ok((
            false,
            format!("gravação não conferiu ({}/{} blocos confirmados)", report.chunks_ok, report.chunks),
        ));
    }
    if !commit {
        return Ok((true, "gravado (sem salvar na memória)".into()));
    }
    match pedal.commit()? {
        Some(0) => Ok((true, "gravado e salvo na memória da pedaleira".into())),
        other => Ok((false, format!("gravado, mas o commit na memória falhou ({other:?})"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(slot: u8, v: u8) -> Command {
        Command::Preview { slot, bytes: vec![v; PATCH_LEN] }
    }

    #[test]
    fn edit_writes_touch_only_changed_fields() {
        let base = PresetTable::factory();
        let from = base.patch(2).unwrap().raw().to_vec();
        assert!(edit_writes(&from, &from).is_empty());

        let mut t = base.clone();
        t.set_param(2, 4, 1, 30); // reverb mix
        t.set_vol(2, 50);
        let to = t.patch(2).unwrap().raw().to_vec();
        let w = edit_writes(&from, &to);
        assert_eq!(
            w,
            vec![
                EditWrite { offset: 0, data: vec![50, 0], settle: false },
                EditWrite { offset: 0x46, data: vec![30, 0], settle: false },
            ]
        );
    }

    #[test]
    fn a_new_variant_rewrites_its_block_after_settling() {
        let base = PresetTable::factory();
        let from = base.patch(2).unwrap().raw().to_vec();
        let mut t = base.clone();
        t.set_unit(2, 3, 2); // delay → dTape
        t.set_param(2, 3, 0, 10);
        t.set_on(2, 3, false);
        t.move_chain_position(2, 0, 5);
        let to = t.patch(2).unwrap().raw().to_vec();
        let w = edit_writes(&from, &to);
        assert_eq!(w[0], EditWrite { offset: 0x11, data: vec![2], settle: true });
        assert_eq!(w[1].offset, 0x14 + 3 * 12);
        assert_eq!(w[1].data.len(), 12);
        assert_eq!(w[2], EditWrite { offset: 0x0b, data: vec![0], settle: false });
        assert_eq!(w[3].offset, 2);
        assert_eq!(w.len(), 4, "the delay params ride along with its block");
        // Applying the writes to `from` gives `to`.
        let mut applied = from.clone();
        for x in &w {
            applied[x.offset..x.offset + x.data.len()].copy_from_slice(&x.data);
        }
        assert_eq!(applied, to);
    }

    #[test]
    fn keeps_only_the_newest_preview_per_slot() {
        let cmds = vec![preview(2, 1), preview(3, 1), preview(2, 2), preview(2, 3)];
        assert_eq!(coalesce(cmds), vec![preview(3, 1), preview(2, 3)]);
    }

    #[test]
    fn table_writes_supersede_previews_and_selects_collapse() {
        let save = Command::Save { slot: 2, bytes: vec![9; PATCH_LEN] };
        let cmds = vec![preview(2, 1), Command::Select(2), preview(4, 1), save.clone()];
        assert_eq!(coalesce(cmds.clone()), cmds, "a save keeps the preview before it");

        let cmds = vec![Command::Select(1), preview(1, 1), Command::Select(2), Command::Select(3)];
        assert_eq!(coalesce(cmds), vec![preview(1, 1), Command::Select(3)]);

        let table = Command::WriteTable { table: PresetTable::factory(), commit: false, note: String::new() };
        let cmds = vec![preview(1, 1), table.clone(), preview(1, 2)];
        assert_eq!(coalesce(cmds), vec![table, preview(1, 2)]);
    }
}
