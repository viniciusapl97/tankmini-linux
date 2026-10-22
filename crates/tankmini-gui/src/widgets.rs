//! Small widget helpers shared by the views.

use std::f64::consts::PI;

use gtk::prelude::*;
use gtk::Orientation;

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(Orientation::Vertical, spacing)
}

pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(Orientation::Horizontal, spacing)
}

pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

pub fn clear(container: &gtk::Box) {
    while let Some(c) = container.first_child() {
        container.remove(&c);
    }
}

pub fn dot(class: &str) -> gtk::Box {
    let d = hbox(0);
    d.add_css_class("tm-dot");
    d.add_css_class(class);
    d.set_valign(gtk::Align::Center);
    d
}

/// The slot badge ("1C"), inverted when that preset is playing.
pub fn slot_badge(text: &str, playing: bool, hero: bool) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(if hero { "tm-hero-slot" } else { "tm-slot" });
    if playing {
        l.add_css_class("playing");
    }
    l.set_valign(gtk::Align::Center);
    l
}

/// Section header: eyebrow text with optional trailing widget.
pub fn section(title: &str) -> gtk::Box {
    let row = hbox(8);
    row.set_margin_top(28);
    row.set_margin_bottom(10);
    row.append(&label(title, &["tm-eyebrow"]));
    row
}

/// A stylised TANK MINI: body, two knobs, three footswitches, a LED.
pub fn pedal_art(size: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(size);
    area.set_content_height(size * 4 / 5);
    area.set_draw_func(|_, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        let dark = crate::theme::is_dark();
        let (t0, t1, t2) = crate::theme::text_rgb();
        let x = 2.0;
        let y = 2.0;
        let bw = w - 4.0;
        let bh = h - 4.0;
        rounded(cr, x, y, bw, bh, bw * 0.14);
        if dark {
            cr.set_source_rgb(0.09, 0.09, 0.10);
        } else {
            cr.set_source_rgb(1.0, 1.0, 1.0);
        }
        let _ = cr.fill_preserve();
        cr.set_source_rgba(t0, t1, t2, 0.18);
        cr.set_line_width(1.2);
        let _ = cr.stroke();
        // LED
        let (r, g, b) = crate::theme::family_rgb(tankmini_core::catalog::Role::Amp);
        cr.set_source_rgb(r, g, b);
        cr.arc(x + bw * 0.5, y + bh * 0.13, bw * 0.025, 0.0, 2.0 * PI);
        let _ = cr.fill();
        // knobs
        for kx in [0.32, 0.68] {
            cr.set_source_rgba(t0, t1, t2, 0.85);
            cr.arc(x + bw * kx, y + bh * 0.36, bw * 0.095, 0.0, 2.0 * PI);
            let _ = cr.fill();
            if dark {
                cr.set_source_rgb(0.09, 0.09, 0.10);
            } else {
                cr.set_source_rgb(1.0, 1.0, 1.0);
            }
            cr.set_line_width(bw * 0.018);
            cr.move_to(x + bw * kx, y + bh * 0.36);
            cr.line_to(x + bw * kx + bw * 0.05, y + bh * 0.30);
            let _ = cr.stroke();
        }
        // footswitches
        for (i, fx) in [0.22, 0.5, 0.78].iter().enumerate() {
            cr.set_source_rgba(t0, t1, t2, 0.22);
            cr.arc(x + bw * fx, y + bh * 0.75, bw * 0.085, 0.0, 2.0 * PI);
            let _ = cr.fill();
            cr.set_source_rgba(t0, t1, t2, 0.7);
            cr.arc(x + bw * fx, y + bh * 0.75, bw * 0.05, 0.0, 2.0 * PI);
            let _ = cr.fill();
            let _ = i;
        }
    });
    area
}

pub fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}
