//! The pedal's built-in tuner (type 4 `0x70100000`, 6 bytes).
//!
//! Byte map, from the official app's `TunerController` and checked on the
//! device where marked:
//!
//! | Off | Meaning |
//! |---|---|
//! | 0 | Volume while tuning, 0..100 (**verified** read: 100) |
//! | 1 | Reference pitch as a signed offset from 440 Hz, -10..+10 (**verified** read: 0) |
//! | 2 | Write `1` to start, `0` to stop (**verified**); reads back `0xFF` |
//! | 3 | `1` while the tuner runs (**verified**) |
//! | 4 | Note, 1 = G#, 2 = A … 12 = G; `0` = signal but no stable note, `0xFF` = silent (**verified**: open A → 2, open D → 7) |
//! | 5 | Deviation, 127 = in tune; the app's in-tune window is 118..=136 (**verified** direction: a flat string reads below 127) |

pub const OFF_BYPASS_VOLUME: u32 = 0;
pub const OFF_REFERENCE: u32 = 1;
pub const OFF_ENABLE: u32 = 2;

/// The app clamps the reference to this range.
pub const REFERENCE_MIN: u16 = 430;
pub const REFERENCE_MAX: u16 = 450;

/// Indexed by the note byte minus one.
const NOTES: [&str; 12] = ["G#", "A", "A#", "B", "C", "C#", "D", "D#", "E", "F", "F#", "G"];
/// The official app shows "in tune" inside this distance from the centre.
pub const IN_TUNE: i16 = 9;
const CENTRE: i16 = 127;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub bypass_volume: u8,
    /// Reference A4 in Hz.
    pub reference: u16,
    pub running: bool,
    /// Note byte minus one: 0 = G#, 1 = A … 11 = G.
    pub note: Option<u8>,
    /// Signed deviation around the centre; the unit is not confirmed.
    pub deviation: Option<i16>,
}

impl Reading {
    pub fn parse(raw: &[u8]) -> Option<Self> {
        if raw.len() < 6 {
            return None;
        }
        let note = match raw[4] {
            n @ 1..=12 => Some(n - 1),
            _ => None,
        };
        let deviation = (raw[5] != 0xff && note.is_some()).then(|| raw[5] as i16 - CENTRE);
        Some(Self {
            bypass_volume: raw[0],
            reference: (440 + raw[1] as i8 as i16) as u16,
            running: raw[3] == 1,
            note,
            deviation,
        })
    }

    pub fn note_name(&self) -> Option<&'static str> {
        self.note.map(|n| NOTES[n as usize])
    }
}

/// The byte for a reference pitch, clamped to what the pedal accepts.
pub fn reference_byte(hz: u16) -> u8 {
    (hz.clamp(REFERENCE_MIN, REFERENCE_MAX) as i16 - 440) as i8 as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_reading_from_the_device() {
        let r = Reading::parse(&[0x64, 0x00, 0xff, 0x00, 0xff, 0x00]).unwrap();
        assert_eq!(r.bypass_volume, 100);
        assert_eq!(r.reference, 440);
        assert!(!r.running);
        assert_eq!(r.note, None);
        assert_eq!(r.deviation, None);
    }

    #[test]
    fn running_with_a_note() {
        // Captured from the device: open A string, then open D string.
        let r = Reading::parse(&[0x64, 0xfe, 0xff, 0x01, 0x02, 0x78]).unwrap();
        assert!(r.running);
        assert_eq!(r.reference, 438);
        assert_eq!(r.note_name(), Some("A"));
        assert_eq!(r.deviation, Some(-7));
        let d = Reading::parse(&[0x64, 0x00, 0xff, 0x01, 0x07, 0x30]).unwrap();
        assert_eq!(d.note_name(), Some("D"));
        // Signal without a stable note.
        assert_eq!(Reading::parse(&[0x64, 0x00, 0xff, 0x01, 0x00, 0x43]).unwrap().note, None);
    }

    #[test]
    fn reference_round_trip() {
        assert_eq!(reference_byte(432), (-8i8) as u8);
        assert_eq!(reference_byte(500), 10);
        assert_eq!(reference_byte(440), 0);
    }
}
