//! AMP model files (`.am4data`): the record the pedal stores in an AMP slot.
//!
//! What the official app sends (**verified**, firmware `TANK MINI_004`, type 5
//! buffer read back after an audition, see `docs/protocol/app-findings.md` §13):
//! the file verbatim, then a 68-byte `LMT` block: `"LMT\0"` + 16 LE floats.
//!
//! The block is the final hidden state of the model's GRU after one second of
//! a decaying input, computed by the app's `calc_bias_gru_am4`. This module
//! reimplements it: same fixed-point arithmetic, same lookup tables (they are
//! generated from their formulas, not copied). It matches the app's library,
//! run under an emulator, byte for byte on every AM4 model of the community
//! site (2026-10-22); `tools/lmt_reference.py` is the reference.
//!
//! Model layout (PyTorch `GRU(1, 16)` + `Linear(16, 1)`), 929 floats at
//! `WEIGHTS`: W_ih[48], W_hh[48×16], b_ih[48], b_hh[48], then the output layer
//! (not needed here). Gates in PyTorch order r, z, n.

pub const MAGIC_AM4: &[u8; 4] = b"AM4\0";
/// Bytes of an AM4 model before its LMT block.
pub const AM4_LEN: usize = 0x1ebc;
pub const LMT_LEN: usize = 68;
pub const RECORD_LEN: usize = AM4_LEN + LMT_LEN;
const WEIGHTS: usize = 0x83c;
const HIDDEN: usize = 16;
/// Name stored after the magic (14 bytes, NUL padded), like the CAB records.
pub const NAME_LEN: usize = 14;
pub const SLOT_COUNT: usize = 20;

/// Writing `[0]` to `ADDR_SAVE_AMP + slot` stores the buffer in that AMP slot
/// (flash). **from app** (`0xF0000000` in the AMP import module); not run yet.
pub const ADDR_SAVE_AMP: u32 = 0xF000_0000;
/// Stored AMP slots, 0x2000 bytes each (read-only for us).
pub const ADDR_SLOTS: u32 = 0x7000_0000;
pub const SLOT_LEN: usize = 0x2000;

/// Q21 with the app's scale: 1.0 → 2097151.
const ONE: f32 = 2_097_151.0;

/// Sigmoid and tanh as the app's tables: 512 points from −16 to +16 in steps
/// of 1/16, value in Q23 (sigmoid floored, tanh truncated), plus the slope to
/// the next point for linear interpolation.
struct Tables {
    sig: [i32; 512],
    sig_d: [i32; 512],
    tanh: [i32; 512],
    tanh_d: [i32; 512],
}

impl Tables {
    fn new() -> Self {
        let scale = 8_388_608.0f64;
        let x = |i: usize| (i as f64 - 256.0) / 16.0;
        let sig = |i: usize| ((1.0 / (1.0 + (-x(i)).exp())) * scale).floor() as i32;
        let tanh = |i: usize| (x(i).tanh() * scale).trunc() as i32;
        let mut t = Tables { sig: [0; 512], sig_d: [0; 512], tanh: [0; 512], tanh_d: [0; 512] };
        for i in 0..512 {
            t.sig[i] = sig(i);
            t.tanh[i] = tanh(i);
            t.sig_d[i] = sig(i + 1) - sig(i);
            t.tanh_d[i] = tanh(i + 1) - tanh(i);
        }
        t
    }

    /// Q21 in, Q21 out (`k_tanh_tab_q23` and its sigmoid twin).
    fn lookup(table: &[i32; 512], slope: &[i32; 512], a: i64) -> i64 {
        let v = a.clamp(-0x200_0000, 0x1ff_ffff);
        let i = ((v + 0x200_0000) >> 17) as usize;
        let frac = v & 0x1_ffff;
        // C division: truncates toward zero.
        let step = (frac * slope[i] as i64) / 0x2_0000;
        (table[i] as i64 + step) >> 2
    }

    fn sigmoid(&self, a: i64) -> i64 {
        Self::lookup(&self.sig, &self.sig_d, a)
    }

