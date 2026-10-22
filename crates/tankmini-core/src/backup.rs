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
        crate::paths::data_dir().join("backups")
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

    /// Newest first, with the metadata written next to each block.
    pub fn list_info(&self) -> io::Result<Vec<BackupInfo>> {
        Ok(self.list()?.into_iter().map(|p| BackupInfo::read(&p)).collect())
    }

    /// Delete a backup and its metadata file. Only paths inside this store.
    pub fn delete(&self, path: &Path) -> io::Result<()> {
        if path.parent() != Some(self.root.as_path()) {
            return Err(io::Error::other("backup fora da pasta de backups"));
        }
        fs::remove_file(path)?;
        let _ = fs::remove_file(path.with_extension("txt"));
        Ok(())
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

/// What we know about one backup file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub timestamp: u64,
    pub slot: Option<u8>,
    pub note: String,
}

impl BackupInfo {
    fn read(path: &Path) -> Self {
        let meta = fs::read_to_string(path.with_extension("txt")).unwrap_or_default();
        let field = |k: &str| {
            meta.lines()
                .find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix('=')))
                .map(str::trim)
                .unwrap_or("")
                .to_string()
        };
        let stem_ts = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.split('-').next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Self {
            path: path.to_path_buf(),
            timestamp: field("timestamp").parse().unwrap_or(stem_ts),
            slot: field("slot").parse().ok(),
            note: field("note"),
        }
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
        let info = &store.list_info().unwrap()[0];
        assert_eq!(info.slot, Some(3));
        assert_eq!(info.note, "unit test");
        assert!(info.timestamp > 0);
        assert!(store.delete(Path::new("/etc/passwd")).is_err());
        store.delete(&path).unwrap();
        assert!(store.list().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
