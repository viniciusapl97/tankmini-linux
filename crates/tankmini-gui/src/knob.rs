//! A guitar-pedal style knob drawn with Cairo + Pango.
//!
//! Thin 270° ring with a lit arc in the family colour, a solid face with a
//! pointer, the value in tabular figures and the label underneath. Drag
//! vertically, scroll, or use the keyboard (↑/↓ ±1, PgUp/PgDn ±10, Home/End);
//! double-click resets to the value the knob was created with.

use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib, pango};

const START: f64 = 0.75 * PI;
const SWEEP: f64 = 1.5 * PI;

/// Sizes: the Tocar macros are big, editor knobs compact.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KnobSize {
    Regular,
    /// Compact knob for grid cards, where horizontal room is scarce.
    Compact,
    Large,
    /// A stompbox knob: black cap, printed scale, label printed below.
    Pedal,
}

impl KnobSize {
    fn dims(self) -> (i32, i32, f64) {
        match self {
            KnobSize::Regular => (84, 104, 1.0),
            KnobSize::Compact => (68, 86, 1.0),
            KnobSize::Large => (148, 178, 1.75),
            KnobSize::Pedal => (106, 134, 1.0),
        }
    }
}

type Colour = Rc<dyn Fn() -> (f64, f64, f64)>;

#[derive(Clone)]
pub struct Knob {
    area: gtk::DrawingArea,
    /// Parameter name for real-unit display (see `catalog::format_value`).
    unit_of: Rc<Cell<Option<&'static str>>>,
}

