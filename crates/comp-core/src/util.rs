//! Small DSP helpers: dB conversion, one-pole smoothing, delay lines, a biquad high-pass and a
//! DC blocker.

/// Floor used for level-to-dB conversion (about -180 dBFS).
pub const TINY: f32 = 1e-9;

/// Linear gain to dB.
#[inline]
pub fn lin_to_db(x: f32) -> f32 {
    20.0 * x.max(TINY).log10()
}

/// dB to linear gain.
#[inline]
pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// One-pole coefficient for a time constant in milliseconds at `sr`: the fraction of the gap
/// closed per sample.
#[inline]
pub fn coef(ms: f32, sr: f32) -> f32 {
    let n = ms.max(1e-4) * 0.001 * sr;
    1.0 - (-1.0 / n.max(1e-6)).exp()
}

/// A one-pole parameter smoother: no zipper noise when a knob moves.
#[derive(Clone, Copy, Debug)]
pub struct Smooth {
    v: f32,
    a: f32,
    init: bool,
}

impl Smooth {
    /// A smoother with a `ms` time constant.
    pub fn new(ms: f32, sr: f32) -> Self {
        Self {
            v: 0.0,
            a: coef(ms, sr),
            init: false,
        }
    }

    /// Next value toward `target`. The first call jumps straight there.
    #[inline]
    pub fn next(&mut self, target: f32) -> f32 {
        if !self.init {
            self.v = target;
            self.init = true;
        }
        self.v += (target - self.v) * self.a;
        // Land exactly, so a settled value is bit-stable.
        if (target - self.v).abs() < 1e-6 {
            self.v = target;
        }
        self.v
    }

    /// Current value.
    pub fn value(&self) -> f32 {
        self.v
    }

    /// Jumps to `v`.
    pub fn reset(&mut self, v: f32) {
        self.v = v;
        self.init = true;
    }
}

/// A fixed-capacity integer delay line.
#[derive(Clone, Debug)]
pub struct Delay {
    buf: Vec<f32>,
    pos: usize,
    len: usize,
}

impl Delay {
    /// A delay line that can hold up to `max` samples of delay.
    pub fn new(max: usize) -> Self {
        Self {
            buf: vec![0.0; max + 1],
            pos: 0,
            len: 0,
        }
    }

    /// Sets the delay, samples (clamped to the capacity).
    pub fn set_len(&mut self, len: usize) {
        self.len = len.min(self.buf.len() - 1);
    }

    /// Delay, samples.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the delay is zero.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Pushes `x`, returns the sample from `len` samples ago.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.write(x);
        self.read(self.len)
    }

    /// Pushes `x` without reading.
    #[inline]
    pub fn write(&mut self, x: f32) {
        self.buf[self.pos] = x;
        self.pos = (self.pos + 1) % self.buf.len();
    }

    /// The sample written `k` writes ago (0 = the last one written).
    #[inline]
    pub fn read(&self, k: usize) -> f32 {
        let n = self.buf.len();
        let k = k.min(n - 1);
        self.buf[(self.pos + 2 * n - 1 - k) % n]
    }

    /// Clears the contents.
    pub fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

/// A second-order Butterworth high-pass (the sidechain filter).
#[derive(Clone, Copy, Debug, Default)]
pub struct HighPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    freq: f32,
    on: bool,
}

impl HighPass {
    /// Sets the cutoff. At or below 20 Hz the filter is off (passes the signal untouched).
    pub fn set(&mut self, freq: f32, sr: f32) {
        if (freq - self.freq).abs() < 0.01 {
            return;
        }
        self.freq = freq;
        let was_on = self.on;
        self.on = freq > 20.5;
        if !self.on {
            return;
        }
        if !was_on {
            self.x1 = 0.0;
            self.x2 = 0.0;
            self.y1 = 0.0;
            self.y2 = 0.0;
        }
        let w = std::f32::consts::TAU * freq.min(sr * 0.45) / sr;
        let (s, c) = w.sin_cos();
        let q = std::f32::consts::FRAC_1_SQRT_2;
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha;
        self.b0 = (1.0 + c) / 2.0 / a0;
        self.b1 = -(1.0 + c) / a0;
        self.b2 = (1.0 + c) / 2.0 / a0;
        self.a1 = -2.0 * c / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    /// Filters one sample.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        if !self.on {
            return x;
        }
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// A first-order DC blocker (removes the offset asymmetric saturation makes).
#[derive(Clone, Copy, Debug, Default)]
pub struct DcBlock {
    r: f32,
    x1: f32,
    y1: f32,
}

impl DcBlock {
    /// A blocker with its corner at about 5 Hz.
    pub fn new(sr: f32) -> Self {
        Self {
            r: 1.0 - std::f32::consts::TAU * 5.0 / sr,
            x1: 0.0,
            y1: 0.0,
        }
    }

    /// Filters one sample.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + self.r * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_round_trip() {
        for db in [-60.0f32, -12.0, 0.0, 6.0] {
            assert!((lin_to_db(db_to_lin(db)) - db).abs() < 1e-4);
        }
    }

    #[test]
    fn delay_delays() {
        let mut d = Delay::new(8);
        d.set_len(3);
        let out: Vec<f32> = (1..=6).map(|i| d.process(i as f32)).collect();
        assert_eq!(out, vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn highpass_cuts_lows_keeps_highs() {
        let sr = 48_000.0;
        let gain = |f: f32| {
            let mut h = HighPass::default();
            h.set(200.0, sr);
            let mut peak = 0.0f32;
            for i in 0..48_000 {
                let y = h.process((std::f32::consts::TAU * f * i as f32 / sr).sin());
                if i > 24_000 {
                    peak = peak.max(y.abs());
                }
            }
            lin_to_db(peak)
        };
        assert!(gain(50.0) < -20.0);
        assert!(gain(2000.0).abs() < 0.2);
    }
}
