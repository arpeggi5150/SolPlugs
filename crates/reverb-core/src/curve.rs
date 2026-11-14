//! The decay curve the display draws: the tail's level over time, computed from the same
//! numbers the tank uses, for the lows, the mids and the highs.

use crate::tank::{self, damping_hz, low_ratio};
use crate::{Version, VersionSettings, spring_params};

/// Which part of the spectrum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// Around 100 Hz.
    Low,
    /// Around 1 kHz (this is Decay).
    Mid,
    /// Around 8 kHz.
    High,
}

/// RT60 of `band` for these settings, s (infinite when frozen).
pub fn rt60(v: Version, s: &VersionSettings, band: Band, sr: f32) -> f32 {
    if s.freeze {
        return f32::INFINITY;
    }
    let mid = s.decay_s.max(0.05);
    match band {
        Band::Mid => mid,
        Band::Low => mid * low_ratio(s.low_damp),
        Band::High => {
            // Extra loss per pass from the damping low-pass at 8 kHz, spread over the mean
            // loop length.
            let (fc, len) = if v == Version::Spring {
                let p = spring_params(s);
                (p.top_hz, 0.045 * p.scale * sr)
            } else {
                (damping_hz(s.high_damp), tank::mean_len(v.scale(s.size), sr))
            };
            let a = tank::one_pole(fc, sr);
            let w = std::f32::consts::TAU * 8000.0f32.min(sr * 0.45) / sr;
            // One-pole low-pass magnitude at w.
            let b = 1.0 - a;
            let mag2 = a * a / (1.0 + b * b - 2.0 * b * w.cos());
            let loss_db = -10.0 * mag2.max(1e-9).log10();
            let per_s_mid = 60.0 / mid;
            let per_s = per_s_mid + loss_db * sr / len.max(1.0);
            60.0 / per_s
        }
    }
}

/// Echo build-up time, ms: how long until the tail is fully dense (lower Diffusion and
/// bigger spaces take longer).
pub fn build_up_ms(v: Version, s: &VersionSettings) -> f32 {
    let base = match v {
        Version::Plate => 4.0,
        Version::Room => 8.0,
        Version::Spring => 25.0,
        Version::Clean => 20.0,
        Version::Hall => 35.0,
    };
    base * v.scale(s.size).max(0.2) * (1.6 - s.diffusion.clamp(0.0, 1.0))
}

/// Level of the tail at `t_ms` after the input, dB (0 = the peak).
pub fn level_db(v: Version, s: &VersionSettings, band: Band, t_ms: f32, sr: f32) -> f32 {
    let pre = s.predelay_ms.max(0.0);
    if t_ms < pre {
        return -90.0;
    }
    let t = t_ms - pre;
    let rise = build_up_ms(v, s).max(0.5);
    let rt = rt60(v, s, band, sr);
    let decay = if rt.is_finite() {
        -60.0 * (t / 1000.0) / rt
    } else {
        0.0
    };
    // Smooth onset: rises over the build-up, then the exponential decay.
    let onset = 1.0 - (-t / rise * 2.5).exp();
    let onset_db = 20.0 * onset.max(1e-5).log10();
    (decay + onset_db).max(-90.0)
}