    fn tanh(&self, a: i64) -> i64 {
        Self::lookup(&self.tanh, &self.tanh_d, a)
    }
}

fn q21(x: f32) -> i64 {
    // fcvtzs: truncate, saturate.
    (x * ONE) as i32 as i64
}

struct Gru {
    w_ih: [i64; 48],
    w_hh: [[i64; HIDDEN]; 48],
    b_ih: [i64; 48],
    b_hh: [i64; 48],
    h: [i64; HIDDEN],
}

impl Gru {
    fn from_floats(f: &[f32]) -> Self {
        let mut g = Gru { w_ih: [0; 48], w_hh: [[0; HIDDEN]; 48], b_ih: [0; 48], b_hh: [0; 48], h: [0; HIDDEN] };
        for r in 0..48 {
            g.w_ih[r] = q21(f[r]);
            for c in 0..HIDDEN {
                g.w_hh[r][c] = q21(f[48 + r * HIDDEN + c]);
            }
            g.b_ih[r] = q21(f[816 + r]);
            g.b_hh[r] = q21(f[864 + r]);
        }
        g
    }

    fn step(&mut self, t: &Tables, input: f32) {
        let x = q21(input);
        let h = self.h;
        let mac = |row: usize| -> i64 { (0..HIDDEN).map(|c| self.w_hh[row][c] * h[c]).sum() };
        let mut next = [0i64; HIDDEN];
        for k in 0..HIDDEN {
            let r = t.sigmoid(((mac(k) + self.w_ih[k] * x) >> 21) + self.b_ih[k] + self.b_hh[k]);
            let z = t.sigmoid(((mac(16 + k) + self.w_ih[16 + k] * x) >> 21) + self.b_ih[16 + k] + self.b_hh[16 + k]);
            let hn = (mac(32 + k) >> 21) + self.b_hh[32 + k];
            // Both products summed in 64 bits before the shift, as the app does.
            let n = t.tanh(self.b_ih[32 + k] + ((self.w_ih[32 + k] * x + r * hn) >> 21));
            next[k] = (((h[k] - n) * z) >> 21) + n;
        }
        self.h = next;
    }
}

/// The 68-byte LMT block for an AM4 model.
pub fn lmt_block(model: &[u8]) -> Result<[u8; LMT_LEN], String> {
    if model.len() < WEIGHTS + 929 * 4 {
        return Err("arquivo de amp curto demais".into());
    }
    let f: Vec<f32> = model[WEIGHTS..WEIGHTS + 929 * 4]
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    if f.iter().any(|v| !v.is_finite()) {
        return Err("o modelo tem valores inválidos".into());
    }
    let tables = Tables::new();
    let mut gru = Gru::from_floats(&f);
    // One second of a decay from 1 to about 0.001, as `calc_bias_gru_am4`
    // feeds it: expf(i * -6.908 * (1/22050)), each step rounded to f32.
    let c1 = f32::from_bits(0xc0dd_0c55);
    let c2 = f32::from_bits(0x383e_37c6);
    for i in 0..44_100u32 {
        let arg = (i as f32 * c1) * c2;
        let input = (arg as f64).exp() as f32;
        gru.step(&tables, input);
    }
    let mut out = [0u8; LMT_LEN];
    out[..4].copy_from_slice(b"LMT\0");
    for (k, v) in gru.h.iter().enumerate() {
        let f = (*v as f32) * f32::from_bits(0x3500_0000); // × 2⁻²¹
        out[4 + 4 * k..8 + 4 * k].copy_from_slice(&f.to_le_bytes());
    }
    Ok(out)
}

/// A parsed `.am4data` file.
#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    bytes: Vec<u8>,
}

