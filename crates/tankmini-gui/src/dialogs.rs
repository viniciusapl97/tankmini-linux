//! Dialogs: onboarding, preferences, library, setlists, backups and helpers.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};
use tankmini_core::backup::BackupStore;
use tankmini_core::catalog::Instrument;
use tankmini_core::{amp, ir};
use tankmini_core::patch::{PresetTable, PATCH_COUNT, SLOT_LABELS};
use tankmini_core::templates;
use tankmini_core::worker::Command;

use crate::app::{AppRef, Conn, Redraw};
use crate::widgets::{hbox, label, pedal_art, vbox};

thread_local! {
    static ONBOARD_SHOWN: Cell<bool> = const { Cell::new(false) };
}

/// Show the assistant once, on the very first run.
pub fn maybe_onboard(app: &AppRef) {
    if app.state.borrow().settings.onboarded || ONBOARD_SHOWN.with(Cell::get) {
        return;
    }
    onboarding(app);
}

fn page(title: &str, text: &str) -> gtk::Box {
    let p = vbox(14);
    p.set_valign(gtk::Align::Center);
    p.set_margin_start(36);
    p.set_margin_end(36);
    p.set_margin_top(12);
    p.set_margin_bottom(12);
    p.set_hexpand(true);
    let t = label(title, &["tm-title-l"]);
    t.set_wrap(true);
    p.append(&t);
    if !text.is_empty() {
        let l = label(text, &["tm-dim"]);
        l.set_wrap(true);
        p.append(&l);
    }
    p
}

fn step(n: usize, text: &str) -> gtk::Box {
    let row = hbox(12);
    let num = gtk::Label::new(Some(&n.to_string()));
    num.add_css_class("tm-step-num");
    num.set_valign(gtk::Align::Start);
    row.append(&num);
    let l = label(text, &[]);
    l.set_wrap(true);
    l.set_hexpand(true);
    row.append(&l);
    row
}

pub fn onboarding(app: &AppRef) {
    ONBOARD_SHOWN.with(|s| s.set(true));
    let dialog = adw::Dialog::new();
    dialog.set_title("Bem-vindo");
    dialog.set_content_width(620);
    dialog.set_content_height(560);

    let carousel = adw::Carousel::new();
    carousel.set_allow_scroll_wheel(false);
    carousel.set_vexpand(true);

    // 1. Welcome
    let p1 = page(
        "Sua pedaleira, sem complicação",
        "Este app mostra os 21 sons guardados na TANK MINI e deixa você trocar, ajustar e criar sons pelo computador. Tudo que você mexe toca na hora; só fica gravado quando você clica em Salvar.",
    );
    let art = pedal_art(120);
    art.set_halign(gtk::Align::Start);
    p1.prepend(&art);
    carousel.append(&p1);

    // 2. Instrument
    let p2 = page("O que você vai tocar?", "Isso escolhe os sons sugeridos. Dá para mudar depois.");
    let choices = hbox(10);
    choices.set_homogeneous(true);
    let current = Instrument::from_key(&app.state.borrow().settings.instrument).unwrap_or(Instrument::Guitar);
    let mut first: Option<gtk::ToggleButton> = None;
    for (inst, desc) in [
        (Instrument::Guitar, "Guitarra elétrica"),
        (Instrument::Acoustic, "Violão com captador"),
        (Instrument::Bass, "Contrabaixo"),
    ] {
        let b = gtk::ToggleButton::new();
        b.add_css_class("tm-choice");
        let inner = vbox(4);
        inner.append(&label(inst.label(), &["tm-title-m"]));
        inner.append(&label(desc, &["tm-small", "tm-dim"]));
        b.set_child(Some(&inner));
        if let Some(f) = &first {
            b.set_group(Some(f));
        } else {
            first = Some(b.clone());
        }
        b.set_active(inst == current);
        let app = app.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                app.state.borrow_mut().settings.instrument = inst.key().into();
                app.save_settings();
            }
        });
        choices.append(&b);
    }
    p2.append(&choices);
    carousel.append(&p2);

    // 3. Connect
    let p3 = page("Ligando os cabos", "");
    for (i, text) in [
        "Cabo do instrumento na entrada INPUT da pedaleira.",
        "Fone de ouvido na saída de fone, ou um cabo da saída OUTPUT para o amplificador ou caixa de som.",
        "Cabo USB-C da pedaleira no computador. Ele alimenta a pedaleira e conecta ao app.",
        "Comece com o volume baixo e vá subindo.",
    ]
    .iter()
    .enumerate()
    {
        p3.append(&step(i + 1, text));
    }
    let status = label("", &["tm-status"]);
    status.set_halign(gtk::Align::Start);
    status.set_margin_top(8);
    p3.append(&status);
    let fix = gtk::Button::with_label("Como resolver a permissão");
    fix.add_css_class("tm-pill");
    fix.set_halign(gtk::Align::Start);
    fix.set_visible(false);
    p3.append(&fix);
    {
        let app = app.clone();
        fix.connect_clicked(move |_| {
            let node = match &app.state.borrow().conn {
                Conn::Offline(tankmini_core::worker::Disconnect::Permission(n)) => n.clone(),
                _ => "/dev/snd/midiC*".into(),
            };
            permission_help(&app, &node);
        });
    }
    carousel.append(&p3);

    // 4. Done
    let p4 = page(
        "Pronto para tocar",
        "Na tela Tocar você tem quatro controles grandes (Volume, Drive, Brilho, Ambiente), os efeitos para ligar e desligar e uma galeria de sons prontos. Na tela Editar fica cada detalhe.",
    );
    let try_btn = gtk::Button::new();
    try_btn.add_css_class("tm-sound");
    try_btn.set_halign(gtk::Align::Start);
    let try_label = label("", &["tm-sound-name"]);
    try_btn.set_child(Some(&try_label));
    p4.append(&try_btn);
    let tip = label(
        "Dica: pise nos pedais A, B e C para trocar de som. O app acompanha.",
        &["tm-small", "tm-dim"],
    );
    tip.set_wrap(true);
    p4.append(&tip);
    carousel.append(&p4);

    // Live status of the connection while the dialog is open.
    let alive = Rc::new(Cell::new(true));
    {
        let app = app.clone();
        let alive = alive.clone();
        let status = status.clone();
        let fix = fix.clone();
        let try_label = try_label.clone();
        let try_btn = try_btn.clone();
        let tick = move || {
            let st = app.state.borrow();
            let (text, need_fix) = match &st.conn {
                Conn::Online => (format!("● Pedaleira conectada ({})", st.firmware), false),
                Conn::Searching => ("Procurando a pedaleira…".to_string(), false),
                Conn::Offline(tankmini_core::worker::Disconnect::Permission(_)) => {
                    ("A pedaleira apareceu, mas falta permissão".to_string(), true)
                }
                Conn::Offline(_) => ("Ainda não encontrei a pedaleira no USB".to_string(), false),
            };
            status.set_text(&text);
            fix.set_visible(need_fix);
            let inst = Instrument::from_key(&st.settings.instrument).unwrap_or(Instrument::Guitar);
            let tpl = templates::for_instrument(inst).next();
            if let Some(tpl) = tpl {
                try_label.set_text(&format!("Experimentar o som “{}” agora", tpl.name));
            }
            try_btn.set_sensitive(st.table.is_some());
        };
        tick();
        glib::timeout_add_local(Duration::from_millis(500), move || {
            if !alive.get() {
                return glib::ControlFlow::Break;
            }
            tick();
            glib::ControlFlow::Continue
        });
    }
    {
        let app = app.clone();
        try_btn.connect_clicked(move |_| {
            let inst = Instrument::from_key(&app.state.borrow().settings.instrument).unwrap_or(Instrument::Guitar);
            if let Some(tpl) = templates::for_instrument(inst).next() {
                app.state.borrow_mut().view = crate::app::View::Play;
                app.edit(None, Redraw::Views, |t, i| {
                    tpl.apply(t, i);
                });
                app.toast_undo(&format!("“{}” tocando. Salve se quiser guardar.", tpl.name));
            }
        });
    }

    // Navigation.
    let dots = adw::CarouselIndicatorDots::new();
    dots.set_carousel(Some(&carousel));
    let back = gtk::Button::with_label("Voltar");
    back.add_css_class("tm-pill");
    let next = gtk::Button::with_label("Continuar");
    next.add_css_class("tm-cta");
    let nav = hbox(8);
    nav.set_margin_start(24);
    nav.set_margin_end(24);
    nav.set_margin_bottom(20);
    nav.append(&back);
    let sp = hbox(0);
    sp.set_hexpand(true);
    nav.append(&sp);
    nav.append(&dots);
    let sp2 = hbox(0);
    sp2.set_hexpand(true);
    nav.append(&sp2);
    nav.append(&next);
    let update = {
        let carousel = carousel.clone();
        let back = back.clone();
        let next = next.clone();
        move || {
            let pos = carousel.position().round() as u32;
            back.set_sensitive(pos > 0);
            next.set_label(if pos + 1 >= carousel.n_pages() { "Começar a tocar" } else { "Continuar" });
        }
    };
    update();
    {
        let update = update.clone();
        carousel.connect_page_changed(move |_, _| update());
    }
    {
        let carousel = carousel.clone();
        back.connect_clicked(move |_| {
            let pos = carousel.position().round() as u32;
            if pos > 0 {
                carousel.scroll_to(&carousel.nth_page(pos - 1), true);
            }
        });
    }
    {
        let carousel = carousel.clone();
        let dialog = dialog.clone();
        next.connect_clicked(move |_| {
            let pos = carousel.position().round() as u32;
            if pos + 1 < carousel.n_pages() {
                carousel.scroll_to(&carousel.nth_page(pos + 1), true);
            } else {
                dialog.close();
            }
        });
    }
    {
        let app = app.clone();
        dialog.connect_closed(move |_| {
            alive.set(false);
            app.state.borrow_mut().settings.onboarded = true;
            app.save_settings();
            app.refresh_all();
        });
    }

    let body = vbox(0);
    body.append(&carousel);
    body.append(&nav);
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&body));
    dialog.set_child(Some(&tv));
    dialog.present(Some(&app.window));
}

