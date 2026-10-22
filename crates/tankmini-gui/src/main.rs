//! GTK4 desktop app for the M-VAVE TANK MINI.
//!
//! Two audiences, one window:
//! - a beginner gets a guided start ("connect the pedal, pick a preset, play"),
//!   real effect names and plain sliders;
//! - an advanced user gets every module, every effect variant, every parameter
//!   and a raw-byte view with the address of each control.
//!
//! Layout: left rail = the 21 presets, centre = the six effect blocks of the
//! selected preset in signal order, bottom = write/backup actions.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow, Box as GtkBox, Orientation};
use tankmini_core::backup::BackupStore;
use tankmini_core::catalog::{self, Role};
use tankmini_core::discovery::find_pedal;
use tankmini_core::patch::{Patch, PresetTable, PATCH_COUNT, SLOT_LABELS};
use tankmini_core::Pedal;

const APP_ID: &str = "dev.v1ni.TankMini";
const MODULE_COUNT: usize = 6;
const PARAMS_PER_MODULE: usize = 6;

const WELCOME: &str = r#"<span size="xx-large" weight="bold">TANK MINI</span>

<span size="large">Controle sua pedaleira pelo computador, sem o app do celular.</span>

<b>1.</b> Conecte a pedaleira no USB  (<i>já conectada se o ponto ao lado estiver verde</i>)
<b>2.</b> Escolha um dos 21 presets na lista da esquerda
<b>3.</b> Ajuste os efeitos à vontade e clique em <b>Salvar na pedaleira</b>
<b>4.</b> Toque. O que você mexeu fica no som até você trocar de preset.

Tudo o que você altera é guardado antes em um backup local, e a gravação é
conferida lendo os bytes de volta da pedaleira."#;

const ABOUT_MAP: &str = r#"<b>Como o app conversa com a pedaleira</b>

Tudo foi descoberto por engenharia reversa do protocolo USB-MIDI e do app oficial.

• 21 presets de 92 bytes numa tabela (1942 bytes) — cada preset tem a ordem dos
  6 blocos, o tipo de cada bloco e 36 parâmetros de 0 a 100.
• Cada bloco: ligado/desligado em <tt>0x08+i</tt>, tipo em <tt>0x0e+i</tt>,
  parâmetros em <tt>0x14 + i*12 + p*2</tt>.
• Tipos: dinâmica (gate/boost/comp), amp (20 modelos), modulação (7), delay (5),
  reverb (6), cabinet (20 IRs — os nomes vêm da própria pedaleira).

<b>O que ainda não está fechado</b>

• O comando que salva na memória permanente (desligar a pedaleira ainda volta ao
  preset antigo). Use Backup para não perder ajustes.
• Os nomes de alguns parâmetros de dinâmica e cabinet, e a ordem fina dos
  parâmetros de amp — a estrutura está certa, os rótulos são inferidos."#;

