//! GTK4 control app for the M-VAVE TANK MINI.
//!
//! Design goals:
//! - usable by someone who has never touched a multi-effects pedal: presets on
//!   the left, six blocks in signal order, plain sliders with the real value;
//! - deep enough for advanced users: raw per-preset hex view, backups, and a
//!   byte-level statement of what is verified vs inferred.
//!
//! The two tabs mirror that split: "Simples" and "Avançado".

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow, Box as GtkBox, Orientation};
use tankmini_core::backup::BackupStore;
use tankmini_core::discovery::find_pedal;
use tankmini_core::patch::{Patch, PresetTable, PATCH_COUNT, SLOT_LABELS};
use tankmini_core::Pedal;

const APP_ID: &str = "dev.v1ni.TankMini";
const MODULE_COUNT: usize = 6;
const PARAMS_PER_MODULE: usize = 6;

const HELP_TEXT: &str = r#"<b>O que este app faz</b>

A pedaleira guarda 21 presets (7 bancos × A/B/C). Cada preset é uma cadeia de
6 blocos de efeito, e cada bloco tem até 6 parâmetros de 0 a 100.

• Escolha na lista da esquerda o preset que quer editar.
• Cada cartão é um bloco, na ordem em que o som passa por ele.
• Mexa nos controles: nada é enviado até você clicar em <b>Salvar na pedaleira</b>.
• Antes de qualquer gravação o app guarda um backup automático.

<b>Limite desta versão</b>

Os nomes dos parâmetros (Ganho, Mix, Decay…) ainda não foram mapeados. Os
controles aparecem numerados com a faixa real (0–100) e o endereço de memória.
A estrutura — 21 presets, 6 blocos, ordem da cadeia, parâmetros — está
verificada na pedaleira.

A gravação vai para a memória de trabalho. O comando que salva na memória
permanente (sobrevive a desligar) ainda não foi identificado: use
<b>Backup</b> para guardar o que fez."#;

const MAP_TEXT: &str = r#"<b>Verificado na pedaleira</b>

• USB-MIDI: SysEx com empacotamento de 7 bits, frame 00 59 cmd len body cs.
• Identidade: cmd 0x11 → TANK MINI_004.
• Tabela de presets: 1942 bytes em type 4, 0x0000 = 21 presets de 92 bytes + PATCHEND.
• Um preset pode ser lido/gravado sozinho em 0x0000 + i*92 (testado).
• Em cada preset: 6 bytes de cadeia (permutação de 0–5), 6 bytes de flag por
  bloco, 36 parâmetros u16 LE (0–100) a partir de 0x14.
• Slot ativo: 0x20000000 (1 byte). Trocar preset: escrever 0xE0000000+i com [i].
• Nomes de CAB/amp: 20 entradas de 14 bytes em type 5, 0x80000000.

<b>Inferido (não confirmado)</b>

• Significado dos bytes 0x08–0x13 (flags, modelo de amp/cab).
• Qual bloco da cadeia corresponde a AMP, CAB, MOD, DLY ou RVB.
• A ordem dos parâmetros dentro de cada bloco.
• O comando de salvar na flash."#;

struct State {
    table: Option<PresetTable>,
    original: Option<PresetTable>,
    selected: usize,
    active_slot: Option<u8>,
    firmware: String,
    node: String,
}

impl State {
    fn dirty(&self) -> bool {
        match (&self.table, &self.original) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        }
    }
}

type Shared = Rc<RefCell<State>>;

#[derive(Clone)]
struct Ui {
    header_status: gtk::Label,
    banner: gtk::Label,
    preset_list: gtk::ListBox,
    chain_box: GtkBox,
    modules_box: GtkBox,
    hex_view: gtk::TextView,
    dirty: gtk::Label,
    status: gtk::Label,
    reload: gtk::Button,
    backup: gtk::Button,
    write: gtk::Button,
    restore: gtk::Button,
    select_slot: gtk::Button,
    stack: gtk::Stack,
}

fn set_status(ui: &Ui, text: &str) {
    ui.status.set_text(text);
}

fn with_pedal<T>(f: impl FnOnce(&mut Pedal) -> Result<T, String>) -> Result<T, String> {
    let device = find_pedal().ok_or_else(|| {
        "Pedaleira não encontrada. Verifique o cabo USB e as permissões de /dev/snd/midiC*."
            .to_string()
    })?;
    let mut pedal = Pedal::open(&device.midi_path)
        .map_err(|e| format!("Não consegui abrir {}: {e}", device.midi_path.display()))?;
    f(&mut pedal)
}

