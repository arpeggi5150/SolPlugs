//! Linear-phase mode for Clean: the bands' combined magnitude becomes a symmetric FIR
//! kernel (no phase shift at all), run by uniformly partitioned FFT convolution. Mid and
//! side get their own kernels, so mid/side bands work here too.
//!
//! The magnitude is taken from the band designs evaluated at eight times the sample rate,
//! which is the analogue curve for every practical purpose: no bell squashing near
//! Nyquist. When the bands move, a new kernel is built (at most once per block) and the
//! output crossfades from the old kernel to the new one over that block, so nothing clicks.
//!
//! Everything is allocated up front; [`LinearPhase::process`] never allocates.

use std::sync::Arc;

use rustfft::num_complex::Complex64;
use rustfft::{Fft, FftPlanner};

use crate::filter::{Design, warp};

/// The two kernels: mid and side.
const CH: usize = 2;

/// Kernel length for a sample rate.
pub fn kernel_len(sr: f32) -> usize {
    if sr <= 50_000.0 {
        8192
    } else if sr <= 100_000.0 {
        16_384
    } else {
        32_768
    }
}

/// Block (partition) length for a sample rate.
pub fn block_len(sr: f32) -> usize {
    if sr <= 50_000.0 { 512 } else { 1024 }
}

/// Latency at `sr`, samples: one block of buffering plus half the kernel.
pub fn latency(sr: f32) -> usize {
    block_len(sr) + kernel_len(sr) / 2
}

/// Linear-phase convolver with two kernels (mid, side).
pub struct LinearPhase {
    sr: f64,
    n: usize,
    b: usize,
    parts: usize,
    fwd: Arc<dyn Fft<f64>>,
    inv: Arc<dyn Fft<f64>>,
    design_inv: Arc<dyn Fft<f64>>,
    scratch: Vec<Complex64>,
    /// Last 2B input samples per channel.
    frame: [Vec<f64>; CH],
    /// Output block being played out, per channel.
    out: [Vec<f64>; CH],
    pos: usize,
    /// Frequency-domain delay line: `parts` spectra of 2B per channel.
    fdl: [Vec<Vec<Complex64>>; CH],
    fdl_pos: usize,
    /// Kernel spectra: [slot][channel][partition].
    kernels: [[Vec<Vec<Complex64>>; CH]; 2],
    live: usize,
    /// Crossfade from the other slot during the next block.
    fade_pending: bool,
    /// A new kernel is wanted at the next block boundary.
    rebuild_wanted: bool,
    /// Warped frequency of each design bin at the virtual rate.
    bin_t: Vec<f64>,
    /// Design work buffers.
    mag: [Vec<f64>; CH],
    work: Vec<Complex64>,
    kernel: Vec<f64>,
    window: Vec<f64>,
    acc: Vec<Complex64>,
    acc_old: Vec<Complex64>,
    /// Kernel designs waiting to be built: per channel, the band designs to multiply.
    pending: [[Design; crate::MAX_BANDS]; CH],
    pending_n: [usize; CH],
}

/// Virtual design rate, relative to the sample rate.
const VIRTUAL: f64 = 8.0;

impl LinearPhase {
    /// A convolver at `sr`, starting as a wire (with its latency).
    pub fn new(sr: f32) -> Self {
        let n = kernel_len(sr);
        let b = block_len(sr);
        let parts = n / b;
        let mut planner = FftPlanner::<f64>::new();
        let fwd = planner.plan_fft_forward(2 * b);
        let inv = planner.plan_fft_inverse(2 * b);
        let design_inv = planner.plan_fft_inverse(n);
        let scratch_len = fwd
            .get_inplace_scratch_len()
            .max(inv.get_inplace_scratch_len())
            .max(design_inv.get_inplace_scratch_len());
        let fs_v = f64::from(sr) * VIRTUAL;
        let bin_t = (0..=n / 2)
            .map(|k| warp(k as f64 * f64::from(sr) / n as f64, fs_v))
            .collect();
        // Blackman window over the kernel.
        let window = (0..n)
            .map(|i| {
                let x = std::f64::consts::TAU * i as f64 / n as f64;
                0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos()
            })
            .collect();
        let empty_k = || -> [Vec<Vec<Complex64>>; CH] {
            std::array::from_fn(|_| vec![vec![Complex64::default(); 2 * b]; parts])
        };
        let mut lp = Self {
            sr: f64::from(sr),
            n,
            b,
            parts,
            fwd,
            inv,
            design_inv,
            scratch: vec![Complex64::default(); scratch_len.max(1)],
            frame: std::array::from_fn(|_| vec![0.0; 2 * b]),
            out: std::array::from_fn(|_| vec![0.0; b]),
            pos: 0,
            fdl: std::array::from_fn(|_| vec![vec![Complex64::default(); 2 * b]; parts]),
            fdl_pos: 0,
            kernels: [empty_k(), empty_k()],
            live: 0,
            fade_pending: false,
            rebuild_wanted: true,
            bin_t,
            mag: std::array::from_fn(|_| vec![1.0; n / 2 + 1]),
            work: vec![Complex64::default(); n],
            kernel: vec![0.0; n],
            window,
            acc: vec![Complex64::default(); 2 * b],
            acc_old: vec![Complex64::default(); 2 * b],
            pending: [[Design::default(); crate::MAX_BANDS]; CH],
            pending_n: [0; CH],
        };
        // Start as a flat (delay-only) kernel, no fade.
        lp.build();
        lp.fade_pending = false;
        lp
    }

