//! Frame format of the M-VAVE protocol.
//!
//! ```text
//! off  0   1   2     3..5           6 .. 6+len-1   6+len
//!      00  59  cmd   len (u24 LE)   body           cs
//! ```
//!
//! `cs = (~sum(body)) & 0xff`. On the wire the whole frame is packed with
//! [`crate::sys7::encode7`] and wrapped in `f0 … f7`.

use crate::sys7::{decode7, encode7};

pub const SYNC: [u8; 2] = [0x00, 0x59];
pub const HEADER_LEN: usize = 6;

/// A protocol frame: command byte plus body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub cmd: u8,
    pub body: Vec<u8>,
}

/// Checksum over the body only.
pub fn checksum(body: &[u8]) -> u8 {
    !(body.iter().fold(0u8, |acc, b| acc.wrapping_add(*b)))
}

impl Frame {
    pub fn new(cmd: u8, body: Vec<u8>) -> Self {
        Self { cmd, body }
    }

    /// Frame without SysEx wrapping or 7-bit packing.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.body.len() + 1);
        out.extend_from_slice(&SYNC);
        out.push(self.cmd);
        let len = self.body.len() as u32;
        out.extend_from_slice(&len.to_le_bytes()[..3]);
        out.extend_from_slice(&self.body);
        out.push(checksum(&self.body));
        out
    }

    /// `f0 <encode7(frame)> f7`
    pub fn to_sysex(&self) -> Vec<u8> {
        let packed = encode7(&self.to_bytes());
        let mut out = Vec::with_capacity(packed.len() + 2);
        out.push(0xf0);
        out.extend_from_slice(&packed);
        out.push(0xf7);
        out
    }
}

/// Split a raw byte stream into complete frames, skipping leading junk.
///
/// Incomplete trailing frames are dropped.
pub fn parse_frames(raw: &[u8]) -> Vec<Frame> {
    let mut frames = Vec::new();
    let mut i = 0usize;
    while i + HEADER_LEN + 1 <= raw.len() {
        if raw[i] != SYNC[0] || raw[i + 1] != SYNC[1] {
            i += 1;
            continue;
        }
        let cmd = raw[i + 2];
        let len = u32::from_le_bytes([raw[i + 3], raw[i + 4], raw[i + 5], 0]) as usize;
        let end = i + HEADER_LEN + len + 1;
        if end > raw.len() {
            break;
        }
        frames.push(Frame::new(cmd, raw[i + HEADER_LEN..i + HEADER_LEN + len].to_vec()));
        i = end;
    }
    frames
}

/// Undo the SysEx envelope and 7-bit packing of a received byte stream.
pub fn unwrap_sysex(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for segment in raw.split(|b| *b == 0xf0).skip(1) {
        let Some(end) = segment.iter().position(|b| *b == 0xf7) else {
            continue;
        };
        if let Some(decoded) = decode7(&segment[..end]) {
            out.extend_from_slice(&decoded);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_has_ff_checksum() {
        let f = Frame::new(crate::cmd::IDENTITY, Vec::new());
        assert_eq!(f.to_bytes(), vec![0x00, 0x59, 0x11, 0x00, 0x00, 0x00, 0xff]);
        // ...which packs to a SysEx starting with f0 00 32.
        let sysex = f.to_sysex();
        assert_eq!(&sysex[..3], &[0xf0, 0x00, 0x32]);
        assert_eq!(*sysex.last().unwrap(), 0xf7);
    }

    #[test]
    fn identity_request_bytes_match_device() {
        // Captured from the unit: `f0 00 32 45 | 00 00 00 | 40 7f | 00 | f7`
        let f = Frame::new(crate::cmd::IDENTITY, Vec::new());
        assert_eq!(
            f.to_sysex(),
            vec![0xf0, 0x00, 0x32, 0x45, 0x00, 0x00, 0x00, 0x40, 0x7f, 0x00, 0xf7]
        );
    }

    #[test]
    fn identity_reply_decodes() {
        // Captured reply to `cmd 11` on this unit, kept as hex to avoid
        // transcription errors in the vector.
        let hex = "f0 00 32 45 58 01 00 00 2a 41 1c 2d 02 52 29 12 27 49 3e 41 01 43 06 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 20 24 f7";
        let raw: Vec<u8> = hex
            .split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect();
        let frames = parse_frames(&unwrap_sysex(&raw));
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        assert_eq!(frame.cmd, crate::cmd::IDENTITY);
        assert_eq!(frame.body.len(), 27);
        assert_eq!(&frame.body[..13], b"TANK MINI_004");
        assert!(frame.body[13..].iter().all(|b| *b == 0));
        // The reply round-trips through the encoder byte for byte.
        assert_eq!(frame.to_sysex(), raw);
    }

    #[test]
    fn sysex_round_trip() {
        let body = vec![0x01, 0x02, 0x03, 0xfe, 0xff, 0x80, 0x7f];
        let frame = Frame::new(crate::cmd::WRITE, body);
        let raw = frame.to_sysex();
        let frames = parse_frames(&unwrap_sysex(&raw));
        assert_eq!(frames, vec![frame]);
    }

    #[test]
    fn parse_skips_junk_between_frames() {
        let a = Frame::new(0x11, vec![1, 2]).to_bytes();
        let b = Frame::new(0x12, vec![]).to_bytes();
        let mut raw = vec![0xde, 0xad];
        raw.extend_from_slice(&a);
        raw.push(0x7e);
        raw.extend_from_slice(&b);
        assert_eq!(parse_frames(&raw), vec![Frame::new(0x11, vec![1, 2]), Frame::new(0x12, vec![])]);
    }
}
