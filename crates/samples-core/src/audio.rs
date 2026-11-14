//! Audio in memory: decoding WAV, AIFF, FLAC and MP3, and writing WAV.
//!
//! Mono stays mono and stereo stays stereo (files with more channels keep the first two), so
//! an edited version has the same shape as its raw.

use std::io::Cursor;
use std::path::Path;

use crate::model::FileType;

/// Longest audio decoded, minutes. Samples are short; this keeps a stray album rip from
/// filling memory.
pub const MAX_MINUTES: f64 = 20.0;
/// Largest file read, bytes.
pub const MAX_BYTES: u64 = 2_000_000_000;

/// Decoded audio: 1 or 2 channels of `f32`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Audio {
    pub sample_rate: f64,
    pub channels: Vec<Vec<f32>>,
}

impl Audio {
    /// Audio from channel buffers (all the same length).
    #[must_use]
    pub fn new(sample_rate: f64, channels: Vec<Vec<f32>>) -> Self {
        Self {
            sample_rate,
            channels,
        }
    }

    /// Frames per channel.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    /// Length in seconds.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        if self.sample_rate > 0.0 {
            self.frames() as f64 / self.sample_rate
        } else {
            0.0
        }
    }

    /// The channels folded to mono.
    #[must_use]
    pub fn mono(&self) -> Vec<f32> {
        let n = self.frames();
        let k = self.channels.len().max(1) as f32;
        (0..n)
            .map(|i| self.channels.iter().map(|c| c[i]).sum::<f32>() / k)
            .collect()
    }

    /// Frames `start..end` (clamped) as new audio.
    #[must_use]
    pub fn slice(&self, start: usize, end: usize) -> Self {
        let end = end.min(self.frames());
        let start = start.min(end);
        Self {
            sample_rate: self.sample_rate,
            channels: self
                .channels
                .iter()
                .map(|c| c[start..end].to_vec())
                .collect(),
        }
    }

    /// Absolute peak over all channels.
    #[must_use]
    pub fn peak(&self) -> f32 {
        self.channels
            .iter()
            .flat_map(|c| c.iter())
            .fold(0.0f32, |m, v| m.max(v.abs()))
    }

    /// Seconds to a frame index (clamped).
    #[must_use]
    pub fn frame_at(&self, sec: f64) -> usize {
        ((sec.max(0.0) * self.sample_rate) as usize).min(self.frames())
    }
}

/// Why decoding failed.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("can't read the file: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a WAV, AIFF, FLAC or MP3 file")]
    Unsupported,
    #[error("the audio is damaged: {0}")]
    Corrupt(String),
    #[error("the audio is longer than {MAX_MINUTES} minutes")]
    TooLong,
    #[error("the file holds no audio")]
    Empty,
}

fn corrupt(e: impl std::fmt::Display) -> DecodeError {
    DecodeError::Corrupt(e.to_string())
}

/// Sniffs the container from the first bytes.
#[must_use]
pub fn sniff(head: &[u8]) -> Option<FileType> {
    if head.len() >= 12
        && (&head[0..4] == b"RIFF" || &head[0..4] == b"RF64")
        && &head[8..12] == b"WAVE"
    {
        Some(FileType::Wav)
    } else if head.len() >= 12
        && &head[0..4] == b"FORM"
        && (&head[8..12] == b"AIFF" || &head[8..12] == b"AIFC")
    {
        Some(FileType::Aiff)
    } else if head.starts_with(b"fLaC") {
        Some(FileType::Flac)
    } else if head.starts_with(b"ID3")
        || (head.len() >= 2 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0)
    {
        Some(FileType::Mp3)
    } else {
        None
    }
}

/// Decodes a file.
pub fn decode_file(path: &Path) -> Result<Audio, DecodeError> {
    if std::fs::metadata(path)?.len() > MAX_BYTES {
        return Err(DecodeError::TooLong);
    }
    decode_bytes(&std::fs::read(path)?)
}

/// Decodes audio in memory. Decoders are third-party code fed untrusted bytes: a panic in one
/// becomes an error here and never unwinds into a host.
pub fn decode_bytes(bytes: &[u8]) -> Result<Audio, DecodeError> {
    let format = sniff(bytes).ok_or(DecodeError::Unsupported)?;
    let decoded = std::panic::catch_unwind(|| match format {
        FileType::Wav => decode_wav(bytes),
        FileType::Aiff => crate::aiff::decode(bytes),
        FileType::Flac => decode_flac(bytes),
        FileType::Mp3 => decode_mp3(bytes),
    })
    .map_err(|_| DecodeError::Corrupt("the decoder failed on this file".into()))??;
    if decoded.frames() == 0 || decoded.sample_rate.is_nan() || decoded.sample_rate < 1000.0 {
        return Err(DecodeError::Empty);
    }
    Ok(decoded)
}

/// Collects interleaved samples into at most two channels, checking length as it goes.
pub(crate) struct Deinterleave {
    src_channels: usize,
    max_frames: usize,
    frame: usize,
    out: Vec<Vec<f32>>,
}

impl Deinterleave {
    pub(crate) fn new(src_channels: usize, sample_rate: f64) -> Self {
        let src_channels = src_channels.max(1);
        Self {
            src_channels,
            max_frames: (sample_rate.max(8000.0) * 60.0 * MAX_MINUTES) as usize,
            frame: 0,
            out: vec![Vec::new(); src_channels.min(2)],
        }
    }