pub fn permission_help(app: &AppRef, node: &str) {
    let dialog = adw::AlertDialog::new(
        Some("Liberar o acesso à pedaleira"),
        Some(&format!(
            "O Linux só deixa programas do grupo “audio” abrirem {node}. Rode este comando no terminal, depois saia e entre de novo na sessão:"
        )),
    );
    let cmd = "sudo usermod -aG audio $USER";
    let row = hbox(8);
    let code = label(cmd, &["tm-code"]);
    code.set_selectable(true);
    code.set_hexpand(true);
    let copy = gtk::Button::from_icon_name("edit-copy-symbolic");
    copy.add_css_class("tm-icon");
    copy.set_tooltip_text(Some("Copiar"));
    copy.set_valign(gtk::Align::Center);
    {
        let app = app.clone();
        copy.connect_clicked(move |b| {
            b.clipboard().set_text(cmd);
            app.toast("Comando copiado.");
        });
    }
    row.append(&code);
    row.append(&copy);
    dialog.set_extra_child(Some(&row));
    dialog.add_responses(&[("close", "Fechar"), ("retry", "Tentar de novo")]);
    dialog.set_response_appearance("retry", adw::ResponseAppearance::Suggested);
    let window = app.window.clone();
    let app = app.clone();
    dialog.connect_response(None, move |_, r| {
        if r == "retry" {
            app.send(Command::Reload);
        }
    });
    dialog.present(Some(&window));
}

fn form_row(title: &str, child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = vbox(4);
    row.append(&label(title, &["tm-small", "tm-dim"]));
    row.append(child);
    row
}

pub fn library_save(app: &AppRef) {
    let (name, instrument) = {
        let st = app.state.borrow();
        if st.table.is_none() {
            return;
        }
        (st.title(st.selected), st.instrument(st.selected))
    };
    let dialog = adw::AlertDialog::new(
        Some("Salvar na biblioteca"),
        Some("Guarda uma cópia deste som no computador. A biblioteca não tem limite de sons."),
    );
    let form = vbox(10);
    let name_entry = gtk::Entry::new();
    name_entry.set_text(&name);
    name_entry.set_activates_default(true);
    form.append(&form_row("Nome", &name_entry));
    let labels: Vec<&str> = Instrument::ALL.iter().map(|i| i.label()).collect();
    let inst = gtk::DropDown::from_strings(&labels);
    inst.set_selected(Instrument::ALL.iter().position(|i| *i == instrument).unwrap_or(0) as u32);
    form.append(&form_row("Instrumento", &inst));
    let tags = gtk::Entry::new();
    tags.set_placeholder_text(Some("ex.: rock, show, solo"));
    form.append(&form_row("Etiquetas (separadas por vírgula)", &tags));
    let notes = gtk::Entry::new();
    notes.set_placeholder_text(Some("opcional"));
    form.append(&form_row("Notas", &notes));
    dialog.set_extra_child(Some(&form));
    dialog.add_responses(&[("cancel", "Cancelar"), ("save", "Salvar")]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    let window = app.window.clone();
    let app = app.clone();
    dialog.connect_response(None, move |_, r| {
        if r != "save" {
            return;
        }
        let bytes = {
            let st = app.state.borrow();
            st.current().map(|p| p.raw().to_vec())
        };
        let Some(bytes) = bytes else { return };
        let instrument = Instrument::ALL[inst.selected() as usize].key();
        let tag_list: Vec<String> = tags.text().split(',').map(str::to_string).collect();
        let result = {
            let mut st = app.state.borrow_mut();
            let id = st.library.add(&name_entry.text(), instrument, tag_list, &bytes);
            if let Ok(id) = &id {
                if let Some(e) = st.library.get_mut(id) {
                    e.notes = notes.text().to_string();
                }
            }
            id
        };
        match result {
            Ok(_) => {
                app.save_library();
                app.toast("Salvo na biblioteca.");
                crate::rail::refresh(&app);
            }
            Err(e) => app.toast(&e),
        }
    });
    dialog.present(Some(&window));
}

pub fn rename(app: &AppRef) {
    let (slot, current) = {
        let st = app.state.borrow();
        (st.selected, st.names.get(st.selected).unwrap_or("").to_string())
    };
    let dialog = adw::AlertDialog::new(
        Some(&format!("Nome do preset {}", SLOT_LABELS[slot])),
        Some("A pedaleira não guarda nomes; ele fica salvo neste computador. Deixe vazio para voltar à descrição automática."),
    );
    let entry = gtk::Entry::new();
    entry.set_text(&current);
    entry.set_placeholder_text(Some("ex.: Crunch do refrão"));
    entry.set_activates_default(true);
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", "Cancelar"), ("save", "Salvar")]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    let window = app.window.clone();
    let app = app.clone();
    dialog.connect_response(None, move |_, r| {
        if r == "save" {
            {
                let mut st = app.state.borrow_mut();
                st.names.set(slot, &entry.text());
                let _ = st.names.save_default();
            }
            app.refresh_all();
        }
    });
    dialog.present(Some(&window));
}

pub fn export_preset(app: &AppRef) {
    let (bytes, suggested) = {
        let st = app.state.borrow();
        let Some(p) = st.current() else { return };
        let name: String = format!("{} {}", SLOT_LABELS[st.selected], st.title(st.selected))
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
            .collect();
        (p.raw().to_vec(), format!("{name}.tkm"))
    };
    let dialog = gtk::FileDialog::builder()
        .title("Exportar preset")
        .initial_name(&suggested)
        .filters(&preset_filters())
        .build();
    let app = app.clone();
    dialog.save(Some(&app.window.clone()), gio::Cancellable::NONE, move |res| {
        let Ok(file) = res else { return };
        let Some(path) = file.path() else { return };
        match std::fs::write(&path, &bytes) {
            Ok(()) => app.toast(&format!("Exportado para {}", path.display())),
            Err(e) => app.toast(&format!("Falha ao exportar: {e}")),
        }
    });
}

pub fn import_preset(app: &AppRef) {
    let dialog = gtk::FileDialog::builder()
        .title("Importar preset ou todos os presets")
        .filters(&preset_filters())
        .build();
    let app = app.clone();
    dialog.open(Some(&app.window.clone()), gio::Cancellable::NONE, move |res| {
        let Ok(file) = res else { return };
        let Some(path) = file.path() else { return };
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                app.toast(&format!("Falha ao ler: {e}"));
                return;
            }
        };
        // Validate before touching the model.
        let mut probe = app.state.borrow().table.clone().unwrap_or_else(PresetTable::factory);
        let slot = app.state.borrow().selected;
        if let Err(e) = probe.import(slot, &bytes) {
            app.toast(&format!("Arquivo recusado: {e}"));
            return;
        }
        app.edit(None, Redraw::Views, |t, i| {
            let _ = t.import(i, &bytes);
        });
        app.toast_undo("Importado. Confira e salve para gravar na pedaleira.");
    });
}

