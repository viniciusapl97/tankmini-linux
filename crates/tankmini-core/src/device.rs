//! High-level pedal operations on top of the raw transport.

use std::time::Duration;

use crate::frame::Frame;
use crate::preset::{Preset, PRESET_LEN};
use crate::region;
use crate::transport::Transport;
use crate::{
    cmd, ADDR_ACTIVE_SLOT, ADDR_AMP_NAMES, ADDR_BYPASS, ADDR_GLOB, ADDR_LOOPBACK, ADDR_CAB_NAMES, ADDR_EDIT, ADDR_PRESET, ADDR_SLOT_SELECT, ADDR_TUNER, CAB_NAME_COUNT,
    CAB_NAME_LEN,
};

/// Chunk size used by the official family apps for block writes; verified
/// ACKed on this unit for the whole preset block.
pub const WRITE_CHUNK: usize = 173;

const T_SHORT: Duration = Duration::from_millis(600);
const T_READ: Duration = Duration::from_millis(1500);
const T_WRITE: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub firmware: String,
}

pub struct Pedal {
    transport: Transport,
}

impl Pedal {
    pub fn open(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        Ok(Self {
            transport: Transport::open(path)?,
        })
    }

    pub fn path(&self) -> &std::path::Path {
        self.transport.path()
    }

    /// `cmd 0x11`: 16-byte NUL-padded name plus 11 unused bytes.
    pub fn identify(&mut self) -> std::io::Result<Option<DeviceInfo>> {
        let reply = self
            .transport
            .request_matching(&Frame::new(cmd::IDENTITY, Vec::new()), T_SHORT, |f| f.cmd == cmd::IDENTITY)?;
        Ok(reply.map(|f| {
            let end = f.body.iter().position(|b| *b == 0).unwrap_or(f.body.len());
            DeviceInfo {
                firmware: String::from_utf8_lossy(&f.body[..end.min(16)]).trim().to_string(),
            }
        }))
    }

    /// Generic read. `Ok(None)` when the device answers with an error ACK.
    pub fn read(&mut self, rtype: u8, addr: u32, len: usize) -> std::io::Result<Option<Vec<u8>>> {
        let mut body = Vec::with_capacity(8);
        body.push(rtype);
        body.extend_from_slice(&addr.to_le_bytes());
        body.extend_from_slice(&(len as u32).to_le_bytes()[..3]);
        // The reply echoes type and address: match on them so a late answer to
        // another read of the same length can never be taken for this one.
        let echo = body[..5].to_vec();
        let reply = self.transport.request_matching(&Frame::new(cmd::READ, body), T_READ, |f| {
            (f.cmd == cmd::READ && f.body.starts_with(&echo)) || f.cmd == cmd::ACK
        })?;
        Ok(match reply {
            Some(f) if f.cmd == cmd::READ && f.body.len() >= 8 + len => Some(f.body[8..8 + len].to_vec()),
            _ => None,
        })
    }

    /// Generic write; returns the ACK status (`0x00` = accepted).
    pub fn write(&mut self, rtype: u8, addr: u32, data: &[u8]) -> std::io::Result<Option<u8>> {
        let mut body = Vec::with_capacity(8 + data.len());
        body.push(rtype);
        body.extend_from_slice(&addr.to_le_bytes());
        body.extend_from_slice(&(data.len() as u32).to_le_bytes()[..3]);
        body.extend_from_slice(data);
        let reply = self.transport.request_matching(&Frame::new(cmd::WRITE, body), T_WRITE, |f| f.cmd == cmd::ACK)?;
        Ok(reply.and_then(|f| (f.cmd == cmd::ACK).then(|| f.body.first().copied().unwrap_or(0xff))))
    }

    pub fn read_preset(&mut self) -> std::io::Result<Option<Preset>> {
        let raw = self.read(region::DEV, ADDR_PRESET, PRESET_LEN)?;
        Ok(raw.and_then(|r| Preset::from_raw(r).ok()))
    }

    /// Write the whole block in ACKed chunks and verify the read-back.
    pub fn write_preset(&mut self, preset: &Preset) -> std::io::Result<WriteReport> {
        self.write_block(preset.raw())
    }

