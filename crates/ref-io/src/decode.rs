//! Decodes WAV, FLAC and MP3 into stereo `f32`, from a file or from memory.

use std::io::Cursor;
use std::path::Path;

/// Longest audio we decode (minutes); anything longer is refused rather than filling memory.
pub const MAX_MINUTES: f64 = 20.0;

/// Stereo audio.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Audio {
    pub sample_rate: f64,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl Audio {
    /// Length in seconds.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        if self.sample_rate > 0.0 {
            self.left.len() as f64 / self.sample_rate
        } else {
            0.0
        }
    }

    fn push_frame(&mut self, l: f32, r: f32) {
        self.left.push(l);
        self.right.push(r);
    }

    fn check_len(&self) -> Result<(), DecodeError> {
        if self.left.len() as f64 > self.sample_rate.max(8000.0) * 60.0 * MAX_MINUTES {
            Err(DecodeError::TooLong)
        } else {
            Ok(())
        }
    }
}

/// Why decoding failed.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("can't read the file: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported format (use WAV, FLAC or MP3)")]
    Unsupported,
    #[error("the audio is damaged or not what its name says: {0}")]
    Corrupt(String),
    #[error("the audio is longer than {MAX_MINUTES} minutes")]
    TooLong,
    #[error("the file holds no audio")]
    Empty,
}

/// The container a byte stream holds, sniffed from its first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Wav,
    Flac,
    Mp3,
}

/// Sniffs the format.
#[must_use]
pub fn sniff(head: &[u8]) -> Option<Format> {
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE" {
        Some(Format::Wav)
    } else if head.starts_with(b"fLaC") {
        Some(Format::Flac)
    } else if head.starts_with(b"ID3")
        || (head.len() >= 2 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0)
    {
        Some(Format::Mp3)
    } else {
        None
    }
}

/// Decodes a file.
pub fn decode_file(path: &Path) -> Result<Audio, DecodeError> {
    let meta = std::fs::metadata(path)?;
    // A 20-minute 32-bit 192 kHz stereo WAV is ~1.8 GB; refuse anything larger outright.
    if meta.len() > 2_000_000_000 {
        return Err(DecodeError::TooLong);
    }
    let bytes = std::fs::read(path)?;
    decode_bytes(&bytes)
}

/// Decodes audio held in memory.
pub fn decode_bytes(bytes: &[u8]) -> Result<Audio, DecodeError> {
    let format = sniff(bytes).ok_or(DecodeError::Unsupported)?;
    // Decoders are third-party code fed untrusted bytes. A panic in one must become an
    // error here, never unwind into a host's audio app.
    let decoded = std::panic::catch_unwind(|| match format {
        Format::Wav => decode_wav(bytes),
        Format::Flac => decode_flac(bytes),
        Format::Mp3 => decode_mp3(bytes),
    });
    let audio =
        decoded.map_err(|_| DecodeError::Corrupt("the decoder failed on this file".into()))??;
    if audio.left.is_empty() || audio.sample_rate.is_nan() || audio.sample_rate < 8000.0 {
        return Err(DecodeError::Empty);
    }
    Ok(audio)
}

fn corrupt(e: impl std::fmt::Display) -> DecodeError {
    DecodeError::Corrupt(e.to_string())
}

fn decode_wav(bytes: &[u8]) -> Result<Audio, DecodeError> {
    let mut r = hound::WavReader::new(Cursor::new(bytes)).map_err(corrupt)?;
    let spec = r.spec();
    let ch = usize::from(spec.channels.max(1));
    let mut a = Audio {
        sample_rate: f64::from(spec.sample_rate),
        ..Audio::default()
    };
    let mut frame = Vec::with_capacity(ch);
    let mut push = |a: &mut Audio, x: f32| -> Result<(), DecodeError> {
        frame.push(x);
        if frame.len() == ch {
            let (l, rr) = (frame[0], if ch > 1 { frame[1] } else { frame[0] });
            a.push_frame(l, rr);
            frame.clear();
            if a.left.len().is_multiple_of(65_536) {
                a.check_len()?;
            }
        }
        Ok(())
    };
    match spec.sample_format {
        hound::SampleFormat::Float => {
            for s in r.samples::<f32>() {
                push(&mut a, s.map_err(corrupt)?)?;
            }
        }
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(8, 32) - 1)) as f32;
            for s in r.samples::<i32>() {
                push(&mut a, s.map_err(corrupt)? as f32 * scale)?;
            }
        }
    }
    a.check_len()?;
    Ok(a)
}