fn load_from_device(state: &Shared) -> Result<(), String> {
    let device =
        find_pedal().ok_or_else(|| "Pedaleira não encontrada. Verifique o cabo USB.".to_string())?;
    let node = device.midi_path.display().to_string();
    let mut pedal =
        Pedal::open(&device.midi_path).map_err(|e| format!("Não consegui abrir {node}: {e}"))?;
    let firmware = pedal
        .identify()
        .map_err(|e| e.to_string())?
        .map(|i| i.firmware)
        .ok_or("sem resposta ao pedido de identidade")?;
    let active = pedal.active_slot().map_err(|e| e.to_string())?;
    let table = pedal
        .read_table()
        .map_err(|e| e.to_string())?
        .ok_or("a pedaleira não devolveu a tabela de presets")?;
    let mut st = state.borrow_mut();
    st.firmware = firmware;
    st.node = node;
    st.active_slot = active;
    st.selected = active.unwrap_or(0) as usize;
    st.original = Some(table.clone());
    st.table = Some(table);
    Ok(())
}

fn hexdump(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (i, chunk) in data.chunks(16).enumerate() {
        let _ = write!(out, "{:04x}  ", i * 16);
        for b in chunk {
            let _ = write!(out, "{b:02x} ");
        }
        for _ in chunk.len()..16 {
            out.push_str("   ");
        }
        out.push_str(" |");
        for b in chunk {
            out.push(if (0x20..0x7f).contains(b) { *b as char } else { '.' });
        }
        out.push_str("|\n");
    }
    out
}

/// Rebuild the whole UI from state.
fn refresh(ui: &Ui, state: &Shared, suppress: &Rc<Cell<bool>>) {
    suppress.set(true);
    {
        let st = state.borrow();

        if st.table.is_some() {
            ui.header_status.set_text(&format!(
                "{}  ·  {}  ·  preset ativo {}",
                st.firmware,
                st.node,
                st.active_slot
                    .map(|s| SLOT_LABELS.get(s as usize).copied().unwrap_or("?"))
                    .unwrap_or("?")
            ));
            ui.banner.set_text(
                "Conectada. Escolha um preset, edite os blocos e clique em Salvar na pedaleira.",
            );
            ui.banner.remove_css_class("error");
            ui.banner.add_css_class("dim-label");
            ui.write.set_sensitive(true);
            ui.backup.set_sensitive(true);
            ui.select_slot.set_sensitive(true);
        } else {
            ui.header_status.set_text("sem pedaleira");
            ui.banner
                .set_text("Pedaleira não encontrada. Conecte o cabo USB e clique em Recarregar.");
            ui.banner.remove_css_class("dim-label");
            ui.banner.add_css_class("error");
            ui.write.set_sensitive(false);
            ui.backup.set_sensitive(false);
            ui.select_slot.set_sensitive(false);
        }

        // Preset list.
        while let Some(row) = ui.preset_list.row_at_index(0) {
            ui.preset_list.remove(&row);
        }
        if let Some(table) = &st.table {
            for i in 0..PATCH_COUNT {
                let patch = table.patch(i).expect("index in range");
                let is_active = st.active_slot.map(|s| s as usize) == Some(i);
                let row = gtk::ListBoxRow::new();
                let inner = GtkBox::new(Orientation::Vertical, 0);
                let line = GtkBox::new(Orientation::Horizontal, 6);
                let label = gtk::Label::new(Some(patch.label()));
                label.set_xalign(0.0);
                line.append(&label);
                if is_active {
                    let dot = gtk::Label::new(Some("●"));
                    line.append(&dot);
                }
                let summary = gtk::Label::new(Some(&format!(
                    "cadeia {}",
                    patch
                        .chain()
                        .iter()
                        .map(|m| format!("{}", m + 1))
                        .collect::<Vec<_>>()
                        .join("→")
                )));
                summary.add_css_class("dim-label");
                summary.add_css_class("caption");
                summary.set_xalign(0.0);
                inner.append(&line);
                inner.append(&summary);
                row.set_child(Some(&inner));
                ui.preset_list.append(&row);
            }
            if let Some(row) = ui.preset_list.row_at_index(st.selected as i32) {
                ui.preset_list.select_row(Some(&row));
            }
        }

        // Chain + module cards.
        while let Some(child) = ui.chain_box.first_child() {
            ui.chain_box.remove(&child);
        }
        while let Some(child) = ui.modules_box.first_child() {
            ui.modules_box.remove(&child);
        }

        if let Some(table) = &st.table {
            if let Some(patch) = table.patch(st.selected) {
                let chain = patch.chain();
                for (pos, module) in chain.iter().enumerate() {
                    let chip = GtkBox::new(Orientation::Horizontal, 2);
                    chip.add_css_class("linked");
                    let label = gtk::Label::new(Some(&format!("{}", module + 1)));
                    let up = gtk::Button::from_icon_name("go-up-symbolic");
                    let down = gtk::Button::from_icon_name("go-down-symbolic");
                    up.set_sensitive(pos > 0);
                    down.set_sensitive(pos + 1 < MODULE_COUNT);
                    up.set_tooltip_text(Some("mover para antes do bloco anterior"));
                    down.set_tooltip_text(Some("mover para depois do próximo bloco"));
                    {
                        let state = state.clone();
                        let ui_c = ui.clone();
                        let suppress = suppress.clone();
                        let selected = st.selected;
                        up.connect_clicked(move |_| {
                            if let Some(t) = state.borrow_mut().table.as_mut() {
                                t.swap_chain_positions(selected, pos, pos - 1);
                            }
                            refresh(&ui_c, &state, &suppress);
                        });
                    }
                    {
                        let state = state.clone();
                        let ui_c = ui.clone();
                        let suppress = suppress.clone();
                        let selected = st.selected;
                        down.connect_clicked(move |_| {
                            if let Some(t) = state.borrow_mut().table.as_mut() {
                                t.swap_chain_positions(selected, pos, pos + 1);
                            }
                            refresh(&ui_c, &state, &suppress);
                        });
                    }
                    chip.append(&up);
                    chip.append(&label);
                    chip.append(&down);
                    ui.chain_box.append(&chip);
                    if pos + 1 < MODULE_COUNT {
                        let arrow = gtk::Label::new(Some("→"));
                        arrow.add_css_class("dim-label");
                        ui.chain_box.append(&arrow);
                    }
                }

                let flags = patch.module_flags();
                for (pos, module) in chain.iter().enumerate() {
                    let card = module_card(state, suppress, st.selected, pos, *module as usize, flags[pos], &patch);
                    ui.modules_box.append(&card);
                }
                ui.hex_view.buffer().set_text(&hexdump(patch.raw()));
            }
        }

        ui.dirty.set_text(if st.dirty() {
            "alterações não aplicadas"
        } else {
            ""
        });
    }
    suppress.set(false);
}

