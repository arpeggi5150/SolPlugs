//! ITU-R BS.1770-4 / EBU R128 loudness: momentary, short-term, gated integrated, and
//! loudness range (EBU Tech 3342).
//!
//! The meter is fed in 100 ms steps. Integrated loudness and loudness range are kept in fixed
//! histograms (0.1 LU bins), so the meter can run forever on the audio thread without allocating.

use crate::biquad::{Biquad, Coeffs, k_highpass, k_shelf};

const MIN_LUFS: f64 = -70.0;
const MAX_LUFS: f64 = 10.0;
const BIN_LU: f64 = 0.1;
const BINS: usize = ((MAX_LUFS - MIN_LUFS) / BIN_LU) as usize;
/// Sub-blocks (100 ms) in a momentary window.
const MOMENTARY_STEPS: usize = 4;
/// Sub-blocks (100 ms) in a short-term window.
const SHORT_STEPS: usize = 30;

/// Loudness of a mean-square block energy, in LUFS.
#[must_use]
pub fn energy_to_lufs(energy: f64) -> f64 {
    if energy > 0.0 {
        -0.691 + 10.0 * energy.log10()
    } else {
        f64::NEG_INFINITY
    }
}

/// The energy a block of the given loudness has.
#[must_use]
pub fn lufs_to_energy(lufs: f64) -> f64 {
    10f64.powf((lufs + 0.691) / 10.0)
}

#[derive(Clone)]
struct Histogram {
    counts: Vec<u64>,
    energy: Vec<f64>,
}

impl Histogram {
    fn new() -> Self {
        Self {
            counts: vec![0; BINS],
            energy: vec![0.0; BINS],
        }
    }

    fn clear(&mut self) {
        self.counts.fill(0);
        self.energy.fill(0.0);
    }

    fn add(&mut self, block_energy: f64) {
        let l = energy_to_lufs(block_energy);
        if l.is_nan() || l <= MIN_LUFS {
            return;
        }
        let i = (((l - MIN_LUFS) / BIN_LU) as usize).min(BINS - 1);
        self.counts[i] += 1;
        self.energy[i] += block_energy;
    }

    fn bin_of(lufs: f64) -> usize {
        if lufs <= MIN_LUFS {
            0
        } else {
            (((lufs - MIN_LUFS) / BIN_LU) as usize).min(BINS - 1)
        }
    }

    /// Mean energy of the blocks at or above `gate_lufs`, and how many there were.
    fn mean_above(&self, gate_lufs: f64) -> Option<(f64, u64)> {
        let start = Self::bin_of(gate_lufs);
        let (mut n, mut e) = (0u64, 0.0);
        for i in start..BINS {
            n += self.counts[i];
            e += self.energy[i];
        }
        (n > 0).then(|| (e / n as f64, n))
    }

    /// The loudness at the given fraction (0..=1) of the blocks at or above `gate_lufs`.
    fn percentile_above(&self, gate_lufs: f64, fraction: f64) -> Option<f64> {
        let start = Self::bin_of(gate_lufs);
        let total: u64 = self.counts[start..].iter().sum();
        if total == 0 {
            return None;
        }
        let target = ((total - 1) as f64 * fraction.clamp(0.0, 1.0)).round() as u64;
        let mut seen = 0u64;
        for i in start..BINS {
            seen += self.counts[i];
            if seen > target {
                return Some(MIN_LUFS + (i as f64 + 0.5) * BIN_LU);
            }
        }
        None
    }
}

/// A stereo (or mono) BS.1770 loudness meter.
#[derive(Clone)]
pub struct LoudnessMeter {
    shelf: Coeffs,
    hp: Coeffs,
    filters: [[Biquad; 2]; 2],
    step_len: usize,
    step_fill: usize,
    step_acc: f64,
    /// Ring of the last [`SHORT_STEPS`] sub-block energies (sum over channels of mean squares).
    steps: [f64; SHORT_STEPS],
    step_pos: usize,
    steps_seen: u64,
    blocks: Histogram,
    short: Histogram,
    momentary_max: f64,
    short_max: f64,
}

