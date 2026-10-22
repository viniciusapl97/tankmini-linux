//! Window shell: header, sidebar, pages, banner and the bottom action bar.

use std::cell::RefCell;

use adw::prelude::*;
use gtk::{gio, glib, Orientation};
use tankmini_core::patch::SLOT_LABELS;
use tankmini_core::worker::{Command, Disconnect};

use crate::app::{AppRef, Conn, RailTab, View};
use crate::widgets::{hbox, label, pedal_art, vbox};

pub struct Ui {
    pub toasts: adw::ToastOverlay,
    pub banner: adw::Banner,
    pub split: adw::OverlaySplitView,
    pub modes: crate::toggle::ToggleGroup,
    pub stack: gtk::Stack,
    pub welcome_status: gtk::Label,
    pub welcome_fix: gtk::Box,
    pub rail_tabs: crate::toggle::ToggleGroup,
    pub tuner: crate::tuner::Shared,
    pub search: gtk::SearchEntry,
    pub rail_stack: gtk::Stack,
    pub preset_list: gtk::ListBox,
    pub library_list: gtk::ListBox,
    pub setlist_box: gtk::Box,
    pub rail_marks: RefCell<Vec<crate::rail::RowMarks>>,
    pub play_box: gtk::Box,
    pub edit_box: gtk::Box,
    pub bytes_view: RefCell<Option<gtk::TextView>>,
    pub status_dot: gtk::Box,
    pub status_label: gtk::Label,
    pub spinner: gtk::Spinner,
    pub dirty_label: gtk::Label,
    pub undo_btn: gtk::Button,
    pub redo_btn: gtk::Button,
    pub ab_btn: gtk::ToggleButton,
    pub bypass_btn: gtk::ToggleButton,
    pub save_btn: gtk::Button,
    pub save_all_btn: gtk::Button,
}