fn module_card(
    state: &Shared,
    suppress: &Rc<Cell<bool>>,
    patch_index: usize,
    position: usize,
    module: usize,
    flag: u8,
    patch: &Patch<'_>,
) -> gtk::Frame {
    let frame = gtk::Frame::new(None);
    frame.add_css_class("card");

    let outer = GtkBox::new(Orientation::Vertical, 8);
    outer.set_margin_top(10);
    outer.set_margin_bottom(10);
    outer.set_margin_start(12);
    outer.set_margin_end(12);

    let head = GtkBox::new(Orientation::Horizontal, 8);
    let title = gtk::Label::new(None);
    title.set_markup(&format!(
        "<b>Bloco {} · módulo {}</b>",
        position + 1,
        module + 1
    ));
    title.set_xalign(0.0);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let enabled = gtk::Switch::new();
    enabled.set_active(flag != 0);
    enabled.set_valign(gtk::Align::Center);
    enabled.set_tooltip_text(Some("liga/desliga este bloco no preset"));
    {
        let state = state.clone();
        let suppress = suppress.clone();
        enabled.connect_state_set(move |_, on| {
            if !suppress.get() {
                if let Some(t) = state.borrow_mut().table.as_mut() {
                    t.set_byte(patch_index, 0x08 + position, if on { 1 } else { 0 });
                }
            }
            glib::Propagation::Proceed
        });
    }
    head.append(&title);
    head.append(&spacer);
    head.append(&enabled);
    outer.append(&head);

    let grid = gtk::Grid::builder()
        .row_spacing(4)
        .column_spacing(10)
        .build();

    for param in 0..PARAMS_PER_MODULE {
        let value = patch.param(module, param).unwrap_or(0);
        let name = gtk::Label::new(Some(&format!("Parâmetro {}", param + 1)));
        name.set_xalign(0.0);
        name.set_size_request(130, -1);
        let scale = gtk::Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.set_value(value as f64);
        let spin = gtk::SpinButton::with_range(0.0, 100.0, 1.0);
        spin.set_value(value as f64);
        spin.set_width_chars(4);
        spin.set_numeric(true);
        let offset = Patch::param_offset(module, param);
        spin.set_tooltip_text(Some(&format!("offset 0x{offset:02x} · faixa 0–100")));

        {
            let spin_c = spin.clone();
            let state = state.clone();
            let suppress = suppress.clone();
            scale.connect_value_changed(move |s| {
                if suppress.get() {
                    return;
                }
                let v = s.value().round() as u16;
                spin_c.set_value(v as f64);
                if let Some(t) = state.borrow_mut().table.as_mut() {
                    t.set_param(patch_index, module, param, v);
                }
            });
        }
        {
            let scale_c = scale.clone();
            let state = state.clone();
            let suppress = suppress.clone();
            spin.connect_value_changed(move |s| {
                if suppress.get() {
                    return;
                }
                let v = s.value().round() as u16;
                scale_c.set_value(v as f64);
                if let Some(t) = state.borrow_mut().table.as_mut() {
                    t.set_param(patch_index, module, param, v);
                }
            });
        }

        grid.attach(&name, 0, param as i32, 1, 1);
        grid.attach(&scale, 1, param as i32, 1, 1);
        grid.attach(&spin, 2, param as i32, 1, 1);
    }

    outer.append(&grid);
    frame.set_child(Some(&outer));
    frame
}