struct State {
    table: Option<PresetTable>,
    original: Option<PresetTable>,
    selected: usize,
    active_slot: Option<u8>,
    firmware: String,
    node: String,
    names: Vec<String>,
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
    stack: gtk::Stack,
    device_pill: gtk::Label,
    preset_list: gtk::ListBox,
    preset_title: gtk::Label,
    preset_sub: gtk::Label,
    chain_box: GtkBox,
    modules_box: GtkBox,
    hex_view: gtk::TextView,
    dirty: gtk::Label,
    status: gtk::Label,
    reload: gtk::Button,
    backup: gtk::Button,
    write: gtk::Button,
    restore: gtk::Button,
    play: gtk::Button,
    connect: gtk::Button,
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
        find_pedal().ok_or_else(|| "Pedaleira não encontrada. Conecte no USB.".to_string())?;
    let node = device.midi_path.display().to_string();
    let mut pedal =
        Pedal::open(&device.midi_path).map_err(|e| format!("Não consegui abrir {node}: {e}"))?;
    let firmware = pedal
        .identify()
        .map_err(|e| e.to_string())?
        .map(|i| i.firmware)
        .ok_or("a pedaleira não respondeu")?;
    let active = pedal.active_slot().map_err(|e| e.to_string())?;
    let table = pedal
        .read_table()
        .map_err(|e| e.to_string())?
        .ok_or("a pedaleira não devolveu a tabela de presets")?;
    let names = pedal.cab_names().unwrap_or_default();
    let mut st = state.borrow_mut();
    st.firmware = firmware;
    st.node = node;
    st.active_slot = active;
    st.selected = active.unwrap_or(0) as usize;
    st.names = names;
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

fn badge(role: Role) -> gtk::Label {
    let letter = match role {
        Role::Fx => "FX",
        Role::Amp => "AMP",
        Role::Modulation => "MOD",
        Role::Delay => "DLY",
        Role::Reverb => "RVB",
        Role::Cabinet => "CAB",
    };
    let label = gtk::Label::new(Some(letter));
    label.add_css_class("caption");
    label.add_css_class("dim-label");
    label.set_width_chars(4);
    label
}

fn refresh(ui: &Ui, state: &Shared, suppress: &Rc<Cell<bool>>) {
    suppress.set(true);
    {
        let st = state.borrow();
        let connected = st.table.is_some();
        ui.stack.set_visible_child_name(if connected { "editor" } else { "welcome" });
        ui.device_pill.set_text(&if connected {
            format!(
                "{} conectada · preset {}",
                st.firmware,
                st.active_slot
                    .map(|s| SLOT_LABELS.get(s as usize).copied().unwrap_or("?"))
                    .unwrap_or("?")
            )
        } else {
            "pedaleira desconectada".to_string()
        });
        ui.device_pill.set_tooltip_text(Some(&st.node));

        // Preset rail.
        while let Some(row) = ui.preset_list.row_at_index(0) {
            ui.preset_list.remove(&row);
        }
        if let Some(table) = &st.table {
            for i in 0..PATCH_COUNT {
                let patch = table.patch(i).expect("in range");
                let active = st.active_slot.map(|s| s as usize) == Some(i);
                if i % 3 == 0 {
                    let header = gtk::ListBoxRow::new();
                    header.set_selectable(false);
                    header.set_activatable(false);
                    let label = gtk::Label::new(Some(&format!("Banco {}", i / 3 + 1)));
                    label.set_xalign(0.0);
                    label.add_css_class("caption");
                    label.add_css_class("dim-label");
                    label.set_margin_top(6);
                    label.set_margin_start(4);
                    header.set_child(Some(&label));
                    ui.preset_list.append(&header);
                }
                let row = gtk::ListBoxRow::new();
                if active {
                    row.add_css_class("accent");
                }
                let line = GtkBox::new(Orientation::Vertical, 0);
                let title = gtk::Label::new(None);
                title.set_markup(&format!(
                    "<b>{}</b>{}",
                    patch.label(),
                    if active { "   <small>em uso</small>" } else { "" }
                ));
                title.set_xalign(0.0);
                let chain: Vec<String> = patch
                    .chain()
                    .iter()
                    .map(|m| {
                        match Role::for_slot(*m as usize) {
                            Role::Fx => "DIN",
                            Role::Amp => "AMP",
                            Role::Modulation => "MOD",
                            Role::Delay => "DLY",
                            Role::Reverb => "RVB",
                            Role::Cabinet => "CAB",
                        }
                        .to_string()
                    })
                    .collect();
                let sub = gtk::Label::new(Some(&chain.join(" › ")));
                sub.add_css_class("caption");
                sub.add_css_class("dim-label");
                sub.set_xalign(0.0);
                sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
                line.append(&title);
                line.append(&sub);
                row.set_child(Some(&line));
                ui.preset_list.append(&row);
            }
            if let Some(row) = ui.preset_list.row_at_index(st.selected as i32) {
                ui.preset_list.select_row(Some(&row));
            }
        }

        // Selected preset header.
        if let Some(table) = &st.table {
            if let Some(patch) = table.patch(st.selected) {
                let active = st.active_slot.map(|s| s as usize) == Some(st.selected);
                ui.preset_title
                    .set_markup(&format!("<span size='x-large' weight='bold'>{}</span>", patch.label()));
                ui.preset_sub.set_text(if active {
                    "Este é o preset que está tocando agora."
                } else {
                    "Toque em “Tocar na pedaleira” para ouvir este preset."
                });
                ui.play.set_sensitive(!active);
                ui.play.set_label(if active {
                    "Tocando agora"
                } else {
                    "Tocar na pedaleira"
                });
            }
        }

        // Chain chips + module cards.
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
                    let role = Role::for_slot(*module as usize);
                    let chip = gtk::Label::new(Some(&format!(
                        "{} {}",
                        pos + 1,
                        role.label()
                    )));
                    chip.add_css_class("pill");
                    ui.chain_box.append(&chip);
                    if pos + 1 < MODULE_COUNT {
                        let arrow = gtk::Label::new(Some("›"));
                        arrow.add_css_class("dim-label");
                        ui.chain_box.append(&arrow);
                    }
                }
                for (pos, module) in chain.iter().enumerate() {
                    let card = module_card(state, suppress, st.selected, pos, *module as usize, &patch, &st.names);
                    ui.modules_box.append(&card);
                }
                ui.hex_view.buffer().set_text(&hexdump(patch.raw()));
            }
        }

        let dirty = st.dirty();
        ui.dirty.set_text(if dirty { "alterações não salvas" } else { "" });
        ui.write.set_sensitive(dirty);
    }
    suppress.set(false);
}