/// File types for presets: the official app's `.tkm` (one preset) and `.atkm`
/// (all 21, the 1942-byte table), plus our older `.tankmini`. Import tells them
/// apart by size, so the extension is only a hint.
fn preset_filters() -> gio::ListStore {
    let store = gio::ListStore::new::<gtk::FileFilter>();
    let all = gtk::FileFilter::new();
    all.set_name(Some("Presets TANK MINI (.tkm, .atkm)"));
    for ext in ["tkm", "atkm", "tankmini", "bin"] {
        all.add_suffix(ext);
    }
    store.append(&all);
    let any = gtk::FileFilter::new();
    any.set_name(Some("Todos os arquivos"));
    any.add_pattern("*");
    store.append(&any);
    store
}

/// Export the whole table as an `.atkm`, the official app's "Share all presets".
pub fn export_all(app: &AppRef) {
    let Some(bytes) = app.state.borrow().table.as_ref().map(|t| t.raw().to_vec()) else {
        return;
    };
    let dialog = gtk::FileDialog::builder()
        .title("Exportar todos os presets")
        .initial_name("TankMini-presets.atkm")
        .filters(&preset_filters())
        .build();
    let app = app.clone();
    dialog.save(Some(&app.window.clone()), gio::Cancellable::NONE, move |res| {
        let Ok(file) = res else { return };
        let Some(path) = file.path() else { return };
        match std::fs::write(&path, &bytes) {
            Ok(()) => app.toast(&format!("21 presets exportados para {}", path.display())),
            Err(e) => app.toast(&format!("Falha ao exportar: {e}")),
        }
    });
}

/// Exchange the selected preset with another slot.
pub fn swap_slots(app: &AppRef) {
    let from = app.state.borrow().selected;
    let dialog = adw::AlertDialog::new(
        Some(&format!("Trocar {} de lugar com…", SLOT_LABELS[from])),
        Some("Os dois presets trocam de posição. Nada é gravado até você salvar."),
    );
    let labels: Vec<String> = {
        let st = app.state.borrow();
        (0..PATCH_COUNT).map(|i| format!("{} · {}", SLOT_LABELS[i], st.title(i))).collect()
    };
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let dd = gtk::DropDown::from_strings(&refs);
    dd.set_selected(((from + 1) % PATCH_COUNT) as u32);
    dialog.set_extra_child(Some(&dd));
    dialog.add_responses(&[("cancel", "Cancelar"), ("swap", "Trocar")]);
    dialog.set_response_appearance("swap", adw::ResponseAppearance::Suggested);
    let window = app.window.clone();
    let app = app.clone();
    dialog.connect_response(None, move |_, r| {
        if r != "swap" {
            return;
        }
        let to = dd.selected() as usize;
        if to == from {
            app.toast("Escolha outro preset.");
            return;
        }
        app.edit(None, Redraw::Views, |t, _| {
            t.swap_patches(from, to);
        });
        app.toast_undo(&format!(
            "{} e {} trocados. Use Salvar todos para gravar.",
            SLOT_LABELS[from], SLOT_LABELS[to]
        ));
    });
    dialog.present(Some(&window));
}

pub fn copy_slot(app: &AppRef) {
    let from = app.state.borrow().selected;
    let dialog = adw::AlertDialog::new(
        Some(&format!("Copiar {} para…", SLOT_LABELS[from])),
        Some("O preset de destino é substituído por uma cópia deste. Nada é gravado até você salvar."),
    );
    let labels: Vec<String> = {
        let st = app.state.borrow();
        (0..PATCH_COUNT).map(|i| format!("{} · {}", SLOT_LABELS[i], st.title(i))).collect()
    };
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let dd = gtk::DropDown::from_strings(&refs);
    dd.set_selected(((from + 1) % PATCH_COUNT) as u32);
    dialog.set_extra_child(Some(&dd));
    dialog.add_responses(&[("cancel", "Cancelar"), ("copy", "Copiar")]);
    dialog.set_response_appearance("copy", adw::ResponseAppearance::Suggested);
    let window = app.window.clone();
    let app = app.clone();
    dialog.connect_response(None, move |_, r| {
        if r != "copy" {
            return;
        }
        let to = dd.selected() as usize;
        if to == from {
            app.toast("Escolha outro preset de destino.");
            return;
        }
        app.edit(None, Redraw::Views, |t, _| {
            t.copy_patch(from, to);
        });
        app.toast_undo(&format!("Copiado para {}. Use Salvar todos para gravar.", SLOT_LABELS[to]));
    });
    dialog.present(Some(&window));
}

pub fn factory(app: &AppRef) {
    let dialog = adw::AlertDialog::new(
        Some("Restaurar de fábrica?"),
        Some("Os 21 presets voltam ao estado de fábrica e isso é gravado na memória da pedaleira. Um backup do estado atual é criado antes."),
    );
    dialog.add_responses(&[("cancel", "Cancelar"), ("reset", "Restaurar")]);
    dialog.set_response_appearance("reset", adw::ResponseAppearance::Destructive);
    let window = app.window.clone();
    let app = app.clone();
    dialog.connect_response(None, move |_, r| {
        if r == "reset" {
            app.replace_table(PresetTable::factory(), "antes de restaurar de fábrica");
        }
    });
    dialog.present(Some(&window));
}

fn fmt_time(ts: u64) -> String {
    glib::DateTime::from_unix_local(ts as i64)
        .ok()
        .and_then(|d| d.format("%d/%m/%Y %H:%M").ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| ts.to_string())
}

pub fn backups(app: &AppRef) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Backups");
    dialog.set_content_width(560);
    dialog.set_content_height(560);
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    group.set_description(Some(
        "O app guarda a tabela inteira antes de cada gravação. Restaurar substitui os 21 presets e salva na memória.",
    ));
    page.add(&group);
    let store = BackupStore::new(BackupStore::default_root());
    let list = store.list_info().unwrap_or_default();
    if list.is_empty() {
        let row = adw::ActionRow::new();
        row.set_title("Nenhum backup ainda");
        group.add(&row);
    }
    for info in list.into_iter().take(200) {
        let row = adw::ActionRow::new();
        row.set_title(&fmt_time(info.timestamp));
        let mut sub = if info.note.is_empty() { "backup".to_string() } else { info.note.clone() };
        if let Some(s) = info.slot.and_then(|s| SLOT_LABELS.get(s as usize)) {
            sub.push_str(&format!(" · {s}"));
        }
        row.set_subtitle(&sub);
        let restore = gtk::Button::with_label("Restaurar");
        restore.add_css_class("tm-pill");
        restore.set_valign(gtk::Align::Center);
        let del = gtk::Button::from_icon_name("user-trash-symbolic");
        del.add_css_class("tm-icon");
        del.set_valign(gtk::Align::Center);
        del.set_tooltip_text(Some("Excluir este backup"));
        row.add_suffix(&restore);
        row.add_suffix(&del);
        group.add(&row);
        {
            let app = app.clone();
            let path = info.path.clone();
            let dialog = dialog.clone();
            restore.connect_clicked(move |_| {
                let table = BackupStore::load(&path)
                    .and_then(|p| PresetTable::from_raw(p.raw().to_vec()))
                    .and_then(|t| t.validate().map(|_| t));
                let table = match table {
                    Ok(t) => t,
                    Err(e) => {
                        app.toast(&format!("Backup inválido: {e}"));
                        return;
                    }
                };
                let confirm = adw::AlertDialog::new(
                    Some("Restaurar este backup?"),
                    Some("Os 21 presets da pedaleira e suas alterações não salvas são substituídos."),
                );
                confirm.add_responses(&[("cancel", "Cancelar"), ("go", "Restaurar")]);
                confirm.set_response_appearance("go", adw::ResponseAppearance::Destructive);
                let app2 = app.clone();
                let dialog2 = dialog.clone();
                confirm.connect_response(None, move |_, r| {
                    if r == "go" {
                        app2.replace_table(table.clone(), "antes de restaurar backup");
                        dialog2.close();
                    }
                });
                confirm.present(Some(&dialog));
            });
        }
        {
            let app = app.clone();
            let path = info.path.clone();
            let group = group.clone();
            let row = row.clone();
            del.connect_clicked(move |_| {
                match BackupStore::new(BackupStore::default_root()).delete(&path) {
                    Ok(()) => group.remove(&row),
                    Err(e) => app.toast(&format!("Não consegui excluir: {e}")),
                }
            });
        }
    }
    let make = gtk::Button::with_label("Criar backup agora");
    make.add_css_class("tm-pill");
    {
        let app = app.clone();
        let dialog = dialog.clone();
        make.connect_clicked(move |_| {
            let preset = app.state.borrow().saved.as_ref().map(|t| t.to_preset());
            if let Some(preset) = preset {
                match BackupStore::new(BackupStore::default_root()).save(&preset, None, "backup manual") {
                    Ok(_) => {
                        dialog.close();
                        app.toast("Backup criado.");
                    }
                    Err(e) => app.toast(&format!("Falha no backup: {e}")),
                }
            }
        });
    }
    let header = adw::HeaderBar::new();
    header.pack_start(&make);
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&header);
    tv.set_content(Some(&page));
    dialog.set_child(Some(&tv));
    dialog.present(Some(&app.window));
}

