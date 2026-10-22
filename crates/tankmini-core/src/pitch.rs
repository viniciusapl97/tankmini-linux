//! Pitch detection for the tuner screen.
//!
//! The pedal sends the processed guitar over its USB audio input, so the tuner is a
//! signal-processing problem, not a protocol one: take a window of samples, find the
//! fundamental, turn it into a note name and a cents offset.

/// Result of looking at one window of audio.
#[derive(Debug, Clone, PartialEq)]
pub struct Pitch {
    /// Fundamental frequency in Hz.
    pub hz: f32,
    /// Note name without the octave, e.g. `A`.
    pub note: String,
    /// Scientific octave number (A4 = 440 Hz).
    pub octave: i32,
    /// Distance to the nearest note in cents (100 cents = one semitone).
    pub cents: f32,
    /// Loudness of the window (RMS), so the UI can tell "no signal" from "quiet".
    pub rms: f32,
}

impl Pitch {
    /// `A4`, `E2`, …
    pub fn label(&self) -> String {
        format!("{}{}", self.note, self.octave)
    }

    /// Whether the string is close enough to be considered in tune.
    pub fn in_tune(&self) -> bool {
        self.cents.abs() <= 5.0
    }

    /// How far to pull the string: `None` when in tune already.
    pub fn advice(&self) -> Option<&'static str> {
        if self.in_tune() {
            None
        } else if self.cents < 0.0 {
            Some("aperte a corda")
        } else {
            Some("afrouxe a corda")
        }
    }
}

/// Frames of audio to analyse at 44.1 kHz: about 85 ms, long enough to hold two
/// periods of a low E (82 Hz) and short enough to feel instant.
pub const WINDOW: usize = 4096;

/// Lowest and highest fundamentals we accept. Below is noise/DC, above is pick
/// attack and harmonics that only confuse the estimate.
const MIN_HZ: f32 = 55.0;
const MAX_HZ: f32 = 1300.0;

/// Detect the fundamental of one window.
///
/// Autocorrelation with a normalised difference (the core of YIN): it is cheap, has
/// no FFT dependency and behaves well on a plucked string, where the second harmonic
/// is often louder than the fundamental.
pub fn detect(samples: &[f32], sample_rate: u32) -> Option<Pitch> {
    if samples.len() < 256 || sample_rate == 0 {
        return None;
    }
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    // A guitar at a sane level is well above this; below it we would be tuning noise.
    if rms < 1e-4 {
        return None;
    }

    let rate = sample_rate as f32;
    let min_lag = (rate / MAX_HZ).floor().max(2.0) as usize;
    let max_lag = (rate / MIN_HZ).ceil() as usize;
    if max_lag + 2 >= samples.len() {
        return None;
    }

    // Difference function, normalised so that the scale of the signal does not matter.
    let mut diff = vec![0.0f32; max_lag + 2];
    for lag in min_lag..=max_lag.min(samples.len() - 1) {
        let mut sum = 0.0;
        let n = samples.len() - lag;
        for i in 0..n {
            let d = samples[i] - samples[i + lag];
            sum += d * d;
        }
        diff[lag] = sum / n as f32;
    }

    // Cumulative mean normalised difference: emphasises the first real dip.
    let mut running = 0.0f32;
    let mut cmnd = vec![f32::MAX; max_lag + 2];
    for lag in 1..=max_lag {
        running += diff[lag];
        if running > 0.0 {
            cmnd[lag] = diff[lag] * lag as f32 / running;
        }
    }

    let threshold = 0.25;
    let mut best: Option<usize> = None;
    let mut lag = min_lag;
    while lag <= max_lag {
        if cmnd[lag] < threshold {
            // Walk down to the local minimum of this dip.
            while lag + 1 <= max_lag && cmnd[lag + 1] < cmnd[lag] {
                lag += 1;
            }
            best = Some(lag);
            break;
        }
        lag += 1;
    }
    // Nothing under the threshold: take the global minimum, but only if it is a
    // plausible period at all.
    let lag = match best {
        Some(l) => l,
        None => {
            let mut min_lag_at = 0;
            let mut min = f32::MAX;
            for l in min_lag..=max_lag {
                if cmnd[l] < min {
                    min = cmnd[l];
                    min_lag_at = l;
                }
            }
            if min > 0.6 || min_lag_at == 0 {
                return None;
            }
            min_lag_at
        }
    };

    // Parabolic interpolation around the dip gives sub-sample accuracy: at 82 Hz one
    // sample of error is already ~15 cents.
    let (a, b, c) = (cmnd[lag - 1], cmnd[lag], cmnd[lag + 1]);
    let denom = a - 2.0 * b + c;
    let shift = if denom.abs() > f32::EPSILON {
        0.5 * (a - c) / denom
    } else {
        0.0
    };
    let period = lag as f32 + shift.clamp(-0.5, 0.5);
    if period <= 0.0 {
        return None;
    }
    let hz = rate / period;
    if !(MIN_HZ..=MAX_HZ).contains(&hz) || !hz.is_finite() {
        return None;
    }
    Some(pitch_from_hz(hz, 440.0, rms))
}