impl Model {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 4 {
            return Err("arquivo vazio".into());
        }
        match &bytes[..4] {
            m if m == MAGIC_AM4 => {}
            b"AM2\0" | b"AM3\0" => {
                return Err(format!(
                    "modelo {} ainda não suportado; só AM4 por enquanto",
                    String::from_utf8_lossy(&bytes[..3])
                ))
            }
            _ => return Err("não é um modelo de amp (.am4data)".into()),
        }
        // A file that already carries its LMT (e.g. read back from a slot).
        let len = if bytes.len() >= RECORD_LEN && &bytes[AM4_LEN..AM4_LEN + 4] == b"LMT\0" {
            AM4_LEN
        } else if bytes.len() == AM4_LEN {
            AM4_LEN
        } else {
            return Err(format!("tamanho inesperado: {} bytes (esperado {AM4_LEN})", bytes.len()));
        };
        let end = bytes[4..4 + NAME_LEN].iter().position(|b| *b == 0).unwrap_or(NAME_LEN);
        let name = String::from_utf8_lossy(&bytes[4..4 + end]).trim().to_string();
        Ok(Self { name, bytes: bytes[..len].to_vec() })
    }

    /// The record the pedal receives: the model with `name` (if given) and its LMT.
    pub fn record(&self, name: Option<&str>) -> Result<Vec<u8>, String> {
        let mut r = self.bytes.clone();
        if let Some(name) = name {
            let field = &mut r[4..4 + NAME_LEN];
            field.fill(0);
            for (i, c) in name.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').take(NAME_LEN).enumerate() {
                field[i] = c as u8;
            }
        }
        r.extend_from_slice(&lmt_block(&r)?);
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic model (deterministic pseudo-random weights). The expected LMT
    /// was produced by the app's own `calc_bias_gru_am4` under the emulator
    /// (`tools/lmt_reference.py`), not by this code.
    fn synthetic() -> Vec<u8> {
        let mut m = vec![0u8; AM4_LEN];
        m[..4].copy_from_slice(MAGIC_AM4);
        m[4..12].copy_from_slice(b"TestAmp1");
        let mut seed = 0x1234_5678u32;
        for i in 0..929 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let v = ((seed >> 8) as f32 / 16_777_216.0 - 0.5) * 0.6;
            m[WEIGHTS + 4 * i..WEIGHTS + 4 * i + 4].copy_from_slice(&v.to_le_bytes());
        }
        m
    }

    #[test]
    fn tables_match_the_app() {
        let t = Tables::new();
        // Spot values read from the app's library.
        assert_eq!(&t.sig[..3], &[0, 1, 1]);
        assert_eq!(&t.sig[255..258], &[4_063_274, 4_194_304, 4_325_333]);
        assert_eq!(&t.sig_d[255..258], &[131_030, 131_029, 130_774]);
        assert_eq!(&t.tanh[255..258], &[-523_606, 0, 523_606]);
        assert_eq!(&t.tanh_d[255..258], &[523_606, 523_606, 519_542]);
        assert_eq!(&t.sig_d[510..], &[0, 1]);
        assert_eq!(&t.tanh_d[510..], &[0, 0]);
    }

    #[test]
    fn lmt_matches_the_app_library() {
        let lmt = lmt_block(&synthetic()).unwrap();
        let hex: String = lmt.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, EXPECTED_SYNTHETIC_LMT);
    }

    #[test]
    fn record_appends_lmt_and_renames() {
        let model = Model::parse(&synthetic()).unwrap();
        assert_eq!(model.name, "TestAmp1");
        let r = model.record(Some("Meu Amp")).unwrap();
        assert_eq!(r.len(), RECORD_LEN);
        assert_eq!(&r[4..11], b"Meu Amp");
        assert_eq!(&r[AM4_LEN..AM4_LEN + 4], b"LMT\0");
        // Parsing a record back drops the old LMT.
        assert_eq!(Model::parse(&r).unwrap().record(Some("Meu Amp")).unwrap(), r);
    }

    #[test]
    fn rejects_other_files() {
        assert!(Model::parse(b"RIFF....").is_err());
        let mut am2 = synthetic();
        am2[..4].copy_from_slice(b"AM2\0");
        assert!(Model::parse(&am2).is_err());
    }

    const EXPECTED_SYNTHETIC_LMT: &str = "4c4d540080e6b93d802c7cbd806abe3d00066d3d80b1ccbd8084f83d8029cf3d60ab303ec0bf263e80aef7bdd007a6be006629bc00c4ed3c1082bc3e40cd0ebe20b0793e";
}
