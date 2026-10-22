//! GTK4 desktop control app for the M-VAVE TANK MINI.
//!
//! The UI is intentionally honest about what is known: records and parameters
//! are shown with their offsets and ids, because the field semantics of the
//! preset block are not decoded yet (see `docs/protocol/`). Everything the app
//! writes is backed up first and verified by reading the block back.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow, Box as GtkBox, Orientation};
use tankmini_core::backup::BackupStore;
use tankmini_core::discovery::find_pedal;
use tankmini_core::preset::Preset;
use tankmini_core::Pedal;

const APP_ID: &str = "dev.v1ni.TankMini";
const SLOT_LABELS: [&str; 21] = [
    "1A", "1B", "1C", "2A", "2B", "2C", "3A", "3B", "3C", "4A", "4B", "4C", "5A", "5B", "5C", "6A",
    "6B", "6C", "7A", "7B", "7C",
];

struct State {
    preset: Option<Preset>,
    original: Option<Preset>,
    firmware: String,
    node: String,
    slot: Option<u8>,
}

impl State {
    fn dirty(&self) -> bool {
        match (&self.preset, &self.original) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        }
    }
}

type Shared = Rc<RefCell<State>>;

#[derive(Clone)]
struct Ui {
    firmware: gtk::Label,
    node: gtk::Label,
    slot: gtk::DropDown,
    status: gtk::Label,
    dirty: gtk::Label,
    records: GtkBox,
    hex: gtk::TextView,
    reload: gtk::Button,
    apply: gtk::Button,
    backup: gtk::Button,
    restore: gtk::Button,
}

fn set_status(ui: &Ui, text: &str) {
    ui.status.set_text(text);
}

fn load_from_device(state: &Shared) -> Result<(), String> {
    let device = find_pedal().ok_or_else(|| {
        "Pedaleira não encontrada. Confira o cabo USB e as permissões de /dev/snd/midiC*.".to_string()
    })?;
    let node = device.midi_path.display().to_string();
    let mut pedal = Pedal::open(&device.midi_path)
        .map_err(|e| format!("Não consegui abrir {node}: {e}"))?;
    let firmware = pedal
        .identify()
        .map_err(|e| e.to_string())?
        .map(|i| i.firmware)
        .ok_or("sem resposta ao pedido de identidade")?;
    let slot = pedal.active_slot().map_err(|e| e.to_string())?;
    let preset = pedal
        .read_preset()
        .map_err(|e| e.to_string())?
        .ok_or("a pedaleira não devolveu o bloco de preset")?;
    let mut st = state.borrow_mut();
    st.firmware = firmware;
    st.slot = slot;
    st.node = node;
    st.original = Some(preset.clone());
    st.preset = Some(preset);
    Ok(())
}

fn refresh(ui: &Ui, state: &Shared) {
    let st = state.borrow();
    ui.firmware.set_text(if st.firmware.is_empty() { "—" } else { &st.firmware });
    ui.node.set_text(if st.node.is_empty() { "—" } else { &st.node });
    if let Some(slot) = st.slot {
        ui.slot.set_selected(slot as u32);
    }

    while let Some(child) = ui.records.first_child() {
        ui.records.remove(&child);
    }

    if let Some(preset) = &st.preset {
        for record in preset.records() {
            let title = format!(
                "Record {:02}   off 0x{:04x}   id 0x{:02x}   kind 0x{:02x}",
                record.index,
                record.offset(),
                record.id(),
                record.kind()
            );
            let expander = gtk::Expander::new(Some(&title));
            expander.set_expanded(record.index < 2);

            let grid = gtk::Grid::builder()
                .margin_top(6)
                .margin_bottom(6)
                .margin_start(6)
                .margin_end(6)
                .row_spacing(4)
                .column_spacing(8)
                .build();

            for (i, value) in record.params().iter().enumerate() {
                let label = gtk::Label::new(Some(&format!("p{i:02}")));
                label.set_xalign(1.0);
                label.add_css_class("dim-label");
                let spin = gtk::SpinButton::with_range(0.0, 65535.0, 1.0);
                spin.set_value(*value as f64);
                spin.set_width_chars(6);
                spin.set_numeric(true);
                spin.set_tooltip_text(Some(&format!(
                    "record {} · param {} · offset 0x{:04x}",
                    record.index,
                    i,
                    record.offset() + 0x20 + i * 2
                )));

                let state_c = state.clone();
                let dirty_label = ui.dirty.clone();
                let rec = record.index;
                spin.connect_value_changed(move |s| {
                    if let Some(preset) = state_c.borrow_mut().preset.as_mut() {
                        preset.set_param(rec, i, s.value() as u16);
                    }
                    let dirty = state_c.borrow().dirty();
                    dirty_label.set_text(if dirty { "alterações não aplicadas" } else { "" });
                });

                let row = (i / 6) as i32;
                let col = ((i % 6) * 2) as i32;
                grid.attach(&label, col, row, 1, 1);
                grid.attach(&spin, col + 1, row, 1, 1);
            }

            expander.set_child(Some(&grid));
            ui.records.append(&expander);
        }
        ui.hex.buffer().set_text(&preset.hexdump());
    } else {
        ui.hex.buffer().set_text("sem dados");
    }

    let has_preset = st.preset.is_some();
    ui.apply.set_sensitive(has_preset);
    ui.backup.set_sensitive(has_preset);
    ui.dirty.set_text(if st.dirty() { "alterações não aplicadas" } else { "" });
}

