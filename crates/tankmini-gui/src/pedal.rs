//! A block drawn as a guitar stompbox: enamelled enclosure in the family
//! colour, corner screws, status LED, black knobs over a printed scale, the
//! effect name as the pedal's logo (click to change model) and a metal
//! footswitch that turns the block on and off.

use adw::prelude::*;
use tankmini_core::catalog::Role;

use crate::app::{AppRef, Redraw};
use crate::edit::{block_tools, param_knobs, set_variant, variant_labels};
use crate::knob::KnobSize;
use crate::widgets::{hbox, label, vbox};

/// `realistic`: enamel, screws, LED, black knobs and footswitch. Otherwise the
/// same pedal shape in the app's own style (surface card, family-coloured
/// knobs, a switch).
pub fn pedal(app: &AppRef, position: usize, module: usize, realistic: bool) -> gtk::Box {
    let (name, on, unit, labels) = {
        let st = app.state.borrow();
        let p = st.current().expect("table loaded");
        let role = Role::for_slot(module);
        (st.effect(&p, module), p.is_on(module), p.unit(module), variant_labels(&st, role))
    };
    let role = Role::for_slot(module);

    let slot = vbox(8);
    slot.add_css_class("tm-pedal-slot");
    slot.set_valign(gtk::Align::Start);

    // Above the pedal, on the "board": order, role and the block tools.
    let top = hbox(6);
    let order = label(&format!("{} · {}", position + 1, role.label().to_uppercase()), &["tm-eyebrow"]);
    order.set_hexpand(true);
    order.set_valign(gtk::Align::Center);
    order.set_tooltip_text(Some(role.description()));
    top.append(&order);
    top.append(&block_tools(app, position, module));
    slot.append(&top);

    // Enclosure.
    let body = vbox(0);
    if realistic {
        body.add_css_class("tm-pedal");
        body.add_css_class(enclosure_class(role));
    } else {
        body.add_css_class("tm-card");
        body.add_css_class("tm-flat-pedal");
    }
    if !on {
        body.add_css_class("off");
    }
    // Narrower minimum so the board can reflow to two or one column on small
    // windows instead of forcing a horizontal overflow.
    body.set_size_request(if realistic { 260 } else { 230 }, -1);
    // Let the card take the width its column gives it, so the board fills the
    // page instead of leaving a dead band on the right.
    body.set_hexpand(true);
    body.set_halign(gtk::Align::Fill);

    let led = hbox(0);
    if realistic {
        led.add_css_class("tm-led");
        if on {
            led.add_css_class("on");
        }
    } else {
        led.add_css_class("tm-family");
        led.add_css_class(crate::theme::family_class(role));
        if !on {
            led.set_opacity(0.3);
        }
    }
    led.set_halign(gtk::Align::Center);
    led.set_margin_top(16);
    led.set_tooltip_text(Some(if on { "Ligado" } else { "Desligado" }));
    body.append(&led);

    // Knobs.
    let knobs = param_knobs(app, module, if realistic { KnobSize::Pedal } else { KnobSize::Regular });
    if knobs.is_empty() {
        let none = label("sem ajustes", &["tm-pedal-print", "tm-pedal-role"]);
        none.set_halign(gtk::Align::Center);
        none.set_margin_top(18);
        body.append(&none);
    } else {
        let grid = gtk::Grid::new();
        grid.set_halign(gtk::Align::Center);
        grid.set_column_spacing(4);
        grid.set_row_spacing(2);
        grid.set_margin_top(8);
        let cols = if knobs.len() == 4 { 2 } else { 3.min(knobs.len()) };
        for (i, k) in knobs.iter().enumerate() {
            grid.attach(k.widget(), (i % cols) as i32, (i / cols) as i32, 1, 1);
        }
        body.append(&grid);
    }

    // Logo: the model name. Clicking it swaps the model, like changing pedals.
    let logo = gtk::MenuButton::new();
    logo.add_css_class("tm-pedal-logo");
    logo.set_halign(gtk::Align::Center);
    logo.set_margin_top(14);
    let logo_box = vbox(0);
    let title = label(&name, &[if realistic { "tm-pedal-name" } else { "tm-title-l" }]);
    title.set_xalign(0.5);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(16);
    logo_box.append(&title);
    let sub_row = hbox(4);
    sub_row.set_halign(gtk::Align::Center);
    let role_class = if realistic { "tm-pedal-role" } else { "tm-eyebrow" };
    sub_row.append(&label(&role.label().to_uppercase(), &[role_class]));
    if labels.len() > 1 {
        let arrow = gtk::Image::from_icon_name("pan-down-symbolic");
        arrow.add_css_class(role_class);
        arrow.set_pixel_size(12);
        sub_row.append(&arrow);
    }
    logo_box.append(&sub_row);
    logo.set_child(Some(&logo_box));
    logo.set_always_show_arrow(false);
    if labels.len() > 1 {
        logo.set_tooltip_text(Some("Trocar o modelo"));
        logo.set_popover(Some(&model_picker(app, module, unit, &labels)));
    } else {
        logo.set_sensitive(false);
    }
    body.append(&logo);

    if !realistic {
        let switch = gtk::Switch::new();
        switch.set_active(on);
        switch.set_halign(gtk::Align::Center);
        switch.set_margin_top(16);
        switch.set_margin_bottom(20);
        switch.set_tooltip_text(Some("Ligar/desligar este bloco"));
        let app = app.clone();
        switch.connect_state_set(move |_, value| {
            app.edit(None, Redraw::Views, |t, i| {
                t.set_on(i, module, value);
            });
            gtk::glib::Propagation::Proceed
        });
        body.append(&switch);
        slot.append(&body);
        return slot;
    }

    // Footswitch.
    let stomp = gtk::Button::new();
    stomp.add_css_class("tm-footswitch");
    stomp.set_halign(gtk::Align::Center);
    stomp.set_margin_top(16);
    stomp.set_tooltip_text(Some(if on { "Pisar para desligar" } else { "Pisar para ligar" }));
    {
        let app = app.clone();
        stomp.connect_clicked(move |_| {
            app.edit(None, Redraw::Views, |t, i| {
                t.set_on(i, module, !on);
            });
        });
    }
    body.append(&stomp);
    let state = label(if on { "ON" } else { "OFF" }, &["tm-pedal-role"]);
    state.set_halign(gtk::Align::Center);
    state.set_margin_top(6);
    state.set_margin_bottom(18);
    body.append(&state);

    // Corner screws over the enclosure.
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&body));
    for (h, v) in [
        (gtk::Align::Start, gtk::Align::Start),
        (gtk::Align::End, gtk::Align::Start),
        (gtk::Align::Start, gtk::Align::End),
        (gtk::Align::End, gtk::Align::End),
    ] {
        let screw = hbox(0);
        screw.add_css_class("tm-screw");
        screw.set_halign(h);
        screw.set_valign(v);
        screw.set_margin_start(9);
        screw.set_margin_end(9);
        screw.set_margin_top(9);
        screw.set_margin_bottom(9);
        screw.set_can_target(false);
        overlay.add_overlay(&screw);
    }
    slot.append(&overlay);
    slot
}