    fn write_block(&mut self, data: &[u8]) -> std::io::Result<WriteReport> {
        let mut chunks_ok = 0usize;
        let mut chunks = 0usize;
        for (i, part) in data.chunks(WRITE_CHUNK).enumerate() {
            chunks += 1;
            let addr = ADDR_PRESET + (i * WRITE_CHUNK) as u32;
            if self.write(region::DEV, addr, part)?.unwrap_or(1) == 0 {
                chunks_ok += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(120));
        let readback = self.read(region::DEV, ADDR_PRESET, data.len())?;
        let verified = readback.as_deref() == Some(data);
        Ok(WriteReport {
            chunks,
            chunks_ok,
            verified,
        })
    }

    /// Commit the working preset table to flash: `WRITE type 4 0xF0000000` with an
    /// empty payload, which is exactly what the official app does on save.
    pub fn commit(&mut self) -> std::io::Result<Option<u8>> {
        self.write(region::DEV, crate::ADDR_SAVE, &[])
    }

    pub fn active_slot(&mut self) -> std::io::Result<Option<u8>> {
        Ok(self
            .read(region::DEV, ADDR_ACTIVE_SLOT, 1)?
            .and_then(|v| v.first().copied()))
    }

    /// The 4 global bytes: active slot, unknown, bypass, unknown.
    pub fn glob(&mut self) -> std::io::Result<Option<[u8; 4]>> {
        Ok(self.read(region::DEV, ADDR_GLOB, 4)?.and_then(|v| v.try_into().ok()))
    }

    pub fn set_bypass(&mut self, on: bool) -> std::io::Result<bool> {
        Ok(self.write(region::DEV, ADDR_BYPASS, &[u8::from(on)])? == Some(0))
    }

    pub fn set_loopback(&mut self, on: bool) -> std::io::Result<bool> {
        Ok(self.write(region::DEV, ADDR_LOOPBACK, &[u8::from(on)])? == Some(0))
    }

    /// One tuner reading (see [`crate::tuner`]).
    pub fn tuner(&mut self) -> std::io::Result<Option<crate::tuner::Reading>> {
        Ok(self
            .read(region::DEV, ADDR_TUNER, 6)?
            .and_then(|v| crate::tuner::Reading::parse(&v)))
    }

    /// Write one tuner control byte (`crate::tuner::OFF_*`).
    pub fn tuner_write(&mut self, offset: u32, value: u8) -> std::io::Result<bool> {
        Ok(self.write(region::DEV, ADDR_TUNER + offset, &[value])? == Some(0))
    }

    pub fn select_slot(&mut self, slot: u8) -> std::io::Result<bool> {
        let ack = self.write(region::DEV, ADDR_SLOT_SELECT + slot as u32, &[slot])?;
        Ok(ack == Some(0))
    }

    /// Read the whole preset table (21 patches + trailer).
    pub fn read_table(&mut self) -> std::io::Result<Option<crate::patch::PresetTable>> {
        let raw = self.read(region::DEV, ADDR_PRESET, PRESET_LEN)?;
        Ok(raw.and_then(|r| crate::patch::PresetTable::from_raw(r).ok()))
    }

    /// Write the whole table in ACKed chunks and verify the read-back.
    pub fn write_table(&mut self, table: &crate::patch::PresetTable) -> std::io::Result<WriteReport> {
        self.write_block(table.raw())
    }

    /// Read one patch (92 bytes) by slot index.
    pub fn read_patch(&mut self, index: u8) -> std::io::Result<Option<Vec<u8>>> {
        let addr = ADDR_PRESET + index as u32 * crate::patch::PATCH_LEN as u32;
        self.read(region::DEV, addr, crate::patch::PATCH_LEN)
    }

    /// Write one patch (92 bytes) by slot index.
    pub fn write_patch(&mut self, index: u8, data: &[u8]) -> std::io::Result<u8> {
        let addr = ADDR_PRESET + index as u32 * crate::patch::PATCH_LEN as u32;
        Ok(self.write(region::DEV, addr, data)?.unwrap_or(0xff))
    }

    /// Read the edit buffer (the playing preset, 92 bytes).
    pub fn read_edit(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        self.read(region::DEV, ADDR_EDIT, crate::patch::PATCH_LEN)
    }

    /// Write `data` at `offset` inside the edit buffer: the live-edit path of
    /// the official app. Returns the ACK status (`0x00` = accepted).
    pub fn write_edit(&mut self, offset: usize, data: &[u8]) -> std::io::Result<u8> {
        Ok(self.write(region::DEV, ADDR_EDIT + offset as u32, data)?.unwrap_or(0xff))
    }

    /// Cab names: 20 entries of 14 bytes at `USR` `0x80000000`,
    /// NUL padded (verified: 280 bytes ends exactly after the 20th name).
    /// Put a CAB record (see [`crate::ir`]) into the pedal's type 5 buffer,
    /// where the official app puts it to audition and before saving. RAM only.
    /// Returns whether the read-back matches.
    pub fn cab_upload(&mut self, record: &[u8]) -> std::io::Result<bool> {
        self.model_upload(record)
    }

    /// Same buffer for an AMP record ([`crate::amp`]): the app sends both kinds
    /// to type 5 `0x0`.
    pub fn amp_upload(&mut self, record: &[u8]) -> std::io::Result<bool> {
        self.model_upload(record)
    }

    fn model_upload(&mut self, record: &[u8]) -> std::io::Result<bool> {
        // Small exchanges only: one 3092-byte read of this buffer left the
        // pedal's USB side hung until a power cycle (2026-10-22). 128-byte
        // writes and 512-byte reads are what was tested.
        const CHUNK: usize = 128;
        for (i, part) in record.chunks(CHUNK).enumerate() {
            let addr = crate::ir::ADDR_BUFFER + (i * CHUNK) as u32;
            if self.write(region::USR, addr, part)? != Some(0) {
                return Ok(false);
            }
        }
        let mut back = Vec::with_capacity(record.len());
        for off in (0..record.len()).step_by(512) {
            let n = 512.min(record.len() - off);
            match self.read(region::USR, crate::ir::ADDR_BUFFER + off as u32, n)? {
                Some(part) => back.extend(part),
                None => return Ok(false),
            }
        }
        Ok(back == record)
    }

    /// One stored CAB slot (4096 bytes).
    pub fn cab_slot(&mut self, slot: u8) -> std::io::Result<Option<Vec<u8>>> {
        self.read_slot(crate::ir::ADDR_SLOTS + slot as u32 * crate::ir::SLOT_LEN as u32, crate::ir::SLOT_LEN)
    }

    /// One stored AMP slot (8192 bytes).
    pub fn amp_slot(&mut self, slot: u8) -> std::io::Result<Option<Vec<u8>>> {
        self.read_slot(crate::amp::ADDR_SLOTS + slot as u32 * crate::amp::SLOT_LEN as u32, crate::amp::SLOT_LEN)
    }

    /// Store an AMP record in slot `slot` (**writes flash**): upload, then
    /// `WRITE 0xF0000000 + slot [0]`, verified by reading the slot back.
    pub fn amp_save(&mut self, slot: u8, record: &[u8]) -> std::io::Result<bool> {
        if slot as usize >= crate::amp::SLOT_COUNT || record.len() != crate::amp::RECORD_LEN {
            return Ok(false);
        }
        if !self.amp_upload(record)? {
            return Ok(false);
        }
        if self.write(region::USR, crate::amp::ADDR_SAVE_AMP + slot as u32, &[0])? != Some(0) {
            return Ok(false);
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
            if let Some(stored) = self.amp_slot(slot)? {
                if stored.starts_with(record) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn read_slot(&mut self, addr: u32, len: usize) -> std::io::Result<Option<Vec<u8>>> {
        let mut out = Vec::with_capacity(len);
        for off in (0..len).step_by(512) {
            match self.read(region::USR, addr + off as u32, 512)? {
                Some(part) => out.extend(part),
                None => return Ok(None),
            }
        }
        Ok(Some(out))
    }

    /// Store the buffer into CAB slot `slot` (**writes flash**), the way the app
    /// does it: rewrite the header with the final name and level, then
    /// `WRITE 0xF0000100 + slot [0]`. Verified by reading the slot back
    /// until it starts with `record` (up to 5 s).
    pub fn cab_save(&mut self, slot: u8, record: &[u8]) -> std::io::Result<bool> {
        if slot as usize >= crate::ir::SLOT_COUNT || record.len() != crate::ir::RECORD_LEN {
            return Ok(false);
        }
        if !self.cab_upload(record)? {
            return Ok(false);
        }
        if self.write(region::USR, crate::ir::ADDR_SAVE_CAB + slot as u32, &[0])? != Some(0) {
            return Ok(false);
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
            if let Some(stored) = self.cab_slot(slot)? {
                if stored.starts_with(record) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub fn cab_names(&mut self) -> std::io::Result<Vec<String>> {
        self.name_table(ADDR_CAB_NAMES)
    }

    /// Amp model names: same layout at `USR` `0x90000000` (verified).
    pub fn amp_names(&mut self) -> std::io::Result<Vec<String>> {
        self.name_table(ADDR_AMP_NAMES)
    }

    fn name_table(&mut self, addr: u32) -> std::io::Result<Vec<String>> {
        let len = CAB_NAME_COUNT * CAB_NAME_LEN;
        let Some(raw) = self.read(region::USR, addr, len)? else {
            return Ok(Vec::new());
        };
        Ok(raw
            .chunks(CAB_NAME_LEN)
            .map(|c| {
                let end = c.iter().position(|b| *b == 0).unwrap_or(c.len());
                String::from_utf8_lossy(&c[..end]).trim().to_string()
            })
            .collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteReport {
    pub chunks: usize,
    pub chunks_ok: usize,
    pub verified: bool,
}

impl WriteReport {
    pub fn all_acked(&self) -> bool {
        self.chunks == self.chunks_ok
    }
}