//! "Editar": every block, every parameter, the chain order and raw bytes.

use adw::prelude::*;
use gtk::{gdk, glib};
use tankmini_core::catalog::{self, Role};
use tankmini_core::patch::{Patch, PARAMS_PER_MODULE, PATCH_LEN, SLOT_LABELS};

use crate::app::{AppRef, Redraw};
use crate::knob::{Knob, KnobSize};
use crate::widgets::{clear, hbox, label, section, slot_badge, vbox};

const MODULES: usize = 6;

pub fn refresh(app: &AppRef) {
    let root = &app.ui().edit_box;
    clear(root);
    app.ui().bytes_view.replace(None);
    let page = vbox(0);
    page.set_margin_top(20);
    page.set_margin_bottom(36);
    page.set_margin_start(28);
    page.set_margin_end(28);
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(1240);
    clamp.set_child(Some(&page));
    root.append(&clamp);

    page.append(&header(app));
    page.append(&chain_strip(app));
    page.append(&cards(app));
    if app.state.borrow().settings.show_advanced {
        page.append(&bytes_panel(app));
        refresh_bytes(app);
    }
}

fn header(app: &AppRef) -> gtk::Box {
    let st = app.state.borrow();
    let i = st.selected;
    let row = hbox(14);
    row.append(&slot_badge(SLOT_LABELS[i], st.playing(i), false));
    let texts = vbox(0);
    texts.set_hexpand(true);
    texts.set_valign(gtk::Align::Center);
    let title = label(&st.title(i), &["tm-title-l"]);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    texts.append(&title);
    texts.append(&label(&format!("{} · {}", st.instrument(i).label(), st.subtitle(i)), &["tm-small", "tm-dim"]));
    row.append(&texts);
    let vol = st.current().map(|p| p.vol()).unwrap_or(0);
    let sens = st.settings.knob_sensitivity as f64;
    let app2 = app.clone();
    let knob = Knob::new("VOL", vol, KnobSize::Regular, sens, crate::theme::neutral_rgb, move |v, dragging| {
        app2.edit(Some("vol"), Redraw::Values, |t, i| {
            t.set_vol(i, v);
        });
        if !dragging {
            app2.seal();
        }
    });
    knob.widget().set_tooltip_text(Some("Volume deste preset (VOL), igual ao Volume da tela Tocar."));
    row.append(knob.widget());
    let rename = gtk::Button::with_label("Renomear");
    rename.add_css_class("tm-pill");
    rename.set_action_name(Some("win.rename"));
    rename.set_valign(gtk::Align::Center);
    let lib = gtk::Button::with_label("Salvar");
    lib.set_tooltip_text(Some("Guardar este som na biblioteca do computador (Ctrl+D)"));
    lib.add_css_class("tm-pill");
    lib.set_action_name(Some("win.library-save"));
    lib.set_valign(gtk::Align::Center);
    let revert = gtk::Button::with_label("Descartar alterações");
    revert.add_css_class("tm-pill");
    revert.set_action_name(Some("win.revert"));
    revert.set_valign(gtk::Align::Center);
    revert.set_visible(st.is_dirty(i));
    row.append(&revert);
    row.append(&rename);
    row.append(&lib);
    row
}

