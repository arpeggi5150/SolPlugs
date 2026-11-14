//! Runs the brief's checks on any vocal take and writes the processed audio next to a
//! report, so a take can be judged by ear and by numbers.
//!
//! `cargo run --release -p solplugs-pitch-app --example vocal_report -- take.wav out_dir`
//!
//! Writes: slow.wav (retune 150 ms), hard.wav (retune 0), fifth-formant-on.wav,
//! fifth-formant-off.wav, graph-fixed.wav (most out-of-tune note dragged onto the grid)
//! and report.md.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Instant;

use solplugs_pitch_app::engine::{Engine, Transport};
use solplugs_pitch_app::params::P;
use solplugs_pitch_core::metrics::{
    cents, click_events, envelope, envelope_warp, new_clicks, percentile, track,
};
use solplugs_pitch_core::notes::{Formant, NoteEdits, analyse, render};
use solplugs_pitch_core::realtime::{Config, Params, Range, Realtime};
use solplugs_pitch_core::scale::{freq_to_midi, note_name};

fn load(path: &str) -> (Vec<f32>, f32) {
    let mut r = hound::WavReader::open(path).unwrap_or_else(|e| panic!("{path}: {e}"));
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

fn save(dir: &str, name: &str, x: &[f32], sr: f32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sr as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(format!("{dir}/{name}"), spec).unwrap();
    for &s in x {
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
}

/// Runs the live corrector over `x` in host-like uneven blocks; output aligned to input.
fn run(x: &[f32], sr: f32, range: Range, p: &Params) -> Vec<f32> {
    let mut rt = Realtime::new(Config {
        sample_rate: sr,
        range,
    });
    let lat = rt.latency();
    let mut buf = x.to_vec();
    buf.extend(std::iter::repeat_n(0.0, lat + 1024));
    let sizes = [512usize, 37, 256, 1024, 64, 333, 128];
    let (mut pos, mut i) = (0, 0);
    while pos < buf.len() {
        let n = sizes[i % sizes.len()].min(buf.len() - pos);
        i += 1;
        rt.process(&mut [&mut buf[pos..pos + n]], p, &mut |_| {});
        pos += n;
    }
    buf[lat..lat + x.len()].to_vec()
}

fn voiced_cents_off_grid(x: &[f32], sr: f32) -> Vec<f32> {
    track(x, sr, 256)
        .iter()
        .filter(|(_, f, c)| *c > 0.9 && *f > 0.0)
        .map(|(_, f, _)| {
            let m = freq_to_midi(*f);
            ((m - m.round()) * 100.0).abs()
        })
        .collect()
}

/// Off-grid of the note centres: the pitch with vibrato averaged out (a running median
/// over about 120 ms of clearly voiced frames). Slow retune should pull these in even
/// though it keeps the vibrato.
fn centre_cents_off_grid(x: &[f32], sr: f32) -> Vec<f32> {
    let t = track(x, sr, 256);
    let k = ((0.06 * sr) / 256.0).round().max(1.0) as usize;
    (k..t.len().saturating_sub(k))
        .filter_map(|i| {
            let w = &t[i - k..=i + k];
            if w.iter().any(|f| f.2 <= 0.9 || f.1 <= 0.0) {
                return None;
            }
            let m: Vec<f32> = w.iter().map(|f| freq_to_midi(f.1)).collect();
            let c = percentile(&m, 0.5);
            Some(((c - c.round()) * 100.0).abs())
        })
        .collect()
}

struct Movement {
    input: f32,
    output: f32,
    jitter_p50: f32,
    jitter_p90: f32,
    glitches: f32,
    hotspots: Vec<(f32, f32)>,
}

/// Frame-to-frame pitch movement of input and output, and of the applied shift
/// (warble). Frames where either track jumps more than 3 semitones are tracker
/// glitches (breaths, octave flips) and are counted, not averaged.
fn movement(x: &[f32], y: &[f32], sr: f32) -> Movement {
    let tx = track(x, sr, 256);
    let ty = track(y, sr, 256);
    let (mut dx, mut dy, mut jitter, mut at) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut glitch, mut total) = (0usize, 0usize);
    for i in 1..tx.len().min(ty.len()) {
        if tx[i].2 > 0.9 && tx[i - 1].2 > 0.9 && ty[i].2 > 0.9 && ty[i - 1].2 > 0.9 {
            total += 1;
            let (a, b) = (cents(tx[i].1, tx[i - 1].1), cents(ty[i].1, ty[i - 1].1));
            // Change of the applied shift from one frame to the next: warble shows here.
            let j = cents(ty[i].1, tx[i].1) - cents(ty[i - 1].1, tx[i - 1].1);
            if a.abs() > 300.0 || b.abs() > 300.0 || cents(ty[i].1, tx[i].1).abs() > 300.0 {
                glitch += 1;
                continue;
            }
            dx.push(a.abs());
            dy.push(b.abs());
            jitter.push(j.abs());
            at.push(tx[i].0 as f32 / sr);
        }
    }
    let mut hot: Vec<(f32, f32)> = at.iter().copied().zip(jitter.iter().copied()).collect();
    hot.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut hotspots: Vec<(f32, f32)> = Vec::new();
    for h in hot {
        if hotspots.len() == 5 {
            break;
        }
        if hotspots.iter().all(|o| (o.0 - h.0).abs() > 0.1) {
            hotspots.push(h);
        }
    }
    Movement {
        input: percentile(&dx, 0.5),
        output: percentile(&dy, 0.5),
        jitter_p50: percentile(&jitter, 0.5),
        jitter_p90: percentile(&jitter, 0.9),
        glitches: glitch as f32 / total.max(1) as f32 * 100.0,
        hotspots,
    }
}

fn clicks_line(x: &[f32], y: &[f32], sr: f32) -> String {
    let ev = click_events(x, y, sr, 5);
    if ev.is_empty() {
        return "none".into();
    }
    ev.iter()
        .map(|(t, s, db)| format!("{t:.2} s ({s:.1} at {db:.0} dBFS)"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (input, dir) = (&args[0], args.get(1).map_or(".", String::as_str));
    std::fs::create_dir_all(dir).unwrap();
    let (x, sr) = load(input);
    let mut r = String::new();
    let secs = x.len() as f32 / sr;
    writeln!(
        r,
        "# SolPlugs Pitch vocal report\n\nTake: `{input}`, {secs:.1} s at {sr} Hz\n"
    )
    .unwrap();

    // Range from the take's lowest sung notes.
    let tr = track(&x, sr, 256);
    let hz: Vec<f32> = tr.iter().filter(|t| t.2 > 0.9).map(|t| t.1).collect();
    let low = percentile(&hz, 0.05);
    let range = if low < 90.0 {
        Range::Low
    } else if low > 130.0 {
        Range::High
    } else {
        Range::Mid
    };
    let voiced = hz.len() as f32 / tr.len().max(1) as f32 * 100.0;
    writeln!(
        r,
        "Detected range: lowest notes around {low:.0} Hz ({}), median {}; {voiced:.0}% of frames clearly voiced. Using **{}**.\n",
        note_name(freq_to_midi(low).round() as i32),
        note_name(freq_to_midi(percentile(&hz, 0.5)).round() as i32),
        range.name()
    )
    .unwrap();

    // Latency.
    writeln!(r, "## Latency\n").unwrap();
    for rate in [44_100.0f32, 48_000.0, 96_000.0] {
        let row: Vec<String> = Range::ALL
            .iter()
            .map(|&rg| {
                let l = Config {
                    sample_rate: rate,
                    range: rg,
                }
                .latency();
                format!(
                    "{}: {l} samples = {:.1} ms",
                    rg.name(),
                    l as f32 * 1000.0 / rate
                )
            })
            .collect();
        writeln!(r, "- {rate} Hz: {}", row.join("; ")).unwrap();
    }

    // Bypass through the full engine.
    let engine = Arc::new(Engine::new());
    engine.set(P::Bypass, 1.0);
    engine.set(P::Range, range.index() as f64);
    let mut proc_ = engine.processor(f64::from(sr), 512);
    let lat = proc_.latency();
    let mut y = Vec::with_capacity(x.len() + lat);
    let mut padded = x.clone();
    padded.extend(std::iter::repeat_n(0.0, lat));
    for c in padded.chunks(512) {
        let mut b = c.to_vec();
        proc_.process(&mut [&mut b], Transport::default());
        y.extend_from_slice(&b);
    }
    let skip = (0.02 * sr) as usize;
    let identical = y[lat + skip..lat + x.len()] == x[skip..];
    writeln!(
        r,
        "\n## Bypass\n\nOutput equals input delayed by {lat} samples: **{}** (after the 10 ms fade from the default active state).\n",
        if identical { "bit-identical" } else { "NOT identical" }
    )
    .unwrap();

    // Automatic, slow and hard.
    writeln!(r, "## Automatic mode (chromatic)\n").unwrap();
    let before = voiced_cents_off_grid(&x, sr);
    let c_before = centre_cents_off_grid(&x, sr);
    for (name, ms) in [("slow", 150.0f32), ("medium", 50.0), ("hard", 0.0)] {
        let mut p = Params::default();
        p.correct.retune_ms = ms;
        let t = Instant::now();
        let y = run(&x, sr, range, &p);
        let speed = secs / t.elapsed().as_secs_f32();
        save(dir, &format!("{name}.wav"), &y, sr);
        let after = voiced_cents_off_grid(&y, sr);
        let c_after = centre_cents_off_grid(&y, sr);
        let m = movement(&x, &y, sr);
        writeln!(
            r,
            "- **{name}** (retune {ms} ms, `{name}.wav`): off-grid median {:.1} -> {:.1} cents (90th pct {:.1} -> {:.1}); note centres {:.1} -> {:.1} cents; movement median {:.1} -> {:.1} cents/frame (vibrato/slides kept = similar numbers); shift jitter median {:.2}, 90th pct {:.2} cents/frame (warble shows as high numbers); tracker glitches {:.1}% of frames; {speed:.0}x real time",
            percentile(&before, 0.5),
            percentile(&after, 0.5),
            percentile(&before, 0.9),
            percentile(&after, 0.9),
            percentile(&c_before, 0.5),
            percentile(&c_after, 0.5),
            m.input,
            m.output,
            m.jitter_p50,
            m.jitter_p90,
            m.glitches,
        )
        .unwrap();
        writeln!(
            r,
            "  - new clicks score {:.1} (1 = none, a click is 5+); worst: {}",
            new_clicks(&x, &y, sr),
            clicks_line(&x, &y, sr)
        )
        .unwrap();
        let hs: Vec<String> = m
            .hotspots
            .iter()
            .map(|(t, j)| format!("{t:.2} s ({j:.0} c)"))
            .collect();
        writeln!(r, "  - biggest shift jumps: {}", hs.join(", ")).unwrap();
    }

    // Formants.
    writeln!(r, "\n## Formant check: up a fifth\n").unwrap();
    let voiced_at = |t: usize| {
        tr.iter()
            .min_by_key(|f| f.0.abs_diff(t))
            .is_some_and(|f| f.2 > 0.9)
    };
    let ex = envelope(&x, sr, voiced_at);
    for preserve in [true, false] {
        let p = Params {
            transpose: 7.0,
            formant_preserve: preserve,
            correct: solplugs_pitch_core::correct::CorrectParams {
                amount: 0.0,
                ..Default::default()
            },
            ..Params::default()
        };
        let y = run(&x, sr, range, &p);
        let name = if preserve {
            "fifth-formant-on.wav"
        } else {
            "fifth-formant-off.wav"
        };
        save(dir, name, &y, sr);
        let ty = track(&y, sr, 256);
        let shifts: Vec<f32> = tr
            .iter()
            .zip(&ty)
            .filter(|(a, b)| a.2 > 0.9 && b.2 > 0.9)
            .map(|(a, b)| cents(b.1, a.1))
            .collect();
        let warp = envelope_warp(&ex, &envelope(&y, sr, voiced_at), sr);
        writeln!(
            r,
            "- formant {} (`{name}`): pitch moved {:.0} cents; spectral envelope moved x{warp:.2} (1.00 = same singer, 1.50 = chipmunk); new clicks {:.1}",
            if preserve { "**on**" } else { "off" },
            percentile(&shifts, 0.5),
            new_clicks(&x, &y, sr)
        )
        .unwrap();
    }

    // Graph mode.
    writeln!(r, "\n## Graph mode\n").unwrap();
    let t = Instant::now();
    let an = analyse(&x, sr, range);
    let mut edits = NoteEdits::new(&an);
    let took = t.elapsed().as_secs_f32();
    let n_notes = edits.notes.len();
    writeln!(r, "Found {n_notes} notes in {took:.2} s.\n").unwrap();
    let worst = edits
        .notes
        .iter()
        .enumerate()
        .filter(|(_, n)| (n.src_end - n.src_start) as f32 > 0.15 * sr)
        .max_by(|a, b| {
            (a.1.pitch - a.1.pitch.round())
                .abs()
                .total_cmp(&(b.1.pitch - b.1.pitch.round()).abs())
        })
        .map(|(i, n)| (i, *n));
    if let Some((i, n)) = worst {
        edits.set_shift(i, n.pitch.round() - n.pitch);
        let t = Instant::now();
        let y = render(
            &[&x],
            &an,
            &edits,
            Formant {
                preserve: true,
                shift: 0.0,
            },
        )
        .remove(0);
        let render_s = t.elapsed().as_secs_f32();
        save(dir, "graph-fixed.wav", &y, sr);
        let span = |v: &[f32]| {
            let len = n.src_end - n.src_start;
            let (a, b) = (
                (n.src_start + len / 5) as usize,
                (n.src_end - len / 5) as usize,
            );
            let m: Vec<f32> = track(&v[..b.min(v.len())], sr, 128)
                .iter()
                .filter(|(t, f, c)| *t >= a && *t < b && *c > 0.85 && *f > 0.0)
                .map(|(_, f, _)| freq_to_midi(*f))
                .collect();
            percentile(&m, 0.5)
        };
        let (b, a) = (span(&x), span(&y));
        writeln!(
            r,
            "Dragged the most out-of-tune note ({} at {:.2} s, {:+.0} cents) onto the grid (`graph-fixed.wav`): measured {:+.0} -> {:+.0} cents; new clicks {:.1}; render took {render_s:.2} s.",
            note_name(n.pitch.round() as i32),
            n.src_start as f32 / sr,
            (n.pitch - n.pitch.round()) * 100.0,
            (b - n.pitch.round()) * 100.0,
            (a - n.pitch.round()) * 100.0,
            new_clicks(&x, &y, sr)
        )
        .unwrap();
    }
    std::fs::write(format!("{dir}/report.md"), &r).unwrap();
    println!("{r}");
}