pub fn guide(app: &AppRef) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Como usar a pedaleira");
    dialog.set_content_width(640);
    dialog.set_content_height(640);
    let body = vbox(14);
    body.set_margin_start(28);
    body.set_margin_end(28);
    body.set_margin_top(12);
    body.set_margin_bottom(24);
    body.append(&label(
        "A TANK MINI guarda 21 sons em 7 bancos. Cada banco tem três sons, um para cada pedal: A, B e C.",
        &[],
    ));
    for (i, text) in [
        "Pise em A, B ou C para escolher o som dentro do banco atual.",
        "Para trocar de banco, use a combinação de pedais descrita no manual da TANK MINI, ou clique no som aqui no app.",
        "Cada som passa por 6 blocos em sequência: Dinâmica → Amp → Modulação → Delay → Reverb → Cabinet (a ordem pode mudar na tela Editar).",
        "Ajustes feitos aqui tocam na hora. Clique em Salvar na pedaleira para guardar; se trocar de som sem salvar, a pedaleira volta ao som gravado, mas o app guarda suas alterações.",
    ]
    .iter()
    .enumerate()
    {
        body.append(&step(i + 1, text));
    }
    let grid = gtk::Grid::new();
    grid.set_column_spacing(8);
    grid.set_row_spacing(8);
    grid.set_margin_top(8);
    for (c, l) in ["A", "B", "C"].iter().enumerate() {
        let h = label(&format!("PEDAL {l}"), &["tm-eyebrow"]);
        grid.attach(&h, c as i32 + 1, 0, 1, 1);
    }
    let st = app.state.borrow();
    for bank in 0..7 {
        grid.attach(&label(&format!("Banco {}", bank + 1), &["tm-small", "tm-dim"]), 0, bank + 1, 1, 1);
        for c in 0..3 {
            let i = bank as usize * 3 + c;
            let b = gtk::Button::new();
            b.add_css_class("tm-pill");
            let text = if st.table.is_some() { st.title(i) } else { SLOT_LABELS[i].to_string() };
            let l = label(&format!("{} · {text}", SLOT_LABELS[i]), &["tm-small"]);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            l.set_max_width_chars(18);
            b.set_child(Some(&l));
            b.set_hexpand(true);
            let app = app.clone();
            b.connect_clicked(move |_| app.select(i));
            grid.attach(&b, c as i32 + 1, bank + 1, 1, 1);
        }
    }
    drop(st);
    body.append(&grid);
    let scroll = gtk::ScrolledWindow::builder().child(&body).vexpand(true).build();
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&scroll));
    dialog.set_child(Some(&tv));
    dialog.present(Some(&app.window));
}

