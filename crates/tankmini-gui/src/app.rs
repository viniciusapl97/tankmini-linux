//! Application state and the glue between the UI and the USB worker.
//!
//! Model: `table` is what the user is editing, `saved` is what the pedal has
//! in flash (as far as we know). A preset is "dirty" when the two differ.
//! Clicking a preset makes the pedal play it; with live edit on, the edited
//! bytes of the playing preset are mirrored into the pedal's RAM so every knob
//! is heard immediately, and nothing reaches flash until "Salvar".

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::glib;
use tankmini_core::catalog::{self, Instrument, Role};
use tankmini_core::history::History;
use tankmini_core::library::Library;
use tankmini_core::names::PresetNames;
use tankmini_core::patch::{Block, Patch, PresetTable, PATCH_COUNT, SLOT_LABELS};
use tankmini_core::settings::Settings;
use tankmini_core::worker::{Command, Disconnect, Event, Worker};

use crate::window::Ui;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conn {
    Searching,
    Online,
    Offline(Disconnect),
}

pub struct State {
    pub table: Option<PresetTable>,
    pub saved: Option<PresetTable>,
    pub selected: usize,
    pub active: Option<u8>,
    pub firmware: String,
    pub node: String,
    pub amp_names: Vec<String>,
    pub cab_names: Vec<String>,
    pub conn: Conn,
    pub busy: bool,
    pub bypass: bool,
    /// USB loopback as last set by us (the pedal cannot be asked).
    pub loopback: bool,
    pub names: PresetNames,
    pub library: Library,
    pub library_error: Option<String>,
    pub settings: Settings,
    pub history: History<PresetTable>,
    pub clipboard: Option<Block>,
    /// A/B: the pedal currently plays the saved version.
    pub comparing: bool,
    /// A select we sent and the pedal has not confirmed yet: older
    /// `ActiveSlot` reports must not pull the selection back.
    pub pending_select: Option<(u8, Instant)>,
    /// The next whole-table write replaces local edits too (factory, backup).
    pub replace_on_write: bool,
    /// The last save/table write failed (the close-and-save flow stops).
    pub write_failed: bool,
    pub view: View,
    pub rail: RailTab,
    pub search: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Play,
    Edit,
    Tune,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailTab {
    Pedal,
    Library,
    Setlists,
}

impl State {
    pub fn patch(&self, i: usize) -> Option<Patch<'_>> {
        self.table.as_ref()?.patch(i)
    }