/// A list of every model of this block; picking one swaps it.
fn model_picker(app: &AppRef, module: usize, unit: u8, labels: &[String]) -> gtk::Popover {
    let pop = gtk::Popover::new();
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("navigation-sidebar");
    for (i, name) in labels.iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        let line = hbox(10);
        line.set_margin_start(6);
        line.set_margin_end(6);
        let l = label(name, &[]);
        l.set_hexpand(true);
        line.append(&l);
        if i == unit as usize {
            line.append(&gtk::Image::from_icon_name("object-select-symbolic"));
        }
        row.set_child(Some(&line));
        list.append(&row);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(380)
        .min_content_width(220)
        .child(&list)
        .build();
    pop.set_child(Some(&scroll));
    let app = app.clone();
    let pop_c = pop.clone();
    list.connect_row_activated(move |_, row| {
        pop_c.popdown();
        set_variant(&app, module, row.index() as u8);
    });
    pop
}

fn enclosure_class(role: Role) -> &'static str {
    match role {
        Role::Fx => "enc-fx",
        Role::Amp => "enc-amp",
        Role::Modulation => "enc-mod",
        Role::Delay => "enc-dly",
        Role::Reverb => "enc-rvb",
        Role::Cabinet => "enc-cab",
    }
}

/// The pedalboard: every block of the preset as a pedal, in signal order.
pub fn board(app: &AppRef, realistic: bool) -> gtk::FlowBox {
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(6);
    flow.set_column_spacing(14);
    flow.set_row_spacing(18);
    flow.set_valign(gtk::Align::Start);
    // Fill the page instead of shrinking to a centred natural width: this is what
    // lets the board use a third column once there is room for it.
    flow.set_halign(gtk::Align::Fill);
    flow.set_hexpand(true);
    let chain = app.state.borrow().current().unwrap().chain();
    let mut first = None;
    for (pos, module) in chain.iter().enumerate() {
        let card = pedal(app, pos, *module as usize, realistic);
        if first.is_none() {
            first = Some(card.clone());
        }
        flow.insert(&card, -1);
    }
    if std::env::var_os("TM_DEBUG_MEASURE").is_some() {
        if let Some(card) = first {
            gtk::glib::idle_add_local_once(move || {
                let (cmin, cnat, _, _) = card.measure(gtk::Orientation::Horizontal, -1);
                let (hmin, _, _, _) = card.measure(gtk::Orientation::Vertical, 300);
                eprintln!("[measure] card: min_w={cmin} nat_w={cnat} min_h_at300={hmin}");
            });
        }
    }
    flow
}
