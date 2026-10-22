//! Cabinet IRs: turning a WAV into the record the pedal stores in a CAB slot.
//!
//! Record layout (**verified**: the official app's upload on firmware
//! `TANK MINI_004`, read back from type 5 `0x0`, and the 20 factory slots):
//!
//! ```text
//! 0   "CAB\0"
//! 4   name, 14 bytes, NUL padded
//! 18  0x01 (format; the am4 factory blob uses 0x02)
//! 19  level, 0..127
//! 20  1024 mono samples, 24-bit little endian, 44.1 kHz
//! ```
//!
//! The conversion follows the app's `WaveUtils`: average the channels to mono,
//! resample to 44.1 kHz with a Kaiser-windowed sinc (β = 8.6), scale to 24 bits
//! and clamp. The app does not normalize. A 24-bit 44.1 kHz mono file comes out
//! bit-exact, as it did from the app.

pub const RATE: u32 = 44_100;
/// Samples kept from the IR (a 4096-byte slot holds 20 + 3072 bytes of it).
pub const SAMPLES: usize = 1024;
pub const HEADER_LEN: usize = 20;
pub const RECORD_LEN: usize = HEADER_LEN + SAMPLES * 3;
pub const NAME_LEN: usize = 14;
pub const SLOT_COUNT: usize = 20;
/// Level the app starts with (its "CAB Level 50").
pub const DEFAULT_LEVEL: u8 = 50;

/// Type 5 buffer the app writes a CAB record into before saving it.
pub const ADDR_BUFFER: u32 = 0x0000_0000;
/// Writing `[0]` to `ADDR_SAVE_CAB + slot` stores the buffer into that slot (flash).
pub const ADDR_SAVE_CAB: u32 = 0xF000_0100;
/// Stored slots, `0x1000` bytes each (read-only for us).
pub const ADDR_SLOTS: u32 = 0x7002_8000;
pub const SLOT_LEN: usize = 0x1000;

/// Decoded audio, still at the file's rate.
#[derive(Debug, Clone, PartialEq)]
pub struct Wav {
    pub rate: u32,
    pub channels: u16,
    /// Mono, -1.0..1.0.
    pub samples: Vec<f64>,
}

/// Parse a RIFF/WAVE file: PCM 8/16/24/32-bit or IEEE float 32/64-bit,
/// any channel count (mixed down to mono by averaging, like the app).
pub fn parse_wav(bytes: &[u8]) -> Result<Wav, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("não é um arquivo WAV".into());
    }
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut data: Option<&[u8]> = None;
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
        let body = &bytes[i + 8..(i + 8 + len).min(bytes.len())];
        match id {
            b"fmt " if body.len() >= 16 => {
                let mut format = u16::from_le_bytes([body[0], body[1]]);
                let channels = u16::from_le_bytes([body[2], body[3]]);
                let rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                let bits = u16::from_le_bytes([body[14], body[15]]);
                // WAVE_FORMAT_EXTENSIBLE: the real format is the subformat GUID's first word.
                if format == 0xfffe && body.len() >= 26 {
                    format = u16::from_le_bytes([body[24], body[25]]);
                }
                fmt = Some((format, channels, rate, bits));
            }
            b"data" => data = Some(body),
            _ => {}
        }
        i += 8 + len + (len & 1);
    }
    let (format, channels, rate, bits) = fmt.ok_or("WAV sem bloco fmt")?;
    let data = data.ok_or("WAV sem bloco data")?;
    if channels == 0 || rate == 0 {
        return Err("WAV com formato inválido".into());
    }
    let width = (bits as usize).div_ceil(8);
    let frame = width * channels as usize;
    if width == 0 || frame == 0 {
        return Err("WAV com formato inválido".into());
    }
    let read: Box<dyn Fn(&[u8]) -> f64> = match (format, bits) {
        (1, 8) => Box::new(|b| (b[0] as f64 - 128.0) / 128.0),
        (1, 16) => Box::new(|b| i16::from_le_bytes([b[0], b[1]]) as f64 / 32768.0),
        (1, 24) => Box::new(|b| ((i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f64) / 8388608.0),
        (1, 32) => Box::new(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64 / 2147483648.0),
        (3, 32) => Box::new(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64),
        (3, 64) => Box::new(|b| f64::from_le_bytes(b[..8].try_into().unwrap())),
        _ => return Err(format!("formato de WAV não suportado ({format}, {bits} bits)")),
    };
    let samples = data
        .chunks_exact(frame)
        .map(|f| f.chunks_exact(width).map(&read).sum::<f64>() / channels as f64)
        .collect();
    Ok(Wav { rate, channels, samples })
}

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
    loop {
        term *= (x / (2.0 * k)).powi(2);
        sum += term;
        if term < 1e-15 * sum {
            return sum;
        }
        k += 1.0;
    }
}

