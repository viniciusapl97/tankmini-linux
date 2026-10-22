//! Local preset names.
//!
//! The pedal stores 21 presets with no name field at all — the panel only shows
//! bank + A/B/C. Names therefore live on the computer, keyed by slot index, in a
//! small TSV file under `~/.local/share/tankmini/names.tsv`.
//!
//! Format: `index<TAB>name` per line, UTF-8. Unknown or missing indices simply
//! have no name.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetNames {
    names: Vec<Option<String>>,
}

impl Default for PresetNames {
    fn default() -> Self {
        Self {
            names: vec![None; crate::patch::PATCH_COUNT],
        }
    }
}

impl PresetNames {
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("tankmini/names.tsv")
    }

    pub fn load(path: impl AsRef<Path>) -> Self {
        let mut names: Vec<Option<String>> = vec![None; crate::patch::PATCH_COUNT];
        if let Ok(text) = fs::read_to_string(path) {
            for line in text.lines() {
                let Some((idx, name)) = line.split_once('\t') else { continue };
                if let Ok(i) = idx.trim().parse::<usize>() {
                    if i < names.len() {
                        let name = name.trim();
                        if !name.is_empty() {
                            names[i] = Some(name.to_string());
                        }
                    }
                }
            }
        }
        Self { names }
    }

    pub fn get(&self, index: usize) -> Option<&str> {
        self.names.get(index).and_then(|n| n.as_deref())
    }

    /// Sanitised and length-limited (24 chars keeps the rail tidy).
    pub fn set(&mut self, index: usize, name: &str) {
        if index >= self.names.len() {
            return;
        }
        let clean: String = name
            .chars()
            .filter(|c| !c.is_control())
            .take(24)
            .collect::<String>()
            .trim()
            .to_string();
        self.names[index] = if clean.is_empty() { None } else { Some(clean) };
    }

    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut out = String::new();
        for (i, name) in self.names.iter().enumerate() {
            if let Some(name) = name {
                out.push_str(&format!("{i}\t{name}\n"));
            }
        }
        fs::write(path, out)
    }

    pub fn save_default(&self) -> io::Result<()> {
        self.save(Self::default_path())
    }

    pub fn load_default() -> Self {
        Self::load(Self::default_path())
    }

    /// Label for a slot: the local name when set, else the bank/footswitch label.
    pub fn label(&self, index: usize) -> String {
        match (self.get(index), crate::patch::SLOT_LABELS.get(index)) {
            (Some(name), Some(slot)) => format!("{slot} · {name}"),
            (Some(name), None) => name.to_string(),
            (None, Some(slot)) => slot.to_string(),
            (None, None) => format!("{index}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_round_trips_and_sanitises() {
        let dir = std::env::temp_dir().join(format!("tankmini-names-{}", std::process::id()));
        let path = dir.join("names.tsv");
        let mut names = PresetNames::default();
        names.set(3, "  Mesa crunch  ");
        names.set(4, "");
        names.set(5, &"x".repeat(60));
        names.save(&path).unwrap();
        let back = PresetNames::load(&path);
        assert_eq!(back.get(3), Some("Mesa crunch"));
        assert_eq!(back.get(4), None);
        assert_eq!(back.get(5).unwrap().len(), 24);
        assert_eq!(back.label(3), "2A · Mesa crunch");
        assert_eq!(back.label(0), "1A");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