fn build_window(app: &Application) -> (ApplicationWindow, Ui, Shared) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("TANK MINI")
        .default_width(1000)
        .default_height(780)
        .build();

    let header = gtk::HeaderBar::new();
    let title = gtk::Label::new(Some("TANK MINI"));
    title.add_css_class("title");
    header.set_title_widget(Some(&title));
    window.set_titlebar(Some(&header));

    let root = GtkBox::new(Orientation::Vertical, 0);

    let info = GtkBox::new(Orientation::Horizontal, 24);
    info.set_margin_top(10);
    info.set_margin_bottom(10);
    info.set_margin_start(14);
    info.set_margin_end(14);

    let firmware = gtk::Label::new(Some("—"));
    let node = gtk::Label::new(Some("—"));
    node.add_css_class("dim-label");
    let slot = gtk::DropDown::from_strings(&SLOT_LABELS);
    slot.set_selected(0);

    let columns: [(&str, gtk::Widget); 3] = [
        ("Firmware", firmware.clone().upcast()),
        ("Nó MIDI", node.clone().upcast()),
        ("Preset", slot.clone().upcast()),
    ];
    for (caption, widget) in columns {
        let column = GtkBox::new(Orientation::Vertical, 1);
        let cap = gtk::Label::new(Some(caption));
        cap.set_xalign(0.0);
        cap.add_css_class("dim-label");
        cap.add_css_class("caption");
        column.append(&cap);
        column.append(&widget);
        info.append(&column);
    }
    root.append(&info);
    root.append(&gtk::Separator::new(Orientation::Horizontal));

    let notebook = gtk::Notebook::new();
    notebook.set_vexpand(true);

    let records = GtkBox::new(Orientation::Vertical, 2);
    let records_scroll = gtk::ScrolledWindow::builder().vexpand(true).child(&records).build();
    notebook.append_page(&records_scroll, Some(&gtk::Label::new(Some("Records"))));

    let hex = gtk::TextView::new();
    hex.set_editable(false);
    hex.set_monospace(true);
    let hex_scroll = gtk::ScrolledWindow::builder().vexpand(true).child(&hex).build();
    notebook.append_page(&hex_scroll, Some(&gtk::Label::new(Some("Hex"))));
    root.append(&notebook);

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_margin_top(8);
    actions.set_margin_bottom(8);
    actions.set_margin_start(14);
    actions.set_margin_end(14);

    let reload = gtk::Button::with_label("Recarregar");
    let backup = gtk::Button::with_label("Backup");
    let apply = gtk::Button::with_label("Aplicar");
    apply.add_css_class("suggested-action");
    let restore = gtk::Button::with_label("Restaurar último backup");
    let dirty = gtk::Label::new(None);
    dirty.add_css_class("dim-label");

    actions.append(&reload);
    actions.append(&backup);
    actions.append(&apply);
    actions.append(&restore);
    actions.append(&dirty);
    root.append(&actions);

    let status = gtk::Label::new(Some("Pronto"));
    status.set_xalign(0.0);
    status.set_margin_start(14);
    status.set_margin_end(14);
    status.set_margin_bottom(10);
    status.add_css_class("dim-label");
    root.append(&status);

    window.set_child(Some(&root));

    let ui = Ui {
        firmware,
        node,
        slot,
        status,
        dirty,
        records,
        hex,
        reload,
        apply,
        backup,
        restore,
    };
    let state: Shared = Rc::new(RefCell::new(State {
        preset: None,
        original: None,
        firmware: String::new(),
        node: String::new(),
        slot: None,
    }));

    (window, ui, state)
}

