//! Real-time corrector on real singing (MTG sms-tools sounds, CC BY 4.0; see ATTRIBUTION.md).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use solplugs_pitch_core::correct::CorrectParams;
use solplugs_pitch_core::realtime::{Params, Range};
use solplugs_pitch_core::scale::{Scale, freq_to_midi};

const TAKES: [(&str, Range); 3] = [
    ("singing-female.wav", Range::Mid),
    ("soprano-E4.wav", Range::High),
    ("vignesh.wav", Range::Mid),
];

fn params(f: impl FnOnce(&mut Params)) -> Params {
    let mut p = Params::default();
    f(&mut p);
    p
}

/// Deviation from the nearest semitone, cents, over clearly voiced frames.
fn off_grid(x: &[f32], sr: f32) -> Vec<f32> {
    track(x, sr, 256)
        .iter()
        .filter(|(_, f, c)| *c > 0.9 && *f > 0.0)
        .map(|(_, f, _)| {
            let m = freq_to_midi(*f);
            (m - m.round()) * 100.0
        })
        .collect()
}

#[test]
fn bypass_is_bit_identical() {
    for (name, range) in TAKES {
        let (x, sr) = load(name);
        let (y, _) = run(&x, sr, range, &params(|p| p.bypass = true));
        // The processor starts wet and fades to dry over 10 ms; after that, exact.
        let skip = (0.02 * sr) as usize;
        assert_eq!(&y[skip..], &x[skip..], "{name}");
    }
}

#[test]
fn zero_correction_is_transparent() {
    for (name, range) in TAKES {
        let (x, sr) = load(name);
        let p = params(|p| p.correct.amount = 0.0);
        let (y, _) = run(&x, sr, range, &p);
        let err = x
            .iter()
            .zip(&y)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        println!("{name}: max |error| {err:e}");
        assert!(err < 1e-4, "{name}: {err}");
    }
}

#[test]
fn hard_tune_lands_on_the_grid_without_clicks() {
    for (name, range) in TAKES {
        let (x, sr) = load(name);
        let p = params(|p| p.correct.retune_ms = 0.0);
        let (y, _) = run(&x, sr, range, &p);
        dump(&format!("hard-{name}"), &y, sr);
        let before = off_grid(&x, sr).iter().map(|c| c.abs()).collect::<Vec<_>>();
        let after = off_grid(&y, sr).iter().map(|c| c.abs()).collect::<Vec<_>>();
        let clicks = new_clicks(&x, &y, sr);
        println!(
            "{name}: off-grid median {:.1} -> {:.1} cents, p90 {:.1} -> {:.1}; new clicks {clicks:.1}",
            percentile(&before, 0.5),
            percentile(&after, 0.5),
            percentile(&before, 0.9),
            percentile(&after, 0.9)
        );
        assert!(percentile(&after, 0.5) < 4.0, "{name}");
        assert!(percentile(&after, 0.9) < 15.0, "{name}");
        assert!(clicks < 4.0, "{name}: click score {clicks}");
    }
}

#[test]
fn slow_retune_keeps_the_performance() {
    for (name, range) in TAKES {
        let (x, sr) = load(name);
        let p = params(|p| {
            p.correct = CorrectParams {
                retune_ms: 150.0,
                ..CorrectParams::default()
            }
        });
        let (y, _) = run(&x, sr, range, &p);
        dump(&format!("slow-{name}"), &y, sr);
        // Pitch movement (vibrato, slides) is kept: frame-to-frame pitch changes in the
        // output track those of the input.
        let tx = track(&x, sr, 256);
        let ty = track(&y, sr, 256);
        let mut dx = Vec::new();
        let mut dy = Vec::new();
        for i in 1..tx.len().min(ty.len()) {
            if tx[i].2 > 0.9 && tx[i - 1].2 > 0.9 && ty[i].2 > 0.9 && ty[i - 1].2 > 0.9 {
                dx.push(cents(tx[i].1, tx[i - 1].1));
                dy.push(cents(ty[i].1, ty[i - 1].1));
            }
        }
        let rms = |v: &[f32]| (v.iter().map(|a| a * a).sum::<f32>() / v.len() as f32).sqrt();
        let (mx, my) = (rms(&dx), rms(&dy));
        let clicks = new_clicks(&x, &y, sr);
        println!("{name}: movement {mx:.1} -> {my:.1} cents/frame; new clicks {clicks:.1}");
        assert!(
            my > 0.7 * mx && my < 1.3 * mx,
            "{name}: movement {mx} -> {my}"
        );
        assert!(clicks < 4.0, "{name}: click score {clicks}");
    }
}

