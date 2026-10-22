//! Local preset library and setlists.
//!
//! The pedal holds 21 presets; the library holds as many as the user wants on
//! the computer, with a name, instrument, tags and notes. A setlist is an
//! ordered list of library presets written to consecutive pedal slots.
//!
//! Stored as one JSON file (`~/.local/share/tankmini/library.json`), written
//! atomically (temp file + rename) so a crash never leaves half a library.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::patch::{PATCH_COUNT, PATCH_LEN};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// `guitar`, `acoustic` or `bass` (see [`crate::catalog::Instrument`]).
    #[serde(default)]
    pub instrument: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub favorite: bool,
    /// Unix seconds.
    pub created: u64,
    /// The 92-byte preset as lowercase hex.
    pub bytes: String,
}

impl Entry {
    pub fn patch_bytes(&self) -> Option<Vec<u8>> {
        let bytes = from_hex(&self.bytes)?;
        (bytes.len() == PATCH_LEN).then_some(bytes)
    }

    /// Case- and accent-insensitive match on name, tags and notes.
    pub fn matches(&self, query: &str) -> bool {
        let q = fold(query);
        if q.is_empty() {
            return true;
        }
        q.split_whitespace().all(|word| {
            fold(&self.name).contains(word)
                || self.tags.iter().any(|t| fold(t).contains(word))
                || fold(&self.notes).contains(word)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setlist {
    pub id: String,
    pub name: String,
    /// Library entry ids, in playing order.
    pub entries: Vec<String>,
    /// First pedal slot (0 = 1A) the setlist is written to.
    #[serde(default)]
    pub start_slot: usize,
}

impl Setlist {
    /// Slot → library entry pairs this setlist writes, or an error when it does
    /// not fit in the pedal or refers to missing entries.
    pub fn plan<'a>(&self, library: &'a Library) -> Result<Vec<(usize, &'a Entry)>, String> {
        if self.start_slot + self.entries.len() > PATCH_COUNT {
            return Err(format!(
                "a setlist tem {} sons e não cabe a partir do slot {} (máximo 21)",
                self.entries.len(),
                crate::patch::SLOT_LABELS.get(self.start_slot).copied().unwrap_or("?")
            ));
        }
        self.entries
            .iter()
            .enumerate()
            .map(|(i, id)| {
                library
                    .get(id)
                    .map(|e| (self.start_slot + i, e))
                    .ok_or_else(|| format!("o som {id} não está mais na biblioteca"))
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub entries: Vec<Entry>,
    #[serde(default)]
    pub setlists: Vec<Setlist>,
}

impl Library {
    pub fn default_path() -> PathBuf {
        crate::paths::data_dir().join("library.json")
    }

    /// Missing file → empty library; a corrupt file is an error (never
    /// silently replaced by an empty one).
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        match fs::read_to_string(path.as_ref()) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("biblioteca ilegível: {e}")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut out = self.clone();
        out.version = 1;
        let json = serde_json::to_string_pretty(&out).map_err(io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(tmp, path)
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Add a preset; returns its new id.
    pub fn add(&mut self, name: &str, instrument: &str, tags: Vec<String>, bytes: &[u8]) -> Result<String, String> {
        if bytes.len() != PATCH_LEN {
            return Err(format!("preset precisa de {PATCH_LEN} bytes"));
        }
        let created = now();
        let id = self.fresh_id("p", created);
        self.entries.push(Entry {
            id: id.clone(),
            name: clean_name(name, "Sem nome"),
            instrument: instrument.to_string(),
            tags: tags.into_iter().map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()).collect(),
            notes: String::new(),
            favorite: false,
            created,
            bytes: to_hex(bytes),
        });
        Ok(id)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        for s in &mut self.setlists {
            s.entries.retain(|e| e != id);
        }
        self.entries.len() != before
    }

    /// Favourites first, then newest first, filtered by `query` and instrument.
    pub fn search(&self, query: &str, instrument: Option<&str>) -> Vec<&Entry> {
        let mut out: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| e.matches(query))
            .filter(|e| instrument.is_none_or(|i| e.instrument == i))
            .collect();
        out.sort_by(|a, b| b.favorite.cmp(&a.favorite).then(b.created.cmp(&a.created)));
        out
    }

    pub fn add_setlist(&mut self, name: &str) -> String {
        let id = self.fresh_id("s", now());
        self.setlists.push(Setlist {
            id: id.clone(),
            name: clean_name(name, "Nova setlist"),
            entries: Vec::new(),
            start_slot: 0,
        });
        id
    }

    pub fn setlist_mut(&mut self, id: &str) -> Option<&mut Setlist> {
        self.setlists.iter_mut().find(|s| s.id == id)
    }

    fn fresh_id(&self, prefix: &str, ts: u64) -> String {
        let mut n = 0u32;
        loop {
            let id = format!("{prefix}{ts:x}{n:02x}");
            let taken = self.entries.iter().any(|e| e.id == id) || self.setlists.iter().any(|s| s.id == id);
            if !taken {
                return id;
            }
            n += 1;
        }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn clean_name(name: &str, fallback: &str) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control()).take(48).collect();
    let clean = clean.trim();
    if clean.is_empty() { fallback.to_string() } else { clean.to_string() }
}

/// Lowercase and strip the accents used in Portuguese, for search.
fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'ê' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ô' | 'õ' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(v: u8) -> Vec<u8> {
        vec![v; PATCH_LEN]
    }

    #[test]
    fn add_search_and_round_trip() {
        let mut lib = Library::default();
        let a = lib.add("Crunch da Música", "guitar", vec!["Rock ".into()], &patch(1)).unwrap();
        let b = lib.add("Baixo limpo", "bass", vec![], &patch(2)).unwrap();
        assert_ne!(a, b);
        assert!(lib.add("x", "guitar", vec![], &[1, 2]).is_err());
        assert_eq!(lib.search("musica", None).len(), 1);
        assert_eq!(lib.search("rock", None)[0].id, a);
        assert_eq!(lib.search("", Some("bass"))[0].id, b);
        lib.get_mut(&b).unwrap().favorite = true;
        assert_eq!(lib.search("", None)[0].id, b);

        let dir = std::env::temp_dir().join(format!("tankmini-lib-{}", std::process::id()));
        let path = dir.join("library.json");
        lib.save(&path).unwrap();
        let back = Library::load(&path).unwrap();
        assert_eq!(back.entries, lib.entries);
        assert_eq!(back.get(&a).unwrap().patch_bytes(), Some(patch(1)));
        fs::write(&path, "{not json").unwrap();
        assert!(Library::load(&path).is_err());
        assert_eq!(Library::load(dir.join("missing.json")).unwrap(), Library::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn setlist_plans_consecutive_slots() {
        let mut lib = Library::default();
        let ids: Vec<String> = (0..3).map(|i| lib.add(&format!("s{i}"), "guitar", vec![], &patch(i)).unwrap()).collect();
        let sid = lib.add_setlist("Show sábado");
        let set = lib.setlist_mut(&sid).unwrap();
        set.entries = ids.clone();
        set.start_slot = 3;
        let set = lib.setlists[0].clone();
        let plan = set.plan(&lib).unwrap();
        assert_eq!(plan.iter().map(|(s, _)| *s).collect::<Vec<_>>(), vec![3, 4, 5]);
        assert_eq!(plan[2].1.id, ids[2]);

        let mut late = set.clone();
        late.start_slot = 19;
        assert!(late.plan(&lib).is_err());

        lib.remove(&ids[1]);
        assert_eq!(lib.setlists[0].entries.len(), 2);
    }

    #[test]
    fn hex_round_trip() {
        let bytes = vec![0, 1, 0xab, 0xff];
        assert_eq!(from_hex(&to_hex(&bytes)), Some(bytes));
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex("zz"), None);
    }
}
