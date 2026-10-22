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
//! 0x00   2   preset volume VOL, u16 LE 0..100          from app (PatchFields.vol)
//! 0x02   6   effect chain: order of the 6 modules      verified (always a permutation of 0..5)
//! 0x08   6   per-module on/off                         verified
//! 0x0e   6   per-module type ("unit")                  verified
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

/// Check that 92 bytes are a preset this app can show and the pedal can play:
/// the chain is a permutation of the six modules and every type byte is a
/// known variant. Files, library entries and backups go through this before
/// they reach the model.
pub fn validate_patch(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != PATCH_LEN {
        return Err(format!("preset precisa de {PATCH_LEN} bytes, veio {}", bytes.len()));
    }
    let mut chain = bytes[2..8].to_vec();
    chain.sort_unstable();
    if chain != [0, 1, 2, 3, 4, 5] {
        return Err("ordem dos blocos inválida".into());
    }
    for m in 0..MODULE_COUNT {
        let role = crate::catalog::Role::for_slot(m);
        if bytes[UNIT_OFFSET + m] as usize >= crate::catalog::variant_count(role) {
            return Err(format!("tipo de {} inválido", role.label()));
        }
    }
    Ok(())
}

/// One module's full state, for copy/paste between presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    pub module: usize,
    pub unit: u8,
    pub on: bool,
    pub params: [u16; PARAMS_PER_MODULE],
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

    /// Bytes 0x00..0x02, kept for the CLI; this is the preset volume.
    pub fn id(&self) -> u16 {
        self.vol()
    }

    /// Preset volume VOL (0..100), the per-preset level slider of the app.
    pub fn vol(&self) -> u16 {
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
        slot < MODULE_COUNT && self.raw[ON_OFFSET + slot] != 0
    }

    /// Module `slot` unit/type byte (`0x0e + slot`, verified via the app accessor).
    pub fn unit(&self, slot: usize) -> u8 {
        if slot >= MODULE_COUNT {
            return 0;
        }
        self.raw[UNIT_OFFSET + slot]
    }

    /// What the module in `slot` does (inferred slot→role binding).
    pub fn role(&self, slot: usize) -> crate::catalog::Role {
        crate::catalog::Role::for_slot(slot)
    }

    /// Display name of the effect in `slot`, using the device name tables for
    /// amp and cabinet slots.
    pub fn effect_name(&self, slot: usize, amp_names: &[String], cab_names: &[String]) -> String {
        let role = self.role(slot);
        let unit = self.unit(slot) as usize;
        let names = if role == crate::catalog::Role::Amp { amp_names } else { cab_names };
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

    pub fn set_vol(&mut self, patch: usize, vol: u16) -> bool {
        if patch >= PATCH_COUNT {
            return false;
        }
        let off = patch * PATCH_LEN;
        self.raw[off..off + 2].copy_from_slice(&vol.min(100).to_le_bytes());
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

    /// Byte-identical factory table for this firmware (`TANK MINI_004`), captured
    /// from the unit before any write and shipped as a 1942-byte asset.
    pub fn factory() -> Self {
        let raw = include_bytes!("../../../assets/factory-table.bin").to_vec();
        Self::from_raw(raw).expect("embedded factory table is valid")
    }

    pub fn is_factory(&self) -> bool {
        self.raw == Self::factory().raw
    }

    /// Copy one preset onto another slot (bank/footswitch copy, like the pedal
    /// does with the footswitches).
    pub fn copy_patch(&mut self, from: usize, to: usize) -> bool {
        if from >= PATCH_COUNT || to >= PATCH_COUNT || from == to {
            return false;
        }
        let src = self.raw[from * PATCH_LEN..(from + 1) * PATCH_LEN].to_vec();
        self.raw[to * PATCH_LEN..(to + 1) * PATCH_LEN].copy_from_slice(&src);
        true
    }

    /// Validate every preset of the table (see [`validate_patch`]).
    pub fn validate(&self) -> Result<(), String> {
        for p in self.patches() {
            validate_patch(p.raw()).map_err(|e| format!("{}: {e}", p.label()))?;
        }
        Ok(())
    }

    /// Import bytes into a slot. Accepts a single 92-byte preset or a whole
    /// 1942-byte table; both are validated first.
    pub fn import(&mut self, slot: usize, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() == PATCH_LEN {
            validate_patch(bytes)?;
            if slot >= PATCH_COUNT {
                return Err("slot fora da faixa".into());
            }
            self.raw[slot * PATCH_LEN..(slot + 1) * PATCH_LEN].copy_from_slice(bytes);
            return Ok(());
        }
        if bytes.len() == super::preset::PRESET_LEN {
            if !bytes[PATCH_LEN * PATCH_COUNT..].starts_with(b"PATCHEND") {
                return Err("arquivo de tabela sem PATCHEND".into());
            }
            PresetTable::from_raw(bytes.to_vec())?.validate()?;
            self.raw.copy_from_slice(bytes);
            return Ok(());
        }
        Err(format!(
            "tamanho inesperado: {} bytes (esperado {PATCH_LEN} ou {})",
            bytes.len(),
            super::preset::PRESET_LEN
        ))
    }

    /// Replace one preset wholesale (92 bytes).
    pub fn set_patch_raw(&mut self, index: usize, bytes: &[u8]) -> bool {
        if index >= PATCH_COUNT || bytes.len() != PATCH_LEN {
            return false;
        }
        self.raw[index * PATCH_LEN..(index + 1) * PATCH_LEN].copy_from_slice(bytes);
        true
    }

    /// Everything one module carries: type, on/off and its 6 parameter words.
    pub fn block(&self, patch: usize, module: usize) -> Option<Block> {
        let p = self.patch(patch)?;
        if module >= MODULE_COUNT {
            return None;
        }
        let mut params = [0u16; PARAMS_PER_MODULE];
        for (i, v) in params.iter_mut().enumerate() {
            *v = p.param(module, i).unwrap_or(0);
        }
        Some(Block {
            module,
            unit: p.unit(module),
            on: p.is_on(module),
            params,
        })
    }

    /// Paste a block into the same module of another preset. A block only fits
    /// the module it came from (an amp block is meaningless in the reverb slot).
    pub fn set_block(&mut self, patch: usize, block: &Block) -> bool {
        if patch >= PATCH_COUNT || block.module >= MODULE_COUNT {
            return false;
        }
        self.set_unit(patch, block.module, block.unit);
        self.set_on(patch, block.module, block.on);
        for (i, v) in block.params.iter().enumerate() {
            self.set_param(patch, block.module, i, *v);
        }
        true
    }

    /// Indices of the bytes that differ from `other` inside preset `patch`.
    pub fn diff_offsets(&self, other: &PresetTable, patch: usize) -> Vec<usize> {
        match (self.patch(patch), other.patch(patch)) {
            (Some(a), Some(b)) => (0..PATCH_LEN).filter(|i| a.raw()[*i] != b.raw()[*i]).collect(),
            _ => Vec::new(),
        }
    }

    /// Move the module at chain position `from` to position `to`, shifting the
    /// ones in between (drag-and-drop semantics, unlike a swap).
    pub fn move_chain_position(&mut self, patch: usize, from: usize, to: usize) -> bool {
        let Some(p) = self.patch(patch) else { return false };
        if from >= MODULE_COUNT || to >= MODULE_COUNT {
            return false;
        }
        let mut chain = p.chain().to_vec();
        let m = chain.remove(from);
        chain.insert(to, m);
        let mut out = [0u8; MODULE_COUNT];
        out.copy_from_slice(&chain);
        self.set_chain(patch, out)
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
    fn factory_asset_is_a_valid_table() {
        let f = PresetTable::factory();
        assert!(f.is_factory());
        assert_eq!(f.raw().len(), crate::preset::PRESET_LEN);
        assert!(f.patch(0).is_some());
    }

    #[test]
    fn copy_import_and_export() {
        let mut t = table();
        t.set_param(0, 1, 0, 11);
        assert!(t.copy_patch(0, 5));
        assert_eq!(t.patch(5).unwrap().param(1, 0), Some(11));
        let exported = t.patch(0).unwrap().raw().to_vec();
        assert_eq!(exported.len(), PATCH_LEN);
        assert!(t.import(9, &exported).is_ok());
        assert_eq!(t.patch(9).unwrap().param(1, 0), Some(11));
        assert!(t.import(9, &[1, 2, 3]).is_err());
    }

    #[test]
    fn block_copy_paste_and_diff() {
        let mut t = table();
        t.set_unit(0, 4, 3);
        t.set_on(0, 4, true);
        t.set_param(0, 4, 1, 42);
        let block = t.block(0, 4).unwrap();
        assert_eq!(block.params[1], 42);
        let before = t.clone();
        assert!(t.set_block(7, &block));
        assert_eq!(t.block(7, 4), Some(block));
        let diff = t.diff_offsets(&before, 7);
        assert!(diff.contains(&(UNIT_OFFSET + 4)));
        assert!(diff.contains(&(ON_OFFSET + 4)));
        assert!(diff.contains(&Patch::param_offset(4, 1)));
        assert!(t.diff_offsets(&before, 0).is_empty());
    }

    #[test]
    fn move_chain_shifts_instead_of_swapping() {
        let mut t = table();
        // [0, 1, 5, 2, 3, 4]: drag position 5 (module 4) to the front.
        assert!(t.move_chain_position(0, 5, 0));
        assert_eq!(t.patch(0).unwrap().chain(), [4, 0, 1, 5, 2, 3]);
        assert!(t.move_chain_position(0, 0, 5));
        assert_eq!(t.patch(0).unwrap().chain(), [0, 1, 5, 2, 3, 4]);
    }

    #[test]
    fn validation_rejects_broken_presets() {
        let f = PresetTable::factory();
        assert!(f.validate().is_ok());
        let good = f.patch(0).unwrap().raw().to_vec();
        assert!(validate_patch(&good).is_ok());
        let mut bad = good.clone();
        bad[2] = 90; // chain byte out of range
        assert!(validate_patch(&bad).is_err());
        let mut bad = good.clone();
        bad[UNIT_OFFSET + 2] = 7; // 8th modulation does not exist
        assert!(validate_patch(&bad).is_err());
        let mut t = f.clone();
        assert!(t.import(0, &bad).is_err());
        assert_eq!(t, f, "a refused import leaves the table alone");
        // Accessors never panic on garbage module indices.
        let p = f.patch(0).unwrap();
        assert!(!p.is_on(90));
        assert_eq!(p.unit(90), 0);
    }

    #[test]
    fn rejects_bad_table() {
        assert!(PresetTable::from_raw(vec![0u8; 10]).is_err());
        assert!(PresetTable::from_raw(vec![0u8; PRESET_LEN]).is_err());
    }
}