pub fn build(app: &AppRef) {
    let window = &app.window;
    window.set_title(Some("TANK MINI"));
    window.set_default_size(1280, 860);
    window.set_size_request(380, 560);
    window.add_css_class("tm-window");

    // ---- header ----
    let header = adw::HeaderBar::new();
    header.add_css_class("tm-header");
    let sidebar_btn = gtk::ToggleButton::new();
    sidebar_btn.set_icon_name("sidebar-show-symbolic");
    sidebar_btn.add_css_class("tm-icon");
    sidebar_btn.set_tooltip_text(Some("Mostrar/ocultar a lista (F9)"));
    header.pack_start(&sidebar_btn);
    let brand = vbox(0);
    brand.set_valign(gtk::Align::Center);
    brand.append(&label("TANK MINI", &["tm-brand"]));
    header.pack_start(&brand);

    let modes = crate::toggle::ToggleGroup::new();
    modes.add_css_class("tm-modes");
    modes.add_css_class("round");
    for (name, text, tip) in [
        ("play", crate::i18n::t("view.play"), "Sons prontos e controles simples (Ctrl+1)"),
        ("edit", crate::i18n::t("view.edit"), "Todos os blocos e parâmetros (Ctrl+2)"),
        ("tune", crate::i18n::t("view.tune"), "Afinador cromático pela entrada USB (Ctrl+3)"),
    ] {
        let t = crate::toggle::Toggle::new();
        t.set_name(Some(name));
        t.set_label(Some(text));
        t.set_tooltip(tip);
        modes.add(t);
    }
    header.set_title_widget(Some(&modes.root));

    let menu = gio::Menu::new();
    let s1 = gio::Menu::new();
    s1.append(Some("Salvar na biblioteca…"), Some("win.library-save"));
    s1.append(Some("Exportar preset…"), Some("win.export"));
    s1.append(Some("Exportar todos os presets…"), Some("win.export-all"));
    s1.append(Some("Importar preset…"), Some("win.import"));
    s1.append(Some("Copiar para outro slot…"), Some("win.copy-slot"));
    s1.append(Some("Trocar de lugar com…"), Some("win.swap-slots"));
    s1.append(Some("Importar IR (cab)…"), Some("win.import-ir"));
    s1.append(Some("Importar amp…"), Some("win.import-amp"));
    s1.append(Some("Comunidade M-VAVE…"), Some("win.community"));
    s1.append(Some("Renomear…"), Some("win.rename"));
    menu.append_section(None, &s1);
    let s2 = gio::Menu::new();
    s2.append(Some("Recarregar da pedaleira"), Some("win.reload"));
    s2.append(Some("Backups…"), Some("win.backups"));
    s2.append(Some("Restaurar de fábrica…"), Some("win.factory"));
    menu.append_section(None, &s2);
    let s3 = gio::Menu::new();
    s3.append(Some("Assistente inicial"), Some("win.onboarding"));
    s3.append(Some("Como usar a pedaleira"), Some("win.guide"));
    s3.append(Some("Atalhos de teclado"), Some("win.shortcuts"));
    s3.append(Some("Preferências"), Some("win.preferences"));
    s3.append(Some("Sobre"), Some("win.about"));
    menu.append_section(None, &s3);
    let menu_btn = gtk::MenuButton::new();
    menu_btn.set_icon_name("open-menu-symbolic");
    menu_btn.set_menu_model(Some(&menu));
    menu_btn.add_css_class("tm-icon");
    menu_btn.set_tooltip_text(Some("Menu"));
    header.pack_end(&menu_btn);

    // ---- sidebar ----
    let rail = vbox(0);
    rail.add_css_class("tm-rail");
    let rail_tabs = crate::toggle::ToggleGroup::new();
    rail_tabs.add_css_class("tm-rail-tabs");
    rail_tabs.add_css_class("round");
    rail_tabs.set_homogeneous(false);
    rail_tabs.set_can_shrink(true);
    for (name, text) in [("pedal", "Pedaleira"), ("library", "Biblioteca"), ("setlists", "Setlists")] {
        let t = crate::toggle::Toggle::new();
        t.set_name(Some(name));
        t.set_label(Some(text));
        rail_tabs.add(t);
    }
    rail.append(&rail_tabs.root);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Buscar sons"));
    search.add_css_class("tm-search");
    rail.append(&search);
    let rail_stack = gtk::Stack::new();
    rail_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    rail_stack.set_vexpand(true);
    let preset_list = gtk::ListBox::new();
    preset_list.set_selection_mode(gtk::SelectionMode::Single);
    let library_list = gtk::ListBox::new();
    library_list.set_selection_mode(gtk::SelectionMode::None);
    let setlist_box = vbox(6);
    for (name, child) in [
        ("pedal", preset_list.clone().upcast::<gtk::Widget>()),
        ("library", library_list.clone().upcast()),
        ("setlists", setlist_box.clone().upcast()),
    ] {
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&child)
            .vexpand(true)
            .build();
        rail_stack.add_named(&scroll, Some(name));
    }
    rail.append(&rail_stack);

    // ---- pages ----
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(180);
    // Only the visible page should set the window's minimum size: the bytes view
    // in the editor page is much wider than the play page.
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);

    let (welcome, welcome_status, welcome_fix) = welcome_page(app);
    stack.add_named(&welcome, Some("welcome"));

    let play_box = vbox(0);
    play_box.set_margin_bottom(28);
    let play_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_width(0)
        .child(&play_box)
        .build();
    play_scroll.add_css_class("tm-page");
    play_scroll.set_overlay_scrolling(false);
    stack.add_named(&play_scroll, Some("play"));

    let edit_box = vbox(0);
    edit_box.set_margin_bottom(28);
    let edit_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_width(0)
        .child(&edit_box)
        .build();
    edit_scroll.add_css_class("tm-page");
    edit_scroll.set_overlay_scrolling(false);
    stack.add_named(&edit_scroll, Some("edit"));

    // The tuner owns a capture thread; it starts once and reconnects on unplug.
    let tuner_shared = crate::tuner::Shared::new(440.0);
    crate::tuner::start(&tuner_shared);
    stack.add_named(&crate::tuner::page(app, &tuner_shared), Some("tune"));

    let split = adw::OverlaySplitView::new();
    split.set_sidebar(Some(&rail));
    split.set_content(Some(&stack));
    split.set_min_sidebar_width(240.0);
    split.set_max_sidebar_width(320.0);
    split
        .bind_property("show-sidebar", &sidebar_btn, "active")
        .bidirectional()
        .sync_create()
        .build();

    // ---- action bar ----
    let actions = hbox(8);
    actions.add_css_class("tm-actions");
    let status = hbox(8);
    status.add_css_class("tm-status");
    let status_dot = gtk::Box::new(Orientation::Horizontal, 0);
    status_dot.add_css_class("tm-dot");
    status_dot.set_valign(gtk::Align::Center);
    let status_label = label("", &[]);
    let spinner = gtk::Spinner::new();
    spinner.set_visible(false);
    status.append(&status_dot);
    status.append(&status_label);
    status.append(&spinner);
    actions.append(&status);
    let dirty_label = label("", &["tm-small", "tm-dim"]);
    dirty_label.set_margin_start(6);
    dirty_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    actions.append(&dirty_label);
    let spacer = hbox(0);
    spacer.set_hexpand(true);
    actions.append(&spacer);
    let bypass_btn = gtk::ToggleButton::with_label("Bypass");
    bypass_btn.add_css_class("tm-pill");
    bypass_btn.add_css_class("tm-ab");
    bypass_btn.set_tooltip_text(Some("Desliga todos os efeitos da pedaleira: som limpo do instrumento (Ctrl+Shift+B)"));
    actions.append(&bypass_btn);
    let undo_btn = icon_button("edit-undo-symbolic", "Desfazer (Ctrl+Z)");
    let redo_btn = icon_button("edit-redo-symbolic", "Refazer (Ctrl+Shift+Z)");
    actions.append(&undo_btn);
    actions.append(&redo_btn);
    let ab_btn = gtk::ToggleButton::with_label("A/B");
    ab_btn.add_css_class("tm-pill");
    ab_btn.add_css_class("tm-ab");
    ab_btn.set_tooltip_text(Some("Ouvir a versão salva para comparar (Ctrl+B)"));
    actions.append(&ab_btn);
    let save_all_btn = gtk::Button::with_label("Salvar todos");
    save_all_btn.add_css_class("tm-pill");
    save_all_btn.set_tooltip_text(Some("Salvar todos os presets alterados de uma vez"));
    actions.append(&save_all_btn);
    let save_btn = gtk::Button::with_label("Salvar na pedaleira");
    save_btn.add_css_class("tm-cta");
    save_btn.set_tooltip_text(Some("Gravar este preset na memória da pedaleira (Ctrl+S)"));
    actions.append(&save_btn);

    let banner = adw::Banner::new("");
    banner.add_css_class("tm-banner");

    let content = vbox(0);
    content.append(&banner);
    content.append(&split);
    split.set_vexpand(true);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    toolbar.add_bottom_bar(&actions);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&toolbar));
    window.set_content(Some(&toasts));

    if std::env::var_os("TM_DEBUG_MEASURE").is_some() {
        let split_c = split.clone();
        let win_c = window.clone();
        let probes: Vec<(&str, gtk::Widget)> = vec![
            ("actions", actions.clone().upcast()),
            ("header", header.clone().upcast()),
            ("split", split.clone().upcast()),
            ("stack", stack.clone().upcast()),
            ("rail", rail.clone().upcast()),
            ("play_box", play_box.clone().upcast()),
            ("edit_box", edit_box.clone().upcast()),
        ];
        gtk::glib::idle_add_local_once(move || {
            for (name, w) in probes {
                let (min, nat, _, _) = w.measure(gtk::Orientation::Horizontal, -1);
                eprintln!("[measure] {name}: min={min} natural={nat}");
                if name == "split" {
                    eprintln!(
                        "[measure] split.collapsed={} window_width={}",
                        split_c.is_collapsed(),
                        win_c.width()
                    );
                }
            }
        });
    }

    // Narrow windows: the sidebar becomes an overlay.
    // Collapse the rail earlier than Adwaita's default: at ~1000 px the board wins
    // more from the extra width (three columns) than the list does from being
    // permanently visible.
    let bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        1000.0,
        adw::LengthUnit::Sp,
    ));
    bp.add_setter(&split, "collapsed", Some(&true.to_value()));
    bp.add_setter(&save_all_btn, "visible", Some(&false.to_value()));
    bp.add_setter(&brand, "visible", Some(&false.to_value()));
    window.add_breakpoint(bp);

    // The action bar is the widest row in the window: shed the secondary
    // actions progressively instead of letting the bar clip.
    let bp_wide = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        1180.0,
        adw::LengthUnit::Sp,
    ));
    bp_wide.add_setter(&save_all_btn, "visible", Some(&false.to_value()));
    window.add_breakpoint(bp_wide);

    let bp_mid = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        1040.0,
        adw::LengthUnit::Sp,
    ));
    bp_mid.add_setter(&ab_btn, "visible", Some(&false.to_value()));
    window.add_breakpoint(bp_mid);

    let bp_small = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        950.0,
        adw::LengthUnit::Sp,
    ));
    bp_small.add_setter(&undo_btn, "visible", Some(&false.to_value()));
    bp_small.add_setter(&redo_btn, "visible", Some(&false.to_value()));
    bp_small.add_setter(&dirty_label, "visible", Some(&false.to_value()));
    window.add_breakpoint(bp_small);

    let ui = Ui {
        toasts,
        banner,
        split,
        modes,
        stack,
        welcome_status,
        welcome_fix,
        rail_tabs,
        tuner: tuner_shared,
        search,
        rail_stack,
        preset_list,
        library_list,
        setlist_box,
        rail_marks: RefCell::new(Vec::new()),
        play_box,
        edit_box,
        bytes_view: RefCell::new(None),
        status_dot,
        status_label,
        spinner,
        dirty_label,
        undo_btn,
        redo_btn,
        ab_btn,
        bypass_btn,
        save_btn,
        save_all_btn,
    };
    let _ = app.ui.set(ui);
    wire(app);
    install_actions(app);
}

