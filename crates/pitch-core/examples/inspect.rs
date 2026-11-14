//! Dev tool: prints the real-time analysis frames of a WAV over a time range.
//! `cargo run --example inspect -- file.wav [from_s] [to_s] [retune_ms]`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use solplugs_pitch_core::realtime::{Config, Params, Range, Realtime};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut r = hound::WavReader::open(&args[0]).unwrap();
    let spec = r.spec();
    let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
    let x: Vec<f32> = r
        .samples::<i32>()
        .map(|s| s.unwrap() as f32 * scale)
        .collect();
    let sr = spec.sample_rate as f32;
    let from: f32 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(0.0);
    let to: f32 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(1e9);
    let mut p = Params::default();
    if let Some(ms) = args.get(3).and_then(|a| a.parse().ok()) {
        p.correct.retune_ms = ms;
    }
    let mut rt = Realtime::new(Config {
        sample_rate: sr,
        range: Range::Mid,
    });
    let mut buf = x.clone();
    for chunk in buf.chunks_mut(256) {
        let mut ch = [chunk];
        rt.process(&mut ch, &p, &mut |f| {
            let t = f.time as f32 / sr;
            if t >= from && t <= to {
                println!(
                    "{t:7.4}s in {:6.2} out {:6.2} note {:3} shift {:+.2} clar {:.2} rms {:.4}",
                    f.corrected.input,
                    f.corrected.output,
                    f.corrected.note,
                    f.corrected.shift,
                    f.clarity,
                    f.rms
                );
            }
        });
    }
}