/// Turn a frequency into the nearest note of the equal-tempered scale.
pub fn pitch_from_hz(hz: f32, a4: f32, rms: f32) -> Pitch {
    // MIDI note number, where 69 = A4.
    let midi = 69.0 + 12.0 * (hz / a4).log2();
    let nearest = midi.round();
    let cents = (midi - nearest) * 100.0;
    let index = nearest as i32;
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let name = names[(index.rem_euclid(12)) as usize];
    let octave = index.div_euclid(12) - 1;
    Pitch { hz, note: name.to_string(), octave, cents, rms }
}

/// The six strings of standard tuning, low to high, with their labels.
pub const STRINGS: &[(f32, &str)] = &[
    (82.41, "E2"),
    (110.00, "A2"),
    (146.83, "D3"),
    (196.00, "G3"),
    (246.94, "B3"),
    (329.63, "E4"),
];

/// Nearest string of standard tuning, for players who do not read note names.
pub fn nearest_string(pitch: &Pitch) -> Option<&'static str> {
    let mut best: Option<(&'static str, f32)> = None;
    for (hz, label) in STRINGS {
        let cents = 1200.0 * (pitch.hz / hz).log2();
        let distance = cents.abs();
        if distance <= 250.0 && best.is_none_or(|(_, d)| distance < d) {
            best = Some((label, distance));
        }
    }
    best.map(|(label, _)| label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, rate: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                0.3 * (2.0 * std::f32::consts::PI * hz * t).sin()
            })
            .collect()
    }

    /// A plucked string: fundamental plus a louder second harmonic, which is exactly
    /// the case that fools naive peak picking.
    fn plucked(hz: f32, rate: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let w = 2.0 * std::f32::consts::PI * hz * t;
                0.2 * w.sin() + 0.4 * (2.0 * w).sin() + 0.1 * (3.0 * w).sin()
            })
            .collect()
    }

    #[test]
    fn detects_a_sine() {
        let p = detect(&sine(440.0, 44_100, WINDOW), 44_100).expect("pitch");
        assert!((p.hz - 440.0).abs() < 1.5, "hz = {}", p.hz);
        assert_eq!(p.label(), "A4");
        assert!(p.cents.abs() < 10.0, "cents = {}", p.cents);
        assert!(p.in_tune());
    }

    #[test]
    fn detects_low_e_with_a_strong_second_harmonic() {
        let p = detect(&plucked(82.41, 44_100, WINDOW), 44_100).expect("pitch");
        assert!((p.hz - 82.41).abs() < 2.0, "hz = {}", p.hz);
        assert_eq!(p.label(), "E2");
        assert_eq!(nearest_string(&p), Some("E2"));
    }

    #[test]
    fn detects_every_standard_string() {
        for (hz, label) in STRINGS {
            let p = detect(&sine(*hz, 44_100, WINDOW), 44_100).expect("pitch");
            assert_eq!(&p.label(), label, "expected {label}, got {} ({:.2} Hz)", p.label(), p.hz);
        }
    }

    #[test]
    fn silence_and_noise_are_not_a_pitch() {
        assert!(detect(&vec![0.0; WINDOW], 44_100).is_none());
        assert!(detect(&vec![0.00001; WINDOW], 44_100).is_none());
        let mut seed = 0x12345678u32;
        let noise: Vec<f32> = (0..WINDOW)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                ((seed >> 16) as f32 / 32_768.0 - 1.0) * 0.2
            })
            .collect();
        assert!(detect(&noise, 44_100).is_none(), "white noise is not a note");
    }

    #[test]
    fn cents_and_advice_point_the_right_way() {
        let flat = pitch_from_hz(435.0, 440.0, 0.1);
        assert_eq!(flat.label(), "A4");
        assert!(flat.cents < -15.0, "cents = {}", flat.cents);
        assert_eq!(flat.advice(), Some("aperte a corda"));

        let sharp = pitch_from_hz(446.0, 440.0, 0.1);
        assert!(sharp.cents > 15.0);
        assert_eq!(sharp.advice(), Some("afrouxe a corda"));

        let good = pitch_from_hz(441.0, 440.0, 0.1);
        assert!(good.in_tune());
        assert_eq!(good.advice(), None);
    }

    #[test]
    fn a4_reference_moves_the_scale() {
        // Same 432 Hz signal: in tune with A=432, flat by ~32 cents against A=440.
        let p = pitch_from_hz(432.0, 432.0, 0.1);
        assert!(p.in_tune(), "cents = {}", p.cents);
        let p = pitch_from_hz(432.0, 440.0, 0.1);
        assert!(p.cents < -25.0, "cents = {}", p.cents);
    }

    #[test]
    fn nearest_string_is_none_far_from_any_string() {
        let p = pitch_from_hz(500.0, 440.0, 0.1);
        assert_eq!(nearest_string(&p), None);
    }
}