pub fn shortcuts(app: &AppRef) {
    // Built from plain rows instead of AdwShortcutsDialog: that widget only exists
    // in libadwaita 1.8, and this keeps the app on 1.5+ (Ubuntu 24.04, Fedora 40,
    // Debian 13 and newer).
    let dialog = adw::Dialog::new();
    dialog.set_title("Atalhos");
    dialog.set_content_width(560);
    dialog.set_content_height(640);
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    let page = adw::PreferencesPage::new();
    for (title, items) in [
        (
            "Sons",
            &[
                ("Som anterior / próximo", "<Control>Left <Control>Right"),
                ("Salvar este preset", "<Control>s"),
                ("Salvar todos", "<Control><Shift>s"),
                ("Comparar com o salvo (A/B)", "<Control>b"),
                ("Bypass (tudo desligado)", "<Control><Shift>b"),
                ("Salvar na biblioteca", "<Control>d"),
                ("Recarregar da pedaleira", "<Control>r"),
            ][..],
        ),
        (
            "Edição",
            &[
                ("Desfazer", "<Control>z"),
                ("Refazer", "<Control><Shift>z"),
                ("Knob: ajuste fino", "Up Down"),
                ("Knob: passo de 10", "Page_Up Page_Down"),
                ("Knob: mínimo / máximo", "Home End"),
            ],
        ),
        (
            "Janela",
            &[
                ("Tela Tocar / Editar", "<Control>1 <Control>2"),
                ("Buscar", "<Control>f"),
                ("Mostrar/ocultar a lista", "F9"),
                ("Preferências", "<Control>comma"),
                ("Atalhos", "<Control>question"),
            ],
        ),
    ] {
        let group = adw::PreferencesGroup::new();
        group.set_title(title);
        for (t, accel) in items {
            let row = adw::ActionRow::new();
            row.set_title(t);
            let keys = gtk::Label::new(Some(&pretty_accel(accel)));
            keys.add_css_class("dim-label");
            keys.add_css_class("monospace");
            row.add_suffix(&keys);
            group.add(&row);
        }
        page.add(&group);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .child(&page)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    tv.set_content(Some(&scroll));
    dialog.set_child(Some(&tv));
    dialog.present(Some(&app.window));
}

/// `<Control><Shift>s` -> `Ctrl+Shift+S`, arrows as glyphs, alternatives with `/`.
///
/// Kept pure on purpose: this is the kind of formatting that quietly regresses
/// ("Ctrl+commA"), so it is covered by tests instead of only by eye.
fn pretty_accel(accel: &str) -> String {
    accel.split_whitespace().map(pretty_key).collect::<Vec<_>>().join(" / ")
}

fn pretty_key(one: &str) -> String {
    let mut mods = String::new();
    let mut rest = one;
    for (tag, label) in [
        ("<Control>", "Ctrl+"),
        ("<Shift>", "Shift+"),
        ("<Alt>", "Alt+"),
        ("<Super>", "Super+"),
    ] {
        while let Some(stripped) = rest.strip_prefix(tag) {
            mods.push_str(label);
            rest = stripped;
        }
    }
    let key = match rest {
        "Left" => "\u{2190}",
        "Right" => "\u{2192}",
        "Up" => "\u{2191}",
        "Down" => "\u{2193}",
        "Page_Up" => "PgUp",
        "Page_Down" => "PgDn",
        "comma" => ",",
        "question" => "?",
        "space" => "Espaço",
        other => other,
    };
    let key = if key.chars().count() == 1 {
        key.to_uppercase()
    } else {
        key.to_string()
    };
    format!("{mods}{key}")
}

pub fn preferences(app: &AppRef) {
    let dialog = adw::PreferencesDialog::new();
    let page = adw::PreferencesPage::new();
    page.set_title("Geral");
    dialog.add(&page);
    let s = app.state.borrow().settings.clone();

    let look = adw::PreferencesGroup::new();
    look.set_title("Aparência");
    let theme = adw::ComboRow::new();
    theme.set_title("Tema");
    theme.set_model(Some(&gtk::StringList::new(&["Sistema", "Escuro", "Claro"])));
    theme.set_selected(match s.theme.as_str() {
        "system" => 0,
        "light" => 2,
        _ => 1,
    });
    look.add(&theme);
    let pedals = adw::ComboRow::new();
    pedals.set_title("Visual dos blocos");
    pedals.set_subtitle("Como cada efeito aparece na tela Editar");
    pedals.set_model(Some(&gtk::StringList::new(&["Pedais realistas", "Pedais minimalistas", "Cartões"])));
    pedals.set_selected(match s.edit_style.as_str() {
        "flat" => 1,
        "cards" => 2,
        _ => 0,
    });
    look.add(&pedals);
    let english = adw::SwitchRow::new();
    english.set_title("Nomes dos parâmetros como nos amplificadores");
    english.set_subtitle("Gain, Treble, Mix… Desligado mostra em português");
    english.set_active(s.english_param_names);
    look.add(&english);
    page.add(&look);

    let play = adw::PreferencesGroup::new();
    play.set_title("Tocar e editar");
    let live = adw::SwitchRow::new();
    live.set_title("Edição ao vivo");
    live.set_subtitle("O som muda enquanto você gira os knobs, sem gravar na memória");
    live.set_active(s.live_edit);
    play.add(&live);
    let sens = adw::SpinRow::with_range(10.0, 200.0, 5.0);
    sens.set_title("Sensibilidade dos knobs");
    sens.set_subtitle("Unidades por 100 pixels arrastados");
    sens.set_value(s.knob_sensitivity as f64);
    play.add(&sens);
    let inst = adw::ComboRow::new();
    inst.set_title("Instrumento");
    inst.set_subtitle("Escolhe os sons prontos sugeridos");
    let labels: Vec<&str> = Instrument::ALL.iter().map(|i| i.label()).collect();
    inst.set_model(Some(&gtk::StringList::new(&labels)));
    inst.set_selected(Instrument::ALL.iter().position(|i| i.key() == s.instrument).unwrap_or(0) as u32);
    play.add(&inst);
    let confirm = adw::SwitchRow::new();
    confirm.set_title("Avisar ao fechar com alterações não salvas");
    confirm.set_active(s.confirm_discard);
    play.add(&confirm);
    page.add(&play);

    let adv = adw::PreferencesGroup::new();
    adv.set_title("Avançado");
    let all = adw::SwitchRow::new();
    all.set_title("Mostrar todos os parâmetros");
    all.set_subtitle("Inclui os 6 valores de cada bloco, mesmo os sem nome conhecido");
    all.set_active(s.show_all_params);
    adv.add(&all);
    let bytes = adw::SwitchRow::new();
    bytes.set_title("Modo técnico");
    bytes.set_subtitle("Bytes do preset, offsets nos tooltips e todos os parâmetros");
    bytes.set_active(s.show_advanced);
    adv.add(&bytes);
    let loopback = adw::SwitchRow::new();
    loopback.set_title("Loopback USB");
    loopback.set_subtitle("Manda o som do computador de volta pela entrada USB, para gravar ou transmitir junto (controle do app oficial)");
    loopback.set_active(app.state.borrow().loopback);
    adv.add(&loopback);
    {
        let app = app.clone();
        loopback.connect_active_notify(move |r| {
            app.state.borrow_mut().loopback = r.is_active();
            app.send(Command::SetLoopback(r.is_active()));
        });
    }
    let redo = adw::ActionRow::new();
    redo.set_title("Abrir o assistente inicial");
    redo.set_activatable(true);
    redo.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    adv.add(&redo);
    page.add(&adv);

    let apply = {
        let app = app.clone();
        let pedals = pedals.clone();
        let (theme, english, live, sens, inst, confirm, all, bytes) = (
            theme.clone(),
            english.clone(),
            live.clone(),
            sens.clone(),
            inst.clone(),
            confirm.clone(),
            all.clone(),
            bytes.clone(),
        );
        Rc::new(move || {
            {
                let mut st = app.state.borrow_mut();
                let s = &mut st.settings;
                s.theme = ["system", "dark", "light"][theme.selected() as usize].into();
                s.english_param_names = english.is_active();
                s.edit_style = ["pedals", "flat", "cards"][pedals.selected() as usize].into();
                s.live_edit = live.is_active();
                s.knob_sensitivity = sens.value() as u32;
                s.instrument = Instrument::ALL[inst.selected() as usize].key().into();
                s.confirm_discard = confirm.is_active();
                s.show_all_params = all.is_active();
                s.show_advanced = bytes.is_active();
            }
            let theme = app.state.borrow().settings.theme.clone();
            crate::theme::set_theme(&theme);
            app.save_settings();
            app.refresh_all();
        })
    };
    for row in [&english, &live, &confirm, &all, &bytes] {
        let apply = apply.clone();
        row.connect_active_notify(move |_| apply());
    }
    for row in [&theme, &inst, &pedals] {
        let apply = apply.clone();
        row.connect_selected_notify(move |_| apply());
    }
    {
        let apply = apply.clone();
        sens.connect_value_notify(move |_| apply());
    }
    {
        let app = app.clone();
        let dialog = dialog.clone();
        redo.connect_activated(move |_| {
            dialog.close();
            onboarding(&app);
        });
    }
    dialog.present(Some(&app.window));
}

pub fn about(app: &AppRef) {
    let about = adw::AboutDialog::new();
    about.set_application_name("TANK MINI");
    about.set_version(env!("CARGO_PKG_VERSION"));
    about.set_developer_name("v1ni");
    about.set_comments(
        "Controle a pedaleira M-VAVE TANK MINI pelo Linux. Protocolo USB-MIDI obtido por engenharia reversa, para interoperar com o equipamento próprio.",
    );
    about.set_license_type(gtk::License::MitX11);
    let fw = app.state.borrow().firmware.clone();
    if !fw.is_empty() {
        about.set_debug_info(&format!("firmware: {fw}\nnó: {}", app.state.borrow().node));
    }
    about.present(Some(&app.window));
}

pub fn setlist_editor(app: &AppRef, id: &str) {
    let id = id.to_string();
    let Some(set) = app.state.borrow().library.setlists.iter().find(|s| s.id == id).cloned() else { return };
    let dialog = adw::Dialog::new();
    dialog.set_title(&set.name);
    dialog.set_content_width(560);
    dialog.set_content_height(640);
    let page = adw::PreferencesPage::new();

    let meta = adw::PreferencesGroup::new();
    let name = adw::EntryRow::new();
    name.set_title("Nome");
    name.set_text(&set.name);
    meta.add(&name);
    let start = adw::ComboRow::new();
    start.set_title("Começa no preset");
    start.set_subtitle("Os sons são gravados em sequência a partir daqui");
    let slots: Vec<&str> = SLOT_LABELS.to_vec();
    start.set_model(Some(&gtk::StringList::new(&slots)));
    start.set_selected(set.start_slot as u32);
    meta.add(&start);
    page.add(&meta);

    let songs = adw::PreferencesGroup::new();
    songs.set_title("Sons, na ordem do show");
    page.add(&songs);

    let add_group = adw::PreferencesGroup::new();
    let pick = adw::ComboRow::new();
    pick.set_title("Adicionar da biblioteca");
    let entries: Vec<(String, String)> =
        app.state.borrow().library.entries.iter().map(|e| (e.id.clone(), e.name.clone())).collect();
    let names: Vec<&str> = entries.iter().map(|(_, n)| n.as_str()).collect();
    pick.set_model(Some(&gtk::StringList::new(&names)));
    pick.set_sensitive(!entries.is_empty());
    if entries.is_empty() {
        pick.set_subtitle("A biblioteca está vazia: salve sons nela primeiro (Ctrl+D)");
    }
    let add_btn = gtk::Button::from_icon_name("list-add-symbolic");
    add_btn.add_css_class("tm-icon");
    add_btn.set_valign(gtk::Align::Center);
    add_btn.set_sensitive(!entries.is_empty());
    pick.add_suffix(&add_btn);
    add_group.add(&pick);
    page.add(&add_group);

    let rows: Rc<std::cell::RefCell<Vec<adw::ActionRow>>> = Rc::default();
    let rebuild: Rc<Box<dyn Fn()>> = {
        let app = app.clone();
        let id = id.clone();
        let songs = songs.clone();
        let rows = rows.clone();
        Rc::new_cyclic(move |me: &std::rc::Weak<Box<dyn Fn()>>| {
            let me = me.clone();
            Box::new(move || {
                for r in rows.borrow_mut().drain(..) {
                    songs.remove(&r);
                }
                let st = app.state.borrow();
                let Some(set) = st.library.setlists.iter().find(|s| s.id == id) else { return };
                for (k, eid) in set.entries.iter().enumerate() {
                    let row = adw::ActionRow::new();
                    let slot = set.start_slot + k;
                    let name = st.library.get(eid).map(|e| e.name.clone()).unwrap_or_else(|| "(removido)".into());
                    row.set_title(&name);
                    row.set_subtitle(SLOT_LABELS.get(slot).copied().unwrap_or("não cabe"));
                    for (icon, tip, delta) in [("go-up-symbolic", "Subir", -1i32), ("go-down-symbolic", "Descer", 1)] {
                        let b = gtk::Button::from_icon_name(icon);
                        b.add_css_class("tm-icon");
                        b.set_valign(gtk::Align::Center);
                        b.set_tooltip_text(Some(tip));
                        let app = app.clone();
                        let id = id.clone();
                        let me = me.clone();
                        b.connect_clicked(move |_| {
                            {
                                let mut st = app.state.borrow_mut();
                                if let Some(s) = st.library.setlist_mut(&id) {
                                    let to = k as i32 + delta;
                                    if to >= 0 && (to as usize) < s.entries.len() {
                                        s.entries.swap(k, to as usize);
                                    }
                                }
                            }
                            app.save_library();
                            if let Some(f) = me.upgrade() {
                                glib::idle_add_local_once(move || f());
                            }
                        });
                        row.add_suffix(&b);
                    }
                    let del = gtk::Button::from_icon_name("list-remove-symbolic");
                    del.add_css_class("tm-icon");
                    del.set_valign(gtk::Align::Center);
                    let app = app.clone();
                    let id = id.clone();
                    let me = me.clone();
                    del.connect_clicked(move |_| {
                        if let Some(s) = app.state.borrow_mut().library.setlist_mut(&id) {
                            if k < s.entries.len() {
                                s.entries.remove(k);
                            }
                        }
                        app.save_library();
                        if let Some(f) = me.upgrade() {
                            glib::idle_add_local_once(move || f());
                        }
                    });
                    row.add_suffix(&del);
                    songs.add(&row);
                    rows.borrow_mut().push(row);
                }
            })
        })
    };
    rebuild();

    {
        let app = app.clone();
        let id = id.clone();
        let rebuild = rebuild.clone();
        let pick = pick.clone();
        add_btn.connect_clicked(move |_| {
            let Some((eid, _)) = entries.get(pick.selected() as usize).cloned() else { return };
            if let Some(s) = app.state.borrow_mut().library.setlist_mut(&id) {
                s.entries.push(eid);
            }
            app.save_library();
            rebuild();
        });
    }
    {
        let app = app.clone();
        let id = id.clone();
        let rebuild = rebuild.clone();
        start.connect_selected_notify(move |c| {
            if let Some(s) = app.state.borrow_mut().library.setlist_mut(&id) {
                s.start_slot = c.selected() as usize;
            }
            app.save_library();
            rebuild();
        });
    }
    {
        let app = app.clone();
        let id = id.clone();
        let dialog = dialog.clone();
        name.connect_changed(move |e| {
            let text = e.text().to_string();
            if let Some(s) = app.state.borrow_mut().library.setlist_mut(&id) {
                s.name = if text.trim().is_empty() { "Setlist".into() } else { text.clone() };
            }
            dialog.set_title(&text);
            app.save_library();
        });
    }

    let write = gtk::Button::with_label("Gravar na pedaleira");
    write.add_css_class("tm-cta");
    let delete = gtk::Button::with_label("Excluir");
    delete.add_css_class("tm-pill");
    {
        let app = app.clone();
        let id = id.clone();
        let dialog = dialog.clone();
        delete.connect_clicked(move |_| {
            app.state.borrow_mut().library.setlists.retain(|s| s.id != id);
            app.save_library();
            crate::rail::refresh(&app);
            dialog.close();
        });
    }
    {
        let app = app.clone();
        let id = id.clone();
        let dialog = dialog.clone();
        write.connect_clicked(move |_| write_setlist(&app, &id, &dialog));
    }
    {
        let app = app.clone();
        dialog.connect_closed(move |_| crate::rail::refresh(&app));
    }
    let header = adw::HeaderBar::new();
    header.pack_start(&delete);
    header.pack_end(&write);
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&header);
    tv.set_content(Some(&page));
    dialog.set_child(Some(&tv));
    dialog.present(Some(&app.window));
}