    /// Latency, samples.
    pub fn latency(&self) -> usize {
        self.b + self.n / 2
    }

    /// The rate the band designs passed to [`LinearPhase::set_bands`] must be made at.
    pub fn design_rate(&self) -> f64 {
        self.sr * VIRTUAL
    }

    /// Sets the bands for channel `ch` (0 = mid, 1 = side). Designs must be made at
    /// [`LinearPhase::design_rate`]. Takes effect at the next block boundary.
    pub fn set_bands(&mut self, ch: usize, designs: &[Design]) {
        let ch = ch.min(CH - 1);
        let n = designs.len().min(crate::MAX_BANDS);
        let same = self.pending_n[ch] == n && self.pending[ch][..n] == designs[..n];
        if !same {
            self.pending[ch][..n].copy_from_slice(&designs[..n]);
            self.pending_n[ch] = n;
            self.rebuild_wanted = true;
        }
    }

    /// Clears the audio history (kernels are kept).
    pub fn reset(&mut self) {
        for c in 0..CH {
            self.frame[c].fill(0.0);
            self.out[c].fill(0.0);
            for s in &mut self.fdl[c] {
                s.fill(Complex64::default());
            }
        }
        self.pos = 0;
    }

    /// One sample of mid and side in, one sample of each out (delayed by the latency).
    #[inline]
    pub fn process(&mut self, m: f64, s: f64) -> (f64, f64) {
        let b = self.b;
        let y = (self.out[0][self.pos], self.out[1][self.pos]);
        self.frame[0][b + self.pos] = m;
        self.frame[1][b + self.pos] = s;
        self.pos += 1;
        if self.pos == b {
            self.pos = 0;
            self.block();
        }
        y
    }

    fn block(&mut self) {
        if self.rebuild_wanted {
            self.build();
        }
        let b = self.b;
        let nfft = 2 * b;
        let norm = 1.0 / nfft as f64;
        self.fdl_pos = (self.fdl_pos + 1) % self.parts;
        let old = 1 - self.live;
        for c in 0..CH {
            // Spectrum of the last 2B samples into the delay line.
            let slot = &mut self.fdl[c][self.fdl_pos];
            for (d, x) in slot.iter_mut().zip(&self.frame[c]) {
                *d = Complex64::new(*x, 0.0);
            }
            self.fwd.process_with_scratch(slot, &mut self.scratch);
            // Slide the frame: the current block becomes the previous one.
            self.frame[c].copy_within(b.., 0);

            accumulate(
                &mut self.acc,
                &self.fdl[c],
                &self.kernels[self.live][c],
                self.fdl_pos,
            );
            self.inv
                .process_with_scratch(&mut self.acc, &mut self.scratch);
            if self.fade_pending {
                accumulate(
                    &mut self.acc_old,
                    &self.fdl[c],
                    &self.kernels[old][c],
                    self.fdl_pos,
                );
                self.inv
                    .process_with_scratch(&mut self.acc_old, &mut self.scratch);
                for i in 0..b {
                    let t = (i as f64 + 0.5) / b as f64;
                    let new = self.acc[b + i].re;
                    let prev = self.acc_old[b + i].re;
                    self.out[c][i] = (prev + (new - prev) * t) * norm;
                }
            } else {
                for i in 0..b {
                    self.out[c][i] = self.acc[b + i].re * norm;
                }
            }
        }
        self.fade_pending = false;
    }