/// The six blocks in signal order. Drag a chip onto another to move it there.
fn chain_strip(app: &AppRef) -> gtk::Box {
    let wrap = vbox(0);
    let head = section("CAMINHO DO SINAL");
    head.append(&label("arraste para mudar a ordem", &["tm-small", "tm-faint"]));
    wrap.append(&head);
    let strip = gtk::FlowBox::new();
    strip.set_selection_mode(gtk::SelectionMode::None);
    strip.set_max_children_per_line(12);
    strip.set_column_spacing(4);
    strip.set_row_spacing(6);
    strip.add_css_class("tm-chain");
    // Keep the signal path on a single line: a wrap in the middle of the chain
    // reads as if the order changed.
    let strip_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(0)
        .propagate_natural_height(true)
        .child(&strip)
        .build();
    wrap.append(&strip_scroll);

    let st = app.state.borrow();
    let p = st.current().unwrap();
    let chain = p.chain();
    for (pos, module) in chain.iter().enumerate() {
        let module = *module as usize;
        let role = Role::for_slot(module);
        let chip = hbox(8);
        chip.add_css_class("tm-chip");
        if !p.is_on(module) {
            chip.add_css_class("off");
        }
        let d = hbox(0);
        d.add_css_class("tm-family");
        d.add_css_class(crate::theme::family_class(role));
        d.set_valign(gtk::Align::Center);
        if !p.is_on(module) {
            d.set_opacity(0.3);
        }
        chip.append(&d);
        chip.append(&label(&st.effect(&p, module), &[]));
        chip.set_tooltip_text(Some(&format!(
            "{} — {}{}",
            role.label(),
            role.hint(),
            if p.is_on(module) { "" } else { " (desligado)" }
        )));
        chip.set_cursor_from_name(Some("grab"));

        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::MOVE);
        source.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&(pos as u32).to_value())));
        chip.add_controller(source);

        let target = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
        {
            let chip_c = chip.clone();
            target.connect_enter(move |_, _, _| {
                chip_c.add_css_class("drop");
                gdk::DragAction::MOVE
            });
            let chip_c = chip.clone();
            target.connect_leave(move |_| chip_c.remove_css_class("drop"));
            let app = app.clone();
            target.connect_drop(move |_, value, _, _| {
                let Ok(from) = value.get::<u32>() else { return false };
                let from = from as usize;
                if from == pos {
                    return false;
                }
                // `edit` defers the rebuild, so the drop target outlives this
                // handler.
                app.edit(None, Redraw::Views, |t, i| {
                    t.move_chain_position(i, from, pos);
                });
                true
            });
        }
        chip.add_controller(target);
        strip.insert(&chip, -1);
        if pos + 1 < MODULES {
            let arrow = label("→", &["tm-arrow"]);
            arrow.set_valign(gtk::Align::Center);
            strip.insert(&arrow, -1);
        }
    }
    wrap
}

fn cards(app: &AppRef) -> gtk::Box {
    let wrap = vbox(0);
    let style = app.state.borrow().settings.edit_style.clone();
    if style != "cards" {
        wrap.append(&section("PEDALBOARD"));
        wrap.append(&crate::pedal::board(app, style == "pedals"));
        return wrap;
    }
    wrap.append(&section("BLOCOS"));
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_min_children_per_line(1);
    // Three columns when the window is wide; the FlowBox falls back to two and
    // then one on its own because each card has a 280 px minimum.
    flow.set_max_children_per_line(3);
    flow.set_hexpand(true);
    flow.set_halign(gtk::Align::Fill);
    flow.set_column_spacing(14);
    flow.set_row_spacing(14);
    flow.set_valign(gtk::Align::Start);
    wrap.append(&flow);
    let chain = app.state.borrow().current().unwrap().chain();
    for (pos, module) in chain.iter().enumerate() {
        let card = card(app, pos, *module as usize);
        flow.insert(&card, -1);
    }
    wrap
}

fn card(app: &AppRef, position: usize, module: usize) -> gtk::Box {
    let st = app.state.borrow();
    let p: Patch<'_> = st.current().unwrap();
    let role = Role::for_slot(module);
    let on = p.is_on(module);

    let frame = vbox(0);
    frame.add_css_class("tm-card");
    if !on {
        frame.add_css_class("off");
    }

    // Head: family dot, name, role, actions, switch.
    let head = hbox(10);
    head.add_css_class("tm-card-head");
    let d = hbox(0);
    d.add_css_class("tm-family");
    d.add_css_class(crate::theme::family_class(role));
    d.set_valign(gtk::Align::Center);
    head.append(&d);
    let titles = vbox(0);
    titles.set_hexpand(true);
    titles.append(&label(&st.effect(&p, module), &["tm-title-m"]));
    titles.append(&label(
        &format!("{} · posição {} de 6", role.label(), position + 1),
        &["tm-small", "tm-dim"],
    ));
    titles.set_tooltip_text(Some(role.description()));
    head.append(&titles);
    drop(st);
    head.append(&block_tools(app, position, module));

    let switch = gtk::Switch::new();
    switch.set_active(on);
    switch.set_valign(gtk::Align::Center);
    switch.set_tooltip_text(Some("Ligar/desligar este bloco"));
    {
        let app = app.clone();
        switch.connect_state_set(move |_, value| {
            app.edit(None, Redraw::Views, |t, i| {
                t.set_on(i, module, value);
            });
            glib::Propagation::Proceed
        });
    }
    head.append(&switch);
    frame.append(&head);

    let body = vbox(10);
    body.add_css_class("tm-card-body");
    // The header of the card already carries the block menu with the model list,
    // so the body stays for knobs only: a second dropdown cost ~50 px of height
    // per card and duplicated the same choice.
    let _ = &module;

    let knobs = gtk::FlowBox::new();
    knobs.set_selection_mode(gtk::SelectionMode::None);
    knobs.set_max_children_per_line(6);
    knobs.set_min_children_per_line(3);
    knobs.set_column_spacing(4);
    knobs.set_row_spacing(4);
    knobs.set_homogeneous(true);
    let list = param_knobs(app, module, KnobSize::Compact);
    if list.is_empty() {
        body.append(&label("Este bloco não tem ajustes conhecidos.", &["tm-small", "tm-faint"]));
    } else {
        for k in &list {
            knobs.insert(k.widget(), -1);
        }
        body.append(&knobs);
    }
    frame.append(&body);
    frame
}

/// Move left/right and the copy/paste/restore menu of one block.
pub(crate) fn block_tools(app: &AppRef, position: usize, module: usize) -> gtk::Box {
    let tools = hbox(2);
    tools.set_valign(gtk::Align::Center);
    let up = gtk::Button::from_icon_name("go-previous-symbolic");
    up.add_css_class("tm-icon");
    up.set_tooltip_text(Some("Mover para antes no caminho do sinal"));
    up.set_sensitive(position > 0);
    up.set_valign(gtk::Align::Center);
    let down = gtk::Button::from_icon_name("go-next-symbolic");
    down.add_css_class("tm-icon");
    down.set_tooltip_text(Some("Mover para depois no caminho do sinal"));
    down.set_sensitive(position + 1 < MODULES);
    down.set_valign(gtk::Align::Center);
    {
        let app = app.clone();
        up.connect_clicked(move |_| {
            app.edit(None, Redraw::Views, |t, i| {
                t.move_chain_position(i, position, position - 1);
            })
        });
    }
    {
        let app = app.clone();
        down.connect_clicked(move |_| {
            app.edit(None, Redraw::Views, |t, i| {
                t.move_chain_position(i, position, position + 1);
            })
        });
    }
    tools.append(&up);
    tools.append(&down);

    let st = app.state.borrow();
    let role = Role::for_slot(module);
    let menu = gtk::MenuButton::new();
    menu.set_icon_name("view-more-symbolic");
    menu.add_css_class("tm-icon");
    menu.set_valign(gtk::Align::Center);
    menu.set_tooltip_text(Some("Copiar, colar ou restaurar este bloco"));
    let pop = gtk::Popover::new();
    let pbox = vbox(2);
    let copy = gtk::Button::with_label("Copiar bloco");
    let paste = gtk::Button::with_label("Colar bloco");
    let restore = gtk::Button::with_label("Voltar ao salvo");
    for b in [&copy, &paste, &restore] {
        b.add_css_class("flat");
        pbox.append(b);
    }
    paste.set_sensitive(st.clipboard.is_some_and(|b| b.module == module));
    if let Some(b) = st.clipboard.filter(|b| b.module == module) {
        let name = match role {
            Role::Amp | Role::Cabinet => st.model_name(role, b.unit as usize),
            other => catalog::catalog_name(other, b.unit),
        };
        paste.set_label(&format!("Colar bloco ({name})"));
    }
    drop(st);
    pop.set_child(Some(&pbox));
    menu.set_popover(Some(&pop));
    {
        let app = app.clone();
        let pop = pop.clone();
        copy.connect_clicked(move |_| {
            pop.popdown();
            let block = {
                let st = app.state.borrow();
                st.table.as_ref().and_then(|t| t.block(st.selected, module))
            };
            app.state.borrow_mut().clipboard = block;
            app.toast("Bloco copiado. Abra outro preset e use Colar bloco.");
        });
    }
    {
        let app = app.clone();
        let pop = pop.clone();
        paste.connect_clicked(move |_| {
            pop.popdown();
            let Some(block) = app.state.borrow().clipboard else { return };
            app.edit(None, Redraw::Views, |t, i| {
                t.set_block(i, &block);
            });
        });
    }
    {
        let app = app.clone();
        restore.connect_clicked(move |_| {
            pop.popdown();
            let block = {
                let st = app.state.borrow();
                st.saved.as_ref().and_then(|t| t.block(st.selected, module))
            };
            if let Some(block) = block {
                app.edit(None, Redraw::Views, |t, i| {
                    t.set_block(i, &block);
                });
            }
        });
    }
    tools.append(&menu);
    tools
}

/// Display names of every variant a module can take.
pub(crate) fn variant_labels(st: &crate::app::State, role: Role) -> Vec<String> {
    match role {
        Role::Amp | Role::Cabinet => (0..catalog::variant_count(role))
            .map(|i| {
                let name = st.model_name(role, i);
                if i as u8 >= catalog::FIRST_BASS_MODEL { format!("{name}  (baixo)") } else { name }
            })
            .collect(),
        other => catalog::variants(other).iter().map(|v| v.name.to_string()).collect(),
    }
}

/// Select the variant (type) of a module.
pub(crate) fn set_variant(app: &AppRef, module: usize, v: u8) {
    if app.state.borrow().current().map(|p| p.unit(module)) == Some(v) {
        return;
    }
    app.edit(None, Redraw::Views, |t, i| {
        t.set_unit(i, module, v);
    });
}

/// One knob per parameter of a module (unnamed ones only when asked for).
pub(crate) fn param_knobs(app: &AppRef, module: usize, size: KnobSize) -> Vec<Knob> {
    let st = app.state.borrow();
    let Some(p) = st.current() else { return Vec::new() };
    let role = Role::for_slot(module);
    let unit = p.unit(module);
    let show_all = st.settings.show_all_params || st.settings.show_advanced;
    let sens = st.settings.knob_sensitivity as f64;
    let mut out = Vec::new();
    for param in 0..PARAMS_PER_MODULE {
        let value = p.param(module, param).unwrap_or(0);
        let raw_name = catalog::param_name(role, unit, param);
        if raw_name.is_none() && !show_all && value == 0 {
            continue;
        }
        let text = st
            .param_label(role, unit, param)
            .unwrap_or_else(|| format!("Ajuste {}", param + 1));
        let help = raw_name.map(catalog::param_help).unwrap_or(catalog::param_help(""));
        let offset = Patch::param_offset(module, param);
        let tip = if st.settings.show_advanced {
            format!("{help}\nbyte 0x{offset:02x} (u16 LE) · valor {value}")
        } else {
            help.to_string()
        };
        let app2 = app.clone();
        let group = format!("knob-{module}-{param}");
        let knob = Knob::new(&text, value, size, sens, move || crate::theme::family_rgb(role), move |v, dragging| {
            app2.edit(Some(&group), Redraw::Values, |t, i| {
                t.set_param(i, module, param, v);
            });
            if !dragging {
                app2.seal();
            }
        });
        knob.set_formatter(raw_name);
        knob.widget().set_tooltip_text(Some(&tip));
        out.push(knob);
    }
    out
}

fn bytes_panel(app: &AppRef) -> gtk::Box {
    let wrap = vbox(0);
    let head = section("BYTES DO PRESET");
    head.append(&label(
        "0x00 VOL · 0x02 ordem · 0x08 ligado · 0x0e tipo · 0x14 parâmetros (u16 LE) — diferenças do salvo em destaque",
        &["tm-small", "tm-faint"],
    ));
    wrap.append(&head);
    let view = gtk::TextView::new();
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_monospace(true);
    view.add_css_class("tm-code");
    view.set_top_margin(10);
    view.set_bottom_margin(10);
    view.set_left_margin(12);
    let buffer = view.buffer();
    let tag = gtk::TextTag::builder().name("diff").weight(800).underline(gtk::pango::Underline::Single).build();
    buffer.tag_table().add(&tag);
    // The hexdump is as wide as its longest line; without its own scroll area it
    // becomes the window's minimum width and the whole layout overflows.
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_width(0)
        .min_content_height(220)
        .vexpand(true)
        .child(&view)
        .build();
    wrap.append(&scroll);
    app.ui().bytes_view.replace(Some(view));
    wrap
}

/// Rewrite the hexdump of the selected preset, highlighting unsaved bytes.
pub fn refresh_bytes(app: &AppRef) {
    let Some(view) = app.ui().bytes_view.borrow().clone() else { return };
    let st = app.state.borrow();
    let (Some(table), Some(saved)) = (st.table.as_ref(), st.saved.as_ref()) else { return };
    let Some(p) = table.patch(st.selected) else { return };
    let diff = table.diff_offsets(saved, st.selected);
    let buffer = view.buffer();
    buffer.set_text("");
    let mut iter = buffer.end_iter();
    for (row, chunk) in p.raw().chunks(16).enumerate() {
        buffer.insert(&mut iter, &format!("{:02x}  ", row * 16));
        for (k, b) in chunk.iter().enumerate() {
            let off = row * 16 + k;
            let text = format!("{b:02x} ");
            if diff.contains(&off) {
                buffer.insert_with_tags_by_name(&mut iter, &text, &["diff"]);
            } else {
                buffer.insert(&mut iter, &text);
            }
        }
        if (row + 1) * 16 < PATCH_LEN {
            buffer.insert(&mut iter, "\n");
        }
    }
}
