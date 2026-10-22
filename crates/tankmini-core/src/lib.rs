//! Protocol core for the M-VAVE TANK MINI over USB-MIDI.
//!
//! Everything here was reverse engineered from the hardware and the official
//! M-EFCS app; see `docs/protocol/tank-mini-usb-sysex.md` for the evidence and
//! the verified/unverified status of each claim.

pub mod backup;
pub mod catalog;
pub mod device;
pub mod discovery;
pub mod frame;
pub mod patch;
pub mod preset;
pub mod sys7;
pub mod transport;

pub use device::{DeviceInfo, Pedal};
pub use discovery::{find_devices, Device};
pub use frame::Frame;
pub use patch::{Patch, PresetTable, PATCH_COUNT, PATCH_LEN, SLOT_LABELS};
pub use preset::{Preset, Record, PRESET_LEN, RECORD_LEN, RECORD_COUNT};

/// USB identity of the pedal family handled by this crate.
pub const VENDOR_ID: u16 = 0x4c4a;
pub const PRODUCT_ID: u16 = 0xc755;

/// Region types of the frame protocol.
pub mod region {
    /// Device region: holds the live preset block and slot control addresses.
    pub const DEV: u8 = 4;
    /// User region: name tables (cabs/amps).
    pub const USR: u8 = 5;
}

/// Commands of the frame protocol.
pub mod cmd {
    pub const ACK: u8 = 0x00;
    pub const IDENTITY: u8 = 0x11;
    pub const OTHER_INFO: u8 = 0x12;
    /// Exists in the app, never needed here. Never sent by this crate.
    pub const ERASE: u8 = 0x21;
    pub const WRITE: u8 = 0x22;
    pub const READ: u8 = 0x23;
}

/// Address of the live preset block inside [`region::DEV`].
pub const ADDR_PRESET: u32 = 0x0000;
/// Address of the active slot index (read, length 1) inside [`region::DEV`].
pub const ADDR_ACTIVE_SLOT: u32 = 0x2000_0000;
/// Writing `[i]` to `ADDR_SLOT_SELECT + i` selects slot `i`.
pub const ADDR_SLOT_SELECT: u32 = 0xE000_0000;
/// Writing an empty payload here commits the preset table to flash.
///
/// Recovered from the app's `PresetStore._write(0xF0000000, const [])` on save;
/// ACKed by the unit.
pub const ADDR_SAVE: u32 = 0xF000_0000;
/// Address of the cab/amp name table inside [`region::USR`].
pub const ADDR_CAB_NAMES: u32 = 0x8000_0000;
/// Length of one name entry in the cab/amp table (ASCII, NUL padded).
pub const CAB_NAME_LEN: usize = 14;
/// Number of cab/amp slots on this unit (the manual says 20 IR CAB slots).
pub const CAB_NAME_COUNT: usize = 20;
