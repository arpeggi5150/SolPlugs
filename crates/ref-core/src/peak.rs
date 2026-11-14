//! Sample peak and true peak (4x oversampled, BS.1770-4 annex 2 style).

const PHASES: usize = 4;
const TAPS_PER_PHASE: usize = 12;

/// A stereo true-peak meter.
#[derive(Clone)]
pub struct TruePeakMeter {
    /// Polyphase FIR, `PHASES` x `TAPS_PER_PHASE`.
    kernel: [[f64; TAPS_PER_PHASE]; PHASES],
    history: [[f64; TAPS_PER_PHASE]; 2],
    pos: usize,
    sample_peak: f64,
    true_peak: f64,
}

impl Default for TruePeakMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl TruePeakMeter {
    /// A meter with its interpolation filter designed.
    #[must_use]
    pub fn new() -> Self {
        // Windowed-sinc low-pass at the original Nyquist, at 4x rate, Blackman window.
        let len = PHASES * TAPS_PER_PHASE;
        let centre = (len as f64 - 1.0) / 2.0;
        let mut kernel = [[0.0; TAPS_PER_PHASE]; PHASES];
        for n in 0..len {
            let x = (n as f64 - centre) / PHASES as f64;
            let sinc = if x.abs() < 1e-12 {
                1.0
            } else {
                (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
            };
            let w = n as f64 / (len as f64 - 1.0);
            let win = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * w).cos()
                + 0.08 * (4.0 * std::f64::consts::PI * w).cos();
            kernel[n % PHASES][n / PHASES] = sinc * win;
        }
        // Normalise each phase to unity DC gain.
        for phase in &mut kernel {
            let s: f64 = phase.iter().sum();
            if s.abs() > 1e-12 {
                for k in phase.iter_mut() {
                    *k /= s;
                }
            }
        }
        Self {
            kernel,
            history: [[0.0; TAPS_PER_PHASE]; 2],
            pos: 0,
            sample_peak: 0.0,
            true_peak: 0.0,
        }
    }

    /// Forgets the held peaks and filter memory.
    pub fn reset(&mut self) {
        self.history = [[0.0; TAPS_PER_PHASE]; 2];
        self.pos = 0;
        self.sample_peak = 0.0;
        self.true_peak = 0.0;
    }

    /// Feeds a block of stereo (or mono) audio.
    pub fn process(&mut self, left: &[f32], right: Option<&[f32]>) {
        let n = right.map_or(left.len(), |r| r.len().min(left.len()));
        for i in 0..n {
            let l = f64::from(left[i]);
            let r = right.map_or(l, |r| f64::from(r[i]));
            self.history[0][self.pos] = l;
            self.history[1][self.pos] = r;
            self.sample_peak = self.sample_peak.max(l.abs()).max(r.abs());
            for ch in &self.history {
                for phase in &self.kernel {
                    let mut acc = 0.0;
                    for (k, coeff) in phase.iter().enumerate() {
                        acc += coeff * ch[(self.pos + TAPS_PER_PHASE - k) % TAPS_PER_PHASE];
                    }
                    self.true_peak = self.true_peak.max(acc.abs());
                }
            }
            self.pos = (self.pos + 1) % TAPS_PER_PHASE;
        }
        self.true_peak = self.true_peak.max(self.sample_peak);
    }

    /// Highest sample magnitude so far (linear).
    #[must_use]
    pub fn sample_peak(&self) -> f64 {
        self.sample_peak
    }

    /// Highest inter-sample magnitude so far (linear).
    #[must_use]
    pub fn true_peak(&self) -> f64 {
        self.true_peak
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_intersample_peak_of_a_quarter_rate_sine() {
        // fs/4 sine with 45 degree phase: samples hit 0.7071, the true peak is 1.0.
        let s: Vec<f32> = (0..4800)
            .map(|i| {
                ((std::f64::consts::FRAC_PI_2 * i as f64 + std::f64::consts::FRAC_PI_4).sin())
                    as f32
            })
            .collect();
        let mut m = TruePeakMeter::new();
        m.process(&s, None);
        assert!(
            (m.sample_peak() - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01,
            "{}",
            m.sample_peak()
        );
        assert!(
            m.true_peak() > 0.95 && m.true_peak() < 1.05,
            "{}",
            m.true_peak()
        );
    }
}
