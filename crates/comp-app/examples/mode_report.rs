//! Runs every mode over a real recording and writes the processed audio next to a report,
//! so each mode can be judged by ear and by numbers.
//!
//! `cargo run --release -p solplugs-comp-app --example mode_report -- take.wav out_dir`
//!
//! For each mode (its starter preset), and for Parallel smash: gain reduction (peak and
//! average), the post meter before and after MATCH, the biggest sample-to-sample jump
//! compared with the input's (a click shows up as a big ratio), and bypass identity.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;
use std::sync::Arc;

use solplugs_comp_app::engine::Engine;
use solplugs_comp_app::params::{G, Pid};
use solplugs_comp_app::presets::PRESETS;
use solplugs_comp_core::Mode;

fn load(path: &str) -> (Vec<f32>, Vec<f32>, f32) {
    let mut r = hound::WavReader::open(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let spec = r.spec();
    let ch = usize::from(spec.channels);
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.unwrap() as f32 * scale)
                .collect()
        }
        hound::SampleFormat::Float => r.samples::<f32>().map(|s| s.unwrap()).collect(),
    };
    let l = raw.chunks(ch).map(|c| c[0]).collect();
    let r = raw.chunks(ch).map(|c| c[c.len().min(2) - 1]).collect();
    (l, r, spec.sample_rate as f32)
}

fn save(dir: &str, name: &str, l: &[f32], r: &[f32], sr: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: sr as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(format!("{dir}/{name}"), spec).unwrap();
    for (a, b) in l.iter().zip(r) {
        w.write_sample(*a).unwrap();
        w.write_sample(*b).unwrap();
    }
    w.finalize().unwrap();
}

/// Plays the file through `e` in uneven host-like blocks. Returns the output and the
/// gain reduction seen (peak, mean over the columns with any reduction).
fn play(e: &Arc<Engine>, l: &[f32], r: &[f32], sr: f32) -> (Vec<f32>, Vec<f32>, f32, f32) {
    let mut p = e.processor(f64::from(sr), 1024);
    let (mut ol, mut or) = (l.to_vec(), r.to_vec());
    let sizes = [512usize, 441, 1024, 64, 777];
    let mut at = 0;
    let mut i = 0;
    let (mut peak, mut sum, mut n) = (0.0f32, 0.0f32, 0usize);
    while at < ol.len() {
        let len = sizes[i % sizes.len()].min(ol.len() - at);
        p.process(&mut [&mut ol[at..at + len], &mut or[at..at + len]], None);
        e.drain_columns(|c| {
            peak = peak.max(c.gr);
            if c.gr > 0.05 {
                sum += c.gr;
                n += 1;
            }
        });
        at += len;
        i += 1;
    }
    (ol, or, peak, sum / n.max(1) as f32)
}

/// Biggest sample-to-sample step.
fn max_step(x: &[f32]) -> f32 {
    x.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

fn row(
    out: &mut String,
    name: &str,
    e: &Arc<Engine>,
    (l, r, sr): (&[f32], &[f32], f32),
    dir: &str,
) {
    let (_, _, peak, mean) = play(e, l, r, sr);
    let (b0, a0) = (e.meters().avg_before(), e.meters().avg_after());
    e.match_now();
    let (ol, or, _, _) = play(e, l, r, sr);
    let (b1, a1) = (e.meters().avg_before(), e.meters().avg_after());
    let step = max_step(&ol) / max_step(l).max(1e-9);
    let lat = e.latency();
    let file = format!(
        "{}.wav",
        name.to_lowercase().replace(" · ", "-").replace(' ', "-")
    );
    save(dir, &file, &ol, &or, sr);
    let _ = writeln!(
        out,
        "| {name} | {lat} ({:.2} ms) | {peak:.1} | {mean:.1} | {b0:.1} / {a0:.1} | {b1:.1} / {a1:.1} ({:+.2}) | {step:.2} | `{file}` |",
        lat as f32 * 1000.0 / sr,
        a1 - b1
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let input = args.first().expect("usage: mode_report take.wav out_dir");
    let dir = args.get(1).map_or("mode_report", String::as_str);
    std::fs::create_dir_all(dir).unwrap();
    let (l, r, sr) = load(input);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Mode report\n\n`{input}`: {:.1} s at {sr} Hz\n",
        l.len() as f32 / sr
    );
    let _ = writeln!(
        out,
        "| Setup | Latency, samples | GR peak dB | GR mean dB | Avg peak before / after | After MATCH (after - before) | Max step vs input | File |\n|---|---|---|---|---|---|---|---|"
    );
    for m in Mode::ALL {
        let e = Arc::new(Engine::new());
        e.set_sample_rate(sr);
        let i = PRESETS.iter().position(|p| p.mode == m).unwrap();
        e.load_preset(i);
        row(&mut out, PRESETS[i].name, &e, (&l, &r, sr), dir);
    }
    for name in [
        "Vocals",
        "Drums",
        "Bass",
        "Mix bus",
        "Mastering",
        "Parallel smash",
    ] {
        let e = Arc::new(Engine::new());
        e.set_sample_rate(sr);
        e.load_preset(PRESETS.iter().position(|p| p.name == name).unwrap());
        row(&mut out, name, &e, (&l, &r, sr), dir);
    }
    // Bypass: output must be the input delayed by the latency, bit for bit.
    let _ = writeln!(out, "\n## Bypass\n");
    for m in Mode::ALL {
        let e = Arc::new(Engine::new());
        e.set_mode(m);
        e.set(Pid::g(G::Bypass), 1.0);
        // Let the 10 ms bypass fade finish before comparing.
        let (ol, _, _, _) = play(&e, &l, &r, sr);
        let lat = e.latency() as usize;
        let skip = (0.05 * sr) as usize;
        let diff = ol[skip + lat..]
            .iter()
            .zip(&l[skip..])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        let _ = writeln!(out, "- {}: max |out - delayed in| = {diff:e}", m.name());
    }
    std::fs::write(format!("{dir}/report.md"), &out).unwrap();
    println!("{out}");
}