    pub(crate) fn push(&mut self, v: f32) -> Result<(), DecodeError> {
        if self.frame < self.out.len() {
            self.out[self.frame].push(v);
        }
        self.frame += 1;
        if self.frame == self.src_channels {
            self.frame = 0;
            if self.out[0].len() > self.max_frames {
                return Err(DecodeError::TooLong);
            }
        }
        Ok(())
    }

    pub(crate) fn finish(mut self, sample_rate: f64) -> Audio {
        let n = self.out.iter().map(Vec::len).min().unwrap_or(0);
        for c in &mut self.out {
            c.truncate(n);
        }
        Audio::new(sample_rate, self.out)
    }
}

fn decode_wav(bytes: &[u8]) -> Result<Audio, DecodeError> {
    let mut r = hound::WavReader::new(Cursor::new(bytes)).map_err(corrupt)?;
    let spec = r.spec();
    let sr = f64::from(spec.sample_rate);
    let mut d = Deinterleave::new(usize::from(spec.channels), sr);
    match spec.sample_format {
        hound::SampleFormat::Float => {
            for s in r.samples::<f32>() {
                d.push(s.map_err(corrupt)?)?;
            }
        }
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(8, 32) - 1)) as f32;
            for s in r.samples::<i32>() {
                d.push(s.map_err(corrupt)? as f32 * scale)?;
            }
        }
    }
    Ok(d.finish(sr))
}

fn decode_flac(bytes: &[u8]) -> Result<Audio, DecodeError> {
    let mut r = claxon::FlacReader::new(Cursor::new(bytes)).map_err(corrupt)?;
    let info = r.streaminfo();
    let sr = f64::from(info.sample_rate);
    let scale = 1.0 / (1u64 << (info.bits_per_sample.clamp(8, 32) - 1)) as f32;
    let mut d = Deinterleave::new(info.channels as usize, sr);
    for s in r.samples() {
        d.push(s.map_err(corrupt)? as f32 * scale)?;
    }
    Ok(d.finish(sr))
}

fn decode_mp3(bytes: &[u8]) -> Result<Audio, DecodeError> {
    let out = nanomp3::decode_all::<f32>(bytes);
    let ch = out
        .channels
        .map(|c| usize::from(c.num()))
        .ok_or(DecodeError::Empty)?;
    let sr = f64::from(out.sample_rate);
    let mut d = Deinterleave::new(ch, sr);
    for &s in &out.samples {
        d.push(s)?;
    }
    Ok(d.finish(sr))
}

/// Bit depth for writing an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavDepth {
    Int16,
    Int24,
    Float32,
}

impl WavDepth {
    /// The closest depth to a source's (24-bit when unknown, e.g. MP3).
    #[must_use]
    pub fn like(bits: Option<u8>, float: bool) -> Self {
        match bits {
            _ if float => Self::Float32,
            Some(16) | Some(8) => Self::Int16,
            Some(32) => Self::Float32,
            _ => Self::Int24,
        }
    }

    /// Bits.
    #[must_use]
    pub fn bits(self) -> u8 {
        match self {
            Self::Int16 => 16,
            Self::Int24 => 24,
            Self::Float32 => 32,
        }
    }
}

/// Writes `audio` as a WAV file (to a temporary name first, so a failed write leaves nothing).
pub fn write_wav(path: &Path, audio: &Audio, depth: WavDepth) -> Result<(), std::io::Error> {
    let ch = audio.channels.len().clamp(1, 2);
    let spec = hound::WavSpec {
        channels: ch as u16,
        sample_rate: audio.sample_rate.round().max(1.0) as u32,
        bits_per_sample: u16::from(depth.bits()),
        sample_format: if depth == WavDepth::Float32 {
            hound::SampleFormat::Float
        } else {
            hound::SampleFormat::Int
        },
    };
    let tmp = path.with_extension("wav.part");
    let io = |e: hound::Error| std::io::Error::other(e.to_string());
    {
        let mut w = hound::WavWriter::create(&tmp, spec).map_err(io)?;
        for i in 0..audio.frames() {
            for c in 0..ch {
                let v = audio.channels[c][i].clamp(-1.0, 1.0);
                match depth {
                    WavDepth::Float32 => w.write_sample(v).map_err(io)?,
                    WavDepth::Int16 => w.write_sample((v * 32_767.0).round() as i16).map_err(io)?,
                    WavDepth::Int24 => w
                        .write_sample((v * 8_388_607.0).round() as i32)
                        .map_err(io)?,
                }
            }
        }
        w.finalize().map_err(io)?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_round_trip_keeps_mono_and_values() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("a.wav");
        let a = Audio::new(
            48_000.0,
            vec![(0..4800).map(|i| (i as f32 / 4800.0) - 0.5).collect()],
        );
        write_wav(&p, &a, WavDepth::Int24)?;
        let b = decode_file(&p)?;
        assert_eq!(b.channels.len(), 1);
        assert_eq!(b.frames(), 4800);
        assert!((b.channels[0][100] - a.channels[0][100]).abs() < 1e-5);
        Ok(())
    }

    #[test]
    fn junk_never_panics() {
        assert!(matches!(
            decode_bytes(b"hello"),
            Err(DecodeError::Unsupported)
        ));
        assert!(decode_bytes(b"RIFF\0\0\0\0WAVEjunk").is_err());
        assert!(decode_bytes(b"FORM\0\0\0\x04AIFF").is_err());
        assert!(decode_bytes(b"fLaC\0").is_err());
        assert!(decode_bytes(&[0xFF, 0xFB, 0, 0, 1, 2, 3]).is_err());
    }
}