impl LoudnessMeter {
    /// A meter for `sample_rate` Hz.
    #[must_use]
    pub fn new(sample_rate: f64) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate >= 8000.0 {
            sample_rate
        } else {
            48_000.0
        };
        Self {
            shelf: k_shelf(sr),
            hp: k_highpass(sr),
            filters: [[Biquad::default(); 2]; 2],
            step_len: ((sr / 10.0).round() as usize).max(1),
            step_fill: 0,
            step_acc: 0.0,
            steps: [0.0; SHORT_STEPS],
            step_pos: 0,
            steps_seen: 0,
            blocks: Histogram::new(),
            short: Histogram::new(),
            momentary_max: f64::NEG_INFINITY,
            short_max: f64::NEG_INFINITY,
        }
    }

    /// Forgets everything measured so far.
    pub fn reset(&mut self) {
        for ch in &mut self.filters {
            for f in ch {
                f.reset();
            }
        }
        self.step_fill = 0;
        self.step_acc = 0.0;
        self.steps = [0.0; SHORT_STEPS];
        self.step_pos = 0;
        self.steps_seen = 0;
        self.blocks.clear();
        self.short.clear();
        self.momentary_max = f64::NEG_INFINITY;
        self.short_max = f64::NEG_INFINITY;
    }

    /// Feeds a block of stereo audio. `right` may be `None` for mono (a mono signal is metered
    /// as one channel, per BS.1770).
    pub fn process(&mut self, left: &[f32], right: Option<&[f32]>) {
        let n = right.map_or(left.len(), |r| r.len().min(left.len()));
        for i in 0..n {
            let mut e = 0.0;
            let l = f64::from(left[i]);
            let [fl0, fl1] = &mut self.filters[0];
            let yl = fl1.tick(&self.hp, fl0.tick(&self.shelf, l));
            e += yl * yl;
            if let Some(r) = right {
                let [fr0, fr1] = &mut self.filters[1];
                let yr = fr1.tick(&self.hp, fr0.tick(&self.shelf, f64::from(r[i])));
                e += yr * yr;
            }
            self.step_acc += e;
            self.step_fill += 1;
            if self.step_fill == self.step_len {
                self.finish_step();
            }
        }
    }

    fn finish_step(&mut self) {
        let e = self.step_acc / self.step_len as f64;
        self.step_acc = 0.0;
        self.step_fill = 0;
        self.steps[self.step_pos] = e;
        self.step_pos = (self.step_pos + 1) % SHORT_STEPS;
        self.steps_seen += 1;
        if self.steps_seen >= MOMENTARY_STEPS as u64 {
            let m = self.window_energy(MOMENTARY_STEPS);
            self.blocks.add(m);
            self.momentary_max = self.momentary_max.max(energy_to_lufs(m));
        }
        if self.steps_seen >= SHORT_STEPS as u64 {
            let s = self.window_energy(SHORT_STEPS);
            self.short.add(s);
            self.short_max = self.short_max.max(energy_to_lufs(s));
        }
    }

    fn window_energy(&self, steps: usize) -> f64 {
        let mut sum = 0.0;
        for k in 1..=steps {
            sum += self.steps[(self.step_pos + SHORT_STEPS - k) % SHORT_STEPS];
        }
        sum / steps as f64
    }

    /// Momentary loudness (last 400 ms), once 400 ms have been seen.
    #[must_use]
    pub fn momentary(&self) -> Option<f64> {
        (self.steps_seen >= MOMENTARY_STEPS as u64)
            .then(|| energy_to_lufs(self.window_energy(MOMENTARY_STEPS)))
    }

    /// Short-term loudness (last 3 s), once 3 s have been seen.
    #[must_use]
    pub fn short_term(&self) -> Option<f64> {
        (self.steps_seen >= SHORT_STEPS as u64)
            .then(|| energy_to_lufs(self.window_energy(SHORT_STEPS)))
    }

    /// Gated integrated loudness (absolute gate -70 LUFS, relative gate -10 LU).
    #[must_use]
    pub fn integrated(&self) -> Option<f64> {
        let (abs_mean, _) = self.blocks.mean_above(MIN_LUFS)?;
        let rel_gate = energy_to_lufs(abs_mean) - 10.0;
        let (mean, _) = self.blocks.mean_above(rel_gate)?;
        Some(energy_to_lufs(mean))
    }

    /// Loudness range in LU (EBU Tech 3342: relative gate -20 LU, 10th to 95th percentile).
    #[must_use]
    pub fn loudness_range(&self) -> Option<f64> {
        let (abs_mean, n) = self.short.mean_above(MIN_LUFS)?;
        if n < 2 {
            return None;
        }
        let rel_gate = energy_to_lufs(abs_mean) - 20.0;
        let lo = self.short.percentile_above(rel_gate, 0.10)?;
        let hi = self.short.percentile_above(rel_gate, 0.95)?;
        Some((hi - lo).max(0.0))
    }

    /// The loudest momentary value so far.
    #[must_use]
    pub fn momentary_max(&self) -> Option<f64> {
        self.momentary_max.is_finite().then_some(self.momentary_max)
    }

    /// The loudest short-term value so far.
    #[must_use]
    pub fn short_term_max(&self) -> Option<f64> {
        self.short_max.is_finite().then_some(self.short_max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, sr: f64, secs: f64, amp_db: f64) -> Vec<f32> {
        let a = 10f64.powf(amp_db / 20.0);
        (0..(sr * secs) as usize)
            .map(|i| (a * (2.0 * std::f64::consts::PI * freq * i as f64 / sr).sin()) as f32)
            .collect()
    }

    #[test]
    fn stereo_1k_sine_at_minus_23_dbfs_reads_minus_23_lufs() {
        // EBU Tech 3341 test 1: a 1 kHz stereo sine at -23 dBFS reads -23.0 LUFS (+/- 0.1).
        for sr in [44_100.0, 48_000.0, 96_000.0] {
            let s = sine(1000.0, sr, 20.0, -23.0);
            let mut m = LoudnessMeter::new(sr);
            m.process(&s, Some(&s));
            let i = m.integrated().unwrap_or(f64::NAN);
            assert!((i + 23.0).abs() < 0.1, "{sr}: {i}");
            let st = m.short_term().unwrap_or(f64::NAN);
            assert!((st + 23.0).abs() < 0.1, "{sr}: {st}");
        }
    }

    #[test]
    fn relative_gate_ignores_the_quiet_part() {
        // Tech 3341 test 3-ish: 10 s at -36, 60 s at -23, 10 s at -36 -> -23.0.
        let sr = 48_000.0;
        let mut m = LoudnessMeter::new(sr);
        for (secs, db) in [(10.0, -36.0), (60.0, -23.0), (10.0, -36.0)] {
            let s = sine(1000.0, sr, secs, db);
            m.process(&s, Some(&s));
        }
        let i = m.integrated().unwrap_or(f64::NAN);
        assert!((i + 23.0).abs() < 0.1, "{i}");
    }

    #[test]
    fn loudness_range_of_two_levels() {
        // Tech 3342 test 1: 20 s at -20 dBFS then 20 s at -30 dBFS -> LRA 10 LU (+/- 1).
        let sr = 48_000.0;
        let mut m = LoudnessMeter::new(sr);
        for db in [-20.0, -30.0] {
            let s = sine(1000.0, sr, 20.0, db);
            m.process(&s, Some(&s));
        }
        let lra = m.loudness_range().unwrap_or(f64::NAN);
        assert!((lra - 10.0).abs() < 1.0, "{lra}");
    }

    #[test]
    fn silence_has_no_integrated_loudness() {
        let mut m = LoudnessMeter::new(48_000.0);
        let z = vec![0.0f32; 48_000 * 5];
        m.process(&z, Some(&z));
        assert!(m.integrated().is_none());
        assert!(m.loudness_range().is_none());
    }
}
