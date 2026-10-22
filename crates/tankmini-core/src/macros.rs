//! Beginner "macro" controls: four big knobs that drive real parameters.
//!
//! Parameter names and order are **from app** (`docs/protocol/app-findings.md`):
//! the preset volume is the VOL word at 0x00, amp params are `Gain, Level,
//! Bass, Mid, Treble`, reverb word 1 and delay word 2 are their `Mix`. Which
//! knobs a beginner needs is our choice.

use crate::patch::{Patch, PresetTable};

const MOD_AMP: usize = 1;
const MOD_DLY: usize = 3;
const MOD_RVB: usize = 4;

const AMP_GAIN: usize = 0;
const AMP_TREBLE: usize = 4;
const RVB_MIX: usize = 1;
const DLY_MIX: usize = 2;

/// Reverb/delay mix at "Ambiente" = 100. Factory mixes top out around 30, so
/// the macro scales instead of driving the mix to a washed-out 100.
const RVB_MIX_MAX: f64 = 45.0;
const DLY_MIX_MAX: f64 = 35.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Macro {
    Volume,
    Drive,
    Brightness,
    Ambience,
}

impl Macro {
    pub const ALL: [Macro; 4] = [Macro::Volume, Macro::Drive, Macro::Brightness, Macro::Ambience];

    pub fn label(self) -> &'static str {
        match self {
            Macro::Volume => "Volume",
            Macro::Drive => "Drive",
            Macro::Brightness => "Brilho",
            Macro::Ambience => "Ambiente",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Macro::Volume => "Volume geral deste som.",
            Macro::Drive => "De limpo (esquerda) a distorcido (direita).",
            Macro::Brightness => "Mais escuro ou mais brilhante.",
            Macro::Ambience => "Quanto eco e reverb o som tem. Zero desliga os dois.",
        }
    }

    /// Current macro value (0..=100) of a preset.
    pub fn read(self, patch: &Patch<'_>) -> u16 {
        let p = |m, i| patch.param(m, i).unwrap_or(0);
        match self {
            Macro::Volume => patch.vol().min(100),
            Macro::Drive => p(MOD_AMP, AMP_GAIN),
            Macro::Brightness => p(MOD_AMP, AMP_TREBLE),
            Macro::Ambience => {
                let rvb = if patch.is_on(MOD_RVB) {
                    p(MOD_RVB, RVB_MIX) as f64 / RVB_MIX_MAX
                } else {
                    0.0
                };
                let dly = if patch.is_on(MOD_DLY) {
                    p(MOD_DLY, DLY_MIX) as f64 / DLY_MIX_MAX
                } else {
                    0.0
                };
                (rvb.max(dly) * 100.0).round().clamp(0.0, 100.0) as u16
            }
        }
    }

    /// Write the macro into the preset. Returns false when the preset index is
    /// out of range.
    pub fn apply(self, table: &mut PresetTable, index: usize, value: u16) -> bool {
        if table.patch(index).is_none() {
            return false;
        }
        let value = value.min(100);
        match self {
            Macro::Volume => {
                table.set_vol(index, value);
            }
            Macro::Drive => {
                table.set_param(index, MOD_AMP, AMP_GAIN, value);
            }
            Macro::Brightness => {
                table.set_param(index, MOD_AMP, AMP_TREBLE, value);
            }
            Macro::Ambience => {
                let f = value as f64 / 100.0;
                let on = value > 0;
                table.set_on(index, MOD_RVB, on);
                table.set_param(index, MOD_RVB, RVB_MIX, (f * RVB_MIX_MAX).round() as u16);
                // The delay joins in the upper half only, so low settings are a
                // touch of room and high settings add echoes.
                let dly = ((f - 0.5).max(0.0) * 2.0 * DLY_MIX_MAX).round() as u16;
                table.set_on(index, MOD_DLY, dly > 0);
                table.set_param(index, MOD_DLY, DLY_MIX, dly);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_and_volume_round_trip() {
        let mut t = PresetTable::factory();
        assert!(Macro::Drive.apply(&mut t, 4, 77));
        assert_eq!(Macro::Drive.read(&t.patch(4).unwrap()), 77);
        assert!(Macro::Volume.apply(&mut t, 4, 250));
        assert_eq!(Macro::Volume.read(&t.patch(4).unwrap()), 100);
        assert!(!Macro::Volume.apply(&mut t, 99, 1));
    }

    #[test]
    fn volume_is_the_preset_vol_word() {
        let mut t = PresetTable::factory();
        // 1A stores VOL 70 (0x46) at 0x00.
        assert_eq!(Macro::Volume.read(&t.patch(0).unwrap()), 70);
        Macro::Volume.apply(&mut t, 0, 55);
        assert_eq!(&t.patch(0).unwrap().raw()[0..2], &[55, 0]);
        // 1A treble (amp word 4) is 95.
        assert_eq!(Macro::Brightness.read(&t.patch(0).unwrap()), 95);
    }

    #[test]
    fn ambience_zero_turns_reverb_and_delay_off() {
        let mut t = PresetTable::factory();
        Macro::Ambience.apply(&mut t, 2, 0);
        let p = t.patch(2).unwrap();
        assert!(!p.is_on(MOD_RVB) && !p.is_on(MOD_DLY));
        assert_eq!(Macro::Ambience.read(&p), 0);
        Macro::Ambience.apply(&mut t, 2, 30);
        let p = t.patch(2).unwrap();
        assert!(p.is_on(MOD_RVB) && !p.is_on(MOD_DLY));
        Macro::Ambience.apply(&mut t, 2, 100);
        let p = t.patch(2).unwrap();
        assert!(p.is_on(MOD_DLY));
        assert_eq!(Macro::Ambience.read(&p), 100);
    }

    #[test]
    fn macros_only_touch_their_bytes() {
        let base = PresetTable::factory();
        let mut t = base.clone();
        Macro::Drive.apply(&mut t, 0, 1);
        assert_eq!(t.diff_offsets(&base, 0), vec![Patch::param_offset(1, 0)]);
    }
}
