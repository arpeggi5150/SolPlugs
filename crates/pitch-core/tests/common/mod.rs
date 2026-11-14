//! Shared helpers for the DSP tests: WAV loading, block-wise processing, pitch tracks,
//! spectral envelopes and click detection.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub use solplugs_pitch_core::metrics::*;
use solplugs_pitch_core::realtime::{Config, FrameInfo, Params, Range, Realtime};

/// Loads a test WAV as mono f32 and its sample rate.
pub fn load(name: &str) -> (Vec<f32>, f32) {
    let path = format!("{}/testdata/{name}", env!("CARGO_MANIFEST_DIR"));
    let mut r = hound::WavReader::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let spec = r.spec();
    let ch = spec.channels as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.unwrap() as f32 * scale)
                .collect()
        }
        hound::SampleFormat::Float => r.samples::<f32>().map(|s| s.unwrap()).collect(),
    };
    let mono = raw
        .chunks(ch)
        .map(|c| c.iter().sum::<f32>() / ch as f32)
        .collect();
    (mono, spec.sample_rate as f32)
}

/// Writes a mono WAV (for listening to test output; set SOLPLUGS_PITCH_DUMP=dir).
pub fn dump(name: &str, x: &[f32], sr: f32) {
    let Ok(dir) = std::env::var("SOLPLUGS_PITCH_DUMP") else {
        return;
    };
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sr as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(format!("{dir}/{name}.wav"), spec).unwrap();
    for &s in x {
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
}

/// Runs `x` through a [`Realtime`] in uneven blocks (as hosts do). Returns the output with
/// the latency removed (same length as `x`) and the analysis frames.
pub fn run(x: &[f32], sr: f32, range: Range, params: &Params) -> (Vec<f32>, Vec<FrameInfo>) {
    let mut rt = Realtime::new(Config {
        sample_rate: sr,
        range,
    });
    let lat = rt.latency();
    let mut buf: Vec<f32> = x.to_vec();
    buf.extend(std::iter::repeat_n(0.0, lat + 1024));
    let mut frames = Vec::new();
    let sizes = [512usize, 37, 256, 1024, 64, 333, 128];
    let mut pos = 0;
    let mut i = 0;
    while pos < buf.len() {
        let n = sizes[i % sizes.len()].min(buf.len() - pos);
        i += 1;
        let mut ch = [&mut buf[pos..pos + n]];
        rt.process(&mut ch, params, &mut |f| frames.push(*f));
        pos += n;
    }
    (buf[lat..lat + x.len()].to_vec(), frames)
}
