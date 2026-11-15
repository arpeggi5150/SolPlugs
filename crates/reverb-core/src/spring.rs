//! The spring tank: a tube driver into one to three springs.
//!
//! A real spring is dispersive: low frequencies travel slower than highs, so every
//! bounce off the end of the spring arrives as a falling chirp, the "drip". Each spring
//! here is a feedback loop: a delay (the spring's length), a chain of first-order
//! allpasses (the dispersion; Diffusion sets how strong), a band-pass (the transducers
//! only pass about 100 Hz to a few kHz; Damping moves the top) and the loop gain (Decay).
//! The springs have different lengths so their bounces don't line up, and are spread
//! across left and right.

use crate::tank::one_pole;

/// Most springs.
pub const MAX_SPRINGS: usize = 3;
/// Allpass stages per spring.
const STAGES: usize = 28;
/// Spring loop lengths at Size 50%, ms.
const LENGTHS_MS: [f32; MAX_SPRINGS] = [37.0, 44.3, 51.9];

/// Spring settings in plain units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringParams {
    /// RT60, s.
    pub decay_s: f32,
    /// Length scale (Size): 0.6..1.6.
    pub scale: f32,
    /// Top of the transducer band, Hz.
    pub top_hz: f32,
    /// Bottom of the band, Hz.
    pub bottom_hz: f32,
    /// Dispersion amount 0..1.
    pub dispersion: f32,
    /// Springs in use, 1..3.
    pub springs: usize,
    /// Tube drive 0..1.
    pub tube: f32,
    /// Freeze.
    pub freeze: bool,
}

#[derive(Clone, Debug)]
struct Spring {
    buf: Vec<f32>,
    pos: usize,
    len: f32,
    target: f32,
    ap: [f32; STAGES],
    ap_x: [f32; STAGES],
    lp: f32,
    hp: f32,
    g: f32,
}

impl Spring {
    fn new(max: usize) -> Self {
        Self {
            buf: vec![0.0; max],
            pos: 0,
            len: 1000.0,
            target: 1000.0,
            ap: [0.0; STAGES],
            ap_x: [0.0; STAGES],
            lp: 0.0,
            hp: 0.0,
            g: 0.0,
        }
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.ap = [0.0; STAGES];
        self.ap_x = [0.0; STAGES];
        self.lp = 0.0;
        self.hp = 0.0;
    }

    #[inline]
    fn read(&self) -> f32 {
        let n = self.buf.len();
        let d = self.len.clamp(1.0, (n - 2) as f32);
        let i = d.floor();
        let f = d - i;
        let i = i as usize;
        let a = self.buf[(self.pos + n - i) % n];
        let b = self.buf[(self.pos + n - i - 1) % n];
        a + (b - a) * f
    }
}

/// The tank: springs plus the tube driver.
pub struct SpringTank {
    sr: f32,
    springs: Vec<Spring>,
    count: usize,
    apc: f32,
    a_lp: f32,
    a_hp: f32,
    drive: f32,
    glide: f32,
    freeze: bool,
    input_gain: f32,
    input_step: f32,
    primed: bool,
}

/// The tube driver: asymmetric soft clip, unity gain for small signals.
#[inline]
pub fn tube(x: f32, drive: f32) -> f32 {
    if drive <= 0.0 {
        return x;
    }
    let k = 1.0 + drive * 6.0;
    let bias = 0.18 * drive;
    let v = k * x + bias;
    ((v.tanh() - bias.tanh()) / k) * (1.0 + drive * 0.6)
}

impl SpringTank {
    /// A tank at `sr`.
    pub fn new(sr: f32) -> Self {
        let max = (LENGTHS_MS[2] * 1.7 * 0.001 * sr) as usize + 8;
        Self {
            sr,
            springs: (0..MAX_SPRINGS).map(|_| Spring::new(max)).collect(),
            count: MAX_SPRINGS,
            apc: 0.6,
            a_lp: 0.5,
            a_hp: 0.01,
            drive: 0.0,
            glide: 1.0 - (-1.0 / (0.08 * sr)).exp(),
            freeze: false,
            input_gain: 1.0,
            input_step: 1.0 / (0.05 * sr),
            primed: false,
        }
    }

