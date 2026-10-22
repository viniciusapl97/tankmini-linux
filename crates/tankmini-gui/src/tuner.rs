//! Tuner screen: reads the pedal's USB audio input and shows the note being played.
//!
//! The capture runs on its own thread (a `pw-record` child process), so the window
//! never blocks on the audio device. The thread keeps the last measurement in a
//! shared slot and the UI polls it on a timer.

use std::io::Read;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use adw::prelude::*;
use gtk::glib;
use tankmini_core::pitch::{self, Pitch};

use tankmini_core::worker::{Command, TunerControl};

use crate::app::AppRef;
use crate::widgets::{hbox, label, vbox};

const RATE: u32 = 44_100;

/// What the capture thread publishes for the UI.
#[derive(Clone)]
pub struct Shared {
    pub pitch: Arc<Mutex<Option<Pitch>>>,
    /// True while a capture process is alive, so the UI can say why it shows nothing.
    pub capturing: Arc<AtomicBool>,
    pub a4: Arc<Mutex<f32>>,
    /// Latest reading of the pedal's own tuner, which runs while this page is
    /// shown. Used when the audio capture has nothing to say.
    pub pedal: Arc<Mutex<Option<tankmini_core::tuner::Reading>>>,
}

impl Shared {
    pub fn new(a4: f32) -> Self {
        Self {
            pitch: Arc::new(Mutex::new(None)),
            capturing: Arc::new(AtomicBool::new(false)),
            a4: Arc::new(Mutex::new(a4)),
            pedal: Arc::new(Mutex::new(None)),
        }
    }
}