fn module_card(
    state: &Shared,
    suppress: &Rc<Cell<bool>>,
    patch_index: usize,
    position: usize,
    module: usize,
    patch: &Patch<'_>,
    names: &[String],
) -> gtk::Frame {
    let role = Role::for_slot(module);
    let unit = patch.unit(module);
    let on = patch.is_on(module);

    let frame = gtk::Frame::new(None);
    frame.add_css_class("card");

    let outer = GtkBox::new(Orientation::Vertical, 10);
    outer.set_margin_top(14);
    outer.set_margin_bottom(14);
    outer.set_margin_start(16);
    outer.set_margin_end(16);

    // Header: position, role, effect name, on/off.
    let head = GtkBox::new(Orientation::Horizontal, 10);
    let badge = badge(role);
    let titles = GtkBox::new(Orientation::Vertical, 0);
    let effect_name = patch.effect_name(module, names);
    let title = gtk::Label::new(None);
    title.set_markup(&format!("<b>{}</b>", gtk::glib::markup_escape_text(&effect_name)));
    title.set_xalign(0.0);
    let subtitle = gtk::Label::new(Some(&format!(
        "Bloco {} de 6 · {} · {}",
        position + 1,
        role.label(),
        role.hint()
    )));
    subtitle.add_css_class("caption");
    subtitle.add_css_class("dim-label");
    subtitle.set_xalign(0.0);
    titles.append(&title);
    titles.append(&subtitle);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let enabled = gtk::Switch::new();
    enabled.set_active(on);
    enabled.set_valign(gtk::Align::Center);
    enabled.set_tooltip_text(Some("Liga ou desliga este efeito no preset"));
    {
        let state = state.clone();
        let suppress = suppress.clone();
        let ui_sink = ui_sink();
        enabled.connect_state_set(move |_, value| {
            if !suppress.get() {
                if let Some(t) = state.borrow_mut().table.as_mut() {
                    t.set_on(patch_index, module, value);
                }
                bump(&ui_sink, &state);
            }
            glib::Propagation::Proceed
        });
    }
    head.append(&badge);
    head.append(&titles);
    head.append(&spacer);
    head.append(&enabled);
    outer.append(&head);

    // Variant selector.
    let labels: Vec<String> = match role {
        Role::Amp | Role::Cabinet => (0..20)
            .map(|i| {
                names
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("{} {}", role.label(), i + 1))
            })
            .collect(),
        other => catalog::variants(other)
            .iter()
            .map(|v| v.name.to_string())
            .collect(),
    };
    if labels.len() > 1 {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        let caption = gtk::Label::new(Some(match role {
            Role::Amp => "Modelo de amp",
            Role::Cabinet => "Caixa (IR)",
            Role::Modulation => "Tipo de modulação",
            Role::Delay => "Tipo de delay",
            Role::Reverb => "Tipo de reverb",
            Role::Fx => "Tipo de dinâmica",
        }));
        caption.add_css_class("dim-label");
        caption.set_size_request(150, -1);
        caption.set_xalign(0.0);
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let dropdown = gtk::DropDown::from_strings(&refs);
        dropdown.set_selected(unit.min(labels.len() as u8 - 1) as u32);
        dropdown.set_hexpand(true);
        {
            let state = state.clone();
            let suppress = suppress.clone();
            let ui_sink = ui_sink();
            dropdown.connect_selected_notify(move |dd| {
                if suppress.get() {
                    return;
                }
                if let Some(t) = state.borrow_mut().table.as_mut() {
                    t.set_unit(patch_index, module, dd.selected() as u8);
                }
                bump(&ui_sink, &state);
            });
        }
        row.append(&caption);
        row.append(&dropdown);
        outer.append(&row);
    }

    // Parameters.
    let grid = gtk::Grid::builder()
        .row_spacing(6)
        .column_spacing(12)
        .margin_top(4)
        .build();

    let mut shown = 0usize;
    for param in 0..PARAMS_PER_MODULE {
        let value = patch.param(module, param).unwrap_or(0);
        let name = catalog::param_name(role, unit, param)
            .map(|raw| catalog::display_name(raw).to_string())
            .unwrap_or_else(|| format!("Parâmetro {}", param + 1));
        // Hide unused slots that are zero and unnamed, to reduce noise.
        if catalog::param_name(role, unit, param).is_none() && value == 0 {
            continue;
        }
        let label = gtk::Label::new(Some(&name));
        label.set_xalign(0.0);
        label.set_size_request(120, -1);
        let scale = gtk::Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.set_value(value as f64);
        scale.set_tooltip_text(Some(&format!(
            "{} · faixa 0–100 · offset 0x{:02x}",
            name,
            Patch::param_offset(module, param)
        )));
        let spin = gtk::SpinButton::with_range(0.0, 100.0, 1.0);
        spin.set_value(value as f64);
        spin.set_width_chars(4);
        spin.set_numeric(true);
        {
            let spin_c = spin.clone();
            let state = state.clone();
            let suppress = suppress.clone();
            let ui_sink = ui_sink();
            scale.connect_value_changed(move |s| {
                if suppress.get() {
                    return;
                }
                let v = s.value().round() as u16;
                spin_c.set_value(v as f64);
                if let Some(t) = state.borrow_mut().table.as_mut() {
                    t.set_param(patch_index, module, param, v);
                }
                bump(&ui_sink, &state);
            });
        }
        {
            let scale_c = scale.clone();
            let state = state.clone();
            let suppress = suppress.clone();
            let ui_sink = ui_sink();
            spin.connect_value_changed(move |s| {
                if suppress.get() {
                    return;
                }
                let v = s.value().round() as u16;
                scale_c.set_value(v as f64);
                if let Some(t) = state.borrow_mut().table.as_mut() {
                    t.set_param(patch_index, module, param, v);
                }
                bump(&ui_sink, &state);
            });
        }
        grid.attach(&label, 0, shown as i32, 1, 1);
        grid.attach(&scale, 1, shown as i32, 1, 1);
        grid.attach(&spin, 2, shown as i32, 1, 1);
        shown += 1;
    }
    if shown > 0 {
        if !on {
            grid.set_opacity(0.55);
        }
        outer.append(&grid);
    }

    frame.set_child(Some(&outer));
    frame
}