    pub fn current(&self) -> Option<Patch<'_>> {
        self.patch(self.selected)
    }

    pub fn is_dirty(&self, i: usize) -> bool {
        match (self.table.as_ref().and_then(|t| t.patch(i)), self.saved.as_ref().and_then(|t| t.patch(i))) {
            (Some(a), Some(b)) => a.raw() != b.raw(),
            _ => false,
        }
    }

    pub fn dirty_slots(&self) -> Vec<usize> {
        (0..PATCH_COUNT).filter(|i| self.is_dirty(*i)).collect()
    }

    pub fn online(&self) -> bool {
        self.conn == Conn::Online
    }

    pub fn playing(&self, i: usize) -> bool {
        self.online() && self.active.map(usize::from) == Some(i)
    }

    /// Display name: the local name, or the amp model when unnamed.
    pub fn title(&self, i: usize) -> String {
        if let Some(name) = self.names.get(i) {
            return name.to_string();
        }
        match self.patch(i) {
            Some(p) if p.is_on(1) => self.effect(&p, 1),
            Some(p) => {
                let fx = self.effects_line(&p);
                if fx.is_empty() { "Sem efeitos".to_string() } else { fx }
            }
            None => SLOT_LABELS[i].to_string(),
        }
    }

    /// The active effects other than the amp: "Boost · Chorus · Hall".
    pub fn effects_line(&self, p: &Patch<'_>) -> String {
        let parts: Vec<String> = p
            .chain()
            .iter()
            .map(|m| *m as usize)
            .filter(|m| *m != 1 && *m != 5 && p.is_on(*m))
            .map(|m| self.effect(p, m))
            .collect();
        parts.join(" · ")
    }

    /// Second line under a title: what else is in the preset.
    pub fn subtitle(&self, i: usize) -> String {
        let Some(p) = self.patch(i) else { return String::new() };
        let fx = self.effects_line(&p);
        let head = if self.names.get(i).is_some() && p.is_on(1) {
            self.effect(&p, 1)
        } else {
            String::new()
        };
        let parts: Vec<String> = [head, fx].into_iter().filter(|s| !s.is_empty()).collect();
        if parts.is_empty() { "Sem efeitos ligados".to_string() } else { parts.join(" · ") }
    }

    /// "FdPrJunir · Chorus · Hall" — amp model plus the active effects.
    pub fn describe(&self, p: &Patch<'_>) -> String {
        let mut parts = Vec::new();
        if p.is_on(1) {
            parts.push(self.effect(p, 1));
        } else {
            parts.push("Sem amp".to_string());
        }
        for m in [0usize, 2, 3, 4] {
            if p.is_on(m) {
                parts.push(self.effect(p, m));
            }
        }
        parts.join(" · ")
    }

    /// Name of the effect in module `m` (device names for amp and cab).
    pub fn effect(&self, p: &Patch<'_>, m: usize) -> String {
        p.effect_name(m, &self.amp_names, &self.cab_names)
    }

    /// Name of model `i` of an amp or cab module.
    pub fn model_name(&self, role: Role, i: usize) -> String {
        let names = if role == Role::Amp { &self.amp_names } else { &self.cab_names };
        names.get(i).cloned().unwrap_or_else(|| format!("{} {}", role.label(), i + 1))
    }

    pub fn instrument(&self, i: usize) -> Instrument {
        self.patch(i).map(|p| catalog::instrument_for_patch(&p)).unwrap_or(Instrument::Guitar)
    }

    pub fn param_label(&self, role: Role, unit: u8, index: usize) -> Option<String> {
        let raw = catalog::param_name(role, unit, index)?;
        Some(if self.settings.english_param_names {
            raw.to_string()
        } else {
            catalog::display_name(raw).to_string()
        })
    }
}

pub struct App {
    pub state: RefCell<State>,
    pub ui: OnceCell<Ui>,
    pub window: adw::ApplicationWindow,
    worker: RefCell<Option<Worker>>,
    preview_timer: RefCell<Option<glib::SourceId>>,
    refresh_queued: Cell<bool>,
}

pub type AppRef = Rc<App>;

/// How much of the UI an edit invalidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redraw {
    /// Values only (a knob moved): marks, bytes and action bar.
    Values,
    /// Structure (variant, chain, on/off, whole preset): rebuild the views.
    Views,
}

impl App {
    pub fn new(window: adw::ApplicationWindow) -> AppRef {
        let library = Library::load(Library::default_path());
        let (library, library_error) = match library {
            Ok(l) => (l, None),
            Err(e) => (Library::default(), Some(e)),
        };
        let settings = Settings::load_default();
        let view = if settings.view == "edit" { View::Edit } else { View::Play };
        Rc::new(App {
            state: RefCell::new(State {
                table: None,
                saved: None,
                selected: 0,
                active: None,
                firmware: String::new(),
                node: String::new(),
                amp_names: Vec::new(),
                cab_names: Vec::new(),
                conn: Conn::Searching,
                busy: false,
                bypass: false,
                loopback: false,
                names: PresetNames::load_default(),
                library,
                library_error,
                settings,
                history: History::new(300),
                clipboard: None,
                comparing: false,
                pending_select: None,
                replace_on_write: false,
                write_failed: false,
                view,
                rail: RailTab::Pedal,
                search: String::new(),
            }),
            ui: OnceCell::new(),
            window,
            worker: RefCell::new(None),
            preview_timer: RefCell::new(None),
            refresh_queued: Cell::new(false),
        })
    }

    pub fn ui(&self) -> &Ui {
        self.ui.get().expect("ui built")
    }

