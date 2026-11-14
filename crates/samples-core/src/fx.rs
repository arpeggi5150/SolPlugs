//! A sample's processing as set in the manager: gain, low/high cut EQ, fades and normalize.
//! Previews play through it, and Post FX renders apply it.

use serde::{Deserialize, Serialize};

use crate::audio::Audio;
use crate::tools;

/// The processing chain. All off by default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fx {
    /// Gain, dB.
    #[serde(default)]
    pub gain_db: f32,
    /// Low cut (12 dB/oct high-pass), Hz. 0 = off.
    #[serde(default)]
    pub low_cut_hz: f32,
    /// High cut (12 dB/oct low-pass), Hz. 0 = off.
    #[serde(default)]
    pub high_cut_hz: f32,
    /// Fade in at the start, ms.
    #[serde(default)]
    pub fade_in_ms: f32,
    /// Fade out at the end, ms.
    #[serde(default)]
    pub fade_out_ms: f32,
    /// Normalize the result to [`tools::NORMALIZE_DBFS`].
    #[serde(default)]
    pub normalize: bool,
}

impl Default for Fx {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            low_cut_hz: 0.0,
            high_cut_hz: 0.0,
            fade_in_ms: 0.0,
            fade_out_ms: 0.0,
            normalize: false,
        }
    }
}

impl Fx {
    /// True if the chain changes nothing.
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        self.gain_db == 0.0
            && self.low_cut_hz <= 0.0
            && self.high_cut_hz <= 0.0
            && self.fade_in_ms <= 0.0
            && self.fade_out_ms <= 0.0
            && !self.normalize
    }

    /// A short description for an edit's history ("low cut 80 Hz, fade out 20 ms").
    #[must_use]
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.gain_db != 0.0 {
            parts.push(format!("gain {:+.1} dB", self.gain_db));
        }
        if self.low_cut_hz > 0.0 {
            parts.push(format!("low cut {:.0} Hz", self.low_cut_hz));
        }
        if self.high_cut_hz > 0.0 {
            parts.push(format!("high cut {:.0} Hz", self.high_cut_hz));
        }
        if self.fade_in_ms > 0.0 {
            parts.push(format!("fade in {:.0} ms", self.fade_in_ms));
        }
        if self.fade_out_ms > 0.0 {
            parts.push(format!("fade out {:.0} ms", self.fade_out_ms));
        }
        if self.normalize {
            parts.push("normalize".into());
        }
        if parts.is_empty() {
            "no processing".into()
        } else {
            parts.join(", ")
        }
    }

    /// Renders `audio` through the chain.
    #[must_use]
    pub fn render(&self, audio: &Audio) -> Audio {
        let mut out = audio.clone();
        let sr = out.sample_rate;
        let nyq = (sr * 0.5 * 0.95) as f32;
        if self.low_cut_hz > 0.0 && self.low_cut_hz < nyq {
            let c = Coeffs::highpass(sr, f64::from(self.low_cut_hz));
            for ch in &mut out.channels {
                c.run(ch);
            }
        }
        if self.high_cut_hz > 0.0 && self.high_cut_hz < nyq {
            let c = Coeffs::lowpass(sr, f64::from(self.high_cut_hz));
            for ch in &mut out.channels {
                c.run(ch);
            }
        }
        if self.gain_db != 0.0 {
            let g = 10f32.powf(self.gain_db / 20.0);
            for ch in &mut out.channels {
                ch.iter_mut().for_each(|v| *v *= g);
            }
        }
        let n = out.frames();
        if self.fade_in_ms > 0.0 {
            let len = ((f64::from(self.fade_in_ms) / 1000.0) * sr) as usize;
            tools::fade_in(&mut out, 0, len.min(n));
        }
        if self.fade_out_ms > 0.0 {
            let len = (((f64::from(self.fade_out_ms) / 1000.0) * sr) as usize).min(n);
            tools::fade_out(&mut out, n - len, n);
        }
        if self.normalize {
            tools::normalize(&mut out);
        }
        out
    }
}

/// RBJ biquad coefficients, Butterworth Q.
#[derive(Clone, Copy, Debug)]
struct Coeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Coeffs {
    fn lowpass(sr: f64, f: f64) -> Self {
        let w = 2.0 * std::f64::consts::PI * f / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - c) / 2.0 / a0,
            b1: (1.0 - c) / a0,
            b2: (1.0 - c) / 2.0 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha) / a0,
        }
    }

    fn highpass(sr: f64, f: f64) -> Self {
        let w = 2.0 * std::f64::consts::PI * f / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 + c) / 2.0 / a0,
            b1: -(1.0 + c) / a0,
            b2: (1.0 + c) / 2.0 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha) / a0,
        }
    }

    fn run(&self, x: &mut [f32]) {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for v in x.iter_mut() {
            let x0 = f64::from(*v);
            let y0 = self.b0 * x0 + self.b1 * x1 + self.b2 * x2 - self.a1 * y1 - self.a2 * y2;
            x2 = x1;
            x1 = x0;
            y2 = y1;
            y1 = y0;
            *v = y0 as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(f: f64, sr: f64, n: usize) -> Audio {
        Audio::new(
            sr,
            vec![
                (0..n)
                    .map(|i| (2.0 * std::f64::consts::PI * f * i as f64 / sr).sin() as f32 * 0.5)
                    .collect(),
            ],
        )
    }

    fn rms(a: &Audio) -> f32 {
        let c = &a.channels[0][a.frames() / 2..];
        (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt()
    }

    #[test]
    fn neutral_changes_nothing() {
        let a = sine(440.0, 48_000.0, 4800);
        assert!(Fx::default().is_neutral());
        assert_eq!(Fx::default().render(&a), a);
    }

    #[test]
    fn low_cut_removes_lows_keeps_highs() {
        let fx = Fx {
            low_cut_hz: 500.0,
            ..Fx::default()
        };
        let low = fx.render(&sine(50.0, 48_000.0, 48_000));
        let high = fx.render(&sine(5000.0, 48_000.0, 48_000));
        assert!(rms(&low) < 0.01, "{}", rms(&low));
        assert!((rms(&high) - 0.3536).abs() < 0.01);
    }

    #[test]
    fn normalize_hits_minus_one() {
        let fx = Fx {
            normalize: true,
            gain_db: -12.0,
            ..Fx::default()
        };
        let out = fx.render(&sine(440.0, 48_000.0, 4800));
        let db = 20.0 * out.peak().log10();
        assert!((db - tools::NORMALIZE_DBFS).abs() < 0.01, "{db}");
    }
}