/// Cheap "dirty" indicator without a full refresh (avoids rebuilding widgets
/// while a slider is being dragged).
thread_local! {
    static DIRTY_LABEL: RefCell<Option<gtk::Label>> = const { RefCell::new(None) };
    static WRITE_BUTTON: RefCell<Option<gtk::Button>> = const { RefCell::new(None) };
}

fn register_dirty_label(label: &gtk::Label) {
    DIRTY_LABEL.with(|d| *d.borrow_mut() = Some(label.clone()));
}

fn register_write_button(button: &gtk::Button) {
    WRITE_BUTTON.with(|b| *b.borrow_mut() = Some(button.clone()));
}

struct UiSink;

fn ui_sink() -> UiSink {
    UiSink
}

fn bump(_sink: &UiSink, state: &Shared) {
    let dirty = state.borrow().dirty();
    DIRTY_LABEL.with(|d| {
        if let Some(label) = d.borrow().as_ref() {
            label.set_text(if dirty { "alterações não salvas" } else { "" });
        }
    });
    WRITE_BUTTON.with(|b| {
        if let Some(button) = b.borrow().as_ref() {
            button.set_sensitive(dirty);
        }
    });
}

fn build_window(app: &Application) -> (ApplicationWindow, Ui, Shared) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("TANK MINI")
        .default_width(1240)
        .default_height(860)
        .build();

    let header = gtk::HeaderBar::new();
    let pill = gtk::Label::new(Some("conectando…"));
    pill.add_css_class("caption");
    pill.add_css_class("dim-label");
    header.set_title_widget(Some(&pill));

    let map = gtk::Button::with_label("Como funciona");
    {
        let window_c = window.clone();
        map.connect_clicked(move |_| {
            let dialog = gtk::MessageDialog::builder()
                .transient_for(&window_c)
                .modal(true)
                .message_type(gtk::MessageType::Info)
                .buttons(gtk::ButtonsType::Close)
                .build();
            dialog.set_markup(ABOUT_MAP);
            dialog.present();
        });
    }
    header.pack_end(&map);
    window.set_titlebar(Some(&header));

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);

    // --- welcome page -----------------------------------------------------
    let welcome = GtkBox::new(Orientation::Vertical, 18);
    welcome.set_valign(gtk::Align::Center);
    welcome.set_halign(gtk::Align::Center);
    welcome.set_margin_top(40);
    welcome.set_margin_bottom(40);
    welcome.set_margin_start(40);
    welcome.set_margin_end(40);
    let welcome_text = gtk::Label::new(None);
    welcome_text.set_markup(WELCOME);
    welcome_text.set_wrap(true);
    welcome_text.set_max_width_chars(64);
    welcome_text.set_xalign(0.0);
    let connect = gtk::Button::with_label("Detectar pedaleira");
    connect.add_css_class("suggested-action");
    connect.add_css_class("pill");
    connect.set_halign(gtk::Align::Start);
    welcome.append(&welcome_text);
    welcome.append(&connect);
    stack.add_named(&welcome, Some("welcome"));

    // --- editor page ------------------------------------------------------
    let editor = GtkBox::new(Orientation::Vertical, 0);

    let paned = gtk::Paned::new(Orientation::Horizontal);
    paned.set_vexpand(true);

    let rail = GtkBox::new(Orientation::Vertical, 6);
    rail.set_size_request(250, -1);
    let rail_title = gtk::Label::new(Some("Meus presets"));
    rail_title.set_xalign(0.0);
    rail_title.add_css_class("heading");
    rail_title.set_margin_start(14);
    rail_title.set_margin_top(10);
    rail.append(&rail_title);
    let rail_hint = gtk::Label::new(Some("21 sons na pedaleira · 7 bancos × A/B/C"));
    rail_hint.set_xalign(0.0);
    rail_hint.add_css_class("caption");
    rail_hint.add_css_class("dim-label");
    rail_hint.set_margin_start(14);
    rail_hint.set_margin_end(8);
    rail_hint.set_wrap(true);
    rail.append(&rail_hint);
    let preset_list = gtk::ListBox::new();
    preset_list.set_selection_mode(gtk::SelectionMode::Single);
    preset_list.add_css_class("navigation-sidebar");
    let list_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&preset_list)
        .build();
    rail.append(&list_scroll);
    let play = gtk::Button::with_label("Tocar na pedaleira");
    play.set_margin_start(14);
    play.set_margin_end(14);
    play.set_margin_bottom(12);
    rail.append(&play);
    paned.set_start_child(Some(&rail));

    let centre = GtkBox::new(Orientation::Vertical, 6);
    centre.set_margin_top(10);
    centre.set_margin_start(18);
    centre.set_margin_end(18);

    let preset_title = gtk::Label::new(None);
    preset_title.set_xalign(0.0);
    let preset_sub = gtk::Label::new(None);
    preset_sub.set_xalign(0.0);
    preset_sub.add_css_class("dim-label");
    preset_sub.add_css_class("caption");
    centre.append(&preset_title);
    centre.append(&preset_sub);

    let chain_box = GtkBox::new(Orientation::Horizontal, 4);
    chain_box.set_margin_top(6);
    centre.append(&chain_box);

    let modules_box = GtkBox::new(Orientation::Vertical, 10);
    modules_box.set_margin_bottom(24);
    let modules_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&modules_box)
        .build();
    centre.append(&modules_scroll);

    // Advanced page inside the same stack area.
    let tabs = gtk::Stack::new();
    tabs.set_vexpand(true);
    tabs.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    tabs.add_titled(&centre, Some("simple"), "Efeitos");

    let advanced = GtkBox::new(Orientation::Vertical, 8);
    advanced.set_margin_top(10);
    advanced.set_margin_start(18);
    advanced.set_margin_end(18);
    let note = gtk::Label::new(Some(
        "Bytes crus do preset selecionado. Offsets: 0x02 cadeia · 0x08 ligado · 0x0e tipo · 0x14 parâmetros (u16 LE).",
    ));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    note.add_css_class("caption");
    advanced.append(&note);
    let hex_view = gtk::TextView::new();
    hex_view.set_editable(false);
    hex_view.set_monospace(true);
    let hex_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&hex_view)
        .build();
    advanced.append(&hex_scroll);
    tabs.add_titled(&advanced, Some("advanced"), "Avançado");

    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&tabs));
    let bar = GtkBox::new(Orientation::Horizontal, 0);
    bar.set_halign(gtk::Align::Start);
    bar.append(&switcher);

    let centre_wrap = GtkBox::new(Orientation::Vertical, 0);
    centre_wrap.append(&bar);
    centre_wrap.append(&tabs);
    paned.set_end_child(Some(&centre_wrap));
    editor.append(&paned);

    // Action bar.
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_margin_top(10);
    actions.set_margin_bottom(10);
    actions.set_margin_start(18);
    actions.set_margin_end(18);
    let reload = gtk::Button::with_label("Recarregar da pedaleira");
    let backup = gtk::Button::with_label("Criar backup");
    let restore = gtk::Button::with_label("Restaurar backup…");
    let write = gtk::Button::with_label("Salvar na pedaleira");
    write.add_css_class("suggested-action");
    let dirty = gtk::Label::new(None);
    dirty.add_css_class("caption");
    dirty.add_css_class("dim-label");
    actions.append(&reload);
    actions.append(&backup);
    actions.append(&restore);
    actions.append(&dirty);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    actions.append(&spacer);
    actions.append(&write);
    editor.append(&actions);

    let status = gtk::Label::new(Some("Pronto"));
    status.set_xalign(0.0);
    status.set_margin_start(18);
    status.set_margin_end(18);
    status.set_margin_bottom(10);
    status.add_css_class("caption");
    status.add_css_class("dim-label");
    editor.append(&status);

    stack.add_named(&editor, Some("editor"));
    window.set_child(Some(&stack));

    register_dirty_label(&dirty);
    register_write_button(&write);

    let ui = Ui {
        stack,
        device_pill: pill,
        preset_list,
        preset_title,
        preset_sub,
        chain_box,
        modules_box,
        hex_view,
        dirty,
        status,
        reload,
        backup,
        write,
        restore,
        play,
        connect,
    };
    let state: Shared = Rc::new(RefCell::new(State {
        table: None,
        original: None,
        selected: 0,
        active_slot: None,
        firmware: String::new(),
        node: String::new(),
        names: Vec::new(),
    }));

    (window, ui, state)
}

