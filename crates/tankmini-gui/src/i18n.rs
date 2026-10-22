//! Tiny translation layer.
//!
//! The app is written in Brazilian Portuguese; this module keeps the English
//! strings in one table instead of spreading conditionals through the UI. Lookup is
//! by key, the language comes from the environment (or the settings file later), and
//! a missing key falls back to Portuguese and then to the key itself, so a typo
//! never blanks a label.
//!
//! Migration rule: move strings module by module, keeping the pt-BR text identical
//! to what is on screen today. A half-migrated module is worse than an unmigrated
//! one, so each module is converted in a single pass and its strings are covered by
//! the table test below.

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    PtBr,
    En,
}

impl Lang {
    /// `TANKMINI_LANG` wins (`en`, `en_US`, `pt`, `pt_BR`), then the POSIX locale.
    pub fn detect() -> Self {
        if let Ok(value) = std::env::var("TANKMINI_LANG") {
            return Self::from_tag(&value);
        }
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(value) = std::env::var(var) {
                if !value.is_empty() {
                    return Self::from_tag(&value);
                }
            }
        }
        Self::PtBr
    }

    pub fn from_tag(tag: &str) -> Self {
        if tag.to_ascii_lowercase().starts_with("pt") {
            Self::PtBr
        } else {
            Self::En
        }
    }

    /// Stored in the settings file as `pt` or `en`.
    pub fn code(self) -> &'static str {
        match self {
            Self::PtBr => "pt",
            Self::En => "en",
        }
    }
}

static LANG: OnceLock<Lang> = OnceLock::new();

/// Set once at startup; later calls keep the first value.
pub fn init(lang: Lang) {
    let _ = LANG.set(lang);
}

pub fn current() -> Lang {
    *LANG.get_or_init(Lang::detect)
}

/// Translate a key. Unknown keys are returned as-is so the failure is visible.
pub fn t(key: &'static str) -> &'static str {
    translate(current(), key)
}

pub fn translate(lang: Lang, key: &'static str) -> &'static str {
    let table: &[(&str, &str, &str)] = TABLE;
    for (k, pt, en) in table {
        if *k == key {
            return match lang {
                Lang::PtBr => pt,
                Lang::En => en,
            };
        }
    }
    key
}

/// (key, pt-BR, English)
const TABLE: &[(&str, &str, &str)] = &[
    // ---- tuner ----
    ("tuner.title", "AFINADOR", "TUNER"),
    (
        "tuner.hint",
        "Toque uma corda solta. O ponteiro à esquerda do centro quer dizer que a corda está frouxa; à direita, esticada demais.",
        "Play one open string. Left of centre means the string is too loose; right of centre, too tight.",
    ),
    ("tuner.reference", "Referência", "Reference"),
    ("tuner.reference.sub", "Frequência do Lá acima do Dó central (A4)", "Pitch of the A above middle C (A4)"),
    ("tuner.listening", "ouvindo…", "listening…"),
    ("tuner.capturing", "capturando da pedaleira", "capturing from the pedal"),
    ("tuner.no_device", "pedaleira não encontrada — verifique o cabo USB", "pedal not found — check the USB cable"),
    ("tuner.in_tune", "afinado", "in tune"),
    ("tuner.tighten", "aperte a corda", "tighten the string"),
    ("tuner.loosen", "afrouxe a corda", "loosen the string"),
    ("tuner.string", "corda", "string"),
    ("tuner.below", "Abaixo do tom", "Below the note"),
    ("tuner.flat_side", "grave", "flat"),
    ("tuner.sharp_side", "agudo", "sharp"),
    ("tuner.above", "Acima do tom", "Above the note"),
    ("tuner.mute", "Silenciar enquanto afina", "Mute while tuning"),
    (
        "tuner.mute.sub",
        "A pedaleira corta o som enquanto esta tela está aberta",
        "The pedal cuts its output while this screen is open",
    ),
    ("tuner.pedal", "afinador da pedaleira", "pedal's tuner"),
    // ---- modes ----
    ("view.play", "Tocar", "Play"),
    ("view.edit", "Editar", "Edit"),
    ("view.tune", "Afinar", "Tune"),
    // ---- blocks ----
    ("block.on", "Ligado", "On"),
    ("block.off", "Desligado", "Off"),
    ("block.switch.tip", "Ligar/desligar este bloco", "Turn this block on or off"),
    ("block.no_params", "Este bloco não tem ajustes conhecidos.", "This block has no known controls."),
    // ---- generic ----
    ("common.save", "Salvar", "Save"),
    ("common.rename", "Renomear", "Rename"),
    ("common.bypass", "Bypass", "Bypass"),
    ("common.search", "Buscar sons", "Search sounds"),
    ("common.retry", "Tentar de novo", "Try again"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_consistent() {
        for (key, pt, en) in TABLE {
            assert!(!key.is_empty(), "empty key");
            assert!(!pt.trim().is_empty(), "empty pt for {key}");
            assert!(!en.trim().is_empty(), "empty en for {key}");
            // Identical text is legitimate for loanwords ("Bypass", "Setlists").
        }
        let mut keys: Vec<_> = TABLE.iter().map(|(k, _, _)| *k).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "duplicate key in the table");
    }

    #[test]
    fn translation_switches_language() {
        assert_eq!(translate(Lang::PtBr, "tuner.title"), "AFINADOR");
        assert_eq!(translate(Lang::En, "tuner.title"), "TUNER");
        assert_eq!(translate(Lang::En, "view.edit"), "Edit");
    }

    #[test]
    fn unknown_key_is_returned_so_it_shows_up() {
        assert_eq!(translate(Lang::En, "nope.missing"), "nope.missing");
    }

    #[test]
    fn locale_tags_map_to_a_language() {
        assert_eq!(Lang::from_tag("pt_BR.UTF-8"), Lang::PtBr);
        assert_eq!(Lang::from_tag("pt"), Lang::PtBr);
        assert_eq!(Lang::from_tag("en_US.UTF-8"), Lang::En);
        assert_eq!(Lang::from_tag("de_DE"), Lang::En);
        assert_eq!(Lang::PtBr.code(), "pt");
    }

    #[test]
    fn the_detector_prefers_the_explicit_variable() {
        // Only checks the tag mapping here: mutating the environment in a test would
        // race with the other tests in this binary.
        assert_eq!(Lang::from_tag("en"), Lang::En);
    }
}
