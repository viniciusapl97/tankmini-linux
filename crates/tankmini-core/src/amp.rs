//! AMP model files (`.am2data`, `.am3data`, `.am4data`): the record the pedal
//! stores in an AMP slot.
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
//! Model layout (PyTorch `GRU(1, 16)` + `Linear(16, 1)`), floats from
//! [`Format::weights`]: W_ih[48], W_hh[48×16], then for AM4 b_ih[48] and
//! b_hh[48] (AM2/AM3 have no GRU biases: `calc_bias_gru`, weights at +0x43c),
//! then the output layer (not needed here). Gates in PyTorch order r, z, n.
//!
//! AM2 is what this pedal's own slots hold (the factory `am3` blob). The app
//! also rewrites the first four bytes to the magic of the file's extension:
//! some community `.am2data` files carry a broken header (**verified** by
//! auditioning one from the app and reading the buffer back).

pub const MAGIC_AM4: &[u8; 4] = b"AM4\0";
/// Bytes of an AM4 model before its LMT block.
pub const AM4_LEN: usize = 0x1ebc;
/// Bytes of an AM2 or AM3 model before its LMT block.
pub const AM2_LEN: usize = 0x183c;
pub const LMT_LEN: usize = 68;
pub const RECORD_LEN: usize = AM4_LEN + LMT_LEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// The pedal's own format (its factory amps).
    Am2,
    Am3,
    Am4,
}