/// The pedal's capture source name in PipeWire/PulseAudio.
fn pedal_source() -> Option<String> {
    let out = std::process::Command::new("pactl")
        .args(["list", "short", "sources"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| l.contains("Jieli") && !l.contains(".monitor"))
        .and_then(|l| l.split_whitespace().nth(1))
        .map(str::to_string)
}

/// Spawn the capture thread once. It reconnects if the pedal is unplugged.
pub fn start(shared: &Shared) {
    let shared = shared.clone();
    std::thread::spawn(move || {
        let window = pitch::WINDOW;
        loop {
            let Some(src) = pedal_source() else {
                shared.capturing.store(false, Ordering::Relaxed);
                *shared.pitch.lock().unwrap() = None;
                std::thread::sleep(std::time::Duration::from_secs(3));
                continue;
            };
            let child = std::process::Command::new("pw-record")
                .args([
                    "--raw",
                    "--format=f32",
                    "--rate=44100",
                    "--channels=1",
                    "--latency=4096/44100",
                    &format!("--target={src}"),
                    "-",
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn();

            let Ok(mut child) = child else {
                shared.capturing.store(false, Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_secs(3));
                continue;
            };
            shared.capturing.store(true, Ordering::Relaxed);

            if let Some(mut out) = child.stdout.take() {
                let mut bytes = vec![0u8; window * 4];
                loop {
                    let mut filled = 0;
                    let mut alive = true;
                    while filled < bytes.len() {
                        match out.read(&mut bytes[filled..]) {
                            Ok(0) => {
                                alive = false;
                                break;
                            }
                            Ok(n) => filled += n,
                            Err(_) => {
                                alive = false;
                                break;
                            }
                        }
                    }
                    if !alive || filled < bytes.len() {
                        break;
                    }
                    let samples: Vec<f32> = bytes
                        .chunks_exact(4)
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect();
                    let a4 = *shared.a4.lock().unwrap();
                    let measured = pitch::detect(&samples, RATE)
                        .map(|p| pitch::pitch_from_hz(p.hz, a4, p.rms));
                    *shared.pitch.lock().unwrap() = measured;
                }
            }
            let _ = child.kill();
            shared.capturing.store(false, Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

/// What the screen shows for one moment, from either source.
#[derive(Clone, PartialEq)]
struct Reading {
    note: String,
    /// Negative = below the note (flat), positive = above (sharp).
    cents: f32,
    hz: Option<f32>,
    /// Index into [`pitch::STRINGS`] of the open string this is closest to.
    string: Option<usize>,
}

/// Within this many cents the string counts as in tune (the official app
/// uses about the same window).
const IN_TUNE: f32 = 5.0;

fn nearest_string_index(hz: f32) -> Option<usize> {
    pitch::STRINGS
        .iter()
        .enumerate()
        .map(|(i, (f, _))| (i, (1200.0 * (hz / f).log2()).abs()))
        .filter(|(_, d)| *d <= 250.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

fn current(shared: &Shared) -> Option<Reading> {
    if let Some(p) = shared.pitch.lock().ok().and_then(|p| p.clone()) {
        return Some(Reading { note: p.label(), cents: p.cents, hz: Some(p.hz), string: nearest_string_index(p.hz) });
    }
    let r = shared.pedal.lock().ok().and_then(|r| *r).filter(|r| r.running)?;
    Some(Reading {
        note: r.note_name()?.to_string(),
        cents: r.deviation.unwrap_or(0) as f32,
        hz: None,
        string: None,
    })
}

/// The meter: a −50…+50 cent scale, a green zone in the middle and a needle.
/// Left of centre is flat (string too loose), right is sharp (too tight).
fn draw_meter(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, w: f64, h: f64, cents: Option<f32>) {
    let (tr, tg, tb) = crate::theme::text_rgb();
    let pad = 28.0;
    let x0 = pad;
    let x1 = w - pad;
    let mid = w / 2.0;
    let y = h * 0.42;
    let at = |c: f32| mid + (c.clamp(-50.0, 50.0) as f64 / 50.0) * (x1 - mid);

    // In-tune zone.
    cr.set_source_rgba(0.26, 0.82, 0.48, 0.22);
    let (zl, zr) = (at(-IN_TUNE), at(IN_TUNE));
    cr.rectangle(zl, y - 22.0, zr - zl, 44.0);
    let _ = cr.fill();

    // Ticks every 10 cents, longer at 0 and ±50.
    for c in (-50..=50).step_by(5) {
        let x = at(c as f32);
        let len = if c == 0 { 26.0 } else if c % 10 == 0 { 14.0 } else { 7.0 };
        let alpha = if c == 0 { 0.9 } else { 0.35 };
        cr.set_source_rgba(tr, tg, tb, alpha);
        cr.set_line_width(if c == 0 { 2.5 } else { 1.5 });
        cr.move_to(x, y - len);
        cr.line_to(x, y + len);
        let _ = cr.stroke();
    }

    // End labels, through Pango so ♭/♯ get a font that has them.
    cr.set_source_rgba(tr, tg, tb, 0.55);
    let mut font = gtk::pango::FontDescription::from_string("Adwaita Sans");
    font.set_weight(gtk::pango::Weight::Bold);
    font.set_absolute_size(13.0 * gtk::pango::SCALE as f64);
    let label = |text: &str, x: f64, align: f64| {
        let layout = area.create_pango_layout(Some(text));
        layout.set_font_description(Some(&font));
        let (lw, lh) = layout.pixel_size();
        cr.move_to(x - lw as f64 * align, h - 8.0 - lh as f64);
        pangocairo::functions::show_layout(cr, &layout);
    };
    label(&format!("♭ {}", crate::i18n::t("tuner.flat_side")), x0, 0.0);
    label("0", mid, 0.5);
    label(&format!("{} ♯", crate::i18n::t("tuner.sharp_side")), x1, 1.0);

    // Needle.
    if let Some(c) = cents {
        let x = at(c);
        let good = c.abs() <= IN_TUNE;
        if good {
            cr.set_source_rgb(0.26, 0.82, 0.48);
        } else {
            cr.set_source_rgb(0.94, 0.71, 0.16);
        }
        cr.set_line_width(5.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.move_to(x, y - 34.0);
        cr.line_to(x, y + 34.0);
        let _ = cr.stroke();
        cr.arc(x, y - 38.0, 6.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
}

/// Build the page. `shared` must already have been started with [`start`].
pub fn page(app: &AppRef, shared: &Shared) -> gtk::Widget {
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(760);
    let page = vbox(16);
    page.set_margin_top(32);
    page.set_margin_bottom(36);
    page.set_margin_start(24);
    page.set_margin_end(24);

    let head = vbox(4);
    head.append(&label(crate::i18n::t("tuner.title"), &["tm-eyebrow"]));
    head.append(&label(crate::i18n::t("tuner.hint"), &["tm-small", "tm-dim"]));
    page.append(&head);

    // Note, with the flat/sharp signs lighting up on the side the string is off.
    let note_row = hbox(28);
    note_row.set_halign(gtk::Align::Center);
    let flat = label("♭", &["tm-tune-side"]);
    let note = label("—", &["tm-tune-note"]);
    note.set_width_chars(4);
    note.set_xalign(0.5);
    let sharp = label("♯", &["tm-tune-side"]);
    note_row.append(&flat);
    note_row.append(&note);
    note_row.append(&sharp);
    page.append(&note_row);

    let advice = label("", &["tm-tune-advice"]);
    advice.set_xalign(0.5);
    page.append(&advice);

    let meter = gtk::DrawingArea::new();
    meter.set_content_height(120);
    meter.set_hexpand(true);
    let needle: Rc<Cell<Option<f32>>> = Rc::new(Cell::new(None));
    {
        let needle = needle.clone();
        meter.set_draw_func(move |area, cr, w, h| draw_meter(area, cr, w as f64, h as f64, needle.get()));
    }
    let meter_card = vbox(0);
    meter_card.add_css_class("tm-card");
    meter_card.append(&meter);
    page.append(&meter_card);

    let detail = label("", &["tm-small", "tm-faint"]);
    detail.set_xalign(0.5);
    page.append(&detail);

    // The six open strings, low to high, so a beginner sees which one they are on.
    let strings_row = hbox(8);
    strings_row.set_halign(gtk::Align::Center);
    let string_pills: Vec<gtk::Box> = pitch::STRINGS
        .iter()
        .enumerate()
        .map(|(i, (_, name))| {
            let pill = vbox(0);
            pill.add_css_class("tm-tune-string");
            let n = name.trim_end_matches(char::is_numeric);
            pill.append(&label(n, &["tm-tune-string-note"]));
            pill.append(&label(&format!("{}ª", 6 - i), &["tm-tune-string-num"]));
            strings_row.append(&pill);
            pill
        })
        .collect();
    page.append(&strings_row);

    // Reference pitch: some records are tuned to A=432.
    let reference = adw::ActionRow::new();
    reference.set_title(crate::i18n::t("tuner.reference"));
    reference.set_subtitle(crate::i18n::t("tuner.reference.sub"));
    let spin = gtk::SpinButton::with_range(415.0, 466.0, 0.5);
    spin.set_value(*shared.a4.lock().unwrap() as f64);
    spin.set_valign(gtk::Align::Center);
    reference.add_suffix(&spin);
    let mute = adw::SwitchRow::new();
    mute.set_title(crate::i18n::t("tuner.mute"));
    mute.set_subtitle(crate::i18n::t("tuner.mute.sub"));
    let group = adw::PreferencesGroup::new();
    group.set_margin_top(8);
    group.add(&reference);
    group.add(&mute);
    page.append(&group);

    let status = label("", &["tm-small", "tm-faint"]);
    status.set_xalign(0.5);
    page.append(&status);

    {
        let shared = shared.clone();
        let app = app.clone();
        spin.connect_value_changed(move |s| {
            *shared.a4.lock().unwrap() = s.value() as f32;
            app.send(Command::Tuner(TunerControl::Reference(s.value().round() as u16)));
        });
    }
    {
        let app = app.clone();
        mute.connect_active_notify(move |m| {
            app.send(Command::Tuner(TunerControl::BypassVolume(if m.is_active() { 0 } else { 100 })));
        });
    }

    // The pedal's tuner runs only while the page is on screen: while it runs the
    // output follows the "while tuning" volume, so it must never be left on.
    {
        let app = app.clone();
        let spin = spin.clone();
        let mute = mute.clone();
        page.connect_map(move |_| {
            app.send(Command::Tuner(TunerControl::Reference(spin.value().round() as u16)));
            app.send(Command::Tuner(TunerControl::BypassVolume(if mute.is_active() { 0 } else { 100 })));
            app.send(Command::Tuner(TunerControl::On));
        });
    }
    {
        let app = app.clone();
        let shared = shared.clone();
        page.connect_unmap(move |_| {
            app.send(Command::Tuner(TunerControl::Off));
            *shared.pedal.lock().unwrap() = None;
        });
    }

    // 20 Hz redraw. The needle is smoothed and the last note is held for a
    // moment after the string fades, so the display does not flicker.
    {
        let shared = shared.clone();
        let page_widget = page.clone();
        let last: Rc<Cell<Option<std::time::Instant>>> = Rc::new(Cell::new(None));
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            if !page_widget.is_mapped() {
                return glib::ControlFlow::Continue;
            }
            let capturing = shared.capturing.load(Ordering::Relaxed);
            let pedal_on = shared.pedal.lock().ok().and_then(|r| *r).is_some_and(|r| r.running);
            status.set_text(if capturing {
                crate::i18n::t("tuner.capturing")
            } else if pedal_on {
                crate::i18n::t("tuner.pedal")
            } else {
                crate::i18n::t("tuner.no_device")
            });

            let reading = current(&shared);
            if reading.is_some() {
                last.set(Some(std::time::Instant::now()));
            }
            let held = last.get().is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(700));
            match reading {
                Some(r) => {
                    let smooth = match needle.get() {
                        Some(prev) if (prev - r.cents).abs() < 30.0 => prev * 0.65 + r.cents * 0.35,
                        _ => r.cents,
                    };
                    needle.set(Some(smooth));
                    note.set_text(&r.note);
                    let good = smooth.abs() <= IN_TUNE;
                    for c in ["tm-tune-flat", "tm-tune-sharp", "tm-tune-good"] {
                        note.remove_css_class(c);
                    }
                    note.add_css_class(if good { "tm-tune-good" } else if smooth < 0.0 { "tm-tune-flat" } else { "tm-tune-sharp" });
                    flat.set_opacity(if !good && smooth < 0.0 { 1.0 } else { 0.18 });
                    sharp.set_opacity(if !good && smooth > 0.0 { 1.0 } else { 0.18 });
                    advice.set_text(&if good {
                        format!("✓ {}", crate::i18n::t("tuner.in_tune"))
                    } else if smooth < 0.0 {
                        format!("{} · {}", crate::i18n::t("tuner.below"), crate::i18n::t("tuner.tighten"))
                    } else {
                        format!("{} · {}", crate::i18n::t("tuner.above"), crate::i18n::t("tuner.loosen"))
                    });
                    detail.set_text(&match r.hz {
                        Some(hz) => format!("{:+.0} cents · {hz:.1} Hz", smooth),
                        None => format!("{:+.0}", smooth),
                    });
                    for (i, pill) in string_pills.iter().enumerate() {
                        if r.string == Some(i) {
                            pill.add_css_class("active");
                        } else {
                            pill.remove_css_class("active");
                        }
                    }
                }
                None if held => {}
                None => {
                    needle.set(None);
                    note.set_text("—");
                    for c in ["tm-tune-flat", "tm-tune-sharp", "tm-tune-good"] {
                        note.remove_css_class(c);
                    }
                    flat.set_opacity(0.18);
                    sharp.set_opacity(0.18);
                    advice.set_text(if capturing || pedal_on { crate::i18n::t("tuner.listening") } else { "" });
                    detail.set_text("");
                    for pill in &string_pills {
                        pill.remove_css_class("active");
                    }
                }
            }
            meter.queue_draw();
            glib::ControlFlow::Continue
        });
    }

    clamp.set_child(Some(&page));
    clamp.upcast()
}