    /// Start the USB worker and route its events to the main loop.
    pub fn start(self: &AppRef) {
        if std::env::var_os("TANKMINI_DEMO").is_some() {
            self.start_demo();
            return;
        }
        let (tx, rx) = async_channel::unbounded::<Event>();
        let worker = Worker::spawn(move |ev| {
            let _ = tx.send_blocking(ev);
        });
        *self.worker.borrow_mut() = Some(worker);
        let app = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(ev) = rx.recv().await {
                app.on_event(ev);
            }
        });
    }

    /// `TANKMINI_DEMO=1`: no USB, the factory table and the name tables of
    /// firmware TANK MINI_004. For screenshots and UI work while the pedal is
    /// busy or absent. Commands go nowhere.
    fn start_demo(self: &AppRef) {
        const AMPS: [&str; 20] = [
            "UWE-Twins", "UK-C30Normal", "MarsVM410", "Victor_Mars", "BritPLEX_100", "MarsFD100",
            "Eagle_SAVAGE", "TH_DieselHgn", "PvEV5150", "FORTIN_CALI", "MESS_MktCln2", "CA-tweed",
            "BogSV20", "JUICE_JIM", "Sur_SL68", "BassADAtube", "BassAlmbic", "BassG800K_7",
            "BassTR-Trad", "BassMarkT501",
        ];
        const CABS: [&str; 20] = [
            "EAGLProV30s", "Sperimental", "JUICE4x12V30", "Mess Bog", "FdChamp", "FdPrJunir",
            "Mar960BV30", "DizzlV30", "Electrovoice", "MessDRectV30", "TwinJensenC", "TwedDlx1X12",
            "FendShowman", "J120Rolnd", "UK-C30Silvers", "BassAgula25", "BassJensen10",
            "BassFdBassman", "BassAmpg410", "BassEDN300",
        ];
        self.on_event(Event::Connected {
            firmware: "DEMO".into(),
            node: "(demo, sem USB)".into(),
            table: PresetTable::factory(),
            active: Some(
                std::env::var("TANKMINI_DEMO").ok().and_then(|v| v.parse().ok()).filter(|v| *v < 21).unwrap_or(2),
            ),
            amp_names: AMPS.iter().map(|s| s.to_string()).collect(),
            cab_names: CABS.iter().map(|s| s.to_string()).collect(),
            bypass: false,
        });
    }

    pub fn send(&self, cmd: Command) {
        if let Some(w) = self.worker.borrow().as_ref() {
            w.send(cmd);
        }
    }

    /// Stop the USB thread, letting a write in progress finish first.
    pub fn shutdown(&self) {
        let worker = self.worker.borrow_mut().take();
        drop(worker);
    }

    pub fn toast(&self, text: &str) {
        let toast = adw::Toast::new(text);
        toast.set_timeout(3);
        self.ui().toasts.add_toast(toast);
    }

    pub fn toast_undo(self: &AppRef, text: &str) {
        let toast = adw::Toast::new(text);
        toast.set_button_label(Some("Desfazer"));
        toast.set_timeout(5);
        let app = self.clone();
        toast.connect_button_clicked(move |_| app.undo());
        self.ui().toasts.add_toast(toast);
    }

    fn on_event(self: &AppRef, ev: Event) {
        match ev {
            Event::Connected { firmware, node, table, active, amp_names, cab_names, bypass } => {
                let first = {
                    let mut st = self.state.borrow_mut();
                    let first = st.table.is_none();
                    // Keep local unsaved edits across a replug.
                    let merged = match (&st.table, &st.saved) {
                        (Some(local), Some(old)) => {
                            let mut m = table.clone();
                            for i in 0..PATCH_COUNT {
                                let (l, o) = (local.patch(i).unwrap(), old.patch(i).unwrap());
                                if l.raw() != o.raw() {
                                    m.set_patch_raw(i, l.raw());
                                }
                            }
                            m
                        }
                        _ => table.clone(),
                    };
                    st.table = Some(merged);
                    st.saved = Some(table);
                    st.firmware = firmware;
                    st.node = node;
                    st.amp_names = amp_names;
                    st.bypass = bypass;
                    st.loopback = false;
                    st.cab_names = cab_names;
                    st.active = active;
                    st.conn = Conn::Online;
                    st.comparing = false;
                    st.pending_select = None;
                    if let Some(a) = active {
                        st.selected = a as usize;
                    }
                    if first {
                        st.history.clear();
                    }
                    first
                };
                if !first && self.state.borrow().is_dirty(self.state.borrow().selected) {
                    self.toast("Pedaleira reconectada. Suas alterações continuam aqui.");
                    self.schedule_preview();
                }
                self.refresh_all();
                if first {
                    crate::rail::refresh(self);
                    crate::dialogs::maybe_onboard(self);
                }
            }
            Event::Disconnected(reason) => {
                self.state.borrow_mut().conn = Conn::Offline(reason);
                self.refresh_all();
            }
            Event::ActiveSlot(slot) => {
                let stale = {
                    let mut st = self.state.borrow_mut();
                    match st.pending_select {
                        Some((want, at)) if want != slot && at.elapsed() < Duration::from_secs(2) => true,
                        Some(_) => {
                            st.pending_select = None;
                            false
                        }
                        None => false,
                    }
                };
                if stale {
                    self.state.borrow_mut().active = Some(slot);
                    self.refresh_marks();
                    return;
                }
                let follow = {
                    let mut st = self.state.borrow_mut();
                    st.active = Some(slot);
                    st.comparing = false;
                    let follow = st.selected != slot as usize;
                    st.selected = slot as usize;
                    follow
                };
                // The pedal reloads from flash on every switch: put the
                // unsaved edits of this preset back into its RAM.
                if self.state.borrow().is_dirty(slot as usize) {
                    self.schedule_preview();
                }
                if follow {
                    self.state.borrow_mut().history.seal();
                    self.refresh_all();
                } else {
                    self.refresh_marks();
                    crate::window::refresh_chrome(self);
                }
            }
            Event::Previewed { ok, .. } => {
                if !ok {
                    self.toast("A pedaleira recusou a prévia ao vivo.");
                }
            }
            Event::Saved { slot, ok, message, bytes } => {
                self.state.borrow_mut().write_failed = !ok;
                if ok {
                    if let Some(saved) = self.state.borrow_mut().saved.as_mut() {
                        saved.set_patch_raw(slot as usize, &bytes);
                    }
                    self.toast(&format!("{} salvo na pedaleira.", SLOT_LABELS[slot as usize]));
                } else {
                    self.toast(&format!("Não salvou {}: {message}", SLOT_LABELS[slot as usize]));
                }
                self.refresh_marks();
            }
            Event::TableWritten { table, ok, message } => {
                {
                    let mut guard = self.state.borrow_mut();
                    let st = &mut *guard;
                    st.write_failed = !ok;
                    let replace = std::mem::take(&mut st.replace_on_write);
                    if ok {
                        // Slots this write changed take the written bytes, unless
                        // the user edited them further while it ran (or before a
                        // setlist write): those stay local and dirty.
                        if let (Some(local), Some(old)) = (st.table.as_mut(), st.saved.as_ref()) {
                            for i in 0..PATCH_COUNT {
                                let written = table.patch(i).unwrap().raw().to_vec();
                                let before = old.patch(i).unwrap().raw().to_vec();
                                let mine = local.patch(i).unwrap().raw().to_vec();
                                if replace || (written != before && (mine == before || mine == written)) {
                                    local.set_patch_raw(i, &written);
                                }
                            }
                        }
                        st.saved = Some(table);
                    }
                }
                self.toast(&if ok {
                    format!("Pedaleira atualizada: {message}.")
                } else {
                    format!("Falhou: {message}")
                });
                self.refresh_all();
            }
            Event::Bypass(on) => {
                if self.state.borrow().bypass == on {
                    return;
                }
                self.state.borrow_mut().bypass = on;
                crate::window::refresh_actions(self);
                crate::window::refresh_status(self);
            }
            Event::Busy(b) => {
                self.state.borrow_mut().busy = b;
                crate::window::refresh_status(self);
            }
        }
    }

    // ---- editing -------------------------------------------------------

    /// Apply an edit to the selected preset with undo, live preview and redraw.
    /// `group` merges a continuous gesture (one knob drag) into one undo step.
    pub fn edit(self: &AppRef, group: Option<&str>, redraw: Redraw, f: impl FnOnce(&mut PresetTable, usize)) {
        let slot = self.state.borrow().selected;
        self.edit_at(slot, group, redraw, f);
    }

    /// Like [`Self::edit`] for an explicit preset (a deferred handler must not
    /// apply to whatever is selected by the time it runs).
    pub fn edit_at(
        self: &AppRef,
        slot: usize,
        group: Option<&str>,
        redraw: Redraw,
        f: impl FnOnce(&mut PresetTable, usize),
    ) {
        {
            let mut guard = self.state.borrow_mut();
            let st = &mut *guard;
            let Some(table) = st.table.as_mut() else { return };
            st.history.record(table, group);
            f(table, slot);
            st.comparing = false;
        }
        self.schedule_preview();
        self.refresh_marks();
        if redraw == Redraw::Views {
            // Never rebuild inside the handler of the widget that changed.
            self.queue_refresh();
        }
    }

    /// Rebuild the views on the next idle (coalesced).
    pub fn queue_refresh(self: &AppRef) {
        if self.refresh_queued.replace(true) {
            return;
        }
        let app = self.clone();
        glib::idle_add_local_once(move || {
            app.refresh_queued.set(false);
            app.refresh_all();
        });
    }

    /// End of a gesture: the next change starts a new undo step.
    pub fn seal(&self) {
        self.state.borrow_mut().history.seal();
    }

    pub fn undo(self: &AppRef) {
        let ok = {
            let mut guard = self.state.borrow_mut();
            let st = &mut *guard;
            match st.table.as_ref().and_then(|t| st.history.undo(t)) {
                Some(prev) => {
                    st.table = Some(prev);
                    true
                }
                None => false,
            }
        };
        if ok {
            self.preview_all_dirty_or_selected();
            self.refresh_all();
        }
    }

    pub fn redo(self: &AppRef) {
        let ok = {
            let mut guard = self.state.borrow_mut();
            let st = &mut *guard;
            match st.table.as_ref().and_then(|t| st.history.redo(t)) {
                Some(next) => {
                    st.table = Some(next);
                    true
                }
                None => false,
            }
        };
        if ok {
            self.preview_all_dirty_or_selected();
            self.refresh_all();
        }
    }

    fn preview_all_dirty_or_selected(self: &AppRef) {
        self.schedule_preview();
    }

    /// Mirror the playing preset into the pedal's RAM, debounced.
    pub fn schedule_preview(self: &AppRef) {
        if let Some(id) = self.preview_timer.borrow_mut().take() {
            id.remove();
        }
        let app = self.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(30), move || {
            app.preview_timer.borrow_mut().take();
            app.send_preview();
        });
        *self.preview_timer.borrow_mut() = Some(id);
    }

    fn send_preview(&self) {
        let cmd = {
            let st = self.state.borrow();
            if !st.settings.live_edit || !st.playing(st.selected) {
                return;
            }
            let source = if st.comparing { st.saved.as_ref() } else { st.table.as_ref() };
            source.and_then(|t| t.patch(st.selected)).map(|p| Command::Preview {
                slot: st.selected as u8,
                bytes: p.raw().to_vec(),
            })
        };
        if let Some(cmd) = cmd {
            self.send(cmd);
        }
    }

    // ---- navigation ----------------------------------------------------

    /// Show preset `i` and make the pedal play it.
    pub fn select(self: &AppRef, i: usize) {
        if i >= PATCH_COUNT {
            return;
        }
        let online = {
            let mut st = self.state.borrow_mut();
            if st.selected == i && st.playing(i) {
                return;
            }
            st.selected = i;
            st.comparing = false;
            st.history.seal();
            if st.online() {
                st.pending_select = Some((i as u8, Instant::now()));
            }
            st.online()
        };
        if online {
            self.send(Command::Select(i as u8));
        }
        // Views and rail marks only: rebuilding the rail would destroy the row
        // that was just activated (and its keyboard focus).
        self.refresh_all();
    }

    pub fn step(self: &AppRef, delta: i32) {
        let i = self.state.borrow().selected as i32 + delta;
        self.select(i.rem_euclid(PATCH_COUNT as i32) as usize);
    }

    /// Ask the pedal to play the selected preset (after looking at another).
    pub fn play_selected(self: &AppRef) {
        let i = self.state.borrow().selected;
        if self.state.borrow().online() {
            self.send(Command::Select(i as u8));
        }
    }

    // ---- saving --------------------------------------------------------

    pub fn save_selected(self: &AppRef) {
        let cmd = {
            let mut st = self.state.borrow_mut();
            let i = st.selected;
            if !st.is_dirty(i) {
                return;
            }
            if !st.online() {
                drop(st);
                self.toast("Conecte a pedaleira para salvar.");
                return;
            }
            let bytes = st.patch(i).unwrap().raw().to_vec();
            st.history.seal();
            st.write_failed = false;
            Command::Save { slot: i as u8, bytes }
        };
        self.end_comparing();
        self.send(cmd);
    }

    /// Leave A/B before writing, so the pedal plays what is being saved.
    fn end_comparing(self: &AppRef) {
        let was = std::mem::replace(&mut self.state.borrow_mut().comparing, false);
        if was {
            self.send_preview();
            crate::window::refresh_actions(self);
        }
    }

    pub fn save_all(self: &AppRef) {
        let cmd = {
            let st = self.state.borrow();
            if st.dirty_slots().is_empty() {
                return;
            }
            if !st.online() {
                drop(st);
                self.toast("Conecte a pedaleira para salvar.");
                return;
            }
            Command::WriteTable {
                table: st.table.clone().unwrap(),
                commit: true,
                note: "antes de salvar todos".into(),
            }
        };
        self.state.borrow_mut().write_failed = false;
        self.end_comparing();
        self.send(cmd);
    }

    /// Write a whole table that replaces local edits too (factory, backup).
    pub fn replace_table(self: &AppRef, table: PresetTable, note: &str) {
        {
            let mut st = self.state.borrow_mut();
            st.replace_on_write = true;
            st.write_failed = false;
        }
        self.end_comparing();
        self.send(Command::WriteTable { table, commit: true, note: note.into() });
    }

    /// Drop the unsaved edits of the selected preset.
    pub fn revert_selected(self: &AppRef) {
        let bytes = {
            let st = self.state.borrow();
            st.saved.as_ref().and_then(|t| t.patch(st.selected)).map(|p| p.raw().to_vec())
        };
        if let Some(bytes) = bytes {
            self.edit(None, Redraw::Views, |t, i| {
                t.set_patch_raw(i, &bytes);
            });
            self.toast_undo("Alterações descartadas.");
        }
    }

    pub fn set_comparing(self: &AppRef, on: bool) {
        self.state.borrow_mut().comparing = on;
        self.send_preview();
        crate::window::refresh_actions(self);
    }

    pub fn save_settings(&self) {
        let _ = self.state.borrow().settings.save_default();
    }

    pub fn save_library(&self) {
        if let Err(e) = self.state.borrow().library.save(Library::default_path()) {
            self.toast(&format!("Não consegui salvar a biblioteca: {e}"));
        }
    }

    // ---- redraw --------------------------------------------------------

    pub fn refresh_all(self: &AppRef) {
        crate::window::refresh(self);
    }

    /// Cheap refresh for value changes: rail marks, bytes, action bar.
    pub fn refresh_marks(self: &AppRef) {
        crate::rail::refresh_marks(self);
        crate::edit::refresh_bytes(self);
        crate::window::refresh_actions(self);
    }
}
