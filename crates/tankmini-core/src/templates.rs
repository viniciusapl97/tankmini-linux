//! "Sons prontos": starting points for people who just want to play.
//!
//! Each template starts from a factory preset (voiced by M-VAVE on this
//! firmware) and adjusts a few well-understood controls, so the result stays
//! close to something the manufacturer tuned. Amp and cab models are picked by
//! the names the pedal reports (`UWE-Twins` for clean, `BritPLEX_100` for
//! rock…); that the names match the character is an **inferred** choice.

use crate::catalog::Instrument;
use crate::macros::Macro;
use crate::patch::{PresetTable, PATCH_LEN};

pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub instrument: Instrument,
    pub description: &'static str,
    /// Factory slot the sound is derived from.
    pub base: usize,
    build: fn(&mut PresetTable, usize),
}

const DYN: usize = 0;
const AMP: usize = 1;
const MOD: usize = 2;
const DLY: usize = 3;
const RVB: usize = 4;
const CAB: usize = 5;

fn keep(_: &mut PresetTable, _: usize) {}

/// Amp model indices in the device table (t5 0x90000000, verified).
mod amp {
    pub const TWIN: u8 = 0; // UWE-Twins
    pub const AC30: u8 = 1; // UK-C30Normal
    pub const PLEXI: u8 = 4; // BritPLEX_100
    pub const MARS_FD: u8 = 5; // MarsFD100
}

/// Cab indices in the device table (t5 0x80000000, verified).
mod cab {
    pub const MAR_1960: u8 = 6; // Mar960BV30
    pub const DIEZEL: u8 = 7; // DizzlV30
    pub const TWIN: u8 = 10; // TwinJensenC
    pub const AC30: u8 = 14; // UK-C30Silvers
}

fn rig(t: &mut PresetTable, i: usize, amp: u8, cab: u8) {
    t.set_unit(i, AMP, amp);
    t.set_on(i, AMP, true);
    t.set_unit(i, CAB, cab);
    t.set_on(i, CAB, true);
}

pub const TEMPLATES: &[Template] = &[
    Template {
        id: "clean",
        name: "Limpo",
        instrument: Instrument::Guitar,
        description: "Som limpo com um pouco de sala. Bom para acordes e dedilhados.",
        base: 3, // 2A: lowest guitar gain (24), reverb on
        build: |t, i| {
            rig(t, i, amp::TWIN, cab::TWIN);
            Macro::Drive.apply(t, i, 22);
            Macro::Ambience.apply(t, i, 35);
        },
    },
    Template {
        id: "clean-chorus",
        name: "Limpo com chorus",
        instrument: Instrument::Guitar,
        description: "Limpo, largo e brilhante, estilo anos 80.",
        base: 3,
        build: |t, i| {
            rig(t, i, amp::TWIN, cab::TWIN);
            Macro::Drive.apply(t, i, 22);
            t.set_unit(i, MOD, 0); // Chorus
            t.set_on(i, MOD, true);
            t.set_param(i, MOD, 0, 30);
            t.set_param(i, MOD, 1, 55);
            t.set_param(i, MOD, 2, 45);
            Macro::Ambience.apply(t, i, 45);
        },
    },
    Template {
        id: "crunch",
        name: "Crunch",
        instrument: Instrument::Guitar,
        description: "Levemente sujo: limpa quando você toca leve, ruge quando bate forte.",
        base: 6, // 3A: gain 47
        build: |t, i| {
            rig(t, i, amp::AC30, cab::AC30);
            Macro::Drive.apply(t, i, 55);
            Macro::Ambience.apply(t, i, 25);
        },
    },
    Template {
        id: "rock",
        name: "Rock",
        instrument: Instrument::Guitar,
        description: "Distorção clássica para riffs e power chords.",
        base: 8, // 3C: gain 81, boost first
        build: |t, i| {
            rig(t, i, amp::PLEXI, cab::MAR_1960);
            Macro::Drive.apply(t, i, 70);
            t.set_on(i, DLY, false);
            Macro::Ambience.apply(t, i, 20);
        },
    },
    Template {
        id: "high-gain",
        name: "Pesado",
        instrument: Instrument::Guitar,
        description: "High gain com noise gate, para metal e riffs abafados.",
        base: 9, // 4A: TH_DieselHgn, gain 88, gate on
        build: |t, i| {
            t.set_unit(i, CAB, cab::DIEZEL);
            t.set_unit(i, DYN, 0); // Noise Gate
            t.set_on(i, DYN, true);
            t.set_on(i, MOD, false);
            Macro::Ambience.apply(t, i, 15);
        },
    },
    Template {
        id: "lead",
        name: "Solo",
        instrument: Instrument::Guitar,
        description: "Mais ganho e um eco para os solos cantarem.",
        base: 8, // 3C
        build: |t, i| {
            rig(t, i, amp::MARS_FD, cab::MAR_1960);
            Macro::Drive.apply(t, i, 78);
            t.set_on(i, DLY, true);
            t.set_unit(i, DLY, 0); // Analog
            t.set_param(i, DLY, 0, 55);
            t.set_param(i, DLY, 1, 30);
            t.set_param(i, DLY, 2, 22);
            t.set_on(i, RVB, true);
            t.set_param(i, RVB, 1, 14);
        },
    },
    Template {
        id: "ambient",
        name: "Ambiente",
        instrument: Instrument::Guitar,
        description: "Limpo e espaçoso, com reverb longo. Para climas e worship.",
        base: 3,
        build: |t, i| {
            rig(t, i, amp::TWIN, cab::TWIN);
            Macro::Drive.apply(t, i, 20);
            t.set_on(i, RVB, true);
            t.set_unit(i, RVB, 4); // Shimmer
            t.set_param(i, RVB, 0, 70);
            t.set_param(i, RVB, 1, 32);
            t.set_on(i, DLY, true);
            t.set_unit(i, DLY, 3); // Dual
            t.set_param(i, DLY, 0, 60);
            t.set_param(i, DLY, 1, 40);
            t.set_param(i, DLY, 2, 24);
        },
    },
    Template {
        id: "acoustic",
        name: "Violão",
        instrument: Instrument::Acoustic,
        description: "Sem amp e sem caixa: o som do captador do violão com compressor leve e sala.",
        base: 13, // 5B: modest settings everywhere
        build: |t, i| {
            t.set_on(i, AMP, false);
            t.set_on(i, CAB, false);
            t.set_on(i, MOD, false);
            t.set_on(i, DLY, false);
            t.set_unit(i, DYN, 2); // Compress
            t.set_on(i, DYN, true);
            t.set_param(i, DYN, 0, 20); // Gate
            t.set_param(i, DYN, 1, 45); // Sustain
            t.set_param(i, DYN, 2, 50); // Attack
            t.set_param(i, DYN, 3, 60); // Level
            t.set_on(i, RVB, true);
            t.set_unit(i, RVB, 0); // Room
            t.set_param(i, RVB, 0, 35);
            t.set_param(i, RVB, 1, 14);
        },
    },
    Template {
        id: "bass-clean",
        name: "Baixo limpo",
        instrument: Instrument::Bass,
        description: "Grave cheio e definido, sem efeitos.",
        base: 20, // 7C: bass amp, gain 34
        build: keep,
    },
    Template {
        id: "bass-drive",
        name: "Baixo com drive",
        instrument: Instrument::Bass,
        description: "Baixo com saturação, para rock.",
        base: 15, // 6A: bass amp, gain 71
        build: |t, i| {
            Macro::Drive.apply(t, i, 66);
        },
    },
];