fn icon_button(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("tm-icon");
    b.set_tooltip_text(Some(tip));
    b
}

fn welcome_page(app: &AppRef) -> (gtk::Box, gtk::Label, gtk::Box) {
    let page = vbox(18);
    page.set_valign(gtk::Align::Center);
    page.set_halign(gtk::Align::Center);
    page.set_margin_start(24);
    page.set_margin_end(24);
    let art = pedal_art(140);
    art.set_halign(gtk::Align::Center);
    page.append(&art);
    let title = label("Conecte sua TANK MINI", &["tm-title-xl"]);
    title.set_justify(gtk::Justification::Center);
    page.append(&title);
    let text = label(
        "Use o cabo USB-C no computador. Ele também alimenta a pedaleira.\nAssim que ela aparecer, seus 21 sons surgem aqui.",
        &["tm-dim"],
    );
    text.set_justify(gtk::Justification::Center);
    text.set_wrap(true);
    page.append(&text);
    let status = label("Procurando a pedaleira…", &["tm-status"]);
    status.set_halign(gtk::Align::Center);
    page.append(&status);
    let fix = vbox(8);
    fix.set_visible(false);
    page.append(&fix);
    let row = hbox(8);
    row.set_halign(gtk::Align::Center);
    let assistant = gtk::Button::with_label("Primeira vez? Abrir o assistente");
    assistant.add_css_class("tm-pill");
    assistant.set_action_name(Some("win.onboarding"));
    row.append(&assistant);
    page.append(&row);
    let _ = app;
    (page, status, fix)
}

