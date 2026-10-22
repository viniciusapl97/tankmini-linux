//! A guitar-pedal style knob widget drawn with Cairo.
//!
//! Rounded arc, coloured value ring, pointer, value readout in the middle and a
//! label under the knob. Drag vertically (or scroll) to change the value. The
//! ring animates towards the target value so programmatic changes (loading a
//! preset) feel alive instead of jumping.

use std::cell::Cell;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

const START_ANGLE: f64 = 0.75 * PI; // 135°
const SWEEP: f64 = 1.5 * PI; // 270°

#[derive(Clone)]
pub struct Knob {
    pub area: gtk::DrawingArea,
    value: Rc<Cell<f64>>,
    shown: Rc<Cell<f64>>,
    max: f64,
    color: (f64, f64, f64),
}

impl Knob {
    /// `on_change` receives the new value (already rounded to an integer).
    pub fn new(
        label: &str,
        value: f64,
        max: f64,
        color: &str,
        on_change: impl Fn(u16) + 'static,
    ) -> Knob {
        let area = gtk::DrawingArea::new();
        area.set_content_width(82);
        area.set_content_height(104);
        area.set_focusable(true);
        area.set_tooltip_text(Some(label));

        let rgb = parse_hex(color);
        let value_cell = Rc::new(Cell::new(value));
        let shown = Rc::new(Cell::new(value));
        let on_change = Rc::new(on_change);

        let label_owned = label.to_string();
        {
            let value_cell = value_cell.clone();
            let shown = shown.clone();
            let on_change = on_change.clone();
            area.set_draw_func(move |_, cr, w, h| {
                draw(cr, w, h, shown.get(), max, rgb, &label_owned);
            });
        }

        // Animate towards the target value at ~60 fps.
        {
            let area_c = area.clone();
            let shown = shown.clone();
            let value_cell = value_cell.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(33), move || {
                let target = value_cell.get();
                let current = shown.get();
                let delta = target - current;
                if delta.abs() < 0.4 {
                    if current != target {
                        shown.set(target);
                        area_c.queue_draw();
                    }
                    return glib::ControlFlow::Continue;
                }
                shown.set(current + delta * 0.25);
                area_c.queue_draw();
                glib::ControlFlow::Continue
            });
        }

        // Drag to change: vertical movement, 1 unit per 2 px.
        {
            let area_c = area.clone();
            let value_cell = value_cell.clone();
            let on_change = on_change.clone();
            let drag = gtk::GestureDrag::new();
            let value_cell_d = value_cell.clone();
            let start_begin = Rc::new(Cell::new(0.0f64));
            let start_begin_c = start_begin.clone();
            drag.connect_drag_begin(move |_, _, y| start_begin_c.set(y));
            let area_d = area_c.clone();
            let start_d = start_begin.clone();
            let on_change_d = on_change.clone();
            drag.connect_drag_update(move |_, _, dy| {
                let _ = start_d.get();
                let next = (value_cell_d.get() - dy / 2.0).clamp(0.0, 100.0);
                value_cell_d.set(next);
                area_d.queue_draw();
                on_change_d(next.round() as u16);
            });
            area.add_controller(drag);
        }

        // Scroll wheel.
        {
            let value_cell = value_cell.clone();
            let on_change = on_change.clone();
            let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
            scroll.connect_scroll(move |_, _, dy| {
                let next = (value_cell.get() - dy * 3.0).clamp(0.0, 100.0);
                value_cell.set(next);
                on_change(next.round() as u16);
                glib::Propagation::Stop
            });
            area.add_controller(scroll);
        }

        let _ = max;
        Knob {
            area,
            value: value_cell,
            shown,
            max,
            color: rgb,
        }
    }

    pub fn set_value(&self, value: f64) {
        self.value.set(value.clamp(0.0, self.max));
    }

    pub fn value(&self) -> f64 {
        self.value.get()
    }

    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    pub fn color(&self) -> (f64, f64, f64) {
        self.color
    }

    pub fn shown(&self) -> f64 {
        self.shown.get()
    }
}

fn parse_hex(hex: &str) -> (f64, f64, f64) {
    let cleaned = hex.trim_start_matches('#');
    let parse = |s: &str| u8::from_str_radix(s, 16).unwrap_or(255) as f64 / 255.0;
    if cleaned.len() >= 6 {
        (parse(&cleaned[0..2]), parse(&cleaned[2..4]), parse(&cleaned[4..6]))
    } else {
        (1.0, 1.0, 1.0)
    }
}

fn draw(cr: &gtk::cairo::Context, w: i32, h: i32, value: f64, max: f64, rgb: (f64, f64, f64), label: &str) {
    let w = w as f64;
    let h = h as f64;
    let radius = (w.min(h - 22.0) / 2.0) - 6.0;
    let cx = w / 2.0;
    let cy = radius + 8.0;
    let frac = if max > 0.0 { (value / max).clamp(0.0, 1.0) } else { 0.0 };

    // Track.
    cr.set_line_width(7.0);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.set_source_rgba(0.16, 0.15, 0.14, 1.0);
    cr.arc(cx, cy, radius, START_ANGLE, START_ANGLE + SWEEP);
    let _ = cr.stroke();

    // Filled ring.
    cr.set_source_rgba(rgb.0, rgb.1, rgb.2, 1.0);
    cr.arc(cx, cy, radius, START_ANGLE, START_ANGLE + SWEEP * frac);
    let _ = cr.stroke();

    // Pointer.
    let angle = START_ANGLE + SWEEP * frac;
    let inner = radius - 10.0;
    cr.set_line_width(3.0);
    cr.set_source_rgba(0.95, 0.93, 0.90, 1.0);
    cr.move_to(cx + angle.cos() * (inner - 6.0), cy + angle.sin() * (inner - 6.0));
    cr.line_to(cx + angle.cos() * inner, cy + angle.sin() * inner);
    let _ = cr.stroke();

    // Value text.
    cr.set_font_size(16.0);
    cr.set_source_rgba(0.95, 0.93, 0.90, 1.0);
    let text = format!("{value:.0}");
    if let Ok(ext) = cr.text_extents(&text) {
        cr.move_to(cx - ext.width() / 2.0, cy + 5.0);
        let _ = cr.show_text(&text);
    }

    // Label.
    cr.set_font_size(12.5);
    cr.set_source_rgba(0.90, 0.87, 0.83, 1.0);
    if let Ok(ext) = cr.text_extents(label) {
        cr.move_to(cx - ext.width() / 2.0, h - 4.0);
        let _ = cr.show_text(label);
    }
}

/// Colours of the six effect families, used by the cards, chips and knobs.
pub mod palette {
    pub const FX: &str = "#34d399";
    pub const AMP: &str = "#f59e0b";
    pub const MOD: &str = "#a78bfa";
    pub const DLY: &str = "#60a5fa";
    pub const RVB: &str = "#2dd4bf";
    pub const CAB: &str = "#fb7185";

    pub fn for_role(role: tankmini_core::catalog::Role) -> &'static str {
        use tankmini_core::catalog::Role;
        match role {
            Role::Fx => FX,
            Role::Amp => AMP,
            Role::Modulation => MOD,
            Role::Delay => DLY,
            Role::Reverb => RVB,
            Role::Cabinet => CAB,
        }
    }
}

/// Small helper so callers can pick a display colour for a GDK RGBA.
pub fn rgba(hex: &str) -> gdk::RGBA {
    gdk::RGBA::parse(hex).unwrap_or(gdk::RGBA::new(1.0, 1.0, 1.0, 1.0))
}
