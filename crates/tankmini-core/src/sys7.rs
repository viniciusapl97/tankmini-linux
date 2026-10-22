//! 8→7 bit packing used by every M-VAVE SysEx payload.
//!
//! The payload is treated as one continuous LSB-first bit stream; output byte
//! `k` carries stream bits `7k..7k+6`. The final flush byte is always emitted,
//! so `n` raw bytes become `n + n/7 + 1` packed bytes.

/// Pack bytes into a 7-bit stream.
pub fn encode7(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 7 + 1);
    let mut accum: u32 = 0;
    let mut nbits: u32 = 0;
    for &b in data {
        accum |= (b as u32) << nbits;
        nbits += 8;
        while nbits >= 7 {
            out.push((accum & 0x7f) as u8);
            accum >>= 7;
            nbits -= 7;
        }
    }
    out.push((accum & 0x7f) as u8);
    out
}

/// Unpack a 7-bit stream. Returns `None` if a byte with the high bit is seen.
pub fn decode7(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len() * 7 / 8);
    let mut accum: u32 = 0;
    let mut nbits: u32 = 0;
    for &b in data {
        if b & 0x80 != 0 {
            return None;
        }
        accum |= (b as u32) << nbits;
        nbits += 7;
        while nbits >= 8 {
            out.push((accum & 0xff) as u8);
            accum >>= 8;
            nbits -= 8;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_formula() {
        for n in 0..40usize {
            let data = vec![0xAAu8; n];
            assert_eq!(encode7(&data).len(), n + n / 7 + 1, "n = {n}");
        }
    }

    #[test]
    fn all_output_bytes_are_7bit() {
        let data: Vec<u8> = (0..=255u8).collect();
        assert!(encode7(&data).iter().all(|b| b & 0x80 == 0));
    }

    #[test]
    fn round_trip() {
        for n in 0..64usize {
            let data: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            let packed = encode7(&data);
            let back = decode7(&packed).expect("valid 7-bit stream");
            assert_eq!(&back[..n], &data[..], "n = {n}");
        }
    }

    #[test]
    fn sync_word_packs_to_0032() {
        // Every message of this family starts with f0 00 32: the `00 59` sync
        // word packs to `00 32`.
        let packed = encode7(&[0x00, 0x59]);
        assert_eq!(packed[0], 0x00);
        assert_eq!(packed[1], 0x32);
    }
}