impl Knob {
    /// `colour` is called at draw time so theme switches repaint correctly.
    /// `sensitivity` is value units per 100 px of drag. `on_change` receives
    /// the new integer value and whether the gesture is still running.
    pub fn new(
        label: &str,
        value: u16,
        size: KnobSize,
        sensitivity: f64,
        colour: impl Fn() -> (f64, f64, f64) + 'static,
        on_change: impl Fn(u16, bool) + 'static,
    ) -> Knob {
        let (w, h, scale) = size.dims();
        let area = gtk::DrawingArea::new();
        area.set_content_width(w);
        area.set_content_height(h);
        area.set_focusable(true);
        area.set_can_focus(true);
        area.add_css_class("tm-knob");
        area.set_cursor_from_name(Some("ns-resize"));

        let value_cell = Rc::new(Cell::new(value as f64));
        let shown = Rc::new(Cell::new(value as f64));
        let initial = value;
        let on_change: Rc<dyn Fn(u16, bool)> = Rc::new(on_change);
        let colour: Colour = Rc::new(colour);
        let label = Rc::new(RefCell::new(label.to_string()));
        let unit_of: Rc<Cell<Option<&'static str>>> = Rc::new(Cell::new(None));

        {
            let shown = shown.clone();
            let colour = colour.clone();
            let label = label.clone();
            let unit_of = unit_of.clone();
            area.set_draw_func(move |a, cr, w, h| {
                let v = shown.get();
                let text = unit_of
                    .get()
                    .and_then(|n| tankmini_core::catalog::format_value(n, v.round() as u16))
                    .unwrap_or_else(|| format!("{:.0}", v.round()));
                if size == KnobSize::Pedal {
                    draw_pedal(a, cr, w, h, v, &text, &label.borrow(), a.has_focus());
                } else {
                    draw(a, cr, w, h, v, &text, scale, colour(), &label.borrow(), a.has_focus());
                }
            });
        }

        let set = {
            let value_cell = value_cell.clone();
            let shown = shown.clone();
            let area = area.clone();
            let on_change = on_change.clone();
            Rc::new(move |next: f64, dragging: bool| {
                let next = next.clamp(0.0, 100.0);
                let changed = next.round() != value_cell.get().round();
                value_cell.set(next);
                shown.set(next);
                area.queue_draw();
                if changed || !dragging {
                    on_change(next.round() as u16, dragging);
                }
            })
        };

        // Drag: vertical, relative to where the drag started.
        {
            let drag = gtk::GestureDrag::new();
            let origin = Rc::new(Cell::new(0.0));
            let value_cell_b = value_cell.clone();
            let origin_b = origin.clone();
            let area_b = area.clone();
            drag.connect_drag_begin(move |_, _, _| {
                origin_b.set(value_cell_b.get());
                area_b.grab_focus();
            });
            let set_u = set.clone();
            let origin_u = origin.clone();
            drag.connect_drag_update(move |_, _, dy| {
                set_u(origin_u.get() - dy * sensitivity / 100.0, true);
            });
            let set_e = set.clone();
            let value_cell_e = value_cell.clone();
            drag.connect_drag_end(move |_, _, _| set_e(value_cell_e.get().round(), false));
            area.add_controller(drag);
        }

        // Double-click resets to the value the knob was built with.
        {
            let click = gtk::GestureClick::new();
            let set_c = set.clone();
            click.connect_pressed(move |_, n, _, _| {
                if n == 2 {
                    set_c(initial as f64, false);
                }
            });
            area.add_controller(click);
        }

        // Scroll: one step per notch, fine steps on touchpads.
        {
            let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
            let set_s = set.clone();
            let value_cell_s = value_cell.clone();
            scroll.connect_scroll(move |_, _, dy| {
                set_s((value_cell_s.get() - dy * 2.0).round(), false);
                glib::Propagation::Stop
            });
            area.add_controller(scroll);
        }

        // Keyboard.
        {
            let keys = gtk::EventControllerKey::new();
            let set_k = set.clone();
            let value_cell_k = value_cell.clone();
            keys.connect_key_pressed(move |_, key, _, _| {
                let v = value_cell_k.get().round();
                let next = match key {
                    gdk::Key::Up | gdk::Key::Right | gdk::Key::plus | gdk::Key::KP_Add => v + 1.0,
                    gdk::Key::Down | gdk::Key::Left | gdk::Key::minus | gdk::Key::KP_Subtract => v - 1.0,
                    gdk::Key::Page_Up => v + 10.0,
                    gdk::Key::Page_Down => v - 10.0,
                    gdk::Key::Home => 0.0,
                    gdk::Key::End => 100.0,
                    _ => return glib::Propagation::Proceed,
                };
                set_k(next, false);
                glib::Propagation::Stop
            });
            area.add_controller(keys);
            let focus = gtk::EventControllerFocus::new();
            let a1 = area.clone();
            focus.connect_enter(move |_| a1.queue_draw());
            let a2 = area.clone();
            focus.connect_leave(move |_| a2.queue_draw());
            area.add_controller(focus);
        }

        Knob { area, unit_of }
    }

