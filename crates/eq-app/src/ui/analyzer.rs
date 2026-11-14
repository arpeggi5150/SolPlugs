//! The spectrum analyzer (UI thread): an 8192-point FFT of the last samples of the input
//! (BEFORE) and output (AFTER), Hann-windowed, calibrated so a full-scale sine reads
//! 0 dBFS, with fast rise and slow fall like a hardware analyzer.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

use crate::engine::Engine;

/// FFT size.
pub const N: usize = 8192;

/// One tap's history and smoothed spectrum.
struct Tap {
    ring: Vec<f32>,
    pos: usize,
    /// Smoothed level per bin, dBFS.
    db: Vec<f32>,
    fresh: usize,
}

impl Tap {
    fn new() -> Self {
        Self {
            ring: vec![0.0; N],
            pos: 0,
            db: vec![-140.0; N / 2 + 1],
            fresh: 0,
        }
    }

    fn push(&mut self, v: f32) {
        self.ring[self.pos] = v;
        self.pos = (self.pos + 1) % N;
        self.fresh += 1;
    }
}

/// Before and after spectra.
pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buf: Vec<Complex32>,
    scratch: Vec<Complex32>,
    taps: [Tap; 2],
    sr: f32,
    norm: f32,
}

impl Default for Analyzer {
    fn default() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(N);
        let window: Vec<f32> = (0..N)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos())
            .collect();
        let sum: f32 = window.iter().sum();
        let scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
        Self {
            fft,
            window,
            buf: vec![Complex32::default(); N],
            scratch,
            taps: [Tap::new(), Tap::new()],
            sr: 48_000.0,
            // A sine of amplitude 1 gives a bin of sum/2.
            norm: 2.0 / sum,
        }
    }
}

/// Which spectrum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    /// The dry input.
    Before = 0,
    /// The output.
    After = 1,
}

impl Analyzer {
    /// Pulls new samples from the engine and updates the spectra. `dt` is the frame time.
    pub fn update(&mut self, engine: &Engine, dt: f32) {
        self.sr = engine.sample_rate();
        let [b, a] = &mut self.taps;
        engine.drain_spectrum(|v| b.push(v), |v| a.push(v));
        let rise = 1.0 - (-dt / 0.03).exp();
        // Falls about 30 dB per second.
        let fall = 30.0 * dt.clamp(0.0, 0.1);
        for t in 0..2 {
            if self.taps[t].fresh == 0 {
                // No audio (transport stopped): let it fall.
                for v in &mut self.taps[t].db {
                    *v = (*v - fall).max(-140.0);
                }
                continue;
            }
            self.taps[t].fresh = 0;
            let tap = &self.taps[t];
            for i in 0..N {
                let s = tap.ring[(tap.pos + i) % N];
                self.buf[i] = Complex32::new(s * self.window[i], 0.0);
            }
            self.fft.process_with_scratch(&mut self.buf, &mut self.scratch);
            let norm = self.norm;
            let tap = &mut self.taps[t];
            for (k, v) in tap.db.iter_mut().enumerate() {
                let m = self.buf[k].norm() * norm;
                let db = 20.0 * m.max(1e-7).log10();
                *v = if db > *v {
                    *v + (db - *v) * rise.max(0.5)
                } else {
                    (*v - fall).max(db)
                };
            }
        }
    }

    /// Level at `f`, dBFS: the loudest bin between `f / spread` and `f * spread` (so each
    /// pixel shows its whole slice of the spectrum).
    pub fn level(&self, which: Which, f: f32, spread: f32) -> f32 {
        let bin_hz = self.sr / N as f32;
        let db = &self.taps[which as usize].db;
        let lo = ((f / spread) / bin_hz).ceil().max(1.0) as usize;
        let hi = ((f * spread) / bin_hz).floor() as usize;
        let hi = hi.min(db.len() - 1);
        // Fewer than three bins in this pixel's slice: the bins are wider than the
        // pixels, so draw a smooth line through them instead of flat steps.
        if hi < lo + 2 {
            // Between bins: interpolate.
            let x = (f / bin_hz).clamp(1.0, (db.len() - 2) as f32);
            let i = x.floor() as usize;
            let t = x - i as f32;
            return db[i] * (1.0 - t) + db[i + 1] * t;
        }
        db[lo..=hi].iter().copied().fold(-140.0, f32::max)
    }

    /// The loudest frequency within a third of an octave of `f` (spectrum grab).
    pub fn peak_near(&self, which: Which, f: f32) -> Option<f32> {
        let bin_hz = self.sr / N as f32;
        let db = &self.taps[which as usize].db;
        let lo = ((f / 1.26) / bin_hz).floor().max(1.0) as usize;
        let hi = (((f * 1.26) / bin_hz).ceil() as usize).min(db.len() - 2);
        let (k, v) = (lo..=hi)
            .map(|k| (k, db[k]))
            .fold((0, -140.0f32), |a, b| if b.1 > a.1 { b } else { a });
        // Nothing there, or only the slope of something outside the window: no peak.
        if v < -100.0 || k == 0 || k == lo || k == hi {
            return None;
        }
        // Parabolic interpolation between neighbouring bins.
        let (a, b, c) = (db[k - 1], db[k], db[k + 1]);
        let d = a - 2.0 * b + c;
        let off = if d.abs() > 1e-6 {
            (0.5 * (a - c) / d).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        Some((k as f32 + off) * bin_hz)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::sync::Arc;

    use solplugs_eq_core::Style;
    use solplugs_eq_core::style::neve;

    use super::*;
    use crate::params::Pid;

    fn feed(engine: &Arc<Engine>, an: &mut Analyzer, f: f32, amp: f32) {
        let mut p = engine.processor(48_000.0, 512);
        for block in 0..60 {
            let mut l: Vec<f32> = (0..800)
                .map(|i| {
                    let t = (block * 800 + i) as f32 / 48_000.0;
                    amp * (std::f32::consts::TAU * f * t).sin()
                })
                .collect();
            let mut r = l.clone();
            p.process(&mut [&mut l, &mut r]);
            an.update(engine, 1.0 / 60.0);
        }
    }

    /// Acceptance 6: the before/after curves show what the EQ does, at the right level.
    #[test]
    fn t06_analyzer_shows_the_level_and_the_eq() {
        let engine = Arc::new(Engine::new());
        engine.set_style(Style::Neve);
        engine.set(Pid::k(Style::Neve, neve::MID_FREQ), 3.0); // 3.2 kHz
        engine.set(Pid::k(Style::Neve, neve::MID_GAIN), 6.0);
        let mut an = Analyzer::default();
        feed(&engine, &mut an, 3200.0, 0.1);
        let spread = 2f32.powf(1.0 / 48.0);
        let before = an.level(Which::Before, 3200.0, spread);
        let after = an.level(Which::After, 3200.0, spread);
        // A sine at amplitude 0.1 is -20 dBFS.
        assert!((before + 20.0).abs() < 0.5, "{before}");
        let lift = after - before;
        assert!((lift - 6.0).abs() < 1.0, "{lift}");
    }

    /// Acceptance 17: spectrum grab finds the peak a click lands near.
    #[test]
    fn t17_spectrum_grab_finds_the_peak() {
        let engine = Arc::new(Engine::new());
        let mut an = Analyzer::default();
        feed(&engine, &mut an, 1234.0, 0.3);
        let f = an.peak_near(Which::Before, 1150.0).unwrap();
        assert!((f - 1234.0).abs() < 3.0, "{f}");
        assert!(an.peak_near(Which::Before, 60.0).is_none());
    }
}