fn write_setlist(app: &AppRef, id: &str, parent: &adw::Dialog) {
    let plan = {
        let st = app.state.borrow();
        let Some(set) = st.library.setlists.iter().find(|s| s.id == id) else { return };
        match set.plan(&st.library) {
            Ok(p) => p
                .into_iter()
                .map(|(slot, e)| (slot, e.name.clone(), e.patch_bytes()))
                .collect::<Vec<_>>(),
            Err(e) => {
                drop(st);
                app.toast(&e);
                return;
            }
        }
    };
    if plan.is_empty() {
        app.toast("A setlist está vazia.");
        return;
    }
    if !app.state.borrow().online() {
        app.toast("Conecte a pedaleira para gravar a setlist.");
        return;
    }
    let first = SLOT_LABELS[plan[0].0];
    let last = SLOT_LABELS[plan[plan.len() - 1].0];
    let confirm = adw::AlertDialog::new(
        Some("Gravar a setlist?"),
        Some(&format!(
            "Os presets {first} a {last} serão substituídos pelos {} sons da setlist e salvos na memória. Um backup é feito antes.",
            plan.len()
        )),
    );
    confirm.add_responses(&[("cancel", "Cancelar"), ("go", "Gravar")]);
    confirm.set_response_appearance("go", adw::ResponseAppearance::Suggested);
    let app = app.clone();
    let parent_c = parent.clone();
    confirm.connect_response(None, move |_, r| {
        if r != "go" {
            return;
        }
        let table = {
            let mut st = app.state.borrow_mut();
            let Some(mut table) = st.saved.clone() else { return };
            for (slot, name, bytes) in &plan {
                if let Some(b) = bytes.as_ref().filter(|b| tankmini_core::patch::validate_patch(b).is_ok()) {
                    table.set_patch_raw(*slot, b);
                    st.names.set(*slot, name);
                }
            }
            let _ = st.names.save_default();
            table
        };
        app.send(Command::WriteTable { table, commit: true, note: "antes de gravar setlist".into() });
        parent_c.close();
    });
    confirm.present(Some(parent));
}

#[cfg(test)]
mod tests {
    //! Tests for the parts of the window that need no display server. Anything that
    //! builds a widget needs `gtk::init()` and a compositor, so the logic that can
    //! silently regress — formatting, labelling, ordering — is kept pure and tested.
    use super::pretty_accel;

    #[test]
    fn accelerators_become_readable_keys() {
        assert_eq!(pretty_accel("<Control>s"), "Ctrl+S");
        assert_eq!(pretty_accel("<Control><Shift>s"), "Ctrl+Shift+S");
        assert_eq!(pretty_accel("F9"), "F9");
        assert_eq!(pretty_accel("<Control>comma"), "Ctrl+,");
        assert_eq!(pretty_accel("<Control>question"), "Ctrl+?");
    }

    #[test]
    fn alternatives_are_joined() {
        assert_eq!(
            pretty_accel("<Control>Left <Control>Right"),
            "Ctrl+\u{2190} / Ctrl+\u{2192}"
        );
        assert_eq!(pretty_accel("Page_Up Page_Down"), "PgUp / PgDn");
    }

    #[test]
    fn single_letters_are_uppercased() {
        assert_eq!(pretty_accel("s"), "S");
    }
}

/// Import a cabinet IR from a WAV: convert it like the official app, let the
/// user hear it on the pedal (audition buffer, RAM) and store it in one of the
/// 20 CAB slots (flash, after a confirmation and a backup of that slot).
pub fn import_ir(app: &AppRef) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("IR em WAV"));
    filter.add_suffix("wav");
    filter.add_mime_type("audio/wav");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let picker = gtk::FileDialog::builder().title("Escolher um IR (WAV)").filters(&filters).build();
    let app = app.clone();
    picker.open(Some(&app.window.clone()), gio::Cancellable::NONE, move |res| {
        let Ok(file) = res else { return };
        let Some(path) = file.path() else { return };
        let wav = match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| ir::parse_wav(&b)) {
            Ok(w) => w,
            Err(e) => {
                app.toast(&format!("IR recusado: {e}"));
                return;
            }
        };
        ir_dialog(&app, &path, wav);
    });
}