fn build_window(app: &Application) -> (ApplicationWindow, Ui, Shared) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("TANK MINI")
        .default_width(1180)
        .default_height(820)
        .build();

    let header = gtk::HeaderBar::new();
    let header_status = gtk::Label::new(Some("conectando…"));
    header_status.add_css_class("subtitle");
    header.set_title_widget(Some(&header_status));

    let help = gtk::Button::from_icon_name("help-about-symbolic");
    help.set_tooltip_text(Some("Como usar"));
    {
        let window_c = window.clone();
        help.connect_clicked(move |_| {
            let dialog = gtk::MessageDialog::builder()
                .transient_for(&window_c)
                .modal(true)
                .message_type(gtk::MessageType::Info)
                .buttons(gtk::ButtonsType::Close)
                .build();
            dialog.set_markup(HELP_TEXT);
            dialog.present();
        });
    }
    let map = gtk::Button::with_label("Mapeamento");
    {
        let window_c = window.clone();
        map.connect_clicked(move |_| {
            let dialog = gtk::MessageDialog::builder()
                .transient_for(&window_c)
                .modal(true)
                .message_type(gtk::MessageType::Info)
                .buttons(gtk::ButtonsType::Close)
                .build();
            dialog.set_markup(MAP_TEXT);
            dialog.present();
        });
    }
    header.pack_end(&help);
    header.pack_end(&map);
    window.set_titlebar(Some(&header));

    let root = GtkBox::new(Orientation::Vertical, 0);

    let banner = gtk::Label::new(Some("conectando…"));
    banner.set_xalign(0.0);
    banner.set_wrap(true);
    banner.set_margin_top(8);
    banner.set_margin_bottom(8);
    banner.set_margin_start(14);
    banner.set_margin_end(14);
    root.append(&banner);

    let paned = gtk::Paned::new(Orientation::Horizontal);
    paned.set_vexpand(true);

    let left = GtkBox::new(Orientation::Vertical, 6);
    left.set_size_request(240, -1);
    let left_title = gtk::Label::new(Some("Presets"));
    left_title.set_xalign(0.0);
    left_title.add_css_class("heading");
    left_title.set_margin_start(10);
    left_title.set_margin_top(6);
    left.append(&left_title);

    let preset_list = gtk::ListBox::new();
    preset_list.set_selection_mode(gtk::SelectionMode::Single);
    let list_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&preset_list)
        .build();
    left.append(&list_scroll);

    let select_slot = gtk::Button::with_label("Tocar na pedaleira");
    select_slot.set_margin_start(10);
    select_slot.set_margin_end(10);
    select_slot.set_margin_bottom(10);
    left.append(&select_slot);
    paned.set_start_child(Some(&left));

    let stack = gtk::Stack::new();
    stack.set_hexpand(true);

    let simple = GtkBox::new(Orientation::Vertical, 8);
    simple.set_margin_top(8);
    simple.set_margin_start(12);
    simple.set_margin_end(12);
    let chain_title = gtk::Label::new(None);
    chain_title.set_markup("<b>Ordem do sinal</b>  <span size='small'>(setas reordenam os blocos)</span>");
    chain_title.set_xalign(0.0);
    simple.append(&chain_title);
    let chain_box = GtkBox::new(Orientation::Horizontal, 4);
    simple.append(&chain_box);
    let modules_box = GtkBox::new(Orientation::Vertical, 8);
    let modules_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&modules_box)
        .build();
    simple.append(&modules_scroll);
    stack.add_titled(&simple, Some("simple"), "Simples");

    let advanced = GtkBox::new(Orientation::Vertical, 8);
    advanced.set_margin_top(8);
    advanced.set_margin_start(12);
    advanced.set_margin_end(12);
    let adv_note = gtk::Label::new(Some(
        "Bytes do preset selecionado (92 bytes). Somente leitura; a edição é feita na aba Simples.",
    ));
    adv_note.set_xalign(0.0);
    adv_note.set_wrap(true);
    adv_note.add_css_class("dim-label");
    advanced.append(&adv_note);
    let hex_view = gtk::TextView::new();
    hex_view.set_editable(false);
    hex_view.set_monospace(true);
    let hex_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&hex_view)
        .build();
    advanced.append(&hex_scroll);
    stack.add_titled(&advanced, Some("advanced"), "Avançado");

    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    let stack_bar = GtkBox::new(Orientation::Horizontal, 0);
    stack_bar.set_halign(gtk::Align::Center);
    stack_bar.append(&switcher);
    let right = GtkBox::new(Orientation::Vertical, 4);
    right.append(&stack_bar);
    right.append(&stack);
    paned.set_end_child(Some(&right));
    root.append(&paned);

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_margin_top(8);
    actions.set_margin_bottom(8);
    actions.set_margin_start(14);
    actions.set_margin_end(14);
    let reload = gtk::Button::with_label("Recarregar");
    let backup = gtk::Button::with_label("Backup");
    let restore = gtk::Button::with_label("Restaurar backup");
    let write = gtk::Button::with_label("Salvar na pedaleira");
    write.add_css_class("suggested-action");
    let dirty = gtk::Label::new(None);
    dirty.add_css_class("dim-label");
    actions.append(&reload);
    actions.append(&backup);
    actions.append(&restore);
    actions.append(&dirty);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    actions.append(&spacer);
    actions.append(&write);
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
        header_status,
        banner,
        preset_list,
        chain_box,
        modules_box,
        hex_view,
        dirty,
        status,
        reload,
        backup,
        write,
        restore,
        select_slot,
        stack,
    };
    let state: Shared = Rc::new(RefCell::new(State {
        table: None,
        original: None,
        selected: 0,
        active_slot: None,
        firmware: String::new(),
        node: String::new(),
    }));

    (window, ui, state)
}