fn decode_flac(bytes: &[u8]) -> Result<Audio, DecodeError> {
    let mut r = claxon::FlacReader::new(Cursor::new(bytes)).map_err(corrupt)?;
    let info = r.streaminfo();
    let ch = info.channels.max(1) as usize;
    let scale = 1.0 / (1u64 << (info.bits_per_sample.clamp(8, 32) - 1)) as f32;
    let mut a = Audio {
        sample_rate: f64::from(info.sample_rate),
        ..Audio::default()
    };
    let mut frame = Vec::with_capacity(ch);
    for s in r.samples() {
        frame.push(s.map_err(corrupt)? as f32 * scale);
        if frame.len() == ch {
            let (l, rr) = (frame[0], if ch > 1 { frame[1] } else { frame[0] });
            a.push_frame(l, rr);
            frame.clear();
            if a.left.len().is_multiple_of(65_536) {
                a.check_len()?;
            }
        }
    }
    a.check_len()?;
    Ok(a)
}

fn decode_mp3(bytes: &[u8]) -> Result<Audio, DecodeError> {
    let d = nanomp3::decode_all::<f32>(bytes);
    let ch = match d.channels {
        Some(c) => usize::from(c.num()),
        None => return Err(DecodeError::Empty),
    };
    let mut a = Audio {
        sample_rate: f64::from(d.sample_rate),
        ..Audio::default()
    };
    for frame in d.samples.chunks_exact(ch) {
        a.push_frame(frame[0], if ch > 1 { frame[1] } else { frame[0] });
        if a.left.len().is_multiple_of(65_536) {
            a.check_len()?;
        }
    }
    a.check_len()?;
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_round_trip_and_mono_is_duplicated() -> Result<(), Box<dyn std::error::Error>> {
        let mut buf = Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut w = hound::WavWriter::new(&mut buf, spec)?;
            for i in 0..44_100 {
                w.write_sample(((i % 100) as i16 - 50) * 300)?;
            }
            w.finalize()?;
        }
        let a = decode_bytes(buf.get_ref())?;
        assert_eq!(a.sample_rate, 44_100.0);
        assert_eq!(a.left.len(), 44_100);
        assert_eq!(a.left, a.right);
        assert!((a.left[0] - (-50.0 * 300.0 / 32_768.0)).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn junk_is_refused() {
        assert!(matches!(
            decode_bytes(b"hello world"),
            Err(DecodeError::Unsupported)
        ));
        assert!(decode_bytes(b"RIFF\0\0\0\0WAVEjunk").is_err());
        assert!(decode_bytes(b"fLaC\0\0").is_err());
    }

    /// A real LAME-encoded MP3: 1 s of a 1 kHz sine at 44.1 kHz stereo.
    #[test]
    fn decodes_a_real_mp3() -> Result<(), Box<dyn std::error::Error>> {
        let bytes = include_bytes!("../testdata/sine-1k-1s.mp3");
        let a = decode_bytes(bytes)?;
        assert!((a.sample_rate - 44_100.0).abs() < 1.0);
        assert!((a.seconds() - 1.0).abs() < 0.05, "{} s", a.seconds());
        let peak = a.left.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        // ffmpeg decodes this file to a -21.5 dBFS peak (0.084).
        assert!((peak - 0.084).abs() < 0.005, "peak {peak}");
        // Count rising zero crossings over the middle half: about 500 for 1 kHz.
        let mid = &a.left[a.left.len() / 4..a.left.len() * 3 / 4];
        let ups = mid.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((480..=520).contains(&ups), "{ups} crossings");
        Ok(())
    }

    /// Truncated and bit-flipped MP3s must come back as an error or audio, never a panic.
    #[test]
    fn damaged_mp3_never_panics() {
        let good = include_bytes!("../testdata/sine-1k-1s.mp3");
        for cut in [10, 400, 1_000, 5_000] {
            let _ = decode_bytes(&good[..cut.min(good.len())]);
        }
        let mut bad = good.to_vec();
        let mut x: u32 = 0x1234_5678;
        for _ in 0..200 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let i = 4 + (x as usize % (bad.len() - 4));
            bad[i] ^= (x >> 24) as u8;
        }
        let _ = decode_bytes(&bad);
    }
}