    /// Builds the pending kernels into the idle slot and makes it live, with a crossfade.
    fn build(&mut self) {
        self.rebuild_wanted = false;
        let n = self.n;
        let half = n / 2;
        let target = 1 - self.live;
        for c in 0..CH {
            // Magnitude per bin.
            let designs = &self.pending[c][..self.pending_n[c]];
            for (m, t) in self.mag[c].iter_mut().zip(&self.bin_t) {
                let mut h = 1.0;
                for d in designs {
                    h *= d.response_warped(*t).norm();
                }
                *m = h;
            }
            // Zero-phase spectrum (real, symmetric) to impulse.
            for k in 0..n {
                let kk = if k <= half { k } else { n - k };
                self.work[k] = Complex64::new(self.mag[c][kk], 0.0);
            }
            self.design_inv
                .process_with_scratch(&mut self.work, &mut self.scratch);
            // Centre it, window it.
            for i in 0..n {
                let src = (i + half) % n;
                self.kernel[i] = self.work[src].re / n as f64 * self.window[i];
            }
            // Partition spectra.
            let b = self.b;
            for p in 0..self.parts {
                let part = &mut self.kernels[target][c][p];
                for (j, slot) in part.iter_mut().enumerate() {
                    *slot = if j < b {
                        Complex64::new(self.kernel[p * b + j], 0.0)
                    } else {
                        Complex64::default()
                    };
                }
                self.fwd.process_with_scratch(part, &mut self.scratch);
            }
        }
        self.live = target;
        self.fade_pending = true;
    }
}

/// Sums the partitioned products into `acc`.
fn accumulate(
    acc: &mut [Complex64],
    fdl: &[Vec<Complex64>],
    kernel: &[Vec<Complex64>],
    newest: usize,
) {
    acc.fill(Complex64::default());
    let parts = fdl.len();
    for (p, k) in kernel.iter().enumerate() {
        let x = &fdl[(newest + parts - p) % parts];
        for ((a, xv), kv) in acc.iter_mut().zip(x).zip(k) {
            *a += xv * kv;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::BandSpec;

    #[test]
    fn flat_kernel_is_a_pure_delay() {
        let mut lp = LinearPhase::new(48_000.0);
        let lat = lp.latency();
        let n = lat + 2000;
        let out: Vec<f64> = (0..n)
            .map(|i| lp.process(if i == 0 { 1.0 } else { 0.0 }, 0.0).0)
            .collect();
        let (at, peak) =
            out.iter().enumerate().fold(
                (0, 0.0f64),
                |a, (i, v)| if v.abs() > a.1 { (i, v.abs()) } else { a },
            );
        assert_eq!(at, lat);
        assert!((peak - 1.0).abs() < 0.01, "peak {peak}");
    }

    #[test]
    fn bell_magnitude_matches_and_phase_is_linear() {
        let sr = 48_000.0f32;
        let mut lp = LinearPhase::new(sr);
        let spec = BandSpec::bell(1000.0, 9.0, 1.0);
        let d = spec.design(lp.design_rate());
        lp.set_bands(0, &[d]);
        lp.set_bands(1, &[d]);
        let lat = lp.latency();
        for f in [200.0f64, 1000.0, 4000.0] {
            let n = 48_000;
            let x: Vec<f64> = (0..n)
                .map(|i| (std::f64::consts::TAU * f * i as f64 / 48_000.0).sin())
                .collect();
            let y: Vec<f64> = x.iter().map(|v| lp.process(*v, *v).0).collect();
            // Compare against the input delayed by the latency: same phase, scaled.
            let want = spec.magnitude_db(f, 48_000.0 * 8.0);
            let mut num = 0.0;
            let mut den = 0.0;
            for i in 24_000..n {
                num += y[i] * x[i - lat];
                den += x[i - lat] * x[i - lat];
            }
            let gain = num / den;
            assert!(
                (20.0 * gain.log10() - want).abs() < 0.1,
                "{f} Hz: {} dB vs {want}",
                20.0 * gain.log10()
            );
            // In phase: the residual after scaling is tiny.
            let mut res = 0.0;
            for i in 24_000..n {
                res += (y[i] - gain * x[i - lat]).powi(2);
            }
            assert!(res / den < 1e-4, "{f} Hz residual {}", res / den);
        }
    }
}
