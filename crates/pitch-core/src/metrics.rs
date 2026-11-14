//! Objective measures used by the test suite and the vocal report tool: an offline pitch
//! track, spectral envelopes (to check formants stay put), and a click detector.
#![allow(clippy::needless_range_loop)]

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

use crate::detect::Mpm;

/// An offline pitch track: (time in samples, Hz, clarity) every `hop` samples.
pub fn track(x: &[f32], sr: f32, hop: usize) -> Vec<(usize, f32, f32)> {
    let win = (2.0 * sr / 70.0) as usize;
    let mut mpm = Mpm::new(win, (sr / 1200.0) as usize, win / 2);
    let mut out = Vec::new();
    let mut t = 0;
    while t + win <= x.len() {
        let d = mpm.detect(&x[t..t + win]);
        out.push((
            t + win / 2,
            d.freq(sr),
            if d.rms > 0.003 { d.clarity } else { 0.0 },
        ));
        t += hop;
    }
    out
}

/// Cents between two frequencies.
pub fn cents(a: f32, b: f32) -> f32 {
    1200.0 * (a / b).log2()
}

/// Percentile (0..1) of a slice.
pub fn percentile(v: &[f32], p: f32) -> f32 {
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    if s.is_empty() {
        return f32::NAN;
    }
    s[((s.len() - 1) as f32 * p).round() as usize]
}

/// Mean log-magnitude spectral envelope (dB, cepstrally smoothed) over the frames where
/// `voiced(t)` is true. `n` bins up to Nyquist.
pub fn envelope(x: &[f32], sr: f32, voiced: impl Fn(usize) -> bool) -> Vec<f32> {
    let n = 2048;
    let lifter = (sr * 0.0012) as usize; // keep quefrencies under 1.2 ms
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    let ifft = planner.plan_fft_inverse(n);
    let mut acc = vec![0f32; n / 2];
    let mut count = 0;
    let mut t = 0;
    while t + n <= x.len() {
        if voiced(t + n / 2) {
            let mut buf: Vec<Complex32> = (0..n)
                .map(|i| {
                    let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
                    Complex32::new(x[t + i] * w, 0.0)
                })
                .collect();
            fft.process(&mut buf);
            let mut c: Vec<Complex32> = buf
                .iter()
                .map(|v| Complex32::new((v.norm_sqr() + 1e-12).ln(), 0.0))
                .collect();
            ifft.process(&mut c);
            for (i, v) in c.iter_mut().enumerate() {
                let q = i.min(n - i);
                if q > lifter {
                    *v = Complex32::default();
                }
            }
            fft.process(&mut c);
            for k in 0..n / 2 {
                acc[k] += c[k].re / n as f32 * 10.0 / std::f32::consts::LN_10;
            }
            count += 1;
        }
        t += n / 4;
    }
    for v in acc.iter_mut() {
        *v /= count.max(1) as f32;
    }
    acc
}

/// The frequency warp factor `a` that best maps envelope `from` onto `to`
/// (to(f) ≈ from(f / a)), over 250 Hz..5 kHz, mean levels removed.
pub fn envelope_warp(from: &[f32], to: &[f32], sr: f32) -> f32 {
    let bins = from.len();
    let hz = |k: usize| k as f32 * sr / (2.0 * bins as f32);
    let at = |e: &[f32], f: f32| {
        let p = f * 2.0 * bins as f32 / sr;
        let i = p.floor() as usize;
        if i + 1 >= bins {
            return e[bins - 1];
        }
        let fr = p - i as f32;
        e[i] + fr * (e[i + 1] - e[i])
    };
    let range: Vec<usize> = (0..bins)
        .filter(|&k| hz(k) >= 250.0 && hz(k) <= 5000.0)
        .collect();
    let mut best = (f32::MAX, 1.0);
    let mut a = 0.6f32;
    while a <= 2.0 {
        let diffs: Vec<f32> = range.iter().map(|&k| to[k] - at(from, hz(k) / a)).collect();
        let mean = diffs.iter().sum::<f32>() / diffs.len() as f32;
        let err = diffs.iter().map(|d| (d - mean).powi(2)).sum::<f32>();
        if err < best.0 {
            best = (err, a);
        }
        a += 0.01;
    }
    best.1
}

/// Per-millisecond burst score: each 1 ms block's high-frequency (second difference)
/// energy relative to the median of its ±10 ms neighbourhood. A click stands out of its
/// context; sibilants and noise don't, because they are long.
pub fn burst_scores(x: &[f32], sr: f32) -> Vec<f32> {
    let w = (sr * 0.001) as usize;
    let d: Vec<f32> = x.windows(3).map(|v| v[2] - 2.0 * v[1] + v[0]).collect();
    let bursts: Vec<f32> = d
        .chunks(w)
        .map(|c| c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32)
        .collect();
    let level = percentile(&bursts, 0.5).max(1e-12);
    (0..bursts.len())
        .map(|i| {
            let lo = i.saturating_sub(10);
            let hi = (i + 11).min(bursts.len());
            let mut ctx: Vec<f32> = bursts[lo..hi].to_vec();
            ctx.sort_by(f32::total_cmp);
            bursts[i] / ctx[ctx.len() / 2].max(level * 0.05)
        })
        .collect()
}

/// New clicks: the worst burst in `y` relative to the strongest burst `x` already has
/// within ±6 ms (PSOLA moves glottal pulses by up to a period, so a sharp pulse in the
/// input may land a few ms later in the output). 1 = nothing new; a click is 5+.
pub fn new_clicks(x: &[f32], y: &[f32], sr: f32) -> f32 {
    let sx = burst_scores(x, sr);
    let sy = burst_scores(y, sr);
    let mut worst = 0f32;
    for i in 0..sy.len().min(sx.len()) {
        let lo = i.saturating_sub(6);
        let hi = (i + 7).min(sx.len());
        let ctx = sx[lo..hi].iter().copied().fold(1f32, f32::max);
        worst = worst.max(sy[i] / ctx);
    }
    worst
}

/// The `n` worst new clicks in `y` (see [`new_clicks`]), at least 50 ms apart:
/// (time in s, score, level of `y` around it in dBFS) so a report can say where to
/// listen and whether it is loud enough to hear.
pub fn click_events(x: &[f32], y: &[f32], sr: f32, n: usize) -> Vec<(f32, f32, f32)> {
    let sx = burst_scores(x, sr);
    let sy = burst_scores(y, sr);
    let ms = (sr * 0.001) as usize;
    let mut s: Vec<(usize, f32)> = (0..sy.len().min(sx.len()))
        .map(|i| {
            let lo = i.saturating_sub(6);
            let hi = (i + 7).min(sx.len());
            (i, sy[i] / sx[lo..hi].iter().copied().fold(1f32, f32::max))
        })
        .collect();
    s.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut out: Vec<(f32, f32, f32)> = Vec::new();
    for (i, score) in s {
        if out.len() == n || score < 2.0 {
            break;
        }
        let t = i as f32 / 1000.0;
        if out.iter().any(|o| (o.0 - t).abs() < 0.05) {
            continue;
        }
        let (a, b) = (
            (i * ms).saturating_sub(10 * ms),
            ((i + 10) * ms).min(y.len()),
        );
        let rms = (y[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a).max(1) as f32).sqrt();
        out.push((t, score, 20.0 * rms.max(1e-9).log10()));
    }
    out
}
