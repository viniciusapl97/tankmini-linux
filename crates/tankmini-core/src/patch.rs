//! Per-preset ("patch") view of the pedal's preset table.
//!
//! The table returned by `READ type 4 addr 0x0000` is a flat array of 21 patches
//! of 92 bytes each, followed by the ASCII trailer `PATCHEND`. Every patch is a
//! complete preset; the pedal's active slot only selects which one plays.
//!
//! Verified on the unit: reading `addr = index * 92` with length 92 returns the
//! same bytes as the correspondig slice of the whole-table read, so a single
//! patch can be read and written on its own.
//!
//! ```text
//! off  size  meaning                                   evidence
//! 0x00   2   patch id/version (observed 0x3a..0x61)    verified (raw), meaning inferred
//! 0x02   6   effect chain: order of the 6 modules      verified (always a permutation of 0..5)
//! 0x08   6   per-module enable/flag bytes (0..2)       inferred
//! 0x0e   1   unknown (0..2)                            inferred
//! 0x0f   1   unknown, observed 0..19 (likely amp model) inferred
//! 0x10   4   unknown (small values)                    inferred
//! 0x14  72   36 parameters, u16 LE, every value 0..100 verified (range)
//! ```
//!
//! The 36 parameters line up as 6 modules × 6 parameter slots; module 0 is the
//! first in the chain array, and unused slots read 0.

pub const PATCH_LEN: usize = 92;
pub const PATCH_COUNT: usize = 21;
pub const ON_OFFSET: usize = 0x08;
pub const UNIT_OFFSET: usize = 0x0e;
pub const PARAM_OFFSET: usize = 0x14;
pub const MODULE_COUNT: usize = 6;
pub const PARAMS_PER_MODULE: usize = 6;
pub const PARAM_COUNT: usize = MODULE_COUNT * PARAMS_PER_MODULE;

/// Human names for the 21 slots (7 banks × A/B/C), matching the pedal's LEDs.
pub const SLOT_LABELS: [&str; PATCH_COUNT] = [
    "1A", "1B", "1C", "2A", "2B", "2C", "3A", "3B", "3C", "4A", "4B", "4C", "5A", "5B", "5C", "6A",
    "6B", "6C", "7A", "7B", "7C",
];

/// One parameter slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamSlot {
    pub module: usize,
    pub index: usize,
}

/// A single preset, borrowed from the table.
#[derive(Debug, Clone, Copy)]
pub struct Patch<'a> {
    pub index: usize,
    raw: &'a [u8],
}

impl<'a> Patch<'a> {
    pub fn new(index: usize, raw: &'a [u8]) -> Self {
        debug_assert_eq!(raw.len(), PATCH_LEN);
        Self { index, raw }
    }

    pub fn raw(&self) -> &'a [u8] {
        self.raw
    }

    pub fn label(&self) -> &'static str {
        SLOT_LABELS.get(self.index).copied().unwrap_or("??")
    }

    /// Patch id at 0x00. Meaning is unknown; it changes with the preset content.
    pub fn id(&self) -> u16 {
        u16::from_le_bytes([self.raw[0], self.raw[1]])
    }

    /// Chain order: the six module indices, in signal order.
    pub fn chain(&self) -> [u8; MODULE_COUNT] {
        let mut out = [0u8; MODULE_COUNT];
        out.copy_from_slice(&self.raw[2..8]);
        out
    }

    /// Per-module flag bytes at 0x08..0x0e (0/1/2 observed).
    pub fn module_flags(&self) -> [u8; MODULE_COUNT] {
        let mut out = [0u8; MODULE_COUNT];
        out.copy_from_slice(&self.raw[8..14]);
        out
    }

    pub fn byte(&self, offset: usize) -> u8 {
        self.raw[offset]
    }

    /// Module `slot` on/off byte (`0x08 + slot`, verified via the app accessor).
    pub fn is_on(&self, slot: usize) -> bool {
        self.raw[ON_OFFSET + slot] != 0
    }

    /// Module `slot` unit/type byte (`0x0e + slot`, verified via the app accessor).
    pub fn unit(&self, slot: usize) -> u8 {
        self.raw[UNIT_OFFSET + slot]
    }

    /// What the module in `slot` does (inferred slot→role binding).
    pub fn role(&self, slot: usize) -> crate::catalog::Role {
        crate::catalog::Role::for_slot(slot)
    }

    /// Display name of the effect in `slot`, using the device cab/amp table for
    /// amp and cabinet slots.
    pub fn effect_name<'b>(&self, slot: usize, names: &'b [String]) -> String {
        let role = self.role(slot);
        let unit = self.unit(slot) as usize;
        match role {
            crate::catalog::Role::Amp | crate::catalog::Role::Cabinet => names
                .get(unit)
                .cloned()
                .unwrap_or_else(|| format!("{} {}", role.label(), unit + 1)),
            other => crate::catalog::catalog_name(other, unit as u8),
        }
    }

    /// Parameter value (0..100 observed).
    pub fn param(&self, module: usize, index: usize) -> Option<u16> {
        let slot = module * PARAMS_PER_MODULE + index;
        if module >= MODULE_COUNT || index >= PARAMS_PER_MODULE {
            return None;
        }
        let off = PARAM_OFFSET + slot * 2;
        Some(u16::from_le_bytes([self.raw[off], self.raw[off + 1]]))
    }

    /// All parameters, module by module.
    pub fn params(&self) -> Vec<u16> {
        (0..PARAM_COUNT)
            .filter_map(|i| self.param(i / PARAMS_PER_MODULE, i % PARAMS_PER_MODULE))
            .collect()
    }

    pub fn param_offset(module: usize, index: usize) -> usize {
        PARAM_OFFSET + (module * PARAMS_PER_MODULE + index) * 2
    }
}