pub fn ir_dialog(app: &AppRef, path: &std::path::Path, wav: ir::Wav) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Importar IR");
    dialog.set_content_width(480);

    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    let seconds = wav.samples.len() as f64 / wav.rate as f64;
    let used_ms = ir::SAMPLES as f64 * 1000.0 / ir::RATE as f64;
    group.set_title(&path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    group.set_description(Some(&format!(
        "{} Hz · {} canal(is) · {:.0} ms. A pedaleira usa os primeiros {:.0} ms, em mono a 44,1 kHz.",
        wav.rate,
        wav.channels,
        seconds * 1000.0,
        used_ms
    )));
    page.add(&group);

    let name = adw::EntryRow::new();
    name.set_title("Nome na pedaleira (até 14 letras)");
    name.set_text(&ir::name_from_path(path));
    group.add(&name);

    let level = adw::SpinRow::with_range(0.0, 127.0, 1.0);
    level.set_title("Volume do cab");
    level.set_subtitle("O app oficial começa em 50");
    level.set_value(ir::DEFAULT_LEVEL as f64);
    group.add(&level);

    let slots = adw::ComboRow::new();
    slots.set_title("Guardar no lugar de");
    let labels: Vec<String> = {
        let st = app.state.borrow();
        (0..ir::SLOT_COUNT)
            .map(|i| format!("{} · {}", i + 1, st.cab_names.get(i).map(String::as_str).unwrap_or("—")))
            .collect()
    };
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    slots.set_model(Some(&gtk::StringList::new(&refs)));
    // The last slot is the least likely to be one people rely on.
    slots.set_selected(ir::SLOT_COUNT as u32 - 1);
    group.add(&slots);

    let buttons = hbox(12);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(12);
    let listen = gtk::Button::with_label("Ouvir na pedaleira");
    listen.add_css_class("tm-pill");
    listen.set_tooltip_text(Some("Toca o IR na pedaleira sem gravar nada"));
    let save = gtk::Button::with_label("Gravar no cab…");
    save.add_css_class("tm-pill");
    save.add_css_class("tm-cta");
    buttons.append(&listen);
    buttons.append(&save);
    let actions = adw::PreferencesGroup::new();
    actions.add(&buttons);
    page.add(&actions);

    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&page));
    dialog.set_child(Some(&tv));

    let wav = Rc::new(wav);
    let record = {
        let name = name.clone();
        let level = level.clone();
        let wav = wav.clone();
        move || ir::record(name.text().trim(), level.value() as u8, &wav)
    };
    let online = {
        let app = app.clone();
        move || {
            let ok = matches!(app.state.borrow().conn, Conn::Online);
            if !ok {
                app.toast("Conecte a pedaleira pelo USB primeiro.");
            }
            ok
        }
    };
    {
        let app = app.clone();
        let record = record.clone();
        let online = online.clone();
        listen.connect_clicked(move |_| {
            if online() {
                app.send(Command::CabAudition(record()));
            }
        });
    }
    {
        let app = app.clone();
        let dialog_c = dialog.clone();
        save.connect_clicked(move |_| {
            if !online() {
                return;
            }
            let slot = slots.selected() as u8;
            let new_name = name.text().trim().to_string();
            if new_name.is_empty() {
                app.toast("Dê um nome ao IR.");
                return;
            }
            let old = labels[slot as usize].clone();
            let confirm = adw::AlertDialog::new(
                Some(&format!("Substituir o cab {old}?")),
                Some(&format!(
                    "\"{new_name}\" é gravado na memória da pedaleira no lugar dele. Uma cópia do cab atual fica guardada em {} para voltar depois.",
                    tankmini_core::worker::cab_backup_dir().display()
                )),
            );
            confirm.add_responses(&[("cancel", "Cancelar"), ("save", "Gravar")]);
            confirm.set_response_appearance("save", adw::ResponseAppearance::Destructive);
            let app = app.clone();
            let record = record.clone();
            let dialog_c2 = dialog_c.clone();
            confirm.connect_response(None, move |_, r| {
                if r == "save" {
                    app.send(Command::CabSave { slot, record: record() });
                    app.toast("Gravando o IR…");
                    dialog_c2.close();
                }
            });
            confirm.present(Some(&dialog_c));
        });
    }
    dialog.present(Some(&app.window.clone()));
}

/// Import an amp model (`.am4data`, e.g. from the M-VAVE community): audition
/// it on the pedal (RAM) or store it in one of the 20 AMP slots (flash, after a
/// confirmation and a backup of that slot).
pub fn import_amp(app: &AppRef) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Modelo de amp (.am4data)"));
    filter.add_suffix("am4data");
    filter.add_suffix("am4Data");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let picker = gtk::FileDialog::builder().title("Escolher um modelo de amp").filters(&filters).build();
    let app = app.clone();
    picker.open(Some(&app.window.clone()), gio::Cancellable::NONE, move |res| {
        let Ok(file) = res else { return };
        let Some(path) = file.path() else { return };
        match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| amp::Model::parse(&b)) {
            Ok(model) => amp_dialog(&app, &path, model),
            Err(e) => app.toast(&format!("Modelo recusado: {e}")),
        }
    });
}

pub fn amp_dialog(app: &AppRef, path: &std::path::Path, model: amp::Model) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Importar amp");
    dialog.set_content_width(480);

    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    group.set_title(&path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    group.set_description(Some(
        "Modelo AM4 (captura de amp por rede neural). Teste antes com \"Ouvir\": modelos AM4 ainda não foram confirmados em todas as versões da pedaleira.",
    ));
    page.add(&group);

    let name = adw::EntryRow::new();
    name.set_title("Nome na pedaleira (até 14 letras)");
    // Community models often carry a number as their internal name; the file
    // name says more.
    let from_file = ir::name_from_path(path);
    let looks_numeric = model.name.chars().all(|c| c.is_ascii_digit());
    name.set_text(if looks_numeric || model.name.is_empty() { &from_file } else { &model.name });
    group.add(&name);

    let slots = adw::ComboRow::new();
    slots.set_title("Guardar no lugar de");
    let labels: Vec<String> = {
        let st = app.state.borrow();
        (0..amp::SLOT_COUNT)
            .map(|i| format!("{} · {}", i + 1, st.amp_names.get(i).map(String::as_str).unwrap_or("—")))
            .collect()
    };
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    slots.set_model(Some(&gtk::StringList::new(&refs)));
    slots.set_selected(amp::SLOT_COUNT as u32 - 1);
    group.add(&slots);

    let buttons = hbox(12);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(12);
    let listen = gtk::Button::with_label("Ouvir na pedaleira");
    listen.add_css_class("tm-pill");
    listen.set_tooltip_text(Some("Toca o amp na pedaleira sem gravar nada"));
    let save = gtk::Button::with_label("Gravar no amp…");
    save.add_css_class("tm-pill");
    save.add_css_class("tm-cta");
    buttons.append(&listen);
    buttons.append(&save);
    let actions = adw::PreferencesGroup::new();
    actions.add(&buttons);
    page.add(&actions);

    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&page));
    dialog.set_child(Some(&tv));

    let model = Rc::new(model);
    let record = {
        let name = name.clone();
        let model = model.clone();
        let app = app.clone();
        move || match model.record(Some(name.text().trim())) {
            Ok(r) => Some(r),
            Err(e) => {
                app.toast(&format!("Não deu para preparar o amp: {e}"));
                None
            }
        }
    };
    let online = {
        let app = app.clone();
        move || {
            let ok = matches!(app.state.borrow().conn, Conn::Online);
            if !ok {
                app.toast("Conecte a pedaleira pelo USB primeiro.");
            }
            ok
        }
    };
    {
        let app = app.clone();
        let record = record.clone();
        let online = online.clone();
        listen.connect_clicked(move |_| {
            if online() {
                if let Some(r) = record() {
                    app.send(Command::AmpAudition(r));
                }
            }
        });
    }
    {
        let app = app.clone();
        let dialog_c = dialog.clone();
        save.connect_clicked(move |_| {
            if !online() {
                return;
            }
            let slot = slots.selected() as u8;
            let new_name = name.text().trim().to_string();
            if new_name.is_empty() {
                app.toast("Dê um nome ao amp.");
                return;
            }
            let Some(r) = record() else { return };
            let old = labels[slot as usize].clone();
            let confirm = adw::AlertDialog::new(
                Some(&format!("Substituir o amp {old}?")),
                Some(&format!(
                    "\"{new_name}\" é gravado na memória da pedaleira no lugar dele. Uma cópia do amp atual fica guardada em {} para voltar depois.",
                    tankmini_core::worker::amp_backup_dir().display()
                )),
            );
            confirm.add_responses(&[("cancel", "Cancelar"), ("save", "Gravar")]);
            confirm.set_response_appearance("save", adw::ResponseAppearance::Destructive);
            let app = app.clone();
            let dialog_c2 = dialog_c.clone();
            confirm.connect_response(None, move |_, resp| {
                if resp == "save" {
                    app.send(Command::AmpSave { slot, record: r.clone() });
                    app.toast("Gravando o amp…");
                    dialog_c2.close();
                }
            });
            confirm.present(Some(&dialog_c));
        });
    }
    dialog.present(Some(&app.window.clone()));
}

