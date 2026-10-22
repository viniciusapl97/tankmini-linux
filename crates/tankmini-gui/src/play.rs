//! "Tocar": the beginner view. One preset, four big knobs, quick block
//! toggles and a gallery of ready-made sounds.

use adw::prelude::*;
use tankmini_core::catalog::{Instrument, Role};
use tankmini_core::macros::Macro;
use tankmini_core::patch::SLOT_LABELS;
use tankmini_core::templates;

use crate::app::{AppRef, Redraw};
use crate::knob::{Knob, KnobSize};
use crate::widgets::{clear, dot, hbox, label, section, slot_badge, vbox};

pub fn refresh(app: &AppRef) {
    let root = &app.ui().play_box;
    clear(root);
    let page = vbox(0);
    page.set_margin_top(28);
    page.set_margin_bottom(36);
    page.set_margin_start(32);
    page.set_margin_end(32);
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(1140);
    clamp.set_tightening_threshold(700);
    clamp.set_child(Some(&page));
    root.append(&clamp);

    page.append(&hero(app));
    page.append(&macros(app));
    page.append(&quick_blocks(app));
    page.append(&sounds(app));
}

fn hero(app: &AppRef) -> gtk::Box {
    let st = app.state.borrow();
    let i = st.selected;
    let row = hbox(20);
    let badge = slot_badge(SLOT_LABELS[i], st.playing(i), true);
    row.append(&badge);

    let texts = vbox(2);
    texts.set_hexpand(true);
    texts.set_valign(gtk::Align::Center);
    let letter = ["A", "B", "C"][i % 3];
    texts.append(&label(&format!("BANCO {} · PEDAL {letter}", i / 3 + 1), &["tm-eyebrow"]));
    let title = label(&st.title(i), &["tm-title-xl"]);
    title.set_wrap(true);
    texts.append(&title);
    let state_line = hbox(8);
    state_line.set_margin_top(4);
    let (cls, text) = if st.playing(i) {
        if st.is_dirty(i) && st.settings.live_edit {
            ("live", "Tocando agora, com suas alterações")
        } else {
            ("playing", "Tocando agora")
        }
    } else if st.online() {
        ("off", "Não é o som que está tocando")
    } else {
        ("off", "Pedaleira desconectada")
    };
    state_line.append(&dot(cls));
    state_line.append(&label(text, &["tm-dim"]));
    let fx = label(&st.subtitle(i), &["tm-dim"]);
    fx.set_wrap(true);
    texts.append(&fx);
    texts.append(&state_line);
    row.append(&texts);

    let nav = hbox(6);
    nav.set_valign(gtk::Align::Center);
    let prev = gtk::Button::from_icon_name("go-previous-symbolic");
    prev.add_css_class("tm-pill");
    prev.set_tooltip_text(Some("Som anterior (Ctrl+←)"));
    prev.set_action_name(Some("win.prev"));
    let next = gtk::Button::from_icon_name("go-next-symbolic");
    next.add_css_class("tm-pill");
    next.set_tooltip_text(Some("Próximo som (Ctrl+→)"));
    next.set_action_name(Some("win.next"));
    nav.append(&prev);
    nav.append(&next);
    row.append(&nav);
    row
}

fn macros(app: &AppRef) -> gtk::Box {
    let wrap = vbox(0);
    wrap.append(&section("CONTROLES"));
    let card = hbox(0);
    card.add_css_class("tm-card");
    card.set_homogeneous(true);
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    // Knobs go in pairs so a narrow window shows 2×2, never 3 + 1.
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(2);
    flow.set_column_spacing(8);
    flow.set_row_spacing(12);
    flow.set_margin_top(18);
    flow.set_margin_bottom(18);
    flow.set_margin_start(12);
    flow.set_margin_end(12);
    let (values, sens) = {
        let st = app.state.borrow();
        let p = st.current().expect("table loaded");
        let v: Vec<u16> = Macro::ALL.iter().map(|m| m.read(&p)).collect();
        (v, st.settings.knob_sensitivity as f64)
    };
    let mut pair = hbox(8);
    pair.set_homogeneous(true);
    for (n, (m, value)) in Macro::ALL.into_iter().zip(values).enumerate() {
        let cell = vbox(4);
        cell.set_halign(gtk::Align::Center);
        let app2 = app.clone();
        let group = format!("macro-{}", m.label());
        let knob = Knob::new(m.label(), value, KnobSize::Large, sens, crate::theme::neutral_rgb, move |v, dragging| {
            app2.edit(Some(&group), Redraw::Values, |t, i| {
                m.apply(t, i, v);
            });
            if !dragging {
                app2.seal();
                // Ambience toggles blocks: keep the quick toggles honest.
                if m == Macro::Ambience {
                    let app3 = app2.clone();
                    gtk::glib::idle_add_local_once(move || refresh_toggles(&app3));
                }
            }
        });
        knob.widget().set_tooltip_text(Some(m.help()));
        cell.append(knob.widget());
        let help = label(m.help(), &["tm-small", "tm-faint"]);
        help.set_justify(gtk::Justification::Center);
        help.set_xalign(0.5);
        help.set_wrap(true);
        help.set_max_width_chars(18);
        cell.append(&help);
        pair.append(&cell);
        if n % 2 == 1 {
            flow.insert(&pair, -1);
            pair = hbox(8);
            pair.set_homogeneous(true);
        }
    }
    card.append(&flow);
    wrap.append(&card);
    wrap
}

