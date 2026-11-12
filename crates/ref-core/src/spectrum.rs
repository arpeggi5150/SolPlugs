//! Short-time spectra reduced to 1/6-octave band powers, for the mid and side signals.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use crate::bands::{BANDS, lower_edge, upper_edge};

/// Band powers (mean square) of one frame, for mid and side.
#[derive(Clone, Copy, Debug)]
pub struct BandFrame {
    pub mid: [f64; BANDS],
    pub side: [f64; BANDS],
}

impl BandFrame {
    /// Total (mid + side) power of band `i`.
    #[must_use]
    pub fn total(&self, i: usize) -> f64 {
        self.mid[i] + self.side[i]
    }

    /// Total power of the whole frame.
    #[must_use]
    pub fn sum(&self) -> f64 {
        (0..BANDS).map(|i| self.total(i)).sum()
    }
}

/// A streaming STFT that emits one [`BandFrame`] per hop.
pub struct BandAnalyzer {
    size: usize,
    hop: usize,
    fft: Arc<dyn Fft<f64>>,
    window: Vec<f64>,
    norm: f64,
    /// Per band: (bin, weight) pairs.
    weights: Vec<Vec<(usize, f64)>>,
    mid: Vec<f64>,
    side: Vec<f64>,
    fill: usize,
    since_hop: usize,
    buf_m: Vec<Complex<f64>>,
    buf_s: Vec<Complex<f64>>,
    scratch: Vec<Complex<f64>>,
}

impl BandAnalyzer {
    /// An analyzer with an FFT of `size` samples (a power of two) and a hop of `hop` samples.
    #[must_use]
    pub fn new(sample_rate: f64, size: usize, hop: usize) -> Self {
        let size = size.max(64).next_power_of_two();
        let hop = hop.clamp(1, size);
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(size);
        let window: Vec<f64> = (0..size)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / size as f64).cos())
            .collect();
        let wsq: f64 = window.iter().map(|w| w * w).sum();
        let df = sample_rate / size as f64;
        let weights = (0..BANDS)
            .map(|b| {
                let (lo, hi) = (lower_edge(b), upper_edge(b));
                let mut v = Vec::new();
                for k in 1..size / 2 {
                    let (blo, bhi) = ((k as f64 - 0.5) * df, (k as f64 + 0.5) * df);
                    let overlap = (bhi.min(hi) - blo.max(lo)).max(0.0);
                    if overlap > 0.0 {
                        v.push((k, overlap / df));
                    }
                }
                v
            })
            .collect();
        let scratch_len = fft.get_inplace_scratch_len();
        Self {
            size,
            hop,
            fft,
            window,
            norm: 2.0 / (size as f64 * wsq),
            weights,
            mid: vec![0.0; size],
            side: vec![0.0; size],
            fill: 0,
            since_hop: 0,
            buf_m: vec![Complex::default(); size],
            buf_s: vec![Complex::default(); size],
            scratch: vec![Complex::default(); scratch_len],
        }
    }

    /// Seconds between frames.
    #[must_use]
    pub fn hop_secs(&self, sample_rate: f64) -> f64 {
        self.hop as f64 / sample_rate
    }

    /// Clears the buffered audio.
    pub fn reset(&mut self) {
        self.mid.fill(0.0);
        self.side.fill(0.0);
        self.fill = 0;
        self.since_hop = 0;
    }

    /// Feeds stereo audio; calls `on_frame` for every completed frame.
    pub fn process(&mut self, left: &[f32], right: &[f32], mut on_frame: impl FnMut(&BandFrame)) {
        let n = left.len().min(right.len());
        for i in 0..n {
            let (l, r) = (f64::from(left[i]), f64::from(right[i]));
            // Ring buffer of the last `size` samples, written at `fill`.
            let pos = self.fill % self.size;
            self.mid[pos] = 0.5 * (l + r);
            self.side[pos] = 0.5 * (l - r);
            self.fill += 1;
            self.since_hop += 1;
            if self.fill >= self.size && self.since_hop >= self.hop {
                self.since_hop = 0;
                let frame = self.frame();
                on_frame(&frame);
            }
        }
    }

    fn frame(&mut self) -> BandFrame {
        let start = self.fill % self.size;
        for k in 0..self.size {
            let idx = (start + k) % self.size;
            self.buf_m[k] = Complex::new(self.mid[idx] * self.window[k], 0.0);
            self.buf_s[k] = Complex::new(self.side[idx] * self.window[k], 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.buf_m, &mut self.scratch);
        self.fft
            .process_with_scratch(&mut self.buf_s, &mut self.scratch);
        let mut out = BandFrame {
            mid: [0.0; BANDS],
            side: [0.0; BANDS],
        };
        for (b, w) in self.weights.iter().enumerate() {
            let (mut pm, mut ps) = (0.0, 0.0);
            for &(k, wt) in w {
                pm += self.buf_m[k].norm_sqr() * wt;
                ps += self.buf_s[k].norm_sqr() * wt;
            }
            out.mid[b] = pm * self.norm;
            out.side[b] = ps * self.norm;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bands::band_of;

    #[test]
    fn a_sine_lands_in_its_band_with_its_mean_square() {
        let sr = 48_000.0;
        let f = 1000.0;
        let s: Vec<f32> = (0..48_000)
            .map(|i| (0.5 * (2.0 * std::f64::consts::PI * f * i as f64 / sr).sin()) as f32)
            .collect();
        let mut a = BandAnalyzer::new(sr, 8192, 4096);
        let mut last = None;
        a.process(&s, &s, |fr| last = Some(*fr));
        let fr = last.unwrap_or(BandFrame {
            mid: [0.0; BANDS],
            side: [0.0; BANDS],
        });
        let b = band_of(f).unwrap_or(0);
        // Mean square of a 0.5 sine is 0.125. Allow leakage into the neighbours.
        let near: f64 = (b.saturating_sub(1)..=b + 1).map(|i| fr.total(i)).sum();
        assert!((near - 0.125).abs() < 0.01, "{near}");
        assert!(fr.side.iter().sum::<f64>() < 1e-9);
    }
}