fn wire(app: &AppRef) {
    let ui = app.ui();
    {
        let app = app.clone();
        ui.modes.connect_active_name_notify(move |g| {
            let view = match g.active_name().as_deref() {
                Some("edit") => View::Edit,
                Some("tune") => View::Tune,
                _ => View::Play,
            };
            {
                let mut st = app.state.borrow_mut();
                if st.view == view {
                    return;
                }
                st.view = view;
                st.settings.view = if view == View::Edit { "edit".into() } else { "play".into() };
            }
            app.save_settings();
            app.refresh_all();
        });
    }
    {
        let app = app.clone();
        ui.rail_tabs.connect_active_name_notify(move |g| {
            let tab = match g.active_name().as_deref() {
                Some("library") => RailTab::Library,
                Some("setlists") => RailTab::Setlists,
                _ => RailTab::Pedal,
            };
            app.state.borrow_mut().rail = tab;
            crate::rail::refresh(&app);
        });
    }
    {
        let app = app.clone();
        ui.search.connect_search_changed(move |e| {
            app.state.borrow_mut().search = e.text().to_string();
            crate::rail::refresh(&app);
        });
    }
    {
        let app = app.clone();
        ui.undo_btn.connect_clicked(move |_| app.undo());
    }
    {
        let app = app.clone();
        ui.redo_btn.connect_clicked(move |_| app.redo());
    }
    {
        let app = app.clone();
        ui.save_btn.connect_clicked(move |_| app.save_selected());
    }
    {
        let app = app.clone();
        ui.save_all_btn.connect_clicked(move |_| app.save_all());
    }
    {
        let app = app.clone();
        ui.bypass_btn.connect_toggled(move |b| {
            let on = b.is_active();
            if app.state.borrow().bypass != on {
                // Optimistic: the worker reports the real state back.
                app.state.borrow_mut().bypass = on;
                app.send(Command::SetBypass(on));
                refresh_actions(&app);
            }
        });
    }
    {
        let app = app.clone();
        ui.ab_btn.connect_toggled(move |b| {
            if app.state.borrow().comparing != b.is_active() {
                app.set_comparing(b.is_active());
            }
        });
    }
    {
        let app = app.clone();
        ui.banner.connect_button_clicked(move |_| {
            let conn = app.state.borrow().conn.clone();
            match conn {
                Conn::Offline(Disconnect::Permission(node)) => crate::dialogs::permission_help(&app, &node),
                Conn::Online => app.play_selected(),
                _ => app.send(Command::Reload),
            }
        });
    }
    // Unsaved edits: ask before closing.
    {
        let app = app.clone();
        app.window.clone().connect_close_request(move |w| {
            let dirty = app.state.borrow().dirty_slots().len();
            if dirty == 0 || !app.state.borrow().settings.confirm_discard {
                return glib::Propagation::Proceed;
            }
            let dialog = adw::AlertDialog::new(
                Some("Sair sem salvar?"),
                Some(&format!(
                    "{dirty} preset(s) têm alterações que ainda não estão na memória da pedaleira."
                )),
            );
            dialog.add_responses(&[("cancel", "Voltar"), ("discard", "Sair sem salvar"), ("save", "Salvar e sair")]);
            dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
            dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("save"));
            dialog.set_close_response("cancel");
            let app2 = app.clone();
            let win = w.clone();
            dialog.connect_response(None, move |_, r| match r {
                "discard" => {
                    forget_edits(&app2);
                    win.close();
                }
                "save" => {
                    if !app2.state.borrow().online() {
                        app2.toast("Conecte a pedaleira para salvar. Nada foi descartado.");
                        return;
                    }
                    app2.save_all();
                    let win = win.clone();
                    let app3 = app2.clone();
                    // Close once the write has landed; stay open if it fails.
                    glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
                        let (clean, failed, online) = {
                            let st = app3.state.borrow();
                            (st.dirty_slots().is_empty(), st.write_failed, st.online())
                        };
                        if clean {
                            win.close();
                            return glib::ControlFlow::Break;
                        }
                        if failed || !online {
                            app3.toast("Não deu para salvar. A janela continua aberta com suas alterações.");
                            return glib::ControlFlow::Break;
                        }
                        glib::ControlFlow::Continue
                    });
                }
                _ => {}
            });
            dialog.present(Some(w));
            glib::Propagation::Stop
        });
    }
}