fn connect(ui: &Ui, state: &Shared, suppress: &Rc<Cell<bool>>) {
    // First run: detect.
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let suppress = suppress.clone();
        ui.clone().connect.connect_clicked(move |_| {
            match load_from_device(&state) {
                Ok(()) => set_status(&ui_c, "Pedaleira conectada. Escolha um preset."),
                Err(e) => set_status(&ui_c, &e),
            }
            refresh(&ui_c, &state, &suppress);
        });
    }

    // Preset selection.
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

    // Reload.
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

    // Backup.
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

    // Write the selected preset.
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let suppress = suppress.clone();
        ui.clone().write.connect_clicked(move |_| {
            let (bytes, selected, snapshot) = {
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
                    .write_patch(selected as u8, &bytes)
                    .map_err(|e| e.to_string())?;
                std::thread::sleep(Duration::from_millis(150));
                let readback = pedal
                    .read_patch(selected as u8)
                    .map_err(|e| e.to_string())?
                    .unwrap_or_default();
                Ok((ack, readback == bytes))
            });
            match result {
                Ok((0, true)) => {
                    state.borrow_mut().original = Some(snapshot);
                    set_status(
                        &ui_c,
                        &format!("Preset {} gravado e conferido na pedaleira.", SLOT_LABELS[selected]),
                    );
                }
                Ok((ack, verified)) => set_status(
                    &ui_c,
                    &format!("Gravação não confirmada (ack 0x{ack:02x}, verificação: {verified})."),
                ),
                Err(e) => set_status(&ui_c, &format!("Falha ao gravar: {e}")),
            }
            refresh(&ui_c, &state, &suppress);
        });
    }

    // Restore.
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
                    &format!("Restauração parcial ({}/{} blocos).", report.chunks_ok, report.chunks),
                ),
                Err(e) => set_status(&ui_c, &format!("Falha ao restaurar: {e}")),
            }
            refresh(&ui_c, &state, &suppress);
        });
    }

    // Play the selected preset on the pedal.
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let suppress = suppress.clone();
        ui.clone().play.connect_clicked(move |_| {
            let selected = state.borrow().selected as u8;
            let result = with_pedal(|pedal| {
                pedal
                    .select_slot(selected)
                    .map_err(|e| e.to_string())
                    .and_then(|ok| ok.then_some(()).ok_or_else(|| "recusado".to_string()))
            });
            match result {
                Ok(()) => {
                    std::thread::sleep(Duration::from_millis(200));
                    state.borrow_mut().active_slot = Some(selected);
                    set_status(
                        &ui_c,
                        &format!(
                            "A pedaleira está tocando o preset {}.",
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
            Ok(()) => set_status(&ui, "Pedaleira conectada."),
            Err(e) => set_status(&ui, &e),
        }
        refresh(&ui, &state, &suppress);
        window.present();
    });

    app.run()
}