impl Template {
    /// The 92-byte preset this template produces.
    pub fn bytes(&self) -> Vec<u8> {
        let mut t = PresetTable::factory();
        (self.build)(&mut t, self.base);
        t.patch(self.base).expect("base in range").raw().to_vec()
    }

    /// Write the template into slot `index` of `table`.
    pub fn apply(&self, table: &mut PresetTable, index: usize) -> bool {
        table.set_patch_raw(index, &self.bytes())
    }
}

pub fn by_id(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}

pub fn for_instrument(instrument: Instrument) -> impl Iterator<Item = &'static Template> {
    TEMPLATES.iter().filter(move |t| t.instrument == instrument)
}

const _: () = assert!(PATCH_LEN == 92);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{variant_count, Role};

    #[test]
    fn every_template_is_a_valid_preset() {
        for tpl in TEMPLATES {
            let bytes = tpl.bytes();
            assert_eq!(bytes.len(), PATCH_LEN, "{}", tpl.id);
            let mut t = PresetTable::factory();
            assert!(tpl.apply(&mut t, 0));
            let p = t.patch(0).unwrap();
            let mut chain = p.chain();
            chain.sort();
            assert_eq!(chain, [0, 1, 2, 3, 4, 5], "{} chain", tpl.id);
            for m in 0..6 {
                let role = Role::for_slot(m);
                assert!((p.unit(m) as usize) < variant_count(role), "{} unit {m}", tpl.id);
                for k in 0..6 {
                    assert!(p.param(m, k).unwrap() <= 100, "{} param {m}/{k}", tpl.id);
                }
            }
        }
    }

    #[test]
    fn instruments_match_the_sound() {
        let mut t = PresetTable::factory();
        for tpl in TEMPLATES {
            tpl.apply(&mut t, 0);
            let got = crate::catalog::instrument_for_patch(&t.patch(0).unwrap());
            assert_eq!(got, tpl.instrument, "{}", tpl.id);
        }
        assert!(for_instrument(Instrument::Bass).count() >= 2);
        assert!(by_id("acoustic").is_some());
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = TEMPLATES.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), TEMPLATES.len());
    }
}