/// The full 21-preset table, owned and editable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetTable {
    raw: Vec<u8>,
}

impl PresetTable {
    pub fn from_raw(raw: Vec<u8>) -> Result<Self, String> {
        if raw.len() != super::preset::PRESET_LEN {
            return Err(format!(
                "expected {} bytes, got {}",
                super::preset::PRESET_LEN,
                raw.len()
            ));
        }
        if !raw[PATCH_LEN * PATCH_COUNT..].starts_with(b"PATCHEND") {
            return Err("missing PATCHEND trailer".into());
        }
        Ok(Self { raw })
    }

    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    pub fn patch(&self, index: usize) -> Option<Patch<'_>> {
        if index >= PATCH_COUNT {
            return None;
        }
        Some(Patch::new(index, &self.raw[index * PATCH_LEN..(index + 1) * PATCH_LEN]))
    }

    pub fn patches(&self) -> impl Iterator<Item = Patch<'_>> {
        (0..PATCH_COUNT).filter_map(move |i| self.patch(i))
    }

    pub fn set_param(&mut self, patch: usize, module: usize, index: usize, value: u16) -> bool {
        if patch >= PATCH_COUNT || module >= MODULE_COUNT || index >= PARAMS_PER_MODULE {
            return false;
        }
        let off = patch * PATCH_LEN + Patch::param_offset(module, index);
        self.raw[off..off + 2].copy_from_slice(&value.to_le_bytes());
        true
    }

    pub fn set_byte(&mut self, patch: usize, offset: usize, value: u8) -> bool {
        if patch >= PATCH_COUNT || offset >= PATCH_LEN {
            return false;
        }
        self.raw[patch * PATCH_LEN + offset] = value;
        true
    }

    pub fn set_unit(&mut self, patch: usize, slot: usize, unit: u8) -> bool {
        if patch >= PATCH_COUNT || slot >= MODULE_COUNT {
            return false;
        }
        self.raw[patch * PATCH_LEN + UNIT_OFFSET + slot] = unit;
        true
    }

    pub fn set_on(&mut self, patch: usize, slot: usize, on: bool) -> bool {
        if patch >= PATCH_COUNT || slot >= MODULE_COUNT {
            return false;
        }
        self.raw[patch * PATCH_LEN + ON_OFFSET + slot] = u8::from(on);
        true
    }

    pub fn set_chain(&mut self, patch: usize, chain: [u8; MODULE_COUNT]) -> bool {
        if patch >= PATCH_COUNT {
            return false;
        }
        let base = patch * PATCH_LEN + 2;
        self.raw[base..base + MODULE_COUNT].copy_from_slice(&chain);
        true
    }

    /// Swap two chain positions of a patch (module order, not module content).
    pub fn swap_chain_positions(&mut self, patch: usize, a: usize, b: usize) -> bool {
        let chain = match self.patch(patch) {
            Some(p) => p.chain(),
            None => return false,
        };
        if a >= MODULE_COUNT || b >= MODULE_COUNT {
            return false;
        }
        let mut chain = chain;
        chain.swap(a, b);
        self.set_chain(patch, chain)
    }

    pub fn to_preset(&self) -> super::preset::Preset {
        super::preset::Preset::from_raw(self.raw.clone()).expect("validated on construction")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::PRESET_LEN;

    fn table() -> PresetTable {
        let mut raw = vec![0u8; PRESET_LEN];
        raw[PATCH_LEN * PATCH_COUNT..PATCH_LEN * PATCH_COUNT + 8].copy_from_slice(b"PATCHEND");
        // Give patch 0 a distinguishable chain and a param.
        raw[2..8].copy_from_slice(&[0, 1, 5, 2, 3, 4]);
        PresetTable::from_raw(raw).unwrap()
    }

    #[test]
    fn layout_constants_line_up() {
        assert_eq!(PATCH_LEN * PATCH_COUNT, 0x078c);
        assert_eq!(PARAM_OFFSET + PARAM_COUNT * 2, PATCH_LEN);
    }

    #[test]
    fn chain_and_params_round_trip() {
        let mut t = table();
        assert_eq!(t.patch(0).unwrap().chain(), [0, 1, 5, 2, 3, 4]);
        assert!(t.set_param(0, 1, 4, 95));
        assert_eq!(t.patch(0).unwrap().param(1, 4), Some(95));
        let off = Patch::param_offset(1, 4);
        assert_eq!(&t.raw()[off..off + 2], &95u16.to_le_bytes());
        assert!(t.swap_chain_positions(0, 2, 5));
        assert_eq!(t.patch(0).unwrap().chain(), [0, 1, 4, 2, 3, 5]);
    }

    #[test]
    fn rejects_bad_table() {
        assert!(PresetTable::from_raw(vec![0u8; 10]).is_err());
        assert!(PresetTable::from_raw(vec![0u8; PRESET_LEN]).is_err());
    }
}
