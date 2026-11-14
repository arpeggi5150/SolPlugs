//! A clean-room AIFF / AIFF-C reader, written from Apple's published format description:
//! big-endian PCM (8-32 bit), `sowt` little-endian PCM, and `fl32`/`fl64` floats.

use crate::audio::{Audio, DecodeError, Deinterleave};

fn bad(why: &str) -> DecodeError {
    DecodeError::Corrupt(format!("AIFF: {why}"))
}

fn be_u32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// The 80-bit IEEE extended float AIFF uses for the sample rate.
fn extended(b: &[u8]) -> f64 {
    let exp = (i32::from(b[0] & 0x7F) << 8) | i32::from(b[1]);
    let mut mant: u64 = 0;
    for &x in &b[2..10] {
        mant = (mant << 8) | u64::from(x);
    }
    if exp == 0 && mant == 0 {
        return 0.0;
    }
    let v = mant as f64 * 2f64.powi(exp - 16_383 - 63);
    if b[0] & 0x80 != 0 { -v } else { v }
}

#[derive(Clone, Copy, PartialEq)]
enum Coding {
    BigPcm,
    LittlePcm,
    UnsignedPcm,
    F32,
    F64,
}

struct Comm {
    channels: usize,
    frames: usize,
    bits: usize,
    rate: f64,
    coding: Coding,
}

fn parse_comm(c: &[u8], aifc: bool) -> Result<Comm, DecodeError> {
    if c.len() < 18 {
        return Err(bad("short COMM chunk"));
    }
    let channels = usize::from(u16::from_be_bytes([c[0], c[1]]));
    let frames = be_u32(&c[2..6]) as usize;
    let bits = usize::from(u16::from_be_bytes([c[6], c[7]]));
    let rate = extended(&c[8..18]);
    let coding = if aifc && c.len() >= 22 {
        match &c[18..22] {
            b"NONE" | b"twos" => Coding::BigPcm,
            b"sowt" => Coding::LittlePcm,
            b"raw " => Coding::UnsignedPcm,
            b"fl32" | b"FL32" => Coding::F32,
            b"fl64" | b"FL64" => Coding::F64,
            other => {
                return Err(bad(&format!(
                    "compression '{}' isn't supported",
                    String::from_utf8_lossy(other)
                )));
            }
        }
    } else {
        Coding::BigPcm
    };
    if channels == 0 || !(1..=64).contains(&bits) || !(1000.0..=1_000_000.0).contains(&rate) {
        return Err(bad("impossible format"));
    }
    Ok(Comm {
        channels,
        frames,
        bits,
        rate,
        coding,
    })
}

