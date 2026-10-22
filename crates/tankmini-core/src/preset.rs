//! The 1942-byte preset block returned by `READ type 4 addr 0x0000`.
//!
//! Layout (verified on the unit):
//!
//! ```text
//! 0x0000 .. 0x078b   21 records × 92 bytes
//! 0x078c .. 0x0793   "PATCHEND"
//! 0x0794 .. 0x0795   00 00 padding
//! ```
//!
//! Inside a record the parameter area is 30 little-endian u16 values at
//! `0x20..0x5c`. Field semantics are not decoded yet — see
//! `docs/protocol/tank-mini-usb-sysex.md` §6 and the open questions there.

pub const PRESET_LEN: usize = 1942;
pub const RECORD_LEN: usize = 92;
pub const RECORD_COUNT: usize = 21;
pub const TRAILER: &[u8; 8] = b"PATCHEND";
pub const RECORD_PARAM_OFFSET: usize = 0x20;
pub const RECORD_PARAM_COUNT: usize = 30;

/// A full preset block, byte-for-byte as the pedal serves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    raw: Vec<u8>,
}

impl Preset {
    /// Wrap raw bytes, validating length and trailer.
    pub fn from_raw(raw: Vec<u8>) -> Result<Self, String> {
        if raw.len() != PRESET_LEN {
            return Err(format!("expected {PRESET_LEN} bytes, got {}", raw.len()));
        }
        let preset = Self { raw };
        if !preset.has_trailer() {
            return Err("missing PATCHEND trailer".into());
        }
        Ok(preset)
    }

    /// Build from a record list, appending the trailer.
    pub fn from_records(records: &[[u8; RECORD_LEN]]) -> Self {
        let mut raw = Vec::with_capacity(PRESET_LEN);
        for r in records {
            raw.extend_from_slice(r);
        }
        raw.extend_from_slice(TRAILER);
        raw.extend_from_slice(&[0, 0]);
        debug_assert_eq!(raw.len(), PRESET_LEN);
        Self { raw }
    }

    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    pub fn raw_mut(&mut self) -> &mut [u8] {
        &mut self.raw
    }

    pub fn has_trailer(&self) -> bool {
        self.raw[RECORD_LEN * RECORD_COUNT..].starts_with(TRAILER)
    }

    pub fn records(&self) -> impl Iterator<Item = Record<'_>> {
        (0..RECORD_COUNT).map(move |i| Record {
            index: i,
            raw: &self.raw[i * RECORD_LEN..(i + 1) * RECORD_LEN],
        })
    }

    pub fn record(&self, index: usize) -> Option<Record<'_>> {
        if index >= RECORD_COUNT {
            return None;
        }
        Some(Record {
            index,
            raw: &self.raw[index * RECORD_LEN..(index + 1) * RECORD_LEN],
        })
    }

    pub fn record_mut(&mut self, index: usize) -> Option<RecordMut<'_>> {
        if index >= RECORD_COUNT {
            return None;
        }
        Some(RecordMut {
            index,
            raw: &mut self.raw[index * RECORD_LEN..(index + 1) * RECORD_LEN],
        })
    }

    /// Get a parameter value (u16 LE) of a record.
    pub fn param(&self, record: usize, param: usize) -> Option<u16> {
        self.record(record)?.param(param)
    }

    /// Set a parameter value (u16 LE) of a record.
    pub fn set_param(&mut self, record: usize, param: usize, value: u16) -> bool {
        match self.record_mut(record) {
            Some(mut r) => r.set_param(param, value),
            None => false,
        }
    }

    /// Human-readable hexdump, 16 bytes per line with the offset column.
    pub fn hexdump(&self) -> String {
        hexdump(&self.raw)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Record<'a> {
    pub index: usize,
    raw: &'a [u8],
}

impl<'a> Record<'a> {
    /// Record id at `0x00` (observed range `0x3a..0x61`).
    pub fn id(&self) -> u16 {
        u16::from_le_bytes([self.raw[0], self.raw[1]])
    }

    /// Byte at `0x0f`, the block/effect kind id observed in factory presets.
    pub fn kind(&self) -> u8 {
        self.raw[0x0f]
    }

    pub fn flags(&self) -> &'a [u8] {
        &self.raw[0x08..0x14]
    }

    pub fn param(&self, param: usize) -> Option<u16> {
        if param >= RECORD_PARAM_COUNT {
            return None;
        }
        let off = RECORD_PARAM_OFFSET + param * 2;
        Some(u16::from_le_bytes([self.raw[off], self.raw[off + 1]]))
    }

    pub fn params(&self) -> Vec<u16> {
        (0..RECORD_PARAM_COUNT).filter_map(|i| self.param(i)).collect()
    }

    pub fn raw(&self) -> &'a [u8] {
        self.raw
    }

    /// Byte offset of this record inside the preset block.
    pub fn offset(&self) -> usize {
        self.index * RECORD_LEN
    }
}

pub struct RecordMut<'a> {
    pub index: usize,
    raw: &'a mut [u8],
}

impl RecordMut<'_> {
    pub fn set_param(&mut self, param: usize, value: u16) -> bool {
        if param >= RECORD_PARAM_COUNT {
            return false;
        }
        let off = RECORD_PARAM_OFFSET + param * 2;
        self.raw[off..off + 2].copy_from_slice(&value.to_le_bytes());
        true
    }

    pub fn raw_mut(&mut self) -> &mut [u8] {
        self.raw
    }
}

/// Classic hexdump with an ASCII gutter.
pub fn hexdump(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (i, chunk) in data.chunks(16).enumerate() {
        let _ = write!(out, "{:06x}  ", i * 16);
        for b in chunk {
            let _ = write!(out, "{b:02x} ");
        }
        for _ in chunk.len()..16 {
            out.push_str("   ");
        }
        out.push_str(" |");
        for b in chunk {
            out.push(if (0x20..0x7f).contains(b) { *b as char } else { '.' });
        }
        out.push_str("|\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> Preset {
        Preset::from_records(&[[0u8; RECORD_LEN]; RECORD_COUNT])
    }

    #[test]
    fn layout_matches_device_dump() {
        assert_eq!(RECORD_LEN * RECORD_COUNT + TRAILER.len() + 2, PRESET_LEN);
        assert_eq!(RECORD_LEN * RECORD_COUNT, 0x078c);
        let p = blank();
        assert!(p.has_trailer());
        assert_eq!(&p.raw()[0x078c..0x0794], b"PATCHEND");
    }

    #[test]
    fn records_and_params_round_trip() {
        let mut p = blank();
        assert_eq!(p.records().count(), RECORD_COUNT);
        assert!(p.set_param(3, 7, 1234));
        assert_eq!(p.param(3, 7), Some(1234));
        assert_eq!(p.param(3, RECORD_PARAM_COUNT), None);
        // Parameter 7 of record 3 lives at 3*92 + 0x20 + 7*2.
        let off = 3 * RECORD_LEN + RECORD_PARAM_OFFSET + 7 * 2;
        assert_eq!(&p.raw()[off..off + 2], &1234u16.to_le_bytes());
    }

    #[test]
    fn rejects_wrong_length_or_missing_trailer() {
        assert!(Preset::from_raw(vec![0; 100]).is_err());
        let mut raw = blank().raw().to_vec();
        raw[0x078c] = b'X';
        assert!(Preset::from_raw(raw).is_err());
    }
}
