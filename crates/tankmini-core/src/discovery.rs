//! Device discovery: map `/dev/snd/midiC*D*` nodes to their USB identity.
//!
//! The pedal exposes USB audio plus USB-MIDI, so the rawmidi node is all we
//! need. We resolve it through sysfs instead of asking the user.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{PRODUCT_ID, VENDOR_ID};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub vid: u16,
    pub pid: u16,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub midi_path: PathBuf,
    pub card: u32,
    /// True when VID/PID match the TANK MINI family.
    pub known: bool,
}

impl Device {
    pub fn display_name(&self) -> String {
        self.product
            .clone()
            .unwrap_or_else(|| "M-VAVE device".to_string())
    }
}

fn read_hex(path: &Path) -> Option<u16> {
    let s = fs::read_to_string(path).ok()?;
    u16::from_str_radix(s.trim(), 16).ok()
}

fn read_str(path: &Path) -> Option<String> {
    let s = fs::read_to_string(path).ok()?;
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Find the USB device directory that owns a sysfs sound device.
fn usb_parent(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(fs::canonicalize(start).ok()?);
    for _ in 0..6 {
        let dir = cur?;
        if dir.join("idVendor").exists() && dir.join("idProduct").exists() {
            return Some(dir);
        }
        cur = dir.parent().map(Path::to_path_buf);
    }
    None
}

/// All rawmidi nodes that belong to a USB device, whether known or not.
pub fn find_devices() -> Vec<Device> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/dev/snd") else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix("midiC") else {
            continue;
        };
        let (card_str, _dev_str) = rest.split_once('D').unwrap_or((rest, "0"));
        let Ok(card) = card_str.parse::<u32>() else {
            continue;
        };
        let sys = PathBuf::from(format!("/sys/class/sound/{name}/device"));
        let Some(usb) = usb_parent(&sys) else {
            continue;
        };
        let (Some(vid), Some(pid)) = (read_hex(&usb.join("idVendor")), read_hex(&usb.join("idProduct"))) else {
            continue;
        };
        out.push(Device {
            vid,
            pid,
            product: read_str(&usb.join("product")),
            serial: read_str(&usb.join("serial")),
            midi_path: entry.path(),
            card,
            known: vid == VENDOR_ID && pid == PRODUCT_ID,
        });
    }
    out.sort_by(|a, b| a.midi_path.cmp(&b.midi_path));
    out
}

/// First device matching the pedal's VID/PID, if attached.
pub fn find_pedal() -> Option<Device> {
    find_devices().into_iter().find(|d| d.known)
}
