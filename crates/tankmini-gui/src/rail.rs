//! Sidebar: the 21 presets on the pedal, the local library and setlists.

use adw::prelude::*;
use tankmini_core::catalog::Instrument;
use tankmini_core::patch::{PATCH_COUNT, SLOT_LABELS};

use crate::app::{AppRef, RailTab, Redraw};
use crate::widgets::{clear, dot, hbox, label, slot_badge, vbox};

/// Widgets of one preset row that change without a rebuild.
pub struct RowMarks {
    pub slot: usize,
    pub row: gtk::ListBoxRow,
    pub badge: gtk::Label,
    pub dirty: gtk::Box,
    pub title: gtk::Label,
    pub sub: gtk::Label,
}

pub fn refresh(app: &AppRef) {
    let tab = app.state.borrow().rail;
    let ui = app.ui();
    ui.rail_stack.set_visible_child_name(match tab {
        RailTab::Pedal => "pedal",
        RailTab::Library => "library",
        RailTab::Setlists => "setlists",
    });
    ui.search.set_visible(tab != RailTab::Setlists);
    match tab {
        RailTab::Pedal => presets(app),
        RailTab::Library => library(app),
        RailTab::Setlists => setlists(app),
    }
}

fn presets(app: &AppRef) {
    let ui = app.ui();
    let list = &ui.preset_list;
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
    let mut marks = Vec::new();
    {
        let st = app.state.borrow();
        if st.table.is_none() {
            ui.rail_marks.replace(marks);
            return;
        }
        let query = st.search.to_lowercase();
        for i in 0..PATCH_COUNT {
            let title_text = st.title(i);
            let p = st.patch(i).unwrap();
            let sub_text = sub_line(&st, i);
            if !query.is_empty()
                && !format!("{} {} {}", SLOT_LABELS[i], title_text, sub_text).to_lowercase().contains(&query)
            {
                continue;
            }
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&format!("slot-{i}"));
            let line = hbox(12);
            let badge = slot_badge(SLOT_LABELS[i], st.playing(i), false);
            line.append(&badge);
            let texts = vbox(1);
            texts.set_hexpand(true);
            texts.set_valign(gtk::Align::Center);
            let title = label(&title_text, &["tm-row-title"]);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let sub = label(&sub_text, &["tm-row-sub"]);
            sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
            texts.append(&title);
            texts.append(&sub);
            line.append(&texts);
            let dirty = dot("dirty");
            dirty.set_tooltip_text(Some("Alterações não salvas"));
            dirty.set_visible(st.is_dirty(i));
            line.append(&dirty);
            row.set_child(Some(&line));
            row.set_tooltip_text(Some(&st.describe(&p)));
            list.append(&row);
            marks.push(RowMarks { slot: i, row, badge, dirty, title, sub });
        }
    }
    ui.rail_marks.replace(marks);
    refresh_marks(app);
}

fn sub_line(st: &crate::app::State, i: usize) -> String {
    let instrument = st.instrument(i).label();
    format!("{instrument} · {}", st.subtitle(i))
}

/// Update playing/dirty marks, titles and the selection without a rebuild.
pub fn refresh_marks(app: &AppRef) {
    let ui = app.ui();
    let st = app.state.borrow();
    for m in ui.rail_marks.borrow().iter() {
        if st.playing(m.slot) {
            m.badge.add_css_class("playing");
        } else {
            m.badge.remove_css_class("playing");
        }
        m.dirty.set_visible(st.is_dirty(m.slot));
        let t = st.title(m.slot);
        if m.title.text() != t {
            m.title.set_text(&t);
        }
        let s = sub_line(&st, m.slot);
        if m.sub.text() != s {
            m.sub.set_text(&s);
        }
        if m.slot == st.selected && !m.row.is_selected() {
            ui.preset_list.select_row(Some(&m.row));
        }
    }
}

pub fn wire(app: &AppRef) {
    let ui = app.ui();
    ui.preset_list.set_header_func(|row, before| {
        let slot = |r: &gtk::ListBoxRow| {
            r.widget_name().strip_prefix("slot-").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0)
        };
        let bank = slot(row) / 3;
        let show = before.is_none_or(|b| slot(b) / 3 != bank);
        if show {
            let l = label(&format!("BANCO {}", bank + 1), &["tm-bank"]);
            row.set_header(Some(&l));
        } else {
            row.set_header(None::<&gtk::Widget>);
        }
    });
    let list = app.ui().preset_list.clone();
    let app = app.clone();
    list.connect_row_activated(move |_, row| {
        if let Some(i) = row.widget_name().strip_prefix("slot-").and_then(|s| s.parse::<usize>().ok()) {
            app.select(i);
        }
    });
}

