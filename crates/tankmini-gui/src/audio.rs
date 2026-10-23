//! The "Fone" panel: hear the pedal in your headphones and balance it against
//! everything else, in plain words.
//!
//! The pedal is a USB audio interface. "Ouvir a guitarra no fone" runs a
//! `pw-loopback` from its input to the chosen output while the app is open,
//! with small buffers so the delay stays low. Volumes go through `pactl`.
//! Everything here is PipeWire's own tooling: nothing to install.

use std::cell::RefCell;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;

use adw::prelude::*;
use gtk::glib;
use serde_json::Value;

use crate::app::AppRef;
use crate::widgets::{hbox, label, vbox};

/// Our loopback's node names, also used to find (and replace) one started by
/// hand before the app existed.
const NODE_IN: &str = "tankmini-monitor-in";
const NODE_OUT: &str = "tankmini-monitor-out";
const PEDAL_MATCH: &str = "Jieli";
/// Frames per cycle while monitoring: 128 at 48 kHz ≈ 2.7 ms.
const QUANTUM: &str = "128";

thread_local! {
    static LOOPBACK: RefCell<Option<Child>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug, PartialEq)]
struct Output {
    name: String,
    /// What a person calls it.
    friendly: String,
}

fn pactl_json(args: &[&str]) -> Option<Value> {
    let out = Command::new("pactl").arg("-f").arg("json").args(args).output().ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

fn pactl(args: &[&str]) {
    let _ = Command::new("pactl").args(args).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// "fifine Microphone Analog Stereo" → "Fone no microfone FIFINE", and so on.
fn friendly(name: &str, description: &str) -> String {
    let d = description.replace(" Analog Stereo", "").replace(" Digital Stereo", "");
    let lower = name.to_ascii_lowercase();
    if lower.contains(&PEDAL_MATCH.to_ascii_lowercase()) {
        "Fone ligado na pedaleira".into()
    } else if lower.starts_with("bluez") {
        format!("{d} (Bluetooth)")
    } else if lower.contains("hdmi") {
        "Monitor ou TV (HDMI)".into()
    } else if lower.contains("fifine") {
        "Fone no microfone FIFINE".into()
    } else if lower.contains("pci") {
        "Notebook (alto-falante ou entrada de fone)".into()
    } else {
        d
    }
}

fn outputs() -> Vec<Output> {
    let Some(Value::Array(sinks)) = pactl_json(&["list", "sinks"]) else { return Vec::new() };
    sinks
        .iter()
        .filter_map(|s| {
            let name = s.get("name")?.as_str()?.to_string();
            let desc = s.get("description").and_then(Value::as_str).unwrap_or(&name).to_string();
            Some(Output { friendly: friendly(&name, &desc), name })
        })
        .collect()
}

fn default_output() -> Option<String> {
    let out = Command::new("pactl").arg("get-default-sink").output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

fn pedal_source() -> Option<String> {
    let Some(Value::Array(sources)) = pactl_json(&["list", "sources"]) else { return None };
    sources.iter().find_map(|s| {
        let name = s.get("name")?.as_str()?;
        (name.contains(PEDAL_MATCH) && !name.ends_with(".monitor")).then(|| name.to_string())
    })
}

/// Volume of a sink or stream, percent (first channel).
fn volume_of(v: &Value) -> Option<u32> {
    let ch = v.get("volume")?.as_object()?.values().next()?;
    ch.get("value_percent")?.as_str()?.trim_end_matches('%').trim().parse().ok()
}

/// Streams playing right now: (index, is_ours, volume).
fn streams() -> Vec<(u64, bool, u32)> {
    let Some(Value::Array(list)) = pactl_json(&["list", "sink-inputs"]) else { return Vec::new() };
    list.iter()
        .filter_map(|s| {
            let idx = s.get("index")?.as_u64()?;
            let node = s.get("properties").and_then(|p| p.get("node.name")).and_then(Value::as_str).unwrap_or("");
            Some((idx, node == NODE_OUT, volume_of(s).unwrap_or(100)))
        })
        .collect()
}

fn sink_volume(name: &str) -> Option<u32> {
    let Some(Value::Array(sinks)) = pactl_json(&["list", "sinks"]) else { return None };
    sinks.iter().find(|s| s.get("name").and_then(Value::as_str) == Some(name)).and_then(volume_of)
}

fn set_guitar_volume(pct: u32) {
    for (idx, ours, _) in streams() {
        if ours {
            pactl(&["set-sink-input-volume", &idx.to_string(), &format!("{pct}%")]);
        }
    }
}

fn set_music_volume(pct: u32) {
    for (idx, ours, _) in streams() {
        if !ours {
            pactl(&["set-sink-input-volume", &idx.to_string(), &format!("{pct}%")]);
        }
    }
}

fn music_volume() -> Option<u32> {
    streams().into_iter().find(|(_, ours, _)| !ours).map(|(_, _, v)| v)
}

/// Start (or restart) the route pedal → `output`.
fn start(output: &str, guitar: u32) -> Result<(), String> {
    stop();
    // A route started by hand before (same node names) would double the sound.
    let _ = Command::new("pkill").args(["-f", &format!("pw-loopback.*{NODE_IN}")]).status();
    let source = pedal_source().ok_or("A pedaleira não apareceu como placa de som. Confira o cabo USB.")?;
    // Small buffers while we listen: the default 1024 frames is ~21 ms per
    // stage, which a player feels.
    set_quantum(QUANTUM);
    let latency = format!("{QUANTUM}/48000");
    let child = Command::new("pw-loopback")
        .arg(format!("--latency={latency}"))
        .arg("--delay=0")
        .arg(format!("--capture-props=target.object={source} node.name={NODE_IN} node.latency={latency} node.description=TANK-MINI"))
        .arg(format!("--playback-props=target.object={output} node.name={NODE_OUT} node.latency={latency} node.description=Guitarra"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Não deu para ligar o som: {e}"))?;
    LOOPBACK.with(|l| *l.borrow_mut() = Some(child));
    // The stream needs a moment to appear before its volume can be set.
    glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || set_guitar_volume(guitar));
    Ok(())
}

fn set_quantum(q: &str) {
    let _ = Command::new("pw-metadata")
        .args(["-n", "settings", "0", "clock.force-quantum", q])
        .stdout(Stdio::null())
        .status();
}

/// Stop the route this app started, if any, and give PipeWire its normal
/// buffers back. Another copy of the app (or a route made by hand) is left
/// alone.
pub fn stop() {
    let ours = LOOPBACK.with(|l| l.borrow_mut().take());
    if let Some(mut c) = ours {
        let _ = c.kill();
        let _ = c.wait();
        set_quantum("0");
    }
}

/// Called once at startup: resume listening if it was on last time.
pub fn resume(app: &AppRef) {
    let (on, output, guitar) = {
        let st = app.state.borrow();
        (st.settings.monitor, st.settings.monitor_output.clone(), st.settings.monitor_guitar)
    };
    if on {
        let output = if output.is_empty() { default_output().unwrap_or_default() } else { output };
        if let Err(e) = start(&output, guitar) {
            app.toast(&e);
        }
    }
}

fn slider_row(title: &str, hint: &str, icon: &str) -> (gtk::Box, gtk::Scale) {
    let row = vbox(2);
    let head = hbox(8);
    let img = gtk::Image::from_icon_name(icon);
    head.append(&img);
    let t = label(title, &["tm-row-title"]);
    t.set_hexpand(true);
    t.set_xalign(0.0);
    head.append(&t);
    row.append(&head);
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.set_draw_value(false);
    scale.add_css_class("tm-volbar-scale");
    row.append(&scale);
    if !hint.is_empty() {
        let h = label(hint, &["tm-small", "tm-faint"]);
        h.set_xalign(0.0);
        h.set_wrap(true);
        row.append(&h);
    }
    (row, scale)
}

/// The bottom-bar button and its panel.
pub fn button(app: &AppRef, shared: &crate::tuner::Shared) -> gtk::MenuButton {
    let btn = gtk::MenuButton::new();
    let inner = hbox(6);
    inner.append(&gtk::Image::from_icon_name("audio-headphones-symbolic"));
    inner.append(&label("Fone", &[]));
    btn.set_child(Some(&inner));
    btn.add_css_class("tm-pill");
    btn.set_tooltip_text(Some("Ouvir a guitarra no fone e ajustar os volumes"));

    let pop = gtk::Popover::new();
    let body = vbox(16);
    body.set_margin_top(14);
    body.set_margin_bottom(14);
    body.set_margin_start(14);
    body.set_margin_end(14);
    body.set_size_request(340, -1);
    body.append(&label("Som no fone", &["tm-title-l"]));

    // 1. On/off.
    let on_row = hbox(10);
    let on_texts = vbox(2);
    on_texts.set_hexpand(true);
    let on_title = label("Ouvir a guitarra no fone", &["tm-row-title"]);
    on_title.set_xalign(0.0);
    let on_hint = label("Funciona enquanto este app estiver aberto.", &["tm-small", "tm-faint"]);
    on_hint.set_xalign(0.0);
    on_texts.append(&on_title);
    on_texts.append(&on_hint);
    on_row.append(&on_texts);
    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    on_row.append(&switch);
    body.append(&on_row);

    // 2. Where.
    let where_box = vbox(6);
    let w = label("Onde você ouve", &["tm-row-title"]);
    w.set_xalign(0.0);
    where_box.append(&w);
    let dropdown = gtk::DropDown::from_strings(&[]);
    where_box.append(&dropdown);
    body.append(&where_box);

    // 3. Levels.
    let (guitar_row, guitar) = slider_row("Guitarra", "", "audio-input-microphone-symbolic");
    let (music_row, music) =
        slider_row("Músicas e vídeos", "YouTube, Spotify e o que mais estiver tocando.", "folder-music-symbolic");
    let (main_row, main) = slider_row("Volume geral", "", "audio-volume-high-symbolic");
    body.append(&guitar_row);
    body.append(&music_row);
    body.append(&main_row);

    // 4. Is the pedal sending anything?
    let meter_box = vbox(4);
    let m = label("Sinal da guitarra", &["tm-row-title"]);
    m.set_xalign(0.0);
    meter_box.append(&m);
    let meter = gtk::LevelBar::new();
    meter.set_min_value(0.0);
    meter.set_max_value(1.0);
    meter.add_css_class("tm-meter");
    meter_box.append(&meter);
    let meter_hint = label("", &["tm-small", "tm-dim"]);
    meter_hint.set_xalign(0.0);
    meter_hint.set_wrap(true);
    meter_box.append(&meter_hint);
    body.append(&meter_box);

    pop.set_child(Some(&body));
    btn.set_popover(Some(&pop));

    let list: std::rc::Rc<RefCell<Vec<Output>>> = std::rc::Rc::new(RefCell::new(Vec::new()));
    // Filling the controls fires their handlers; this keeps them quiet then.
    let filling = std::rc::Rc::new(std::cell::Cell::new(false));

    // Refresh the panel each time it opens: devices come and go.
    {
        let app = app.clone();
        let list = list.clone();
        let dropdown = dropdown.clone();
        let switch = switch.clone();
        let (guitar, music, main) = (guitar.clone(), music.clone(), main.clone());
        let filling = filling.clone();
        pop.connect_show(move |_| {
            filling.set(true);
            let outs = outputs();
            let (on, saved, gtr) = {
                let st = app.state.borrow();
                (st.settings.monitor, st.settings.monitor_output.clone(), st.settings.monitor_guitar)
            };
            let current = if saved.is_empty() { default_output().unwrap_or_default() } else { saved };
            let names: Vec<&str> = outs.iter().map(|o| o.friendly.as_str()).collect();
            dropdown.set_model(Some(&gtk::StringList::new(&names)));
            if let Some(i) = outs.iter().position(|o| o.name == current) {
                dropdown.set_selected(i as u32);
            }
            switch.set_active(on);
            guitar.set_value(gtr as f64);
            music.set_value(music_volume().unwrap_or(100) as f64);
            music.set_sensitive(music_volume().is_some());
            main.set_value(sink_volume(&current).unwrap_or(60).min(100) as f64);
            *list.borrow_mut() = outs;
            filling.set(false);
        });
    }

    let selected_output = {
        let list = list.clone();
        let dropdown = dropdown.clone();
        move || list.borrow().get(dropdown.selected() as usize).map(|o| o.name.clone())
    };

    {
        let app = app.clone();
        let filling = filling.clone();
        let selected_output = selected_output.clone();
        let guitar = guitar.clone();
        switch.connect_active_notify(move |s| {
            if filling.get() {
                return;
            }
            let on = s.is_active();
            if on {
                let output = selected_output().or_else(default_output).unwrap_or_default();
                if let Err(e) = start(&output, guitar.value() as u32) {
                    app.toast(&e);
                    s.set_active(false);
                    return;
                }
            } else {
                stop();
            }
            app.state.borrow_mut().settings.monitor = on;
            app.save_settings();
        });
    }
    {
        let app = app.clone();
        let filling = filling.clone();
        let switch = switch.clone();
        let guitar = guitar.clone();
        let main = main.clone();
        dropdown.connect_selected_notify(move |_| {
            if filling.get() {
                return;
            }
            let Some(output) = selected_output() else { return };
            // Everything goes there: music too, not just the guitar.
            pactl(&["set-default-sink", &output]);
            app.state.borrow_mut().settings.monitor_output = output.clone();
            app.save_settings();
            if let Some(v) = sink_volume(&output) {
                main.set_value(v.min(100) as f64);
            }
            if switch.is_active() {
                if let Err(e) = start(&output, guitar.value() as u32) {
                    app.toast(&e);
                }
            }
        });
    }
    {
        let app = app.clone();
        let filling = filling.clone();
        guitar.connect_value_changed(move |s| {
            if filling.get() {
                return;
            }
            let v = s.value() as u32;
            set_guitar_volume(v);
            app.state.borrow_mut().settings.monitor_guitar = v;
            app.save_settings();
        });
    }
    {
        let filling = filling.clone();
        music.connect_value_changed(move |s| {
            if !filling.get() {
                set_music_volume(s.value() as u32);
            }
        });
    }
    {
        let app = app.clone();
        let filling = filling.clone();
        main.connect_value_changed(move |s| {
            if filling.get() {
                return;
            }
            let output = {
                let saved = app.state.borrow().settings.monitor_output.clone();
                if saved.is_empty() { default_output().unwrap_or_default() } else { saved }
            };
            pactl(&["set-sink-volume", &output, &format!("{}%", s.value() as u32)]);
        });
    }

    // Live meter while the panel is open.
    {
        let shared = shared.clone();
        let pop_c = pop.clone();
        let switch = switch.clone();
        let mut silent_for = 0u32;
        glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
            if !pop_c.is_visible() {
                return glib::ControlFlow::Continue;
            }
            let peak = f32::from_bits(shared.level.load(Ordering::Relaxed));
            // A peak meter in dB is what reads naturally: −60 dB = empty.
            let db = 20.0 * (peak.max(1e-6)).log10();
            meter.set_value((((db + 60.0) / 60.0).clamp(0.0, 1.0)) as f64);
            silent_for = if peak < 0.0005 { silent_for + 1 } else { 0 };
            meter_hint.set_text(if !shared.capturing.load(Ordering::Relaxed) {
                "A pedaleira não está mandando som pelo USB. Confira o cabo."
            } else if silent_for > 25 {
                "Nenhum som chegando. Toque algo; se continuar parado, aumente o volume da guitarra."
            } else if peak > 0.97 {
                "Muito alto: está saturando. Baixe o VOL na tela Tocar."
            } else if switch.is_active() {
                "Chegando som da pedaleira."
            } else {
                "Chegando som da pedaleira. Ligue \"Ouvir a guitarra no fone\" para escutar."
            });
            glib::ControlFlow::Continue
        });
    }

    btn
}
