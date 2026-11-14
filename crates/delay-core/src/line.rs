//! Building blocks: a fractional delay line, a state-variable filter, one-pole filters and
//! the saturation curves of each version. Textbook forms, written from scratch.

use std::f32::consts::PI;

/// A delay line with a power-of-two buffer and cubic (Hermite) interpolation.
#[derive(Clone, Debug)]
pub struct Line {
    buf: Vec<f32>,
    mask: usize,
    pos: usize,
}

impl Line {
    /// A line that can delay at least `max` samples.
    pub fn new(max: usize) -> Self {
        let n = (max + 8).next_power_of_two();
        Self {
            buf: vec![0.0; n],
            mask: n - 1,
            pos: 0,
        }
    }

    /// Longest delay it can read, samples.
    pub fn max_delay(&self) -> f32 {
        (self.buf.len() - 4) as f32
    }

    /// The input from `d` samples ago (read before this sample's [`Line::write`]).
    /// `d` below 2 reads at 2; above the capacity at the capacity.
    #[inline]
    pub fn read(&self, d: f32) -> f32 {
        let d = d.clamp(2.0, self.max_delay());
        let k = d as usize;
        let f = d - k as f32;
        let at = |j: usize| self.buf[(self.pos.wrapping_sub(j)) & self.mask];
        let xm1 = at(k - 1);
        let x0 = at(k);
        let x1 = at(k + 1);
        let x2 = at(k + 2);
        if f == 0.0 {
            return x0;
        }
        // Hermite (Catmull-Rom) from x0 toward x1.
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * f + c2) * f + c1) * f + x0
    }

    /// The input exactly `k` samples ago.
    #[inline]
    pub fn at(&self, k: usize) -> f32 {
        self.buf[(self.pos.wrapping_sub(k.max(1))) & self.mask]
    }

    /// Writes this sample's input.
    #[inline]
    pub fn write(&mut self, x: f32) {
        self.buf[self.pos & self.mask] = if x.is_finite() { x } else { 0.0 };
        self.pos = self.pos.wrapping_add(1);
    }

    /// Clears the contents.
    pub fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

/// A two-pole state-variable filter (trapezoidal), low-pass and high-pass outputs.
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    g: f32,
    k: f32,
    ic1: f32,
    ic2: f32,
}

/// Butterworth damping.
const BUTTER_K: f32 = std::f32::consts::SQRT_2;

impl Svf {
    /// Sets the corner, Hz (Butterworth Q).
    pub fn set(&mut self, hz: f32, sr: f32) {
        let f = hz.clamp(5.0, sr * 0.45);
        self.g = (PI * f / sr).tan();
        self.k = BUTTER_K;
    }

    #[inline]
    fn tick(&mut self, x: f32) -> (f32, f32, f32) {
        let a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        let a2 = self.g * a1;
        let a3 = self.g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        let hp = x - self.k * v1 - v2;
        (v2, v1, hp)
    }

    /// Low-pass output.
    #[inline]
    pub fn lp(&mut self, x: f32) -> f32 {
        self.tick(x).0
    }

    /// High-pass output.
    #[inline]
    pub fn hp(&mut self, x: f32) -> f32 {
        self.tick(x).2
    }

    /// Clears the state.
    pub fn clear(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

/// One-pole low-pass coefficient for a corner at `hz`.
#[inline]
pub fn one_pole(hz: f32, sr: f32) -> f32 {
    1.0 - (-2.0 * PI * hz.clamp(1.0, sr * 0.49) / sr).exp()
}

/// A one-pole low-pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct OnePole {
    /// Coefficient (see [`one_pole`]).
    pub a: f32,
    z: f32,
}

impl OnePole {
    /// A filter with coefficient `a`.
    pub fn new(a: f32) -> Self {
        Self { a, z: 0.0 }
    }

    /// Next output.
    #[inline]
    pub fn lp(&mut self, x: f32) -> f32 {
        self.z += (x - self.z) * self.a;
        self.z
    }

    /// Next high-pass output (input minus the low-pass).
    #[inline]
    pub fn hp(&mut self, x: f32) -> f32 {
        x - self.lp(x)
    }
}

/// A Schroeder allpass (smears the repeats on Clean).
#[derive(Clone, Debug)]
pub struct Allpass {
    buf: Vec<f32>,
    pos: usize,
}

impl Allpass {
    /// An allpass of `len` samples.
    pub fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            pos: 0,
        }
    }

    /// Next output with gain `g`.
    #[inline]
    pub fn tick(&mut self, x: f32, g: f32) -> f32 {
        let d = self.buf[self.pos];
        let v = x + g * d;
        self.buf[self.pos] = v;
        self.pos = (self.pos + 1) % self.buf.len();
        d - g * v
    }
}

/// Clean's loop guard: untouched below 0.8, rounds over smoothly to at most 1.0. Self-
/// oscillation settles here instead of growing into a digital scream.
#[inline]
pub fn guard(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.8 {
        x
    } else {
        x.signum() * (0.8 + 0.2 * ((a - 0.8) / 0.2).tanh())
    }
}

/// Tape: a soft, symmetric squash with unity gain for small signals.
#[inline]
pub fn tape_sat(x: f32, amount: f32) -> f32 {
    let k = 0.6 + 2.4 * amount;
    (k * x).tanh() / k
}

/// Bucket-brigade: a harder, slightly asymmetric clip with unity gain for small signals.
#[inline]
pub fn bbd_clip(x: f32, drive: f32) -> f32 {
    let a = 0.4 + 2.0 * drive;
    let v = x + 0.05 * drive * x * x;
    v / (1.0 + a * v.abs())
}

/// The Analog button: warm, asymmetric drive (even harmonics) with unity small-signal gain.
#[inline]
pub fn warmth(x: f32) -> f32 {
    let v = 1.3 * x + 0.25 * x * x;
    v.tanh() / 1.3
}

/// A small noise source (xorshift), uniform -1..1.
#[derive(Clone, Copy, Debug)]
pub struct Noise(u32);

impl Default for Noise {
    fn default() -> Self {
        Self(0x2545_F491)
    }
}

impl Noise {
    /// A source with its own seed.
    pub fn seeded(seed: u32) -> Self {
        Self(seed.max(1))
    }

    /// Next value.
    #[inline]
    pub fn sample(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}