fn library(app: &AppRef) {
    let ui = app.ui();
    let list = &ui.library_list;
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
    let st = app.state.borrow();
    if let Some(err) = &st.library_error {
        list.append(&empty(&format!("A biblioteca não pôde ser lida: {err}")));
        return;
    }
    let found = st.library.search(&st.search, None);
    if found.is_empty() {
        list.append(&empty(if st.library.entries.is_empty() {
            "Sua biblioteca guarda quantos sons você quiser, além dos 21 da pedaleira.\n\nPara guardar o som atual: menu → Salvar na biblioteca (Ctrl+D)."
        } else {
            "Nada encontrado."
        }));
        return;
    }
    for e in found {
        let row = gtk::ListBoxRow::new();
        row.set_activatable(false);
        let line = hbox(10);
        let texts = vbox(1);
        texts.set_hexpand(true);
        let title = label(&e.name, &["tm-row-title"]);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let instrument = Instrument::from_key(&e.instrument).map(|i| i.label()).unwrap_or("—");
        let mut sub = instrument.to_string();
        if !e.tags.is_empty() {
            sub.push_str(" · ");
            sub.push_str(&e.tags.join(", "));
        }
        let sub = label(&sub, &["tm-row-sub"]);
        sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
        texts.append(&title);
        texts.append(&sub);
        if !e.notes.is_empty() {
            row.set_tooltip_text(Some(&e.notes));
        }
        let star = gtk::ToggleButton::new();
        star.set_icon_name(if e.favorite { "starred-symbolic" } else { "non-starred-symbolic" });
        star.set_active(e.favorite);
        star.add_css_class("tm-icon");
        star.set_tooltip_text(Some("Favorito"));
        star.set_valign(gtk::Align::Center);
        let use_btn = gtk::Button::with_label(&format!("Usar em {}", SLOT_LABELS[st.selected]));
        use_btn.add_css_class("tm-pill");
        use_btn.set_valign(gtk::Align::Center);
        use_btn.set_tooltip_text(Some("Coloca este som no preset selecionado (salve para gravar)"));
        let more = gtk::MenuButton::new();
        more.set_icon_name("view-more-symbolic");
        more.add_css_class("tm-icon");
        more.set_valign(gtk::Align::Center);
        let pop = gtk::Popover::new();
        let pbox = vbox(2);
        let del = gtk::Button::with_label("Excluir da biblioteca");
        del.add_css_class("flat");
        pbox.append(&del);
        pop.set_child(Some(&pbox));
        more.set_popover(Some(&pop));
        line.append(&texts);
        line.append(&star);
        line.append(&use_btn);
        line.append(&more);
        row.set_child(Some(&line));
        list.append(&row);

        let id = e.id.clone();
        {
            let app = app.clone();
            let id = id.clone();
            star.connect_toggled(move |b| {
                if let Some(e) = app.state.borrow_mut().library.get_mut(&id) {
                    e.favorite = b.is_active();
                }
                app.save_library();
                let app = app.clone();
                gtk::glib::idle_add_local_once(move || refresh(&app));
            });
        }
        {
            let app = app.clone();
            let id = id.clone();
            use_btn.connect_clicked(move |_| {
                let entry = app.state.borrow().library.get(&id).cloned();
                let Some(entry) = entry else { return };
                let Some(bytes) = entry.patch_bytes().filter(|b| tankmini_core::patch::validate_patch(b).is_ok()) else {
                    app.toast("Este som está corrompido na biblioteca.");
                    return;
                };
                app.edit(None, Redraw::Views, |t, i| {
                    t.set_patch_raw(i, &bytes);
                });
                let slot = app.state.borrow().selected;
                app.toast_undo(&format!("“{}” em {}. Salve para gravar na pedaleira.", entry.name, SLOT_LABELS[slot]));
            });
        }
        {
            let app = app.clone();
            del.connect_clicked(move |_| {
                pop.popdown();
                app.state.borrow_mut().library.remove(&id);
                app.save_library();
                let app = app.clone();
                gtk::glib::idle_add_local_once(move || refresh(&app));
            });
        }
    }
}

fn setlists(app: &AppRef) {
    let ui = app.ui();
    let container = &ui.setlist_box;
    clear(container);
    container.set_margin_start(12);
    container.set_margin_end(12);
    container.set_margin_top(4);
    let intro = label(
        "Uma setlist grava vários sons da biblioteca em sequência na pedaleira, na ordem do show.",
        &["tm-small", "tm-dim"],
    );
    intro.set_wrap(true);
    container.append(&intro);
    let new_btn = gtk::Button::with_label("Nova setlist");
    new_btn.add_css_class("tm-pill");
    new_btn.set_halign(gtk::Align::Start);
    new_btn.set_margin_top(6);
    new_btn.set_margin_bottom(6);
    {
        let app = app.clone();
        new_btn.connect_clicked(move |_| {
            let id = app.state.borrow_mut().library.add_setlist("Nova setlist");
            app.save_library();
            let app = app.clone();
            gtk::glib::idle_add_local_once(move || {
                refresh(&app);
                crate::dialogs::setlist_editor(&app, &id);
            });
        });
    }
    container.append(&new_btn);
    let st = app.state.borrow();
    for s in &st.library.setlists {
        let b = gtk::Button::new();
        b.add_css_class("tm-sound");
        let inner = vbox(2);
        inner.append(&label(&s.name, &["tm-sound-name"]));
        let end = (s.start_slot + s.entries.len()).saturating_sub(1).min(PATCH_COUNT - 1);
        let range = if s.entries.is_empty() {
            "vazia".to_string()
        } else {
            format!("{} sons · {}–{}", s.entries.len(), SLOT_LABELS[s.start_slot.min(20)], SLOT_LABELS[end])
        };
        inner.append(&label(&range, &["tm-sound-desc"]));
        b.set_child(Some(&inner));
        let app = app.clone();
        let id = s.id.clone();
        b.connect_clicked(move |_| crate::dialogs::setlist_editor(&app, &id));
        container.append(&b);
    }
}

fn empty(text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    row.set_selectable(false);
    let l = label(text, &["tm-small", "tm-dim"]);
    l.set_wrap(true);
    l.set_margin_top(12);
    l.set_margin_start(8);
    l.set_margin_end(8);
    row.set_child(Some(&l));
    row
}
