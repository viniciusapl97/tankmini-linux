//! Local backups of preset blocks, so every write is reversible.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::preset::Preset;

pub struct BackupStore {
    root: PathBuf,
}

impl BackupStore {
    /// `$XDG_DATA_HOME/tankmini/backups` (falls back to `~/.local/share`).
    pub fn default_root() -> PathBuf {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("tankmini/backups")
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Store a block, returning the created path.
    pub fn save(&self, preset: &Preset, slot: Option<u8>, note: &str) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.root)?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let slot_part = slot.map(|s| format!("-slot{s:02}")).unwrap_or_default();
        let stem = format!("{ts}{slot_part}");
        let bin = self.root.join(format!("{stem}.bin"));
        fs::write(&bin, preset.raw())?;
        let meta = format!(
            "timestamp={ts}\nslot={}\nnote={note}\nbytes={}\n",
            slot.map(|s| s.to_string()).unwrap_or_else(|| "-".into()),
            preset.raw().len()
        );
        fs::write(self.root.join(format!("{stem}.txt")), meta)?;
        Ok(bin)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Preset, String> {
        let raw = fs::read(path.as_ref()).map_err(|e| e.to_string())?;
        Preset::from_raw(raw)
    }

    /// Newest first.
    pub fn list(&self) -> io::Result<Vec<PathBuf>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut bins: Vec<PathBuf> = fs::read_dir(&self.root)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "bin"))
            .collect();
        bins.sort();
        bins.reverse();
        Ok(bins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::{RECORD_COUNT, RECORD_LEN};

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("tankmini-backup-test-{}", std::process::id()));
        let store = BackupStore::new(&dir);
        let preset = Preset::from_records(&[[7u8; RECORD_LEN]; RECORD_COUNT]);
        let path = store.save(&preset, Some(3), "unit test").unwrap();
        let loaded = BackupStore::load(&path).unwrap();
        assert_eq!(loaded, preset);
        assert_eq!(store.list().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