    /// Show the value in real units when the catalog knows the scale.
    pub fn set_formatter(&self, param_name: Option<&'static str>) {
        self.unit_of.set(param_name);
        self.area.queue_draw();
    }

    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    widget: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    w: i32,
    h: i32,
    value: f64,
    text: &str,
    scale: f64,
    rgb: (f64, f64, f64),
    label: &str,
    focused: bool,
) {
    let (w, h) = (w as f64, h as f64);
    let label_h = 22.0 * scale.sqrt();
    let radius = (w.min(h - label_h) / 2.0) - 5.0 * scale;
    let cx = w / 2.0;
    let cy = radius + 5.0 * scale;
    let frac = (value / 100.0).clamp(0.0, 1.0);
    let ring = 4.0 * scale.sqrt();

    // Track.
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.set_line_width(ring);
    let (tr, tg, tb, ta) = crate::theme::track_rgba();
    cr.set_source_rgba(tr, tg, tb, ta);
    cr.arc(cx, cy, radius, START, START + SWEEP);
    let _ = cr.stroke();

    // Lit arc.
    if frac > 0.0 {
        cr.set_source_rgb(rgb.0, rgb.1, rgb.2);
        cr.arc(cx, cy, radius, START, START + SWEEP * frac);
        let _ = cr.stroke();
    }

    // Face.
    let face_r = radius - ring - 5.0 * scale;
    let (fr, fg, fb) = crate::theme::face_rgb();
    cr.set_source_rgb(fr, fg, fb);
    cr.arc(cx, cy, face_r, 0.0, 2.0 * PI);
    let _ = cr.fill_preserve();
    cr.set_line_width(1.0);
    cr.set_source_rgba(tr, tg, tb, if focused { ta * 4.0 } else { ta * 1.5 });
    let _ = cr.stroke();

    // Pointer.
    let angle = START + SWEEP * frac;
    let (xr, xg, xb) = crate::theme::text_rgb();
    cr.set_source_rgb(xr, xg, xb);
    cr.set_line_width(2.4 * scale.sqrt());
    let p_out = face_r - 4.0 * scale;
    let p_in = face_r - 11.0 * scale;
    cr.move_to(cx + angle.cos() * p_in, cy + angle.sin() * p_in);
    cr.line_to(cx + angle.cos() * p_out, cy + angle.sin() * p_out);
    let _ = cr.stroke();

    // Value.
    let layout = widget.create_pango_layout(Some(text));
    let mut font = pango::FontDescription::from_string("Adwaita Sans");
    font.set_weight(pango::Weight::Semibold);
    let size = if text.len() > 4 { 10.5 } else { 14.0 };
    font.set_absolute_size(size * scale.sqrt() * pango::SCALE as f64);
    layout.set_font_description(Some(&font));
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrFontFeatures::new("tnum"));
    layout.set_attributes(Some(&attrs));
    let (lw, lh) = layout.pixel_size();
    cr.move_to(cx - lw as f64 / 2.0, cy - lh as f64 / 2.0 + 1.0);
    cr.set_source_rgb(xr, xg, xb);
    pangocairo::functions::show_layout(cr, &layout);

    // Label.
    let layout = widget.create_pango_layout(Some(label));
    let mut font = pango::FontDescription::from_string("Adwaita Sans");
    font.set_weight(pango::Weight::Medium);
    font.set_absolute_size(12.0 * scale.sqrt().min(1.2) * pango::SCALE as f64);
    layout.set_font_description(Some(&font));
    layout.set_width((w * pango::SCALE as f64) as i32);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    layout.set_alignment(pango::Alignment::Center);
    let (_, lh) = layout.pixel_size();
    cr.move_to(0.0, h - lh as f64 - 1.0);
    cr.set_source_rgba(xr, xg, xb, 0.72);
    pangocairo::functions::show_layout(cr, &layout);
}

/// Ink printed on the pedal enclosures (cream, like silkscreen).
const INK: (f64, f64, f64) = (0.96, 0.93, 0.86);

