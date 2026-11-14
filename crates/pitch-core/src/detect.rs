//! Pitch detection: the McLeod Pitch Method (P. McLeod and G. Wyvill, "A Smarter Way to Find
//! Pitch", ICMC 2005), implemented from the paper.
//!
//! The normalised square difference function (NSDF) is
//! `n(τ) = 2·r(τ) / m(τ)` with `r` the autocorrelation over the window and
//! `m(τ) = Σ x[j]² + x[j+τ]²`. The autocorrelation comes from an FFT, so one detection costs
//! two FFTs of the padded window. The period is the first "key maximum" (the highest NSDF
//! value between two positive-going zero crossings) that reaches `K` times the highest key
//! maximum, refined by parabolic interpolation. Its NSDF value is the clarity: about 1 for a
//! clean periodic tone, low for noise and breaths.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

/// Fraction of the highest key maximum the chosen peak must reach (the paper's k).
const K: f32 = 0.9;
/// With a continuity hint, how strong a peak near the previous period must be (relative
/// to the highest key maximum) to keep the pitch below the standard choice.
const HINT_K: f32 = 0.75;
/// How strong a hint peak must be, relative to the standard choice, to keep the pitch
/// above a longer period that the standard choice found.
const HINT_LOWER_K: f32 = 0.88;

/// One detection result.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Detection {
    /// Period in samples, fractional. 0 when nothing periodic was found.
    pub period: f32,
    /// NSDF value at the chosen peak, 0..1.
    pub clarity: f32,
    /// RMS of the analysed window.
    pub rms: f32,
}

impl Detection {
    /// Frequency in Hz, or 0.
    pub fn freq(&self, sample_rate: f32) -> f32 {
        if self.period > 0.0 {
            sample_rate / self.period
        } else {
            0.0
        }
    }
}

/// A McLeod pitch detector for a fixed window length and lag range. Allocates once.
pub struct Mpm {
    window: usize,
    min_lag: usize,
    max_lag: usize,
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    buf: Vec<Complex32>,
    scratch: Vec<Complex32>,
    nsdf: Vec<f32>,
}

impl Mpm {
    /// A detector over `window` samples finding periods between `min_lag` and `max_lag`
    /// samples. `max_lag` is clamped to `window / 2`.
    pub fn new(window: usize, min_lag: usize, max_lag: usize) -> Self {
        let window = window.max(32);
        let max_lag = max_lag.min(window / 2).max(4);
        let min_lag = min_lag.clamp(2, max_lag - 1);
        // Linear (not circular) autocorrelation up to max_lag needs window + max_lag points.
        let n = (window + max_lag + 1).next_power_of_two();
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(n);
        let ifft = planner.plan_fft_inverse(n);
        let scratch_len = fft
            .get_inplace_scratch_len()
            .max(ifft.get_inplace_scratch_len());
        Self {
            window,
            min_lag,
            max_lag,
            fft,
            ifft,
            buf: vec![Complex32::default(); n],
            scratch: vec![Complex32::default(); scratch_len],
            nsdf: vec![0.0; max_lag + 2],
        }
    }

    /// Window length in samples.
    pub fn window(&self) -> usize {
        self.window
    }

    /// The NSDF of the last call to [`Mpm::detect`], for lags `0..=max_lag`.
    pub fn nsdf(&self) -> &[f32] {
        &self.nsdf
    }

    /// Detects the period of `x` (its first `window` samples are used; shorter input is
    /// zero-padded). Never allocates.
    pub fn detect(&mut self, x: &[f32]) -> Detection {
        self.detect_near(x, None)
    }

