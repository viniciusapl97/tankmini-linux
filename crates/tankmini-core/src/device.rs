//! High-level pedal operations on top of the raw transport.

use std::time::Duration;

use crate::frame::Frame;
use crate::preset::{Preset, PRESET_LEN};
use crate::region;
use crate::transport::Transport;
use crate::{cmd, ADDR_ACTIVE_SLOT, ADDR_CAB_NAMES, ADDR_PRESET, ADDR_SLOT_SELECT, CAB_NAME_COUNT, CAB_NAME_LEN};

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
            .request_one(&Frame::new(cmd::IDENTITY, Vec::new()), T_SHORT)?;
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
        let reply = self.transport.request_one(&Frame::new(cmd::READ, body), T_READ)?;
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
        let reply = self.transport.request_one(&Frame::new(cmd::WRITE, body), T_WRITE)?;
        Ok(reply.and_then(|f| (f.cmd == cmd::ACK).then(|| f.body.first().copied().unwrap_or(0xff))))
    }

    pub fn read_preset(&mut self) -> std::io::Result<Option<Preset>> {
        let raw = self.read(region::DEV, ADDR_PRESET, PRESET_LEN)?;
        Ok(raw.and_then(|r| Preset::from_raw(r).ok()))
    }

    /// Write the whole block in ACKed chunks and verify the read-back.
    pub fn write_preset(&mut self, preset: &Preset) -> std::io::Result<WriteReport> {
        let data = preset.raw();
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
        let readback = self.read_preset()?;
        let verified = readback.as_ref() == Some(preset);
        Ok(WriteReport {
            chunks,
            chunks_ok,
            verified,
        })
    }

    pub fn active_slot(&mut self) -> std::io::Result<Option<u8>> {
        Ok(self
            .read(region::DEV, ADDR_ACTIVE_SLOT, 1)?
            .and_then(|v| v.first().copied()))
    }

    pub fn select_slot(&mut self, slot: u8) -> std::io::Result<bool> {
        let ack = self.write(region::DEV, ADDR_SLOT_SELECT + slot as u32, &[slot])?;
        Ok(ack == Some(0))
    }

    /// Cab/amp names: 20 entries of 14 bytes at `USR` `0x80000000`,
    /// NUL padded (verified: 280 bytes ends exactly after the 20th name).
    pub fn cab_names(&mut self) -> std::io::Result<Vec<String>> {
        let len = CAB_NAME_COUNT * CAB_NAME_LEN;
        let Some(raw) = self.read(region::USR, ADDR_CAB_NAMES, len)? else {
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