/// Let the window close: nothing counts as unsaved any more.
fn forget_edits(app: &AppRef) {
    let table = app.state.borrow().table.clone();
    app.state.borrow_mut().saved = table;
}

fn install_actions(app: &AppRef) {
    let win = &app.window;
    let add = |name: &str, f: Box<dyn Fn(&AppRef)>| {
        let action = gio::SimpleAction::new(name, None);
        let app = app.clone();
        action.connect_activate(move |_, _| f(&app));
        win.add_action(&action);
    };
    add("save", Box::new(|a| a.save_selected()));
    add("save-all", Box::new(|a| a.save_all()));
    add("undo", Box::new(|a| a.undo()));
    add("redo", Box::new(|a| a.redo()));
    add("prev", Box::new(|a| a.step(-1)));
    add("next", Box::new(|a| a.step(1)));
    add("reload", Box::new(|a| a.send(Command::Reload)));
    add("view-play", Box::new(|a| a.ui().modes.set_active_name(Some("play"))));
    add("view-edit", Box::new(|a| a.ui().modes.set_active_name(Some("edit"))));
    add("view-tune", Box::new(|a| a.ui().modes.set_active_name(Some("tune"))));
    add("ab", Box::new(|a| {
        let on = !a.state.borrow().comparing;
        a.ui().ab_btn.set_active(on);
    }));
    add("bypass", Box::new(|a| {
        let on = !a.state.borrow().bypass;
        a.ui().bypass_btn.set_active(on);
    }));
    add("search", Box::new(|a| {
        a.ui().split.set_show_sidebar(true);
        a.ui().search.grab_focus();
    }));
    add("sidebar", Box::new(|a| {
        let s = &a.ui().split;
        s.set_show_sidebar(!s.shows_sidebar());
    }));
    add("revert", Box::new(|a| a.revert_selected()));
    add("library-save", Box::new(crate::dialogs::library_save));
    add("export", Box::new(crate::dialogs::export_preset));
    add("import", Box::new(crate::dialogs::import_preset));
    add("copy-slot", Box::new(crate::dialogs::copy_slot));
    add("export-all", Box::new(crate::dialogs::export_all));
    add("swap-slots", Box::new(crate::dialogs::swap_slots));
    add("import-ir", Box::new(crate::dialogs::import_ir));
    add("import-amp", Box::new(crate::dialogs::import_amp));
    add("community", Box::new(crate::dialogs::community));
    add("rename", Box::new(crate::dialogs::rename));
    add("backups", Box::new(crate::dialogs::backups));
    add("factory", Box::new(crate::dialogs::factory));
    add("onboarding", Box::new(crate::dialogs::onboarding));
    add("guide", Box::new(crate::dialogs::guide));
    add("shortcuts", Box::new(crate::dialogs::shortcuts));
    add("preferences", Box::new(crate::dialogs::preferences));
    add("about", Box::new(crate::dialogs::about));

    let gtk_app = app.window.application().expect("application");
    for (action, keys) in [
        ("win.save", &["<Control>s"][..]),
        ("win.save-all", &["<Control><Shift>s"]),
        ("win.undo", &["<Control>z"]),
        ("win.redo", &["<Control><Shift>z", "<Control>y"]),
        ("win.prev", &["<Control>Left", "<Control>Page_Up"]),
        ("win.next", &["<Control>Right", "<Control>Page_Down"]),
        ("win.reload", &["<Control>r"]),
        ("win.view-play", &["<Control>1"]),
        ("win.view-edit", &["<Control>2"]),
        ("win.view-tune", &["<Control>3"]),
        ("win.ab", &["<Control>b"]),
        ("win.bypass", &["<Control><Shift>b"]),
        ("win.search", &["<Control>f"]),
        ("win.sidebar", &["F9"]),
        ("win.library-save", &["<Control>d"]),
        ("win.preferences", &["<Control>comma"]),
        ("win.shortcuts", &["<Control>question", "<Control>slash"]),
        ("window.close", &["<Control>w"]),
    ] {
        gtk_app.set_accels_for_action(action, keys);
    }
}

// ---- refresh -----------------------------------------------------------

pub fn refresh(app: &AppRef) {
    let ui = app.ui();
    let (has_table, view, rail) = {
        let st = app.state.borrow();
        (st.table.is_some(), st.view, st.rail)
    };
    ui.modes.set_active_name(Some(match view {
        View::Edit => "edit",
        View::Tune => "tune",
        View::Play => "play",
    }));
    ui.rail_tabs.set_active_name(Some(match rail {
        RailTab::Pedal => "pedal",
        RailTab::Library => "library",
        RailTab::Setlists => "setlists",
    }));
    if !has_table {
        ui.stack.set_visible_child_name("welcome");
    } else {
        match view {
            View::Play => {
                crate::play::refresh(app);
                ui.stack.set_visible_child_name("play");
            }
            View::Tune => {
                ui.stack.set_visible_child_name("tune");
            }
            View::Edit => {
                crate::edit::refresh(app);
                ui.stack.set_visible_child_name("edit");
            }
        }
    }
    crate::rail::refresh_marks(app);
    refresh_chrome(app);
}

/// Banner, status pill and action bar (no view rebuild).
pub fn refresh_chrome(app: &AppRef) {
    refresh_banner(app);
    refresh_status(app);
    refresh_actions(app);
}

fn refresh_banner(app: &AppRef) {
    let ui = app.ui();
    let st = app.state.borrow();
    let (text, button): (String, Option<&str>) = match &st.conn {
        Conn::Offline(Disconnect::Permission(node)) => (
            format!("Sem permissão para abrir {node}. Seu usuário precisa estar no grupo audio."),
            Some("Como resolver"),
        ),
        Conn::Offline(Disconnect::NoReply(_)) => (
            "A pedaleira está no USB mas não respondeu. Desconecte e conecte o cabo de novo.".into(),
            Some("Tentar de novo"),
        ),
        Conn::Offline(Disconnect::Busy(_)) => (
            "Outro programa está usando a pedaleira (outra janela deste app ou o tankmini no terminal).".into(),
            Some("Tentar de novo"),
        ),
        Conn::Offline(Disconnect::Absent) if st.table.is_some() => (
            "Pedaleira desconectada. Suas alterações continuam aqui até ela voltar.".into(),
            None,
        ),
        Conn::Online if !st.playing(st.selected) => (
            format!(
                "Você está vendo {}, mas a pedaleira toca {}.",
                SLOT_LABELS[st.selected],
                st.active.map(|a| SLOT_LABELS[a as usize]).unwrap_or("?")
            ),
            Some("Tocar este"),
        ),
        _ => (String::new(), None),
    };
    ui.banner.set_title(&text);
    ui.banner.set_button_label(button);
    ui.banner.set_revealed(!text.is_empty());

    // Welcome page status line.
    let (line, fix) = match &st.conn {
        Conn::Searching => ("Procurando a pedaleira…".to_string(), None),
        Conn::Offline(Disconnect::Absent) => ("Pedaleira não encontrada no USB".to_string(), None),
        Conn::Offline(Disconnect::Permission(node)) => (format!("Sem permissão para abrir {node}"), Some(node.clone())),
        Conn::Offline(Disconnect::NoReply(_)) => ("A pedaleira não respondeu".to_string(), None),
        Conn::Offline(Disconnect::Busy(_)) => ("A pedaleira está aberta em outro programa".to_string(), None),
        Conn::Online => ("Conectada".to_string(), None),
    };
    ui.welcome_status.set_text(&line);
    while let Some(c) = ui.welcome_fix.first_child() {
        ui.welcome_fix.remove(&c);
    }
    ui.welcome_fix.set_visible(fix.is_some());
    if fix.is_some() {
        let b = gtk::Button::with_label("Como resolver a permissão");
        b.add_css_class("tm-cta");
        b.set_halign(gtk::Align::Center);
        let app2 = app.clone();
        let node = fix.unwrap_or_default();
        b.connect_clicked(move |_| crate::dialogs::permission_help(&app2, &node));
        ui.welcome_fix.append(&b);
    }
}

pub fn refresh_status(app: &AppRef) {
    let ui = app.ui();
    let st = app.state.borrow();
    for c in ["playing", "off", "live"] {
        ui.status_dot.remove_css_class(c);
    }
    let (text, class) = match &st.conn {
        Conn::Online if st.settings.live_edit && st.playing(st.selected) => {
            (format!("{} · ao vivo", st.firmware.replace('_', " ")), "live")
        }
        Conn::Online => (format!("{} · conectada", st.firmware.replace('_', " ")), "playing"),
        Conn::Searching => ("procurando…".to_string(), "off"),
        Conn::Offline(_) => ("desconectada".to_string(), "off"),
    };
    ui.status_dot.add_css_class(class);
    ui.status_label.set_text(&text);
    ui.status_label.set_tooltip_text(Some(&if st.node.is_empty() {
        "Edição ao vivo: o que você ajusta toca na hora, sem salvar".to_string()
    } else {
        format!("{} — ao vivo: o que você ajusta toca na hora, sem salvar", st.node)
    }));
    ui.spinner.set_visible(st.busy);
    if st.busy {
        ui.spinner.start();
    } else {
        ui.spinner.stop();
    }
}

pub fn refresh_actions(app: &AppRef) {
    let ui = app.ui();
    let st = app.state.borrow();
    let dirty_here = st.is_dirty(st.selected);
    let dirty_all = st.dirty_slots().len();
    let has = st.table.is_some();
    ui.save_btn.set_sensitive(dirty_here && st.online());
    ui.save_all_btn.set_visible(dirty_all > 1);
    ui.save_all_btn.set_label(&format!("Salvar todos ({dirty_all})"));
    ui.save_all_btn.set_sensitive(st.online());
    ui.undo_btn.set_sensitive(st.history.can_undo());
    ui.redo_btn.set_sensitive(st.history.can_redo());
    ui.ab_btn.set_visible(has);
    ui.bypass_btn.set_visible(has);
    ui.bypass_btn.set_sensitive(st.online());
    if ui.bypass_btn.is_active() != st.bypass {
        ui.bypass_btn.set_active(st.bypass);
    }
    ui.ab_btn.set_sensitive(dirty_here && st.playing(st.selected) && st.settings.live_edit);
    if ui.ab_btn.is_active() != st.comparing {
        ui.ab_btn.set_active(st.comparing);
    }
    ui.dirty_label.set_text(&if st.bypass && st.online() {
        "Bypass: a pedaleira está sem efeitos".to_string()
    } else if st.comparing {
        "Ouvindo a versão salva".to_string()
    } else if dirty_here {
        if st.settings.live_edit && st.playing(st.selected) {
            "Alterado: já está tocando, salve para guardar".to_string()
        } else {
            "Alterações não salvas".to_string()
        }
    } else if dirty_all > 0 {
        format!("{dirty_all} preset(s) com alterações")
    } else {
        String::new()
    });
}