const TOGGLES: [(usize, &str); 5] = [(0, "Dinâmica"), (1, "Amp"), (2, "Modulação"), (3, "Delay"), (4, "Reverb")];

thread_local! {
    static TOGGLE_ROW: std::cell::RefCell<Option<gtk::FlowBox>> = const { std::cell::RefCell::new(None) };
}

fn quick_blocks(app: &AppRef) -> gtk::Box {
    let wrap = vbox(0);
    let head = section("EFEITOS LIGADOS");
    wrap.append(&head);
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_column_spacing(10);
    flow.set_row_spacing(10);
    flow.set_max_children_per_line(6);
    wrap.append(&flow);
    TOGGLE_ROW.with(|t| *t.borrow_mut() = Some(flow));
    refresh_toggles(app);
    wrap
}

fn refresh_toggles(app: &AppRef) {
    let Some(flow) = TOGGLE_ROW.with(|t| t.borrow().clone()) else { return };
    while let Some(c) = flow.first_child() {
        flow.remove(&c);
    }
    let st = app.state.borrow();
    let Some(p) = st.current() else { return };
    for (module, name) in TOGGLES {
        let role = Role::for_slot(module);
        let b = gtk::ToggleButton::new();
        b.add_css_class("tm-toggle");
        let inner = hbox(8);
        let d = hbox(0);
        d.add_css_class("tm-family");
        d.add_css_class(crate::theme::family_class(role));
        d.set_valign(gtk::Align::Center);
        inner.append(&d);
        let variant = st.effect(&p, module);
        inner.append(&label(&format!("{name} · {variant}"), &[]));
        b.set_child(Some(&inner));
        b.set_active(p.is_on(module));
        if !p.is_on(module) {
            d.set_opacity(0.3);
        }
        b.set_tooltip_text(Some(role.description()));
        let app = app.clone();
        b.connect_toggled(move |b| {
            let on = b.is_active();
            app.edit(None, Redraw::Values, |t, i| {
                t.set_on(i, module, on);
            });
            let app = app.clone();
            gtk::glib::idle_add_local_once(move || refresh_toggles(&app));
        });
        flow.insert(&b, -1);
    }
}

fn sounds(app: &AppRef) -> gtk::Box {
    let wrap = vbox(0);
    let head = section("SONS PRONTOS");
    let spacer = hbox(0);
    spacer.set_hexpand(true);
    head.append(&spacer);
    let group = crate::toggle::ToggleGroup::new();
    group.add_css_class("tm-modes");
    group.add_css_class("round");
    let current = {
        let st = app.state.borrow();
        Instrument::from_key(&st.settings.instrument).unwrap_or(Instrument::Guitar)
    };
    for inst in Instrument::ALL {
        let t = crate::toggle::Toggle::new();
        t.set_name(Some(inst.key()));
        t.set_label(Some(inst.label()));
        group.add(t);
    }
    group.set_active_name(Some(current.key()));
    head.append(&group.root);
    wrap.append(&head);
    let hint = label(
        "Um ponto de partida feito a partir dos presets de fábrica. Ele entra no preset selecionado e toca na hora; salve se gostar.",
        &["tm-small", "tm-dim"],
    );
    hint.set_wrap(true);
    hint.set_margin_bottom(12);
    wrap.append(&hint);

    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_column_spacing(10);
    flow.set_row_spacing(10);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(4);
    wrap.append(&flow);
    fill_sounds(app, &flow, current);
    {
        let app = app.clone();
        let flow = flow.clone();
        group.connect_active_name_notify(move |g| {
            let inst = g.active_name().and_then(|n| Instrument::from_key(&n)).unwrap_or(Instrument::Guitar);
            app.state.borrow_mut().settings.instrument = inst.key().into();
            app.save_settings();
            fill_sounds(&app, &flow, inst);
        });
    }
    wrap
}

fn fill_sounds(app: &AppRef, flow: &gtk::FlowBox, inst: Instrument) {
    while let Some(c) = flow.first_child() {
        flow.remove(&c);
    }
    for tpl in templates::for_instrument(inst) {
        let b = gtk::Button::new();
        b.add_css_class("tm-sound");
        let inner = vbox(4);
        inner.append(&label(tpl.name, &["tm-sound-name"]));
        let d = label(tpl.description, &["tm-sound-desc"]);
        d.set_wrap(true);
        d.set_max_width_chars(26);
        d.set_width_chars(22);
        inner.append(&d);
        b.set_child(Some(&inner));
        let app = app.clone();
        b.connect_clicked(move |_| {
            app.edit(None, Redraw::Views, |t, i| {
                tpl.apply(t, i);
            });
            let slot = app.state.borrow().selected;
            app.toast_undo(&format!("“{}” em {}. Salve para gravar na pedaleira.", tpl.name, SLOT_LABELS[slot]));
        });
        flow.insert(&b, -1);
    }
}