fn connect(ui: &Ui, state: &Shared, suppress: &Rc<Cell<bool>>) {
    // Selecting a preset only changes what is displayed.
    {
        let state = state.clone();
        let ui_c = ui.clone();
        let suppress = suppress.clone();
        ui.clone().preset_list.connect_row_selected(move |_, row| {
            if suppress.get() {
                return;
            }
            if let Some(row) = row {
                state.borrow_mut().selected = row.index() as usize;
                refresh(&ui_c, &state, &suppress);
            }
        });
    }

    // Reload from the pedal (discards local edits).
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let suppress = suppress.clone();
        ui.clone().reload.connect_clicked(move |_| {
            match load_from_device(&state) {
                Ok(()) => set_status(&ui_c, "Lido da pedaleira."),
                Err(e) => set_status(&ui_c, &e),
            }
            refresh(&ui_c, &state, &suppress);
        });
    }

    // Backup the whole table.
    {
        let ui_c = ui.clone();
        let state = state.clone();
        ui.clone().backup.connect_clicked(move |_| {
            let st = state.borrow();
            let Some(table) = st.table.as_ref() else { return };
            let store = BackupStore::new(BackupStore::default_root());
            match store.save(&table.to_preset(), st.active_slot, "backup manual") {
                Ok(path) => set_status(&ui_c, &format!("Backup salvo em {}", path.display())),
                Err(e) => set_status(&ui_c, &format!("Falha no backup: {e}")),
            }
        });
    }

    // Write the selected preset, with an automatic backup first.
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let suppress = suppress.clone();
        ui.clone().write.connect_clicked(move |_| {
            let (patch_bytes, selected, snapshot) = {
                let st = state.borrow();
                let Some(table) = st.table.as_ref() else { return };
                let Some(patch) = table.patch(st.selected) else { return };
                (patch.raw().to_vec(), st.selected, table.clone())
            };
            let result = with_pedal(|pedal| {
                if let Some(current) = pedal.read_table().map_err(|e| e.to_string())? {
                    let store = BackupStore::new(BackupStore::default_root());
                    let path = store
                        .save(&current.to_preset(), None, "antes de gravar")
                        .map_err(|e| e.to_string())?;
                    eprintln!("[tankmini] backup antes de gravar: {}", path.display());
                }
                let ack = pedal
                    .write_patch(selected as u8, &patch_bytes)
                    .map_err(|e| e.to_string())?;
                std::thread::sleep(Duration::from_millis(150));
                let readback = pedal
                    .read_patch(selected as u8)
                    .map_err(|e| e.to_string())?
                    .unwrap_or_default();
                Ok((ack, readback == patch_bytes))
            });
            match result {
                Ok((ack, verified)) if ack == 0 && verified => {
                    state.borrow_mut().original = Some(snapshot);
                    set_status(
                        &ui_c,
                        &format!("Preset {} gravado e verificado.", SLOT_LABELS[selected]),
                    );
                }
                Ok((ack, verified)) => set_status(
                    &ui_c,
                    &format!("Gravação não confirmada (ack 0x{ack:02x}, verificação: {verified}). Use Restaurar backup."),
                ),
                Err(e) => set_status(&ui_c, &format!("Falha ao gravar: {e}")),
            }
            refresh(&ui_c, &state, &suppress);
        });
    }

    // Restore the newest backup (whole table).
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let suppress = suppress.clone();
        ui.clone().restore.connect_clicked(move |_| {
            let store = BackupStore::new(BackupStore::default_root());
            let newest = match store.list() {
                Ok(list) => list.into_iter().next(),
                Err(e) => {
                    set_status(&ui_c, &format!("Falha ao listar backups: {e}"));
                    return;
                }
            };
            let Some(path) = newest else {
                set_status(&ui_c, "Nenhum backup encontrado.");
                return;
            };
            let preset = match BackupStore::load(&path) {
                Ok(p) => p,
                Err(e) => {
                    set_status(&ui_c, &format!("Backup inválido: {e}"));
                    return;
                }
            };
            let table = match PresetTable::from_raw(preset.raw().to_vec()) {
                Ok(t) => t,
                Err(e) => {
                    set_status(&ui_c, &format!("Backup inválido: {e}"));
                    return;
                }
            };
            let result = with_pedal(|pedal| pedal.write_table(&table).map_err(|e| e.to_string()));
            match result {
                Ok(report) if report.verified => {
                    set_status(&ui_c, &format!("Restaurado de {}", path.display()));
                    let _ = load_from_device(&state);
                }
                Ok(report) => set_status(
                    &ui_c,
                    &format!(
                        "Restauração não verificada ({}/{} blocos).",
                        report.chunks_ok, report.chunks
                    ),
                ),
                Err(e) => set_status(&ui_c, &format!("Falha ao restaurar: {e}")),
            }
            refresh(&ui_c, &state, &suppress);
        });
    }

    // Switch the pedal to the selected preset.
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let suppress = suppress.clone();
        ui.clone().select_slot.connect_clicked(move |_| {
            let selected = state.borrow().selected as u8;
            let result = with_pedal(|pedal| {
                pedal
                    .select_slot(selected)
                    .map_err(|e| e.to_string())
                    .and_then(|ok| ok.then_some(()).ok_or_else(|| "recusado pela pedaleira".to_string()))
            });
            match result {
                Ok(()) => {
                    std::thread::sleep(Duration::from_millis(200));
                    state.borrow_mut().active_slot = Some(selected);
                    set_status(
                        &ui_c,
                        &format!(
                            "A pedaleira agora está no preset {}.",
                            SLOT_LABELS.get(selected as usize).copied().unwrap_or("?")
                        ),
                    );
                }
                Err(e) => set_status(&ui_c, &format!("Falha ao trocar de preset: {e}")),
            }
            refresh(&ui_c, &state, &suppress);
        });
    }
}

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(move |app| {
        let (window, ui, state) = build_window(app);
        let suppress: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        connect(&ui, &state, &suppress);

        match load_from_device(&state) {
            Ok(()) => set_status(&ui, "Lido da pedaleira."),
            Err(e) => set_status(&ui, &e),
        }
        refresh(&ui, &state, &suppress);
        ui.stack.set_visible_child_name("simple");
        window.present();
    });

    app.run()
}