    /// As [`Mpm::detect`], but when `hint` (the previous period) is given, a key maximum
    /// within a few semitones of it wins over the standard choice if it is nearly as
    /// strong. This keeps a sung line from slipping an octave (or a twelfth) for a frame
    /// or two when the voice gets rough.
    pub fn detect_near(&mut self, x: &[f32], hint: Option<f32>) -> Detection {
        let w = self.window.min(x.len());
        if w < 2 * self.min_lag {
            return Detection::default();
        }
        let x = &x[..w];
        let energy: f32 = x.iter().map(|v| v * v).sum();
        let rms = (energy / w as f32).sqrt();
        if energy <= 1e-12 {
            return Detection {
                rms,
                ..Detection::default()
            };
        }
        // r(τ) by FFT: |X|² then inverse.
        for (b, &v) in self.buf.iter_mut().zip(x.iter()) {
            *b = Complex32::new(v, 0.0);
        }
        for b in self.buf[w..].iter_mut() {
            *b = Complex32::default();
        }
        self.fft
            .process_with_scratch(&mut self.buf, &mut self.scratch);
        for b in self.buf.iter_mut() {
            *b = Complex32::new(b.norm_sqr(), 0.0);
        }
        self.ifft
            .process_with_scratch(&mut self.buf, &mut self.scratch);
        let scale = 1.0 / self.buf.len() as f32;
        let max_lag = self.max_lag.min(w - 1);
        // m(τ) recursively: m(0) = 2·Σx², m(τ) = m(τ-1) - x[τ-1]² - x[w-τ]².
        let mut m = 2.0 * energy;
        self.nsdf[0] = 1.0;
        for tau in 1..=max_lag {
            m -= x[tau - 1] * x[tau - 1] + x[w - tau] * x[w - tau];
            let r = self.buf[tau].re * scale;
            self.nsdf[tau] = if m > 1e-12 {
                (2.0 * r / m).clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        for v in self.nsdf[max_lag + 1..].iter_mut() {
            *v = 0.0;
        }
        let (period, clarity) = self.pick(max_lag, hint);
        Detection {
            period,
            clarity,
            rms,
        }
    }

    /// Peak picking over the NSDF: key maxima between positive-going zero crossings.
    fn pick(&self, max_lag: usize, hint: Option<f32>) -> (f32, f32) {
        let n = &self.nsdf;
        // Skip the lag-0 lobe: start after the first negative-going zero crossing.
        let mut tau = 1;
        while tau < max_lag && n[tau] > 0.0 {
            tau += 1;
        }
        // Collect key maxima (up to a fixed number, no allocation).
        let mut keys = [(0usize, 0f32); 32];
        let mut nkeys = 0;
        let mut overall = 0f32;
        while tau < max_lag && nkeys < keys.len() {
            // Find the next positive-going crossing.
            while tau < max_lag && n[tau] <= 0.0 {
                tau += 1;
            }
            if tau >= max_lag {
                break;
            }
            let mut best = tau;
            while tau < max_lag && n[tau] > 0.0 {
                if n[tau] > n[best] {
                    best = tau;
                }
                tau += 1;
            }
            // A lobe cut off by max_lag is only trusted if it already turned down.
            if tau >= max_lag && best + 1 >= max_lag {
                break;
            }
            if best >= self.min_lag {
                keys[nkeys] = (best, n[best]);
                nkeys += 1;
                overall = overall.max(n[best]);
            }
        }
        if nkeys == 0 || overall <= 0.0 {
            return (0.0, 0.0);
        }
        let threshold = K * overall;
        let Some(&(mut lag, std_value)) = keys[..nkeys].iter().find(|(_, v)| *v >= threshold)
        else {
            return (0.0, 0.0);
        };
        if let Some(h) = hint.filter(|h| *h > 0.0) {
            let dist = |l: usize| (l as f32 / h).log2().abs();
            // 4 semitones.
            const NEAR: f32 = 4.0 / 12.0;
            if dist(lag) > NEAR
                && let Some(&(near, _)) = keys[..nkeys]
                    .iter()
                    .filter(|(l, v)| {
                        // Against a longer period (the standard choice says the pitch is
                        // lower) the hint must be nearly as strong as that choice: a
                        // weaker peak at a fraction of the true period is a harmonic,
                        // and following the hint there would lock an octave-high error.
                        let need = if *l < lag {
                            HINT_LOWER_K * std_value
                        } else {
                            HINT_K * overall
                        };
                        *v >= need && dist(*l) <= NEAR
                    })
                    .min_by(|a, b| dist(a.0).total_cmp(&dist(b.0)))
            {
                lag = near;
            }
        }
        // Parabolic interpolation around the peak.
        let (a, b, c) = (n[lag - 1], n[lag], n[lag + 1]);
        let denom = a - 2.0 * b + c;
        let (shift, value) = if denom.abs() > 1e-9 {
            let s = (0.5 * (a - c) / denom).clamp(-0.5, 0.5);
            (s, b - 0.25 * (a - c) * s)
        } else {
            (0.0, b)
        };
        (lag as f32 + shift, value.clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, sr: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / sr;
                // A few harmonics, the fundamental not the strongest: a voice-like spectrum.
                0.3 * (std::f32::consts::TAU * freq * t).sin()
                    + 0.5 * (std::f32::consts::TAU * 2.0 * freq * t).sin()
                    + 0.2 * (std::f32::consts::TAU * 3.0 * freq * t).sin()
            })
            .collect()
    }

    #[test]
    fn finds_the_fundamental_not_a_harmonic() {
        let sr = 48_000.0;
        let mut mpm = Mpm::new(1536, 40, 768);
        for f in [70.0, 110.0, 220.0, 441.0, 880.0] {
            let d = mpm.detect(&tone(f, sr, 1536));
            let got = d.freq(sr);
            let cents = 1200.0 * (got / f).log2();
            assert!(cents.abs() < 5.0, "{f} Hz: got {got} ({cents} cents)");
            assert!(d.clarity > 0.95, "{f} Hz clarity {}", d.clarity);
        }
    }

    #[test]
    fn noise_has_low_clarity() {
        let mut s = 12345u32;
        let noise: Vec<f32> = (0..1536)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect();
        let d = Mpm::new(1536, 40, 768).detect(&noise);
        assert!(d.clarity < 0.6, "clarity {}", d.clarity);
    }

    #[test]
    fn silence_is_nothing() {
        let d = Mpm::new(1024, 40, 512).detect(&[0.0; 1024]);
        assert_eq!(d.period, 0.0);
    }
}