impl Format {
    pub fn magic(self) -> &'static [u8; 4] {
        match self {
            Format::Am2 => b"AM2\0",
            Format::Am3 => b"AM3\0",
            Format::Am4 => MAGIC_AM4,
        }
    }

    /// Model bytes before the LMT block.
    pub fn len(self) -> usize {
        match self {
            Format::Am4 => AM4_LEN,
            _ => AM2_LEN,
        }
    }

    fn weights(self) -> usize {
        match self {
            Format::Am4 => 0x83c,
            _ => 0x43c,
        }
    }

    fn has_bias(self) -> bool {
        self == Format::Am4
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Am2 => "AM2",
            Format::Am3 => "AM3",
            Format::Am4 => "AM4",
        }
    }

    /// From a file name: `x.am2data`, `x.am3Data`, `x.am4data`.
    pub fn from_file_name(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        [Format::Am2, Format::Am3, Format::Am4]
            .into_iter()
            .find(|f| lower.ends_with(&format!(".am{}data", &f.label()[2..])))
    }

    fn from_magic(bytes: &[u8]) -> Option<Self> {
        [Format::Am2, Format::Am3, Format::Am4].into_iter().find(|f| bytes.starts_with(f.magic()))
    }
}
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
    fn from_floats(f: &[f32], bias: bool) -> Self {
        let mut g = Gru { w_ih: [0; 48], w_hh: [[0; HIDDEN]; 48], b_ih: [0; 48], b_hh: [0; 48], h: [0; HIDDEN] };
        for r in 0..48 {
            g.w_ih[r] = q21(f[r]);
            for c in 0..HIDDEN {
                g.w_hh[r][c] = q21(f[48 + r * HIDDEN + c]);
            }
            if bias {
                g.b_ih[r] = q21(f[816 + r]);
                g.b_hh[r] = q21(f[864 + r]);
            }
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

/// The 68-byte LMT block for a model of `format`.
pub fn lmt_block(format: Format, model: &[u8]) -> Result<[u8; LMT_LEN], String> {
    let start = format.weights();
    let count = if format.has_bias() { 912 } else { 816 };
    if model.len() < start + count * 4 {
        return Err("arquivo de amp curto demais".into());
    }
    let f: Vec<f32> = model[start..start + count * 4]
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    if f.iter().any(|v| !v.is_finite()) {
        return Err("o modelo tem valores inválidos".into());
    }
    let tables = Tables::new();
    let mut gru = Gru::from_floats(&f, format.has_bias());
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

/// A parsed model file.
#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    pub format: Format,
    bytes: Vec<u8>,
}

impl Model {
    /// Parse by content alone (the magic must be valid).
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        Self::parse_named(bytes, None)
    }

    /// Parse with the file name as a hint: like the app, the extension wins
    /// over a broken magic.
    pub fn parse_named(bytes: &[u8], file_name: Option<&str>) -> Result<Self, String> {
        if bytes.len() < 4 + NAME_LEN {
            return Err("arquivo vazio ou curto demais".into());
        }
        let format = file_name
            .and_then(Format::from_file_name)
            .or_else(|| Format::from_magic(bytes))
            .ok_or("não é um modelo de amp (.am2data, .am3data ou .am4data)")?;
        let len = format.len();
        // A file that already carries its LMT (e.g. read back from a slot).
        let has_lmt = bytes.len() >= len + LMT_LEN && &bytes[len..len + 4] == b"LMT\0";
        if bytes.len() != len && !has_lmt {
            return Err(format!(
                "tamanho inesperado para {}: {} bytes (esperado {len})",
                format.label(),
                bytes.len()
            ));
        }
        let mut model = bytes[..len].to_vec();
        model[..4].copy_from_slice(format.magic());
        let end = model[4..4 + NAME_LEN].iter().position(|b| *b == 0).unwrap_or(NAME_LEN);
        let name = String::from_utf8_lossy(&model[4..4 + end]).trim().to_string();
        Ok(Self { name, format, bytes: model })
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
        r.extend_from_slice(&lmt_block(self.format, &r)?);
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
            m[0x83c + 4 * i..0x83c + 4 * i + 4].copy_from_slice(&v.to_le_bytes());
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
        let lmt = lmt_block(Format::Am4, &synthetic()).unwrap();
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
        assert!(Model::parse(b"RIFF..............").is_err());
        // AM2 is shorter than AM4.
        let mut wrong = synthetic();
        wrong[..4].copy_from_slice(b"AM2\0");
        assert!(Model::parse(&wrong).is_err());
    }

    /// AM2: no GRU biases, weights at +0x43c, 6204 bytes. Expected value from
    /// the app's `calc_bias_gru` under the emulator.
    fn synthetic_am2() -> Vec<u8> {
        let mut m = vec![0u8; AM2_LEN];
        // A broken header, as some community files have: the extension decides.
        m[2] = b'2';
        let mut seed = 0x0bad_cafeu32;
        for i in 0..900 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let v = ((seed >> 8) as f32 / 16_777_216.0 - 0.5) * 0.6;
            m[0x43c + 4 * i..0x43c + 4 * i + 4].copy_from_slice(&v.to_le_bytes());
        }
        m
    }

    #[test]
    fn am2_record_fixes_the_magic_and_matches_the_app() {
        let model = Model::parse_named(&synthetic_am2(), Some("Boss IR-2.am2Data")).unwrap();
        assert_eq!(model.format, Format::Am2);
        let r = model.record(None).unwrap();
        assert_eq!(r.len(), AM2_LEN + LMT_LEN);
        assert_eq!(&r[..4], b"AM2\0");
        let hex: String = r[AM2_LEN..].iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, EXPECTED_SYNTHETIC_AM2_LMT);
    }

    #[test]
    fn format_from_file_name() {
        assert_eq!(Format::from_file_name("a.am2data"), Some(Format::Am2));
        assert_eq!(Format::from_file_name("b.AM3Data"), Some(Format::Am3));
        assert_eq!(Format::from_file_name("c.am4data"), Some(Format::Am4));
        assert_eq!(Format::from_file_name("d.wav"), None);
    }

    const EXPECTED_SYNTHETIC_AM2_LMT: &str = "4c4d5400000080b5000000b500000000000000b5000000b5000080b5000000b5000000b5000000b50000c0b500000000000000b5000000b5000080b5000080b500000000";

    const EXPECTED_SYNTHETIC_LMT: &str = "4c4d540080e6b93d802c7cbd806abe3d00066d3d80b1ccbd8084f83d8029cf3d60ab303ec0bf263e80aef7bdd007a6be006629bc00c4ed3c1082bc3e40cd0ebe20b0793e";
}
