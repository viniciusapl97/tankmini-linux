//! Tuner screen: reads the pedal's USB audio input and shows the note being played.
//!
//! The capture runs on its own thread (a `pw-record` child process), so the window
//! never blocks on the audio device. The thread keeps the last measurement in a
//! shared slot and the UI polls it on a timer.

use std::io::Read;
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

/// Build the page. `shared` must already have been started with [`start`].
pub fn page(app: &AppRef, shared: &Shared) -> gtk::Widget {
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(720);
    let page = vbox(18);
    page.set_margin_top(36);
    page.set_margin_bottom(36);
    page.set_margin_start(24);
    page.set_margin_end(24);

    let head = vbox(4);
    head.append(&label(crate::i18n::t("tuner.title"), &["tm-eyebrow"]));
    head.append(&label(crate::i18n::t("tuner.hint"), &["tm-small", "tm-dim"]));
    page.append(&head);

    let note = label("—", &["tm-tune-note"]);
    note.set_xalign(0.5);
    page.append(&note);

    let string = label("", &["tm-title-l"]);
    string.set_xalign(0.5);
    page.append(&string);

    let bar = gtk::LevelBar::new();
    bar.set_mode(gtk::LevelBarMode::Continuous);
    bar.set_min_value(0.0);
    bar.set_max_value(100.0);
    bar.set_value(50.0);
    bar.add_offset_value("low", 45.0);
    bar.add_offset_value("high", 55.0);
    bar.add_css_class("tm-tune-bar");
    page.append(&bar);

    let cents_row = hbox(12);
    cents_row.set_halign(gtk::Align::Center);
    let cents = label("", &["tm-small", "tm-dim"]);
    let advice = label("", &["tm-title-l"]);
    cents_row.append(&advice);
    cents_row.append(&cents);
    page.append(&cents_row);

    let hertz = label("", &["tm-small", "tm-faint"]);
    hertz.set_xalign(0.5);
    page.append(&hertz);

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

    // Poll the capture thread; 10 Hz is enough to feel live and cheap enough for a
    // timer that runs whenever this page is visible.
    {
        let shared = shared.clone();
        let note = note.clone();
        let string = string.clone();
        let cents = cents.clone();
        let advice = advice.clone();
        let hertz = hertz.clone();
        let bar = bar.clone();
        let status = status.clone();
        let page_widget = page.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if !page_widget.is_mapped() {
                return glib::ControlFlow::Continue;
            }
            let capturing = shared.capturing.load(Ordering::Relaxed);
            let from_pedal = shared.pedal.lock().ok().and_then(|r| *r).filter(|r| r.running);
            status.set_text(if capturing {
                crate::i18n::t("tuner.capturing")
            } else if from_pedal.is_some() {
                crate::i18n::t("tuner.pedal")
            } else {
                crate::i18n::t("tuner.no_device")
            });
            let audio = shared.pitch.lock().ok().and_then(|p| p.clone());
            // No audio capture (no PipeWire, or the USB audio not routed): show
            // what the pedal's own tuner hears instead.
            if audio.is_none() {
                if let Some((name, dev)) = from_pedal.and_then(|r| Some((r.note_name()?, r.deviation.unwrap_or(0)))) {
                    note.set_text(name);
                    string.set_text("");
                    cents.set_text("");
                    advice.set_text(if dev.abs() <= tankmini_core::tuner::IN_TUNE {
                        crate::i18n::t("tuner.in_tune")
                    } else if dev < 0 {
                        crate::i18n::t("tuner.tighten")
                    } else {
                        crate::i18n::t("tuner.loosen")
                    });
                    bar.set_value(50.0 + dev.clamp(-50, 50) as f64);
                    hertz.set_text("");
                    for c in ["tm-tune-flat", "tm-tune-sharp", "tm-tune-good"] {
                        note.remove_css_class(c);
                    }
                    note.add_css_class(if dev.abs() <= tankmini_core::tuner::IN_TUNE {
                        "tm-tune-good"
                    } else if dev < 0 {
                        "tm-tune-flat"
                    } else {
                        "tm-tune-sharp"
                    });
                    return glib::ControlFlow::Continue;
                }
            }
            match audio {
                Some(p) => {
                    note.set_text(&p.label());
                    string.set_text(
                        &pitch::nearest_string(&p)
                            .map(|s| format!("{} {s}", crate::i18n::t("tuner.string")))
                            .unwrap_or_else(|| "—".into()),
                    );
                    cents.set_text(&format!("{:+.1} cents", p.cents));
                    let advice_text = match p.advice() {
                        Some("aperte a corda") => crate::i18n::t("tuner.tighten"),
                        Some("afrouxe a corda") => crate::i18n::t("tuner.loosen"),
                        _ => crate::i18n::t("tuner.in_tune"),
                    };
                    advice.set_text(advice_text);
                    bar.set_value((50.0 + p.cents.clamp(-50.0, 50.0)) as f64);
                    hertz.set_text(&format!("{:.2} Hz", p.hz));
                    note.remove_css_class("tm-tune-flat");
                    note.remove_css_class("tm-tune-sharp");
                    note.remove_css_class("tm-tune-good");
                    note.add_css_class(if p.in_tune() {
                        "tm-tune-good"
                    } else if p.cents < 0.0 {
                        "tm-tune-flat"
                    } else {
                        "tm-tune-sharp"
                    });
                }
                None => {
                    note.set_text("—");
                    string.set_text(if capturing { crate::i18n::t("tuner.listening") } else { "" });
                    cents.set_text("");
                    advice.set_text("");
                    hertz.set_text("");
                    bar.set_value(50.0);
                }
            }
            glib::ControlFlow::Continue
        });
    }

    clamp.set_child(Some(&page));
    clamp.upcast()
}