#[allow(clippy::too_many_arguments)]
fn draw_pedal(
    widget: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    w: i32,
    _h: i32,
    value: f64,
    text: &str,
    label: &str,
    focused: bool,
) {
    let w = w as f64;
    let cx = w / 2.0;
    let r = 30.0;
    let cy = r + 13.0;
    let frac = (value / 100.0).clamp(0.0, 1.0);

    // Printed scale: 11 ticks, lit up to the value.
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    for i in 0..=10 {
        let t = i as f64 / 10.0;
        let a = START + SWEEP * t;
        let (inner, outer) = if i % 5 == 0 { (r + 4.0, r + 11.0) } else { (r + 5.0, r + 9.0) };
        let lit = t <= frac + 1e-6;
        cr.set_source_rgba(INK.0, INK.1, INK.2, if lit { 0.95 } else { 0.32 });
        cr.set_line_width(if i % 5 == 0 { 2.2 } else { 1.5 });
        cr.move_to(cx + a.cos() * inner, cy + a.sin() * inner);
        cr.line_to(cx + a.cos() * outer, cy + a.sin() * outer);
        let _ = cr.stroke();
    }

    // Shadow under the cap.
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.35);
    cr.arc(cx + 1.0, cy + 3.0, r, 0.0, 2.0 * PI);
    let _ = cr.fill();

    // Skirt: knurled-looking dark ring.
    let skirt = gtk::cairo::RadialGradient::new(cx - 6.0, cy - 8.0, 2.0, cx, cy, r);
    skirt.add_color_stop_rgb(0.0, 0.30, 0.30, 0.31);
    skirt.add_color_stop_rgb(1.0, 0.06, 0.06, 0.07);
    let _ = cr.set_source(&skirt);
    cr.arc(cx, cy, r, 0.0, 2.0 * PI);
    let _ = cr.fill();
    cr.set_line_width(0.8);
    for i in 0..48 {
        let a = i as f64 * PI / 24.0;
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.05);
        cr.move_to(cx + a.cos() * (r - 4.5), cy + a.sin() * (r - 4.5));
        cr.line_to(cx + a.cos() * (r - 0.5), cy + a.sin() * (r - 0.5));
        let _ = cr.stroke();
    }

    // Cap.
    let cap_r = r * 0.72;
    let cap = gtk::cairo::LinearGradient::new(cx, cy - cap_r, cx, cy + cap_r);
    cap.add_color_stop_rgb(0.0, 0.24, 0.24, 0.25);
    cap.add_color_stop_rgb(1.0, 0.08, 0.08, 0.09);
    let _ = cr.set_source(&cap);
    cr.arc(cx, cy, cap_r, 0.0, 2.0 * PI);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(1.0, 1.0, 1.0, if focused { 0.55 } else { 0.10 });
    cr.set_line_width(if focused { 1.6 } else { 1.0 });
    let _ = cr.stroke();

    // Pointer, from the cap edge into the skirt like a real pedal knob.
    let a = START + SWEEP * frac;
    cr.set_source_rgb(INK.0, INK.1, INK.2);
    cr.set_line_width(3.0);
    cr.move_to(cx + a.cos() * (cap_r * 0.35), cy + a.sin() * (cap_r * 0.35));
    cr.line_to(cx + a.cos() * (r - 2.0), cy + a.sin() * (r - 2.0));
    let _ = cr.stroke();

    // Label (silkscreen) and value.
    let layout = widget.create_pango_layout(Some(&label.to_uppercase()));
    let mut font = pango::FontDescription::from_string("Adwaita Sans");
    font.set_weight(pango::Weight::Heavy);
    font.set_absolute_size(11.0 * pango::SCALE as f64);
    layout.set_font_description(Some(&font));
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrInt::new_letter_spacing(1100));
    layout.set_attributes(Some(&attrs));
    layout.set_width((w * pango::SCALE as f64) as i32);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    layout.set_alignment(pango::Alignment::Center);
    cr.move_to(0.0, cy + r + 15.0);
    cr.set_source_rgb(INK.0, INK.1, INK.2);
    pangocairo::functions::show_layout(cr, &layout);

    let layout = widget.create_pango_layout(Some(text));
    let mut font = pango::FontDescription::from_string("Adwaita Sans");
    font.set_weight(pango::Weight::Semibold);
    font.set_absolute_size(11.5 * pango::SCALE as f64);
    layout.set_font_description(Some(&font));
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrFontFeatures::new("tnum"));
    layout.set_attributes(Some(&attrs));
    layout.set_width((w * pango::SCALE as f64) as i32);
    layout.set_alignment(pango::Alignment::Center);
    cr.move_to(0.0, cy + r + 31.0);
    cr.set_source_rgba(INK.0, INK.1, INK.2, 0.62);
    pangocairo::functions::show_layout(cr, &layout);
}