#[test]
fn shifting_a_fifth_keeps_the_formants() {
    for (name, range) in TAKES {
        let (x, sr) = load(name);
        let mut warps = [0f32; 2];
        for (i, preserve) in [true, false].into_iter().enumerate() {
            let p = params(|p| {
                p.correct.amount = 0.0;
                p.transpose = 7.0;
                p.formant_preserve = preserve;
            });
            let (y, _) = run(&x, sr, range, &p);
            dump(&format!("fifth-{preserve}-{name}"), &y, sr);
            // Pitch went up 700 cents.
            let tx = track(&x, sr, 256);
            let ty = track(&y, sr, 256);
            let shifts: Vec<f32> = tx
                .iter()
                .zip(&ty)
                .filter(|(a, b)| a.2 > 0.9 && b.2 > 0.9)
                .map(|(a, b)| cents(b.1, a.1))
                .collect();
            let med = percentile(&shifts, 0.5);
            let voiced = |t: usize| {
                tx.iter()
                    .min_by_key(|f| f.0.abs_diff(t))
                    .is_some_and(|f| f.2 > 0.9)
            };
            let ex = envelope(&x, sr, voiced);
            let ey = envelope(&y, sr, voiced);
            warps[i] = envelope_warp(&ex, &ey, sr);
            println!(
                "{name} formant {}: shift median {med:.0} cents, envelope warp {:.2}",
                if preserve { "on " } else { "off" },
                warps[i]
            );
            assert!((med - 700.0).abs() < 25.0, "{name}: shift {med}");
        }
        assert!(
            (warps[0] - 1.0).abs() < 0.1,
            "{name}: formant on warp {}",
            warps[0]
        );
        assert!(warps[1] > 1.3, "{name}: formant off warp {}", warps[1]);
    }
}

#[test]
fn detection_is_right_on_real_singing() {
    // The soprano holds E4 (329.6 Hz).
    let (x, sr) = load("soprano-E4.wav");
    let (_, frames) = run(&x, sr, Range::High, &Params::default());
    let hz: Vec<f32> = frames
        .iter()
        .filter(|f| f.corrected.input > 0.0)
        .map(|f| solplugs_pitch_core::scale::midi_to_freq(f.corrected.input))
        .collect();
    let med = percentile(&hz, 0.5);
    println!("soprano median {med:.1} Hz over {} voiced frames", hz.len());
    assert!(cents(med, 329.63).abs() < 30.0, "{med}");
    // No octave slips on any take: consecutive voiced frames never jump by more than
    // 7 semitones and come back.
    for (name, range) in TAKES {
        let (x, sr) = load(name);
        let (_, frames) = run(&x, sr, range, &Params::default());
        let m: Vec<f32> = frames.iter().map(|f| f.corrected.input).collect();
        let mut slips = 0;
        for i in 1..m.len().saturating_sub(4) {
            if m[i - 1] > 0.0 && m[i] > 0.0 && (m[i] - m[i - 1]).abs() > 7.0 {
                let back = (1..4).any(|j| m[i + j] > 0.0 && (m[i + j] - m[i - 1]).abs() < 2.0);
                if back {
                    slips += 1;
                }
            }
        }
        let voiced = m.iter().filter(|v| **v > 0.0).count();
        println!("{name}: {voiced} voiced frames, {slips} octave slips");
        assert_eq!(slips, 0, "{name}");
    }
}

#[test]
fn scale_mode_uses_only_scale_notes() {
    let (x, sr) = load("singing-female.wav");
    let scale = Scale::new(2, solplugs_pitch_core::scale::ScaleKind::Major, 0);
    let p = params(|p| {
        p.correct.retune_ms = 0.0;
        p.correct.scale = scale;
    });
    let (_, frames) = run(&x, sr, Range::Mid, &p);
    for f in frames.iter().filter(|f| f.corrected.note >= 0) {
        assert!(scale.contains(f.corrected.note), "{f:?}");
    }
}