/// Resample with a Kaiser-windowed sinc (β = 8.6), producing at most `max` samples.
pub fn resample(input: &[f64], from: u32, to: u32, max: usize) -> Vec<f64> {
    if from == to {
        return input.iter().take(max).copied().collect();
    }
    const BETA: f64 = 8.6;
    const HALF: isize = 32;
    let ratio = to as f64 / from as f64;
    // Low-pass at the lower of the two Nyquist frequencies.
    let cutoff = ratio.min(1.0);
    let norm = bessel_i0(BETA);
    let len = ((input.len() as f64 * ratio).ceil() as usize).min(max);
    (0..len)
        .map(|n| {
            let t = n as f64 / ratio;
            let centre = t.floor() as isize;
            let mut acc = 0.0;
            for k in (centre - HALF + 1)..=(centre + HALF) {
                if k < 0 || k as usize >= input.len() {
                    continue;
                }
                let x = t - k as f64;
                let w = x / HALF as f64;
                if w.abs() > 1.0 {
                    continue;
                }
                let window = bessel_i0(BETA * (1.0 - w * w).sqrt()) / norm;
                let arg = std::f64::consts::PI * x * cutoff;
                let sinc = if arg.abs() < 1e-9 { 1.0 } else { arg.sin() / arg };
                acc += input[k as usize] * sinc * cutoff * window;
            }
            acc
        })
        .collect()
}

/// The 3072 sample bytes for a WAV, zero padded when the IR is shorter.
pub fn samples_24bit(wav: &Wav) -> Vec<u8> {
    let mono = resample(&wav.samples, wav.rate, RATE, SAMPLES);
    let mut out = Vec::with_capacity(SAMPLES * 3);
    for i in 0..SAMPLES {
        let v = mono.get(i).copied().unwrap_or(0.0);
        let s = (v * 8388608.0).round().clamp(-8388608.0, 8388607.0) as i32;
        out.extend_from_slice(&s.to_le_bytes()[..3]);
    }
    out
}

/// The 20-byte header. Non-ASCII characters in `name` become `_`.
pub fn header(name: &str, level: u8) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..4].copy_from_slice(b"CAB\0");
    for (i, c) in name.chars().take(NAME_LEN).enumerate() {
        h[4 + i] = if c.is_ascii_graphic() || c == ' ' { c as u8 } else { b'_' };
    }
    h[18] = 1;
    h[19] = level.min(127);
    h
}

/// A complete record: header + samples.
pub fn record(name: &str, level: u8, wav: &Wav) -> Vec<u8> {
    let mut r = header(name, level).to_vec();
    r.extend(samples_24bit(wav));
    r
}

/// A short name for a slot from a file name (the pedal shows 14 characters).
pub fn name_from_path(path: &std::path::Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().chars().filter(|c| c.is_ascii() && !c.is_ascii_control()).take(NAME_LEN).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, bits: u16, format: u16, data: &[u8]) -> Vec<u8> {
        let mut v = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        v.extend(16u32.to_le_bytes());
        v.extend(format.to_le_bytes());
        v.extend(channels.to_le_bytes());
        v.extend(rate.to_le_bytes());
        v.extend((rate * channels as u32 * bits as u32 / 8).to_le_bytes());
        v.extend((channels * bits / 8).to_le_bytes());
        v.extend(bits.to_le_bytes());
        v.extend(b"data");
        v.extend((data.len() as u32).to_le_bytes());
        v.extend(data);
        v
    }

    #[test]
    fn mono_24bit_at_44k_is_copied_bit_exact() {
        // What the official app sent for a 24-bit/44.1k file: the same bytes.
        let pcm: Vec<u8> = [3296050i32, 6436962, -1496087, -8388608]
            .iter()
            .flat_map(|s| s.to_le_bytes()[..3].to_vec())
            .collect();
        let w = parse_wav(&wav(44_100, 1, 24, 1, &pcm)).unwrap();
        let s = samples_24bit(&w);
        assert_eq!(&s[..pcm.len()], &pcm[..]);
        assert!(s[pcm.len()..].iter().all(|b| *b == 0));
        assert_eq!(s.len(), SAMPLES * 3);
    }

    #[test]
    fn stereo_16bit_is_averaged() {
        let data: Vec<u8> = [16384i16, 0, -32768, -32768].iter().flat_map(|s| s.to_le_bytes()).collect();
        let w = parse_wav(&wav(44_100, 2, 16, 1, &data)).unwrap();
        assert_eq!(w.samples, vec![0.25, -1.0]);
    }

    #[test]
    fn long_ir_at_48k_is_resampled_and_cut() {
        let data: Vec<u8> = (0..4800).flat_map(|i| ((i as f32 * 0.01).sin() * 0.5).to_le_bytes()).collect();
        let w = parse_wav(&wav(48_000, 1, 32, 3, &data)).unwrap();
        assert_eq!(samples_24bit(&w).len(), SAMPLES * 3);
        let r = resample(&w.samples, 48_000, RATE, usize::MAX);
        assert_eq!(r.len(), 4410);
    }

    #[test]
    fn header_layout() {
        let h = header("Mesa 4x12 V30 longo", 200);
        assert_eq!(&h[..4], b"CAB\0");
        assert_eq!(&h[4..18], b"Mesa 4x12 V30 ");
        assert_eq!(h[18], 1);
        assert_eq!(h[19], 127);
        assert_eq!(record("x", 50, &Wav { rate: RATE, channels: 1, samples: vec![] }).len(), RECORD_LEN);
    }

    #[test]
    fn rejects_non_wav() {
        assert!(parse_wav(b"hello").is_err());
    }
}
