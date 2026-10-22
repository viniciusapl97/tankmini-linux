//! TANK MINI — GTK4 control app for the M-VAVE guitar pedal.
//!
//! Visual language: a warm dark "pedalboard" surface, one accent colour per
//! effect family, knobs instead of anonymous sliders, and a stylised pedal drawn
//! in the header so the first glance already says "this is a guitar pedal app".
//!
//! Structure: welcome → preset rail (bank grouped) → six effect cards with named
//! knobs → fixed action bar. Raw bytes live in the "Avançado" tab.

mod knob;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, gio, glib, Application, ApplicationWindow, Box as GtkBox, Orientation};
use tankmini_core::backup::BackupStore;
use tankmini_core::catalog::{self, Role};
use tankmini_core::discovery::find_pedal;
use tankmini_core::names::PresetNames;
use tankmini_core::patch::{Patch, PresetTable, PATCH_COUNT, SLOT_LABELS};
use tankmini_core::Pedal;

use knob::{palette, Knob};

const APP_ID: &str = "dev.v1ni.TankMini";
const MODULE_COUNT: usize = 6;
const PARAMS_PER_MODULE: usize = 6;

const CSS: &str = r#"
window {
  background: linear-gradient(160deg, #17130f 0%, #100d0b 55%, #0b0908 100%);
}
headerbar {
  background: transparent;
  box-shadow: none;
  border: none;
  min-height: 46px;
}
.hero {
  background: linear-gradient(120deg, rgba(245,158,11,0.16), rgba(167,139,250,0.10) 45%, rgba(45,212,191,0.10));
  border: 1px solid rgba(255,255,255,0.07);
  border-radius: 22px;
  padding: 16px 20px;
}
.app-title { font-size: 22px; font-weight: 800; letter-spacing: 0.4px; }
.app-sub { font-size: 12px; color: rgba(236,229,222,0.62); }
.pill {
  background: rgba(255,255,255,0.07);
  border-radius: 999px;
  padding: 3px 11px;
  font-size: 11.5px;
  font-weight: 600;
}
.pill-active { background: rgba(52,211,153,0.18); color: #6ee7b7; }
.card {
  background: rgba(255,255,255,0.035);
  border: 1px solid rgba(255,255,255,0.07);
  border-radius: 20px;
  transition: background 160ms ease, border-color 160ms ease;
}
.card:hover {
  background: rgba(255,255,255,0.055);
  border-color: rgba(255,255,255,0.13);
}
.card-off { opacity: 0.55; }
.family-strip {
  border-radius: 20px 20px 0 0;
  min-height: 4px;
  background: rgba(255,255,255,0.10);
}
.effect-name { font-size: 16px; font-weight: 700; }
.effect-role { font-size: 11px; color: rgba(236,229,222,0.55); }
.preset-row { border-radius: 12px; padding: 4px 6px; }
.preset-row:selected { background: rgba(245,158,11,0.18); }
.bank-label { font-size: 10.5px; letter-spacing: 1.2px; color: rgba(236,229,222,0.45); }
.primary {
  background-image: linear-gradient(120deg, #f59e0b, #f97316);
  color: #1a1208;
  font-weight: 700;
  border-radius: 999px;
  padding: 6px 20px;
}
.primary:disabled { background-image: none; background: rgba(255,255,255,0.06); color: rgba(255,255,255,0.35); }
.ghost { background: rgba(255,255,255,0.05); border-radius: 999px; padding: 5px 14px; }
.chain-chip { border-radius: 999px; padding: 4px 12px; font-size: 11.5px; font-weight: 700;
  background: rgba(255,255,255,0.07); }
.strip-fx { background: #34d399; }
.strip-amp { background: #f59e0b; }
.strip-mod { background: #a78bfa; }
.strip-dly { background: #60a5fa; }
.strip-rvb { background: #2dd4bf; }
.strip-cab { background: #fb7185; }
"#;

const WELCOME: &str = r#"<span size="xx-large" weight="bold">Sua pedaleira, na tela.</span>

<span size="large">Controle o TANK MINI pelo computador: escolha um som, gire os botões
e o timbre muda na hora — sem app de celular.</span>

<b>1.</b> Conecte a pedaleira no USB
<b>2.</b> Escolha um dos 21 sons (7 bancos × A/B/C)
<b>3.</b> Gire os botões de cada efeito
<b>4.</b> Clique em <b>Salvar na pedaleira</b> e toque

Antes de gravar, o app guarda um backup e confere lendo os bytes de volta."#;

const ABOUT: &str = r#"<b>O que é cada bloco</b>

A pedaleira tem 6 blocos em série, e você pode mudar a ordem deles. Cada bloco
tem um tipo e até 6 botões de 0 a 100:

• <b>Dinâmica</b> — portão de ruído, boost ou compressor
• <b>Amp</b> — modelo de pré-amplificador (20 modelos, nomes vindos da pedaleira)
• <b>Modulação</b> — chorus, phaser, tremolo, flanger, vibrato, univibe, autofiltro
• <b>Delay</b> — analog, duck, dual, lofi
• <b>Reverb</b> — room, hall, swell, spring, shimmer, cloud
• <b>Cabinet</b> — caixa simulada (IR), 20 opções

<b>Como isso funciona por dentro</b>

Tudo foi obtido por engenharia reversa do protocolo USB-MIDI e do app oficial:
cada preset tem 92 bytes — ordem dos blocos, tipo de cada bloco e 36 parâmetros.

<b>Ainda não fecha</b>

• Salvar na memória permanente da pedaleira (o comando de flash ainda não foi
  identificado): use Backup para guardar seus ajustes.
• Alguns nomes de parâmetros de dinâmica e cabinet são inferidos."#;

struct State {
    table: Option<PresetTable>,
    original: Option<PresetTable>,
    selected: usize,
    active_slot: Option<u8>,
    firmware: String,
    node: String,
    cab_names: Vec<String>,
    preset_names: PresetNames,
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
    status_pill: gtk::Label,
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
    rename: gtk::MenuButton,
    rename_entry: gtk::Entry,
    rename_save: gtk::Button,
    export_btn: gtk::Button,
    import_btn: gtk::Button,
    copy_target: gtk::DropDown,
    copy_confirm: gtk::Button,
    factory_btn: gtk::Button,
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
    st.cab_names = names;
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

/// A small stylised pedal: body, two knobs, three footswitches and a LED.
fn pedal_art(size: i32, accent: &str) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(size);
    area.set_content_height(size * 9 / 10);
    let accent = knob::rgba(accent);
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        let pad = 3.0;
        // Body.
        let x = pad;
        let y = pad;
        let width = w - pad * 2.0;
        let height = h - pad * 2.0;
        let r = width * 0.16;
        rounded_rect(cr, x, y, width, height, r);
        cr.set_source_rgba(0.10, 0.08, 0.07, 1.0);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(accent.red() as f64, accent.green() as f64, accent.blue() as f64, 0.55);
        cr.set_line_width(1.6);
        let _ = cr.stroke();

        // Knobs.
        cr.set_source_rgba(accent.red() as f64, accent.green() as f64, accent.blue() as f64, 0.95);
        for (i, kx) in [x + width * 0.30, x + width * 0.70].iter().enumerate() {
            cr.arc(*kx, y + height * 0.30, width * 0.11, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
            cr.set_source_rgba(0.08, 0.07, 0.06, 1.0);
            cr.arc(*kx, y + height * 0.30, width * 0.055, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
            cr.set_source_rgba(accent.red() as f64, accent.green() as f64, accent.blue() as f64, 0.95);
            let _ = i;
        }

        // Footswitches.
        for i in 0..3 {
            let cx = x + width * (0.24 + 0.26 * i as f64);
            let cy = y + height * 0.72;
            cr.set_source_rgba(0.86, 0.84, 0.80, 0.92);
            cr.arc(cx, cy, width * 0.075, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
            cr.set_source_rgba(0.18, 0.16, 0.15, 1.0);
            cr.arc(cx, cy, width * 0.045, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
        }
    });
    area
}

fn rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

fn refresh(ui: &Ui, state: &Shared, suppress: &Rc<Cell<bool>>) {
    suppress.set(true);
    {
        let st = state.borrow();
        let connected = st.table.is_some();
        ui.stack
            .set_visible_child_name(if connected { "editor" } else { "welcome" });
        ui.status_pill.set_text(&if connected {
            format!(
                "{} · preset {}",
                st.firmware,
                st.active_slot
                    .map(|s| SLOT_LABELS.get(s as usize).copied().unwrap_or("?"))
                    .unwrap_or("?")
            )
        } else {
            "desconectada".to_string()
        });
        ui.status_pill.set_tooltip_text(Some(&st.node));

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
                    let label = gtk::Label::new(Some(&format!("BANCO {}", i / 3 + 1)));
                    label.set_xalign(0.0);
                    label.add_css_class("bank-label");
                    label.set_margin_top(8);
                    label.set_margin_start(6);
                    header.set_child(Some(&label));
                    ui.preset_list.append(&header);
                }
                let row = gtk::ListBoxRow::new();
                row.add_css_class("preset-row");
                let line = GtkBox::new(Orientation::Vertical, 0);
                let title = gtk::Label::new(None);
                title.set_markup(&format!(
                    "<b>{}</b>{}",
                    gtk::glib::markup_escape_text(&st.preset_names.label(i)),
                    if active {
                        "  <span foreground='#6ee7b7' size='small'>● tocando</span>"
                    } else {
                        ""
                    }
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
                let sub = gtk::Label::new(Some(&chain.join(" · ")));
                sub.add_css_class("caption");
                sub.add_css_class("dim-label");
                sub.set_xalign(0.0);
                line.append(&title);
                line.append(&sub);
                row.set_child(Some(&line));
                ui.preset_list.append(&row);
            }
            if let Some(row) = ui.preset_list.row_at_index(st.selected as i32) {
                ui.preset_list.select_row(Some(&row));
            }
        }

        if let Some(table) = &st.table {
            if let Some(patch) = table.patch(st.selected) {
                let active = st.active_slot.map(|s| s as usize) == Some(st.selected);
                ui.preset_title.set_markup(&format!(
                    "<span size='xx-large' weight='bold'>{}</span>",
                    gtk::glib::markup_escape_text(&st.preset_names.label(st.selected))
                ));
                ui.preset_sub.set_text(if active {
                    "Tocando agora na pedaleira"
                } else {
                    "Selecione para ver os detalhes e toque para ouvir"
                });
                ui.play.set_sensitive(!active);
                ui.play.set_label(if active { "Tocando agora" } else { "Tocar na pedaleira" });
            }
        }

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
                    let chip = gtk::Label::new(None);
                    chip.set_margin_end(2);
                    chip.set_markup(&format!(
                        "<span foreground='{}'>●</span>  <b>{}</b>  {}",
                        palette::for_role(role),
                        pos + 1,
                        gtk::glib::markup_escape_text(&patch.effect_name(*module as usize, &st.cab_names))
                    ));
                    chip.add_css_class("chain-chip");
                    chip.set_tooltip_text(Some(role.hint()));
                    let ctx = chip.style_context();
                    ctx.add_provider(&css_provider(), gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
                    ui.chain_box.append(&chip);
                    if pos + 1 < MODULE_COUNT {
                        let arrow = gtk::Label::new(Some("›"));
                        arrow.add_css_class("dim-label");
                        ui.chain_box.append(&arrow);
                    }
                }
                for (pos, module) in chain.iter().enumerate() {
                    let card = module_card(state, suppress, ui, st.selected, pos, *module as usize, &patch, &st.cab_names);
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
    ui: &Ui,
    patch_index: usize,
    position: usize,
    module: usize,
    patch: &Patch<'_>,
    names: &[String],
) -> gtk::Frame {
    let role = Role::for_slot(module);
    let unit = patch.unit(module);
    let on = patch.is_on(module);
    let accent = palette::for_role(role);

    let frame = gtk::Frame::new(None);
    frame.add_css_class("card");
    if !on {
        frame.add_css_class("card-off");
    }

    let outer = GtkBox::new(Orientation::Vertical, 0);

    // Coloured strip at the top of the card.
    let strip = GtkBox::new(Orientation::Horizontal, 0);
    strip.add_css_class("family-strip");
    strip.set_size_request(-1, 4);
    strip.set_widget_name("family-strip");
    let provider = css_provider();
    strip.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    let ctx = strip.style_context();
    ctx.add_class(&format!("strip-{}", role_class(role)));
    outer.append(&strip);

    let body = GtkBox::new(Orientation::Vertical, 10);
    body.set_margin_top(12);
    body.set_margin_bottom(16);
    body.set_margin_start(16);
    body.set_margin_end(16);

    let head = GtkBox::new(Orientation::Horizontal, 10);
    let titles = GtkBox::new(Orientation::Vertical, 2);
    let effect_name = patch.effect_name(module, names);
    let title = gtk::Label::new(None);
    title.set_markup(&format!(
        "<span size='large' weight='bold' foreground='{accent}'>{}</span>",
        gtk::glib::markup_escape_text(&effect_name)
    ));
    title.set_xalign(0.0);
    let subtitle = gtk::Label::new(Some(&format!(
        "{} · bloco {} de 6",
        role.label(),
        position + 1
    )));
    subtitle.add_css_class("effect-role");
    subtitle.set_xalign(0.0);
    titles.append(&title);
    titles.append(&subtitle);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let enabled = gtk::Switch::new();
    enabled.set_active(on);
    enabled.set_valign(gtk::Align::Center);
    enabled.set_tooltip_text(Some("Ligar/desligar este bloco"));
    {
        let state = state.clone();
        let suppress = suppress.clone();
        let ui_sink = UiSink;
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
    head.append(&titles);
    head.append(&spacer);

    // Chain reordering: the pedal lets you change the order of the six blocks.
    let arrows = GtkBox::new(Orientation::Vertical, 0);
    arrows.set_valign(gtk::Align::Center);
    let up = gtk::Button::from_icon_name("go-up-symbolic");
    up.add_css_class("flat");
    up.set_tooltip_text(Some("Mover este bloco para antes do anterior"));
    up.set_sensitive(position > 0);
    let down = gtk::Button::from_icon_name("go-down-symbolic");
    down.add_css_class("flat");
    down.set_tooltip_text(Some("Mover este bloco para depois do próximo"));
    down.set_sensitive(position + 1 < MODULE_COUNT);
    {
        let state = state.clone();
        let suppress = suppress.clone();
        let ui_c = ui.clone();
        up.connect_clicked(move |_| {
            if let Some(t) = state.borrow_mut().table.as_mut() {
                t.swap_chain_positions(patch_index, position, position - 1);
            }
            refresh(&ui_c, &state, &suppress);
        });
    }
    {
        let state = state.clone();
        let suppress = suppress.clone();
        let ui_c = ui.clone();
        down.connect_clicked(move |_| {
            if let Some(t) = state.borrow_mut().table.as_mut() {
                t.swap_chain_positions(patch_index, position, position + 1);
            }
            refresh(&ui_c, &state, &suppress);
        });
    }
    arrows.append(&up);
    arrows.append(&down);
    head.append(&arrows);
    head.append(&enabled);
    body.append(&head);

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
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let dropdown = gtk::DropDown::from_strings(&refs);
        dropdown.set_selected(unit.min(labels.len() as u8 - 1) as u32);
        dropdown.set_hexpand(true);
        {
            let state = state.clone();
            let suppress = suppress.clone();
            let ui_sink = UiSink;
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
        body.append(&dropdown);
    }

    // Knobs.
    let knobs = gtk::FlowBox::new();
    knobs.set_selection_mode(gtk::SelectionMode::None);
    knobs.set_column_spacing(10);
    knobs.set_row_spacing(6);
    knobs.set_max_children_per_line(6);
    knobs.set_homogeneous(false);
    knobs.set_valign(gtk::Align::Start);
    knobs.set_halign(gtk::Align::Start);
    knobs.set_margin_top(4);

    let mut shown = 0usize;
    for param in 0..PARAMS_PER_MODULE {
        let value = patch.param(module, param).unwrap_or(0);
        let raw_name = catalog::param_name(role, unit, param);
        if raw_name.is_none() && value == 0 {
            continue;
        }
        let label = raw_name
            .map(catalog::display_name)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Ajuste {}", param + 1));
        let state_c = state.clone();
        let suppress_c = suppress.clone();
        let knob = Knob::new(&label, value as f64, 100.0, accent, move |v| {
            if suppress_c.get() {
                return;
            }
            if let Some(t) = state_c.borrow_mut().table.as_mut() {
                t.set_param(patch_index, module, param, v);
            }
            bump(&UiSink, &state_c);
        });
        let holder = GtkBox::new(Orientation::Vertical, 0);
        holder.set_halign(gtk::Align::Center);
        holder.append(knob.widget());
        knobs.insert(&holder, -1);
        shown += 1;
    }
    if shown > 0 {
        body.append(&knobs);
    }

    outer.append(&body);
    frame.set_child(Some(&outer));
    frame
}

fn role_class(role: Role) -> &'static str {
    match role {
        Role::Fx => "fx",
        Role::Amp => "amp",
        Role::Modulation => "mod",
        Role::Delay => "dly",
        Role::Reverb => "rvb",
        Role::Cabinet => "cab",
    }
}

thread_local! {
    static DIRTY_LABEL: RefCell<Option<gtk::Label>> = const { RefCell::new(None) };
    static WRITE_BUTTON: RefCell<Option<gtk::Button>> = const { RefCell::new(None) };
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

fn register_dirty_label(label: &gtk::Label) {
    DIRTY_LABEL.with(|d| *d.borrow_mut() = Some(label.clone()));
}

fn register_write_button(button: &gtk::Button) {
    WRITE_BUTTON.with(|b| *b.borrow_mut() = Some(button.clone()));
}

fn css_provider() -> gtk::CssProvider {
    PROVIDER.with(|p| {
        if let Some(provider) = p.borrow().as_ref() {
            return provider.clone();
        }
        let provider = gtk::CssProvider::new();
        provider.load_from_data(CSS);
        *p.borrow_mut() = Some(provider.clone());
        provider
    })
}

struct UiSink;

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
        .default_width(1180)
        .default_height(820)
        .build();

    let header = gtk::HeaderBar::new();
    header.add_css_class("flat");
    let title = gtk::Label::new(Some("TANK MINI"));
    title.add_css_class("app-title");
    let pill = gtk::Label::new(Some("conectando…"));
    pill.add_css_class("pill");
    header.set_title_widget(Some(&title));
    let about = gtk::Button::with_label("Como funciona");
    about.add_css_class("ghost");
    {
        let window_c = window.clone();
        about.connect_clicked(move |_| {
            let dialog = gtk::MessageDialog::builder()
                .transient_for(&window_c)
                .modal(true)
                .message_type(gtk::MessageType::Info)
                .buttons(gtk::ButtonsType::Close)
                .build();
            dialog.set_markup(ABOUT);
            dialog.present();
        });
    }
    // Overflow menu: the power features stay one click away without cluttering
    // the main flow.
    let menu_button = gtk::MenuButton::new();
    menu_button.set_icon_name("open-menu-symbolic");
    menu_button.set_tooltip_text(Some("Mais ações"));
    let menu_box = GtkBox::new(Orientation::Vertical, 2);
    menu_box.set_margin_top(6);
    menu_box.set_margin_bottom(6);
    menu_box.set_margin_start(6);
    menu_box.set_margin_end(6);
    let export_btn = gtk::Button::with_label("Exportar preset…");
    let import_btn = gtk::Button::with_label("Importar preset…");
    let copy_btn = gtk::MenuButton::builder()
        .label("Copiar para outro slot…")
        .build();
    let factory_btn = gtk::Button::with_label("Restaurar de fábrica…");
    for b in [&export_btn, &import_btn, &factory_btn] {
        b.add_css_class("flat");
        b.set_halign(gtk::Align::Fill);
        menu_box.append(b);
    }
    copy_btn.add_css_class("flat");
    copy_btn.set_halign(gtk::Align::Fill);
    menu_box.append(&copy_btn);
    let menu_popover = gtk::Popover::new();
    menu_popover.set_child(Some(&menu_box));

    // Copy dialog: pick the destination slot.
    let copy_labels: Vec<String> = (0..PATCH_COUNT)
        .map(|i| SLOT_LABELS.get(i).copied().unwrap_or("?").to_string())
        .collect();
    let copy_refs: Vec<&str> = copy_labels.iter().map(String::as_str).collect();
    let copy_target = gtk::DropDown::from_strings(&copy_refs);
    let copy_confirm = gtk::Button::with_label("Copiar");
    copy_confirm.add_css_class("primary");
    let copy_row = GtkBox::new(Orientation::Horizontal, 6);
    copy_row.set_margin_top(8);
    copy_row.set_margin_bottom(8);
    copy_row.set_margin_start(8);
    copy_row.set_margin_end(8);
    copy_row.append(&copy_target);
    copy_row.append(&copy_confirm);
    let copy_popover = gtk::Popover::new();
    copy_popover.set_child(Some(&copy_row));
    copy_btn.set_popover(Some(&copy_popover));
    menu_button.set_popover(Some(&menu_popover));
    header.pack_end(&menu_button);
    header.pack_end(&pill);
    header.pack_end(&about);
    window.set_titlebar(Some(&header));

    let css = css_provider();
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("display"),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(220);

    // --- welcome ----------------------------------------------------------
    let welcome = GtkBox::new(Orientation::Vertical, 16);
    welcome.set_valign(gtk::Align::Center);
    welcome.set_halign(gtk::Align::Center);
    welcome.set_margin_top(40);
    welcome.set_margin_bottom(40);
    welcome.set_margin_start(40);
    welcome.set_margin_end(40);

    let art = pedal_art(150, "#f59e0b");
    art.set_halign(gtk::Align::Center);
    welcome.append(&art);

    let welcome_text = gtk::Label::new(None);
    welcome_text.set_markup(WELCOME);
    welcome_text.set_wrap(true);
    welcome_text.set_max_width_chars(62);
    welcome_text.set_xalign(0.5);
    welcome_text.set_justify(gtk::Justification::Center);
    let connect = gtk::Button::with_label("Detectar pedaleira");
    connect.add_css_class("primary");
    connect.set_halign(gtk::Align::Center);
    welcome.append(&welcome_text);
    welcome.append(&connect);
    stack.add_named(&welcome, Some("welcome"));

    // --- editor -----------------------------------------------------------
    let editor = GtkBox::new(Orientation::Vertical, 0);
    let paned = gtk::Paned::new(Orientation::Horizontal);
    paned.set_vexpand(true);

    let rail = GtkBox::new(Orientation::Vertical, 6);
    rail.set_size_request(232, -1);
    rail.set_margin_start(10);
    rail.set_margin_end(4);
    let rail_title = gtk::Label::new(Some("Seus sons"));
    rail_title.set_xalign(0.0);
    rail_title.add_css_class("heading");
    rail_title.set_margin_start(6);
    rail_title.set_margin_top(8);
    rail.append(&rail_title);
    let rail_hint = gtk::Label::new(Some("21 presets na pedaleira"));
    rail_hint.set_xalign(0.0);
    rail_hint.add_css_class("caption");
    rail_hint.add_css_class("dim-label");
    rail_hint.set_margin_start(6);
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
    play.add_css_class("ghost");
    play.set_margin_start(6);
    play.set_margin_end(6);
    play.set_margin_bottom(10);
    rail.append(&play);
    paned.set_start_child(Some(&rail));

    let centre = GtkBox::new(Orientation::Vertical, 6);
    centre.set_margin_top(8);
    centre.set_margin_start(16);
    centre.set_margin_end(16);

    let hero = GtkBox::new(Orientation::Horizontal, 14);
    hero.add_css_class("hero");
    hero.set_margin_bottom(8);
    let hero_art = pedal_art(58, "#f59e0b");
    let hero_text = GtkBox::new(Orientation::Vertical, 0);
    let preset_title = gtk::Label::new(None);
    preset_title.set_xalign(0.0);
    let preset_sub = gtk::Label::new(None);
    preset_sub.add_css_class("app-sub");
    preset_sub.set_xalign(0.0);
    preset_sub.set_margin_top(3);
    hero_text.append(&preset_title);
    hero_text.append(&preset_sub);
    let rename = gtk::MenuButton::builder().label("Renomear").build();
    rename.add_css_class("ghost");
    rename.add_css_class("flat");
    rename.set_valign(gtk::Align::Center);
    let rename_entry = gtk::Entry::new();
    rename_entry.set_placeholder_text(Some("Ex.: Crunch da música X"));
    rename_entry.set_width_chars(22);
    let rename_save = gtk::Button::with_label("Salvar");
    rename_save.add_css_class("primary");
    let rename_row = GtkBox::new(Orientation::Horizontal, 6);
    rename_row.set_margin_top(8);
    rename_row.set_margin_bottom(8);
    rename_row.set_margin_start(8);
    rename_row.set_margin_end(8);
    rename_row.append(&rename_entry);
    rename_row.append(&rename_save);
    let rename_popover = gtk::Popover::new();
    rename_popover.set_child(Some(&rename_row));
    rename.set_popover(Some(&rename_popover));
    let hero_spacer = GtkBox::new(Orientation::Horizontal, 0);
    hero_spacer.set_hexpand(true);
    hero.append(&hero_art);
    hero.append(&hero_text);
    hero.append(&hero_spacer);
    hero.append(&rename);
    centre.append(&hero);

    let chain_box = GtkBox::new(Orientation::Horizontal, 6);
    centre.append(&chain_box);

    let modules_box = GtkBox::new(Orientation::Vertical, 12);
    modules_box.set_margin_bottom(26);
    let modules_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&modules_box)
        .build();
    centre.append(&modules_scroll);

    let tabs = gtk::Stack::new();
    tabs.set_vexpand(true);
    tabs.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    tabs.add_titled(&centre, Some("simple"), "Efeitos");

    let advanced = GtkBox::new(Orientation::Vertical, 8);
    advanced.set_margin_top(8);
    advanced.set_margin_start(16);
    advanced.set_margin_end(16);
    let note = gtk::Label::new(Some(
        "Bytes crus do preset. 0x02 ordem · 0x08 ligado · 0x0e tipo · 0x14 parâmetros (u16 LE).",
    ));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("caption");
    note.add_css_class("dim-label");
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

    let centre_wrap = GtkBox::new(Orientation::Vertical, 6);
    centre_wrap.append(&bar);
    centre_wrap.append(&tabs);
    paned.set_end_child(Some(&centre_wrap));
    editor.append(&paned);

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_margin_top(8);
    actions.set_margin_bottom(10);
    actions.set_margin_start(16);
    actions.set_margin_end(16);
    let reload = gtk::Button::with_label("Recarregar da pedaleira");
    let backup = gtk::Button::with_label("Criar backup");
    let restore = gtk::Button::with_label("Restaurar backup…");
    let write = gtk::Button::with_label("Salvar na pedaleira");
    write.add_css_class("primary");
    for b in [&reload, &backup, &restore] {
        b.add_css_class("ghost");
    }
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
    status.set_margin_start(16);
    status.set_margin_end(16);
    status.set_margin_bottom(14);
    status.add_css_class("caption");
    status.add_css_class("dim-label");
    editor.append(&status);

    stack.add_named(&editor, Some("editor"));
    window.set_child(Some(&stack));

    register_dirty_label(&dirty);
    register_write_button(&write);

    let ui = Ui {
        stack,
        status_pill: pill,
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
        rename,
        rename_entry,
        rename_save,
        export_btn,
        import_btn,
        copy_target,
        copy_confirm,
        factory_btn,
    };
    let state: Shared = Rc::new(RefCell::new(State {
        table: None,
        original: None,
        selected: 0,
        active_slot: None,
        firmware: String::new(),
        node: String::new(),
        cab_names: Vec::new(),
        preset_names: PresetNames::load_default(),
    }));

    (window, ui, state)
}

fn connect(ui: &Ui, state: &Shared, suppress: &Rc<Cell<bool>>, window: &ApplicationWindow) {
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let suppress = suppress.clone();
        ui.clone().connect.connect_clicked(move |_| {
            match load_from_device(&state) {
                Ok(()) => set_status(&ui_c, "Pedaleira conectada. Escolha um som."),
                Err(e) => set_status(&ui_c, &e),
            }
            refresh(&ui_c, &state, &suppress);
        });
    }
    {
        let state = state.clone();
        let ui_c = ui.clone();
        let suppress = suppress.clone();
        ui.clone().preset_list.connect_row_selected(move |_, row| {
            if suppress.get() {
                return;
            }
            if let Some(row) = row {
                let index = row.index() as usize;
                // Skip the bank header rows (not selectable, but be safe).
                if state
                    .borrow()
                    .table
                    .as_ref()
                    .and_then(|t| t.patch(index))
                    .is_none()
                {
                    return;
                }
                state.borrow_mut().selected = index;
                refresh(&ui_c, &state, &suppress);
            }
        });
    }
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
                // Persist: the app commits the table with an empty write to 0xF0000000.
                let commit = pedal.commit().map_err(|e| e.to_string())?.unwrap_or(0xff);
                Ok((ack, readback == bytes, commit))
            });
            match result {
                Ok((0, true, 0)) => {
                    state.borrow_mut().original = Some(snapshot);
                    set_status(
                        &ui_c,
                        &format!(
                            "{} gravado e salvo na memória da pedaleira.",
                            SLOT_LABELS[selected]
                        ),
                    );
                }
                Ok((0, true, commit)) => set_status(
                    &ui_c,
                    &format!("Gravado, mas a pedaleira não confirmou salvar na memória (ack 0x{commit:02x})."),
                ),
                Ok((ack, verified, _)) => set_status(
                    &ui_c,
                    &format!("Gravação não confirmada (ack 0x{ack:02x}, verificação: {verified})."),
                ),
                Err(e) => set_status(&ui_c, &format!("Falha ao gravar: {e}")),
            }
            refresh(&ui_c, &state, &suppress);
        });
    }
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
                    &format!("Restauração parcial ({}/{}).", report.chunks_ok, report.chunks),
                ),
                Err(e) => set_status(&ui_c, &format!("Falha ao restaurar: {e}")),
            }
            refresh(&ui_c, &state, &suppress);
        });
    }
    // Rename the selected preset (names live on the computer — the pedal has none).
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let suppress = suppress.clone();
        ui.clone().rename_save.connect_clicked(move |_| {
            let name = ui_c.rename_entry.text().to_string();
            let selected = {
                let mut st = state.borrow_mut();
                let selected = st.selected;
                st.preset_names.set(selected, &name);
                let _ = st.preset_names.save_default();
                selected
            };
            set_status(&ui_c, &format!("Preset {selected} renomeado."));
            if let Some(popover) = ui_c.rename.popover() {
                popover.popdown();
            }
            refresh(&ui_c, &state, &suppress);
        });
    }

    // Export the selected preset to a file.
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let window_c = window.clone();
        ui.clone().export_btn.connect_clicked(move |_| {
            let (bytes, suggested) = {
                let st = state.borrow();
                let Some(table) = st.table.as_ref() else { return };
                let Some(patch) = table.patch(st.selected) else { return };
                let label = st.preset_names.label(st.selected).replace([' ', '·'], "-");
                (patch.raw().to_vec(), format!("{label}.tankmini"))
            };
            let dialog = gtk::FileDialog::builder()
                .title("Exportar preset")
                .initial_name(&suggested)
                .build();
            let ui_c2 = ui_c.clone();
            dialog.save(Some(&window_c), gio::Cancellable::NONE, move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        match std::fs::write(&path, &bytes) {
                            Ok(()) => set_status(&ui_c2, &format!("Preset salvo em {}", path.display())),
                            Err(e) => set_status(&ui_c2, &format!("Falha ao salvar: {e}")),
                        }
                    }
                }
            });
        });
    }

    // Import a preset file (92 bytes) or a whole table (1942 bytes).
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let suppress = suppress.clone();
        let window_c = window.clone();
        ui.clone().import_btn.connect_clicked(move |_| {
            let dialog = gtk::FileDialog::builder().title("Importar preset").build();
            let ui_c2 = ui_c.clone();
            let state_c = state.clone();
            let suppress_c = suppress.clone();
            dialog.open(Some(&window_c), gio::Cancellable::NONE, move |result| {
                let Ok(file) = result else { return };
                let Some(path) = file.path() else { return };
                let bytes = match std::fs::read(&path) {
                    Ok(b) => b,
                    Err(e) => {
                        set_status(&ui_c2, &format!("Falha ao ler: {e}"));
                        return;
                    }
                };
                let selected = state_c.borrow().selected;
                let outcome = {
                    let mut st = state_c.borrow_mut();
                    match st.table.as_mut() {
                        Some(t) => t.import(selected, &bytes),
                        None => Err("sem tabela carregada".to_string()),
                    }
                };
                match outcome {
                    Ok(()) => set_status(&ui_c2, "Importado. Confira e clique em Salvar na pedaleira."),
                    Err(e) => set_status(&ui_c2, &format!("Importação recusada: {e}")),
                }
                refresh(&ui_c2, &state_c, &suppress_c);
            });
        });
    }

    // Copy the selected preset onto another slot.
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let suppress = suppress.clone();
        ui.clone().copy_confirm.connect_clicked(move |_| {
            let target = ui_c.copy_target.selected() as usize;
            let (from, ok) = {
                let mut st = state.borrow_mut();
                let from = st.selected;
                let ok = st.table.as_mut().map(|t| t.copy_patch(from, target)).unwrap_or(false);
                (from, ok)
            };
            if ok {
                set_status(
                    &ui_c,
                    &format!(
                        "Copiado de {} para {}. Salve para gravar na pedaleira.",
                        SLOT_LABELS.get(from).copied().unwrap_or("?"),
                        SLOT_LABELS.get(target).copied().unwrap_or("?")
                    ),
                );
            } else {
                set_status(&ui_c, "Escolha um slot de destino diferente do atual.");
            }
            refresh(&ui_c, &state, &suppress);
        });
    }

    // Factory reset: back to the table this firmware shipped with.
    {
        let ui_c = ui.clone();
        let state = state.clone();
        let suppress = suppress.clone();
        let window_c = window.clone();
        ui.clone().factory_btn.connect_clicked(move |_| {
            let dialog = gtk::MessageDialog::builder()
                .transient_for(&window_c)
                .modal(true)
                .message_type(gtk::MessageType::Warning)
                .buttons(gtk::ButtonsType::Cancel)
                .text("Restaurar de fábrica?")
                .secondary_text(
                    "Os 21 presets voltam ao estado de fábrica e a pedaleira grava isso na memória.                      Um backup do estado atual é criado antes.",
                )
                .build();
            dialog.add_button("Restaurar", gtk::ResponseType::Accept);
            let ui_c2 = ui_c.clone();
            let state_c = state.clone();
            let suppress_c = suppress.clone();
            dialog.connect_response(move |dialog, response| {
                dialog.close();
                if response != gtk::ResponseType::Accept {
                    return;
                }
                let factory = PresetTable::factory();
                let result = with_pedal(|pedal| {
                    if let Some(current) = pedal.read_table().map_err(|e| e.to_string())? {
                        let store = BackupStore::new(BackupStore::default_root());
                        let path = store
                            .save(&current.to_preset(), None, "antes de restaurar de fábrica")
                            .map_err(|e| e.to_string())?;
                        eprintln!("[tankmini] backup: {}", path.display());
                    }
                    let report = pedal.write_table(&factory).map_err(|e| e.to_string())?;
                    let commit = pedal.commit().map_err(|e| e.to_string())?.unwrap_or(0xff);
                    Ok((report, commit))
                });
                match result {
                    Ok((report, 0)) if report.verified => {
                        set_status(&ui_c2, "Pedaleira restaurada de fábrica.");
                        let _ = load_from_device(&state_c);
                    }
                    Ok((report, commit)) => set_status(
                        &ui_c2,
                        &format!(
                            "Restauração incompleta (verificado: {}, commit 0x{commit:02x}).",
                            report.verified
                        ),
                    ),
                    Err(e) => set_status(&ui_c2, &format!("Falha ao restaurar: {e}")),
                }
                refresh(&ui_c2, &state_c, &suppress_c);
            });
            dialog.present();
        });
    }

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
                            "A pedaleira está tocando {}.",
                            SLOT_LABELS.get(selected as usize).copied().unwrap_or("?")
                        ),
                    );
                }
                Err(e) => set_status(&ui_c, &format!("Falha ao trocar de som: {e}")),
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
        connect(&ui, &state, &suppress, &window);

        match load_from_device(&state) {
            Ok(()) => set_status(&ui, "Pedaleira conectada."),
            Err(e) => set_status(&ui, &e),
        }
        refresh(&ui, &state, &suppress);
        window.present();
    });

    app.run()
}