fn connect(ui: &Ui, state: &Shared) {
    // Reload from the pedal.
    {
        let ui = ui.clone();
        let state = state.clone();
        ui.reload.connect_clicked(move |_| {
            match load_from_device(&state) {
                Ok(()) => set_status(&ui, "Conectado."),
                Err(e) => set_status(&ui, &e),
            }
            refresh(&ui, &state);
        });
    }

    // Slot selection.
    {
        let ui = ui.clone();
        let state = state.clone();
        ui.slot.connect_selected_notify(move |dropdown| {
            let index = dropdown.selected();
            if index > 20 {
                return;
            }
            let slot = index as u8;
            let result = with_pedal(|pedal| {
                pedal
                    .select_slot(slot)
                    .map_err(|e| e.to_string())
                    .and_then(|ok| ok.then_some(()).ok_or_else(|| "slot select recusado".to_string()))
            });
            match result {
                Ok(()) => {
                    std::thread::sleep(Duration::from_millis(200));
                    match load_from_device(&state) {
                        Ok(()) => set_status(
                            &ui,
                            &format!("Preset {} selecionado na pedaleira.", SLOT_LABELS[slot as usize]),
                        ),
                        Err(e) => set_status(&ui, &e),
                    }
                    refresh(&ui, &state);
                }
                Err(e) => set_status(&ui, &format!("Falha ao trocar de preset: {e}")),
            }
        });
    }

    // Backup of the current block.
    {
        let ui = ui.clone();
        let state = state.clone();
        ui.backup.connect_clicked(move |_| {
            let st = state.borrow();
            let Some(preset) = st.preset.as_ref() else {
                return;
            };
            let store = BackupStore::new(BackupStore::default_root());
            match store.save(preset, st.slot, "manual backup from GUI") {
                Ok(path) => set_status(&ui, &format!("Backup salvo em {}", path.display())),
                Err(e) => set_status(&ui, &format!("Falha no backup: {e}")),
            }
        });
    }

    // Apply: backup the pedal's current block, write ours, verify the read-back.
    {
        let ui = ui.clone();
        let state = state.clone();
        ui.apply.connect_clicked(move |_| {
            let (preset, slot) = {
                let st = state.borrow();
                match &st.preset {
                    Some(p) => (p.clone(), st.slot),
                    None => return,
                }
            };
            let result = with_pedal(|pedal| {
                // Always keep a copy of what is on the pedal before overwriting.
                if let Some(current) = pedal.read_preset().map_err(|e| e.to_string())? {
                    let store = BackupStore::new(BackupStore::default_root());
                    let path = store
                        .save(&current, slot, "auto before apply")
                        .map_err(|e| e.to_string())?;
                    eprintln!("[tankmini] backup before apply: {}", path.display());
                }
                let report = pedal.write_preset(&preset).map_err(|e| e.to_string())?;
                Ok(report)
            });
            match result {
                Ok(report) => {
                    if report.verified {
                        state.borrow_mut().original = Some(preset);
                        set_status(
                            &ui,
                            &format!(
                                "Aplicado e verificado ({}/{} chunks ACK).",
                                report.chunks_ok, report.chunks
                            ),
                        );
                    } else {
                        set_status(
                            &ui,
                            &format!(
                                "ATENÇÃO: read-back divergiu ({}/{} chunks ACK). Use o backup.",
                                report.chunks_ok, report.chunks
                            ),
                        );
                    }
                }
                Err(e) => set_status(&ui, &format!("Falha ao aplicar: {e}")),
            }
            refresh(&ui, &state);
        });
    }

    // Restore the newest local backup.
    {
        let ui = ui.clone();
        let state = state.clone();
        ui.restore.connect_clicked(move |_| {
            let store = BackupStore::new(BackupStore::default_root());
            let newest = match store.list() {
                Ok(list) => list.into_iter().next(),
                Err(e) => {
                    set_status(&ui, &format!("Falha ao listar backups: {e}"));
                    return;
                }
            };
            let Some(path) = newest else {
                set_status(&ui, "Nenhum backup encontrado.");
                return;
            };
            let preset = match BackupStore::load(&path) {
                Ok(p) => p,
                Err(e) => {
                    set_status(&ui, &format!("Backup inválido: {e}"));
                    return;
                }
            };
            let result = with_pedal(|pedal| {
                let report = pedal.write_preset(&preset).map_err(|e| e.to_string())?;
                Ok(report)
            });
            match result {
                Ok(report) if report.verified => {
                    set_status(&ui, &format!("Restaurado de {}", path.display()));
                    match load_from_device(&state) {
                        Ok(()) => {}
                        Err(e) => set_status(&ui, &e),
                    }
                }
                Ok(report) => set_status(
                    &ui,
                    &format!("Restauração não verificada ({} chunks ACK).", report.chunks_ok),
                ),
                Err(e) => set_status(&ui, &format!("Falha ao restaurar: {e}")),
            }
            refresh(&ui, &state);
        });
    }
}

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(move |app| {
        let (window, ui, state) = build_window(app);
        connect(&ui, &state);

        match load_from_device(&state) {
            Ok(()) => set_status(&ui, "Conectado."),
            Err(e) => set_status(&ui, &e),
        }
        refresh(&ui, &state);
        window.present();
    });

    app.run()
}
