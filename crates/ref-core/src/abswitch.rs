//! Click-free, loudness-matched A/B switching between the mix and a reference.
//!
//! Runs on the audio thread: no allocation, no locks. The loudness-match gain is computed
//! elsewhere and handed in with [`AbSwitch::set_reference_gain_db`]; gain changes and source
//! flips both ramp, so nothing clicks.

/// What the listener hears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Mix,
    Reference,
}

/// The switch.
#[derive(Clone, Debug)]
pub struct AbSwitch {
    target: Source,
    /// 0 = all mix, 1 = all reference.
    fade: f32,
    fade_step: f32,
    gain: f32,
    gain_target: f32,
    gain_coeff: f32,
}

/// Highest boost a reference gets when matching (dB); quiet references are not blown up past this.
pub const MAX_MATCH_BOOST_DB: f64 = 12.0;
/// Deepest cut a reference gets when matching (dB).
pub const MAX_MATCH_CUT_DB: f64 = -40.0;

impl AbSwitch {
    /// A switch at `sample_rate` with a `fade_ms` crossfade.
    #[must_use]
    pub fn new(sample_rate: f64, fade_ms: f64) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let fade_samples = (sr * fade_ms.max(1.0) / 1000.0).max(1.0);
        Self {
            target: Source::Mix,
            fade: 0.0,
            fade_step: (1.0 / fade_samples) as f32,
            gain: 1.0,
            gain_target: 1.0,
            // ~50 ms one-pole for gain moves.
            gain_coeff: (1.0 - (-1.0 / (sr * 0.05)).exp()) as f32,
        }
    }

    /// Chooses what to hear. The change fades in over the crossfade time.
    pub fn set_source(&mut self, s: Source) {
        self.target = s;
    }

    /// The source being faded towards.
    #[must_use]
    pub fn source(&self) -> Source {
        self.target
    }

    /// Sets the reference's playback gain (the loudness match), clamped to the match limits.
    pub fn set_reference_gain_db(&mut self, db: f64) {
        let db = if db.is_finite() {
            db.clamp(MAX_MATCH_CUT_DB, MAX_MATCH_BOOST_DB)
        } else {
            0.0
        };
        self.gain_target = 10f64.powf(db / 20.0) as f32;
    }

    /// Whether the reference is audible at all (so the caller can skip reading it).
    #[must_use]
    pub fn reference_audible(&self) -> bool {
        self.fade > 0.0 || self.target == Source::Reference
    }

    /// Mixes in place: `out` holds the mix on entry and what to hear on return.
    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32], ref_l: &[f32], ref_r: &[f32]) {
        let n = out_l
            .len()
            .min(out_r.len())
            .min(ref_l.len())
            .min(ref_r.len());
        let goal = if self.target == Source::Reference {
            1.0
        } else {
            0.0
        };
        for i in 0..n {
            if self.fade < goal {
                self.fade = (self.fade + self.fade_step).min(1.0);
            } else if self.fade > goal {
                self.fade = (self.fade - self.fade_step).max(0.0);
            }
            self.gain += (self.gain_target - self.gain) * self.gain_coeff;
            // Equal-power crossfade: the two programmes are unrelated material.
            let theta = self.fade * std::f32::consts::FRAC_PI_2;
            let (wm, wr) = (theta.cos(), theta.sin() * self.gain);
            out_l[i] = out_l[i] * wm + ref_l[i] * wr;
            out_r[i] = out_r[i] * wm + ref_r[i] * wr;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flipping_ramps_without_jumps() {
        let mut ab = AbSwitch::new(48_000.0, 20.0);
        ab.set_reference_gain_db(0.0);
        let n = 4800;
        let mut l = vec![1.0f32; n];
        let mut r = vec![1.0f32; n];
        let zero = vec![0.0f32; n];
        ab.set_source(Source::Reference);
        ab.process(&mut l, &mut r, &zero, &zero);
        // Mix fades out over ~960 samples, never stepping more than a small amount.
        let max_step = l
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(max_step < 0.01, "{max_step}");
        assert!(l[n - 1].abs() < 1e-6);
        assert!(l[100] > 0.5);
    }

    #[test]
    fn match_gain_is_clamped_and_applied() {
        let mut ab = AbSwitch::new(48_000.0, 5.0);
        ab.set_reference_gain_db(-6.0206);
        ab.set_source(Source::Reference);
        let n = 48_000;
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        let one = vec![1.0f32; n];
        ab.process(&mut l, &mut r, &one, &one);
        assert!((l[n - 1] - 0.5).abs() < 1e-3, "{}", l[n - 1]);
        ab.set_reference_gain_db(100.0);
        ab.process(&mut l, &mut r, &one, &one);
        assert!((l[n - 1] - 10f32.powf(0.6)).abs() < 1e-2, "{}", l[n - 1]);
    }
}
