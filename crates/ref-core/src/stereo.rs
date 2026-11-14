//! Stereo correlation and width.

/// Accumulates stereo statistics. With `decay` < 1 the sums fade, giving a live reading;
/// with `decay` = 1 they cover everything since the last reset.
#[derive(Clone, Copy, Debug)]
pub struct StereoMeter {
    decay: f64,
    ll: f64,
    rr: f64,
    lr: f64,
}

impl StereoMeter {
    /// A meter covering everything since reset.
    #[must_use]
    pub fn cumulative() -> Self {
        Self {
            decay: 1.0,
            ll: 0.0,
            rr: 0.0,
            lr: 0.0,
        }
    }

    /// A meter that forgets with the given time constant.
    #[must_use]
    pub fn live(sample_rate: f64, time_constant_secs: f64) -> Self {
        let n = (sample_rate * time_constant_secs).max(1.0);
        Self {
            decay: (-1.0 / n).exp(),
            ll: 0.0,
            rr: 0.0,
            lr: 0.0,
        }
    }

    /// Clears the sums.
    pub fn reset(&mut self) {
        self.ll = 0.0;
        self.rr = 0.0;
        self.lr = 0.0;
    }

    /// Feeds stereo audio.
    pub fn process(&mut self, left: &[f32], right: &[f32]) {
        let d = self.decay;
        for (&l, &r) in left.iter().zip(right) {
            let (l, r) = (f64::from(l), f64::from(r));
            self.ll = self.ll * d + l * l;
            self.rr = self.rr * d + r * r;
            self.lr = self.lr * d + l * r;
        }
    }

    /// Phase correlation, -1 (out of phase) to +1 (mono). `None` for silence.
    #[must_use]
    pub fn correlation(&self) -> Option<f64> {
        let den = (self.ll * self.rr).sqrt();
        (den > 1e-12).then(|| (self.lr / den).clamp(-1.0, 1.0))
    }

    /// Width: 0 = mono, 1 = as much side as mid (uncorrelated), above 1 = more side than mid.
    #[must_use]
    pub fn width(&self) -> Option<f64> {
        // M = (L+R)/2, S = (L-R)/2.
        let mm = 0.25 * (self.ll + self.rr + 2.0 * self.lr);
        let ss = 0.25 * (self.ll + self.rr - 2.0 * self.lr);
        let total = mm + ss;
        (total > 1e-12).then(|| (2.0 * ss / total).clamp(0.0, 2.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_is_narrow_and_correlated() {
        let s: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut m = StereoMeter::cumulative();
        m.process(&s, &s);
        assert!((m.correlation().unwrap_or(0.0) - 1.0).abs() < 1e-9);
        assert!(m.width().unwrap_or(1.0) < 1e-9);
        let inv: Vec<f32> = s.iter().map(|x| -x).collect();
        let mut m = StereoMeter::cumulative();
        m.process(&s, &inv);
        assert!((m.correlation().unwrap_or(0.0) + 1.0).abs() < 1e-9);
        assert!((m.width().unwrap_or(0.0) - 2.0).abs() < 1e-9);
    }
}
