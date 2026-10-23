//! User preferences for the desktop app (`~/.config/tankmini/settings.json`).
//!
//! Unknown or missing keys fall back to defaults, so old files keep working
//! when new options are added.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `system`, `dark` or `light`.
    pub theme: String,
    /// Send edits of the playing preset to the pedal while turning knobs.
    pub live_edit: bool,
    /// Knob drag sensitivity, in value units per 100 px (default 50).
    pub knob_sensitivity: u32,
    /// Show raw bytes, all six parameter slots and protocol details.
    pub show_advanced: bool,
    /// Show every parameter slot, even the ones without a known name.
    pub show_all_params: bool,
    /// Parameter names as printed on amps (Gain, Treble) or in Portuguese.
    pub english_param_names: bool,
    /// Ask before switching presets with unsaved edits.
    pub confirm_discard: bool,
    /// `guitar`, `acoustic` or `bass`; picks the suggested sounds.
    pub instrument: String,
    /// The first-run assistant was completed or skipped.
    pub onboarded: bool,
    /// Last view: `play` or `edit`.
    pub view: String,
    /// How Editar draws the blocks: `pedals` (realistic stompboxes),
    /// `flat` (pedal-shaped cards in the app style) or `cards`.
    pub edit_style: String,
    /// "Ouvir a guitarra no fone": the app routes the pedal's USB audio to
    /// `monitor_output` while it is open.
    pub monitor: bool,
    /// PipeWire sink name to listen on; empty = the system default.
    pub monitor_output: String,
    /// Guitar level in that route, percent.
    pub monitor_guitar: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            live_edit: true,
            knob_sensitivity: 50,
            show_advanced: false,
            show_all_params: false,
            english_param_names: true,
            confirm_discard: true,
            instrument: "guitar".into(),
            onboarded: false,
            view: "play".into(),
            edit_style: "pedals".into(),
            monitor: false,
            monitor_output: String::new(),
            monitor_guitar: 100,
        }
    }
}

impl Settings {
    pub fn default_path() -> PathBuf {
        crate::paths::config_dir().join("settings.json")
    }

    /// A missing or unreadable file gives the defaults.
    pub fn load(path: impl AsRef<Path>) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .map(Self::sanitised)
            .unwrap_or_default()
    }

    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(tmp, path)
    }

    pub fn load_default() -> Self {
        Self::load(Self::default_path())
    }

    pub fn save_default(&self) -> io::Result<()> {
        self.save(Self::default_path())
    }

    fn sanitised(mut self) -> Self {
        let d = Self::default();
        if !["system", "dark", "light"].contains(&self.theme.as_str()) {
            self.theme = d.theme;
        }
        if crate::catalog::Instrument::from_key(&self.instrument).is_none() {
            self.instrument = d.instrument;
        }
        if !["pedals", "flat", "cards"].contains(&self.edit_style.as_str()) {
            self.edit_style = d.edit_style.clone();
        }
        if !["play", "edit"].contains(&self.view.as_str()) {
            self.view = d.view;
        }
        self.knob_sensitivity = self.knob_sensitivity.clamp(10, 200);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_partial_files() {
        let dir = std::env::temp_dir().join(format!("tankmini-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut s = Settings::default();
        s.theme = "light".into();
        s.knob_sensitivity = 80;
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);

        fs::write(&path, r#"{"live_edit": false, "theme": "neon", "knob_sensitivity": 9999}"#).unwrap();
        let s = Settings::load(&path);
        assert!(!s.live_edit);
        assert_eq!(s.theme, "dark");
        assert_eq!(s.knob_sensitivity, 200);
        assert!(s.confirm_discard);

        assert_eq!(Settings::load(dir.join("nope.json")), Settings::default());
        let _ = fs::remove_dir_all(&dir);
    }
}