/// Decodes AIFF or AIFF-C bytes.
pub fn decode(bytes: &[u8]) -> Result<Audio, DecodeError> {
    if bytes.len() < 12 || &bytes[0..4] != b"FORM" {
        return Err(DecodeError::Unsupported);
    }
    let aifc = &bytes[8..12] == b"AIFC";
    let mut comm = None;
    let mut ssnd: Option<&[u8]> = None;
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = be_u32(&bytes[pos + 4..pos + 8]) as usize;
        let start = pos + 8;
        let end = start.saturating_add(size).min(bytes.len());
        let body = &bytes[start..end];
        match id {
            b"COMM" => comm = Some(parse_comm(body, aifc)?),
            b"SSND" if body.len() >= 8 => {
                let offset = be_u32(&body[0..4]) as usize;
                ssnd = Some(body.get(8 + offset..).unwrap_or(&[]));
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        pos = start.saturating_add(size).saturating_add(size & 1);
    }
    let comm = comm.ok_or_else(|| bad("no COMM chunk"))?;
    let data = ssnd.ok_or_else(|| bad("no sound data"))?;
    let width = match comm.coding {
        Coding::F32 => 4,
        Coding::F64 => 8,
        _ => comm.bits.div_ceil(8),
    };
    if width == 0 || width > 8 {
        return Err(bad("impossible sample size"));
    }
    let total = (data.len() / width / comm.channels).min(comm.frames) * comm.channels;
    let mut d = Deinterleave::new(comm.channels, comm.rate);
    let scale = 1.0 / (1u64 << (width * 8 - 1)) as f64;
    for k in 0..total {
        let s = &data[k * width..(k + 1) * width];
        let v = match comm.coding {
            Coding::F32 => f32::from_be_bytes([s[0], s[1], s[2], s[3]]),
            Coding::F64 => {
                f64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) as f32
            }
            Coding::UnsignedPcm if width == 1 => (f64::from(s[0]) - 128.0) as f32 / 128.0,
            Coding::LittlePcm => {
                let mut x: i64 = 0;
                for (i, &b) in s.iter().enumerate() {
                    x |= i64::from(b) << (8 * i);
                }
                let shift = 64 - 8 * width;
                ((x << shift) >> shift) as f64 as f32 * scale as f32
            }
            _ => {
                let mut x: i64 = 0;
                for &b in s {
                    x = (x << 8) | i64::from(b);
                }
                let shift = 64 - 8 * width;
                (((x << shift) >> shift) as f64 * scale) as f32
            }
        };
        d.push(v)?;
    }
    Ok(d.finish(comm.rate))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 80-bit extended from an integer rate (for building test files).
    fn ext(rate: u32) -> [u8; 10] {
        let mut out = [0u8; 10];
        let shift = rate.leading_zeros();
        let mant = u64::from(rate) << (32 + shift);
        let exp = 16_383 + 31 - shift;
        out[0] = (exp >> 8) as u8;
        out[1] = exp as u8;
        out[2..].copy_from_slice(&mant.to_be_bytes());
        out
    }

    /// A 16-bit big-endian AIFF.
    pub(crate) fn aiff16(rate: u32, channels: &[Vec<i16>]) -> Vec<u8> {
        let ch = channels.len();
        let n = channels[0].len();
        let mut comm = Vec::new();
        comm.extend_from_slice(&(ch as u16).to_be_bytes());
        comm.extend_from_slice(&(n as u32).to_be_bytes());
        comm.extend_from_slice(&16u16.to_be_bytes());
        comm.extend_from_slice(&ext(rate));
        let mut snd = vec![0u8; 8];
        for i in 0..n {
            for c in channels {
                snd.extend_from_slice(&c[i].to_be_bytes());
            }
        }
        let mut f = b"FORM\0\0\0\0AIFF".to_vec();
        for (id, body) in [(b"COMM", comm), (b"SSND", snd)] {
            f.extend_from_slice(id);
            f.extend_from_slice(&(body.len() as u32).to_be_bytes());
            f.extend_from_slice(&body);
            if body.len() % 2 == 1 {
                f.push(0);
            }
        }
        let size = (f.len() - 8) as u32;
        f[4..8].copy_from_slice(&size.to_be_bytes());
        f
    }

    #[test]
    fn reads_rate_and_stereo_pcm() -> Result<(), DecodeError> {
        let l: Vec<i16> = (0..1000).map(|i| (i * 30) as i16).collect();
        let r: Vec<i16> = l.iter().map(|v| -v).collect();
        let a = crate::audio::decode_bytes(&aiff16(44_100, &[l, r]))?;
        assert_eq!(a.sample_rate, 44_100.0);
        assert_eq!(a.channels.len(), 2);
        assert_eq!(a.frames(), 1000);
        assert!((a.channels[0][500] - 15_000.0 / 32_768.0).abs() < 1e-6);
        assert!((a.channels[1][500] + 15_000.0 / 32_768.0).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn extended_rates() {
        for r in [8000, 22_050, 44_100, 48_000, 96_000, 192_000] {
            assert_eq!(extended(&ext(r)), f64::from(r));
        }
    }

    #[test]
    fn truncated_never_panics() {
        let f = aiff16(48_000, &[vec![1, 2, 3, 4, 5]]);
        for cut in 0..f.len() {
            let _ = crate::audio::decode_bytes(&f[..cut]);
        }
    }
}