    /// Clears everything.
    pub fn reset(&mut self) {
        for s in &mut self.springs {
            s.clear();
        }
        self.primed = false;
        self.input_gain = 1.0;
    }

    /// Updates the coefficients (once per control block).
    pub fn set(&mut self, p: &SpringParams) {
        let sr = self.sr;
        self.count = p.springs.clamp(1, MAX_SPRINGS);
        self.apc = 0.35 + 0.4 * p.dispersion.clamp(0.0, 1.0);
        // Each allpass stage adds (1 + a) / (1 - a) samples of delay at DC: part of the loop.
        let ap_delay = STAGES as f32 * (1.0 + self.apc) / (1.0 - self.apc);
        self.a_lp = if p.freeze {
            1.0
        } else {
            one_pole(p.top_hz, sr)
        };
        // The high-pass stays on when frozen (it keeps DC out of a loop with no loss), but
        // drops to 20 Hz so the held tail keeps its body.
        self.a_hp = one_pole(if p.freeze { 20.0 } else { p.bottom_hz }, sr);
        self.drive = p.tube.clamp(0.0, 1.0);
        self.freeze = p.freeze;
        for (i, s) in self.springs.iter_mut().enumerate() {
            // Whole samples: a frozen loop then reads without interpolation loss.
            s.target = (LENGTHS_MS[i] * p.scale.clamp(0.5, 1.6) * 0.001 * sr)
                .round()
                .max(8.0);
            if !self.primed {
                s.len = s.target;
            }
            let loop_s = (s.target + ap_delay) / sr;
            s.g = if p.freeze {
                1.0
            } else {
                10f32.powf(-3.0 * loop_s / p.decay_s.max(0.05)).min(0.995)
            };
        }
        self.primed = true;
    }

    /// One stereo sample in, one out.
    #[inline]
    pub fn tick(&mut self, l: f32, r: f32) -> (f32, f32) {
        let tgt = if self.freeze { 0.0 } else { 1.0 };
        if self.input_gain < tgt {
            self.input_gain = (self.input_gain + self.input_step).min(tgt);
        } else if self.input_gain > tgt {
            self.input_gain = (self.input_gain - self.input_step).max(tgt);
        }
        let x = tube((l + r) * 0.5, self.drive) * self.input_gain;
        let (mut ol, mut or) = (0.0, 0.0);
        let a = self.apc;
        for (i, s) in self.springs.iter_mut().enumerate().take(self.count) {
            s.len += (s.target - s.len) * self.glide;
            if (s.target - s.len).abs() < 1e-3 {
                s.len = s.target;
            }
            let mut v = s.read();
            // Dispersion: first-order allpasses, y = -a x + x1 + a y1.
            for k in 0..STAGES {
                let y = -a * v + s.ap_x[k] + a * s.ap[k];
                s.ap_x[k] = v;
                s.ap[k] = y;
                v = y;
            }
            // Transducer band: one-pole low-pass, then a one-pole high-pass.
            s.lp += (v - s.lp) * self.a_lp;
            s.hp += (s.lp - s.hp) * self.a_hp;
            let out = s.lp - s.hp;
            let w = x + out * s.g;
            let w = if w.abs() < 1e-20 || !w.is_finite() {
                0.0
            } else {
                w
            };
            s.pos = (s.pos + 1) % s.buf.len();
            s.buf[s.pos] = w;
            // Spread the springs: first left, second right, third centre.
            let (pl, pr) = match i {
                0 => (0.85, 0.35),
                1 => (0.35, 0.85),
                _ => (0.6, 0.6),
            };
            ol += out * pl;
            or += out * pr;
        }
        let norm = 1.0 / (self.count as f32).sqrt();
        (ol * norm, or * norm)
    }
}