/// Browse the M-VAVE community: search shared amp models and IRs, download
/// one and open it in the import dialog. Network calls run on a thread.
pub fn community(app: &AppRef) {
    use tankmini_core::community::{self, Kind, Post, Sort};

    let dialog = adw::Dialog::new();
    dialog.set_title("Comunidade M-VAVE");
    dialog.set_content_width(640);
    dialog.set_content_height(720);

    let page = vbox(12);
    page.set_margin_top(12);
    page.set_margin_bottom(18);
    page.set_margin_start(18);
    page.set_margin_end(18);

    let intro = label(
        "Amps e IRs compartilhados por outros usuários. Esta pedaleira importa amps AM4 e IRs em WAV; os outros tipos aparecem só para consulta.",
        &["tm-small", "tm-dim"],
    );
    intro.set_wrap(true);
    page.append(&intro);

    let bar = hbox(8);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Buscar (ex.: Marshall, Fender, V30)"));
    search.set_hexpand(true);
    bar.append(&search);
    let sort = gtk::DropDown::from_strings(&["Mais baixados", "Mais recentes"]);
    bar.append(&sort);
    page.append(&bar);

    let only = gtk::CheckButton::with_label("Só o que dá para importar");
    only.set_active(true);
    page.append(&only);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    let status = label("", &["tm-small", "tm-faint"]);
    status.set_xalign(0.5);
    let more = gtk::Button::with_label("Carregar mais");
    more.add_css_class("tm-pill");
    more.set_halign(gtk::Align::Center);
    more.set_visible(false);
    let content = vbox(12);
    content.append(&list);
    content.append(&status);
    content.append(&more);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    page.append(&scroll);

    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&page));
    dialog.set_child(Some(&tv));

    // Page state: (query, sort, page loaded so far, generation to drop stale replies).
    let state = Rc::new(std::cell::RefCell::new((String::new(), Sort::Popular, 0u32, 0u64)));

    let open_file = {
        let app = app.clone();
        let dialog = dialog.clone();
        move |post: &Post, file: &community::File, button: &gtk::Button| {
            button.set_sensitive(false);
            button.set_label("Baixando…");
            let (tx, rx) = async_channel::bounded(1);
            let (post_id, file_c) = (post.id, file.clone());
            std::thread::spawn(move || {
                let r = community::download(post_id, &file_c).and_then(|bytes| {
                    let dir = community::download_dir().join(post_id.to_string());
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let safe: String =
                        file_c.display.chars().map(|c| if c == '/' || c == '\\' { '_' } else { c }).collect();
                    let path = dir.join(safe);
                    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
                    Ok((path, bytes))
                });
                let _ = tx.send_blocking(r);
            });
            let app = app.clone();
            let button = button.clone();
            let kind = file.kind;
            let parent = dialog.clone();
            glib::spawn_future_local(async move {
                button.set_sensitive(true);
                button.set_label("Abrir");
                match rx.recv().await {
                    Ok(Ok((path, bytes))) => {
                        let opened = match kind {
                            Kind::Amp => amp::Model::parse(&bytes).map(|m| amp_dialog(&app, &path, m)),
                            Kind::Ir => ir::parse_wav(&bytes).map(|w| ir_dialog(&app, &path, w)),
                            Kind::Other => Err("tipo não suportado".into()),
                        };
                        match opened {
                            Ok(()) => {
                                parent.close();
                            }
                            Err(e) => app.toast(&format!("Arquivo recusado: {e}")),
                        }
                    }
                    Ok(Err(e)) => app.toast(&e),
                    Err(_) => {}
                }
            });
        }
    };
    let open_file = Rc::new(open_file);

    let fill = {
        let list = list.clone();
        let only = only.clone();
        let open_file = open_file.clone();
        move |posts: Vec<Post>| {
            for post in posts {
                if only.is_active() && !post.importable() {
                    continue;
                }
                let row = adw::ExpanderRow::new();
                row.set_title(&glib::markup_escape_text(&post.title));
                let usable = post.files.iter().filter(|f| f.kind != Kind::Other).count();
                row.set_subtitle(&glib::markup_escape_text(&format!(
                    "{} · {} downloads · {} de {} arquivo(s) importáveis",
                    post.author,
                    post.downloads,
                    usable,
                    post.files.len()
                )));
                for file in &post.files {
                    let fr = adw::ActionRow::new();
                    fr.set_title(&glib::markup_escape_text(&file.display));
                    fr.set_subtitle(match file.kind {
                        Kind::Amp => "amp (AM4)",
                        Kind::Ir => "IR de caixa (WAV)",
                        Kind::Other => "não suportado por esta pedaleira",
                    });
                    if file.kind != Kind::Other {
                        let b = gtk::Button::with_label("Abrir");
                        b.add_css_class("tm-pill");
                        b.set_valign(gtk::Align::Center);
                        let open_file = open_file.clone();
                        let (post, file) = (post.clone(), file.clone());
                        b.connect_clicked(move |b| open_file(&post, &file, b));
                        fr.add_suffix(&b);
                    } else {
                        fr.set_sensitive(false);
                    }
                    row.add_row(&fr);
                }
                list.append(&row);
            }
        }
    };
    let fill = Rc::new(fill);

    // Load page `page` of the current query; page 1 clears the list.
    let load: Rc<dyn Fn(bool)> = {
        let state = state.clone();
        let list = list.clone();
        let status = status.clone();
        let more = more.clone();
        let fill = fill.clone();
        Rc::new(move |next: bool| {
            let (query, sort, page, gen) = {
                let mut s = state.borrow_mut();
                s.2 = if next { s.2 + 1 } else { 1 };
                s.3 += 1;
                (s.0.clone(), s.1, s.2, s.3)
            };
            if !next {
                while let Some(c) = list.first_child() {
                    list.remove(&c);
                }
            }
            status.set_text("Carregando…");
            more.set_visible(false);
            let (tx, rx) = async_channel::bounded(1);
            std::thread::spawn(move || {
                let _ = tx.send_blocking(community::search(&query, sort, page, 20));
            });
            let state = state.clone();
            let status = status.clone();
            let more = more.clone();
            let fill = fill.clone();
            let list = list.clone();
            glib::spawn_future_local(async move {
                let Ok(result) = rx.recv().await else { return };
                if state.borrow().3 != gen {
                    return; // a newer search replaced this one
                }
                match result {
                    Ok((posts, total)) => {
                        let got = posts.len();
                        fill(posts);
                        let shown = list.observe_children().n_items();
                        status.set_text(&if shown == 0 {
                            "Nada encontrado.".to_string()
                        } else {
                            format!("{total} publicações na comunidade")
                        });
                        more.set_visible(got > 0 && (page as u64) * 20 < total);
                    }
                    Err(e) => status.set_text(&format!("Sem conexão com a comunidade: {e}")),
                }
            });
        })
    };
    {
        let state = state.clone();
        let load = load.clone();
        search.connect_search_changed(move |e| {
            state.borrow_mut().0 = e.text().to_string();
            load(false);
        });
    }
    {
        let state = state.clone();
        let load = load.clone();
        sort.connect_selected_notify(move |d| {
            state.borrow_mut().1 = if d.selected() == 0 { Sort::Popular } else { Sort::Latest };
            load(false);
        });
    }
    {
        let load = load.clone();
        only.connect_toggled(move |_| load(false));
    }
    {
        let load = load.clone();
        more.connect_clicked(move |_| load(true));
    }
    load(false);
    dialog.present(Some(&app.window.clone()));
}
