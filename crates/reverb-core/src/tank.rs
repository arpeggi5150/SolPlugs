//! The reverb tank: a 16-line feedback delay network.
//!
//! - Input diffusion: four Schroeder allpasses per input, their gain set by Diffusion.
//! - 16 delay lines with fractional, slowly modulated read positions (Size scales them).
//! - Each line's loop filter sets the decay: a three-band split (lows below 250 Hz, highs
//!   above the damping corner) whose gains give Low Damping, Decay (the mids) and High
//!   Damping their own decay times. All gains stay below 1, so the tank is stable.
//! - Feedback mixing: four butterfly stages of 2x2 orthogonal rotations. At full
//!   Diffusion each stage is a Hadamard butterfly (every line feeds every other one: a
//!   dense, smooth wash); at zero the rotations close and the lines ring on their own as
//!   separate echoes. Every setting in between is still orthogonal (lossless), so the decay
//!   comes from the loop filters alone.
//! - Freeze: the loop gains go to exactly 1 with no filtering and the input fades out, so
//!   the tail holds forever with no looping artifacts (there is no loop point: the network
//!   keeps circulating what it had).

use std::f32::consts::{PI, TAU};

/// Number of delay lines.
pub const N: usize = 16;
/// Largest size scale a version uses.
pub const MAX_SCALE: f32 = 2.2;
/// Longest line at scale 1, ms.
const LONGEST_MS: f32 = 82.0;
/// Shortest line at scale 1, ms.
const SHORTEST_MS: f32 = 17.0;
/// Most modulation depth, ms.
const MAX_MOD_MS: f32 = 0.45;
/// Crossover between the low and mid bands of the loop filter, Hz.
const LOW_SPLIT_HZ: f32 = 250.0;

/// Line lengths at scale 1, ms: spread geometrically with a little jitter so no two lines
/// share a common period.
fn base_ms(i: usize) -> f32 {
    let t = i as f32 / (N - 1) as f32;
    let jitter = 1.0 + 0.045 * (i as f32 * 2.399).sin();
    SHORTEST_MS * (LONGEST_MS / SHORTEST_MS).powf(t) * jitter
}

/// One-pole coefficient for a corner at `hz`.
#[inline]
pub fn one_pole(hz: f32, sr: f32) -> f32 {
    1.0 - (-TAU * hz.clamp(1.0, sr * 0.49) / sr).exp()
}

/// A Schroeder allpass with an integer delay.
#[derive(Clone, Debug)]
struct Allpass {
    buf: Vec<f32>,
    pos: usize,
    len: usize,
}

impl Allpass {
    fn new(max: usize) -> Self {
        Self {
            buf: vec![0.0; max.max(1)],
            pos: 0,
            len: 1,
        }
    }

    fn set_len(&mut self, n: usize) {
        self.len = n.clamp(1, self.buf.len());
    }

    #[inline]
    fn tick(&mut self, x: f32, g: f32) -> f32 {
        let n = self.buf.len();
        let d = self.buf[(self.pos + n - self.len) % n];
        let v = x + g * d;
        self.buf[self.pos] = v;
        self.pos = (self.pos + 1) % n;
        d - g * v
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

/// One delay line with a fractional, modulated read.
#[derive(Clone, Debug)]
struct Line {
    buf: Vec<f32>,
    pos: usize,
    /// Current length (samples), glides toward the target.
    len: f32,
}

impl Line {
    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let n = self.buf.len();
        let d = delay.clamp(1.0, (n - 2) as f32);
        let i = d.floor();
        let f = d - i;
        let i = i as usize;
        let a = self.buf[(self.pos + n - i) % n];
        let b = self.buf[(self.pos + n - i - 1) % n];
        a + (b - a) * f
    }

    #[inline]
    fn write(&mut self, x: f32) {
        self.pos = (self.pos + 1) % self.buf.len();
        self.buf[self.pos] = x;
    }
}

/// What the tank should do, in plain units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TankParams {
    /// Mid-band RT60, s.
    pub decay_s: f32,
    /// Line length scale (1 = the base set).
    pub scale: f32,
    /// High damping, 0..1.
    pub high_damp: f32,
    /// Low damping, 0..1.
    pub low_damp: f32,
    /// Diffusion, 0..1.
    pub diffusion: f32,
    /// Most input-diffuser gain (0.75 hall, 0.8 plate).
    pub diffuser_max: f32,
    /// Modulation, 0..1.
    pub modulation: f32,
    /// Freeze.
    pub freeze: bool,
}

/// High-damping corner for `high_damp` 0..1: 18 kHz down to 1.2 kHz.
pub fn damping_hz(high_damp: f32) -> f32 {
    18_000.0 * (1200.0f32 / 18_000.0).powf(high_damp.clamp(0.0, 1.0))
}

/// Low-band RT60 relative to the mid band for `low_damp` 0..1.
pub fn low_ratio(low_damp: f32) -> f32 {
    1.0 - 0.8 * low_damp.clamp(0.0, 1.0)
}

/// The mean line length at `scale`, samples (for the decay-curve display).
pub fn mean_len(scale: f32, sr: f32) -> f32 {
    (0..N).map(|i| base_ms(i) * scale).sum::<f32>() / N as f32 * 0.001 * sr
}

/// The FDN.
pub struct Tank {
    sr: f32,
    lines: Vec<Line>,
    target: [f32; N],
    g_mid: [f32; N],
    g_low: [f32; N],
    low_state: [f32; N],
    hi_state: [f32; N],
    a_low: f32,
    a_hi: f32,
    /// Butterfly rotation (cos, sin).
    rot: (f32, f32),
    diff: [[Allpass; 4]; 2],
    diff_g: f32,
    lfo: [f32; N],
    lfo_step: [f32; N],
    mod_depth: f32,
    glide: f32,
    input_gain: f32,
    input_step: f32,
    freeze: bool,
    primed: bool,
    out_l: [f32; N],
    out_r: [f32; N],
    in_l: [f32; N],
    in_r: [f32; N],
}

impl Tank {
    /// A tank at `sr`.
    pub fn new(sr: f32) -> Self {
        let max =
            ((LONGEST_MS * 1.05 * MAX_SCALE + MAX_MOD_MS * 2.0) * 0.001 * sr).ceil() as usize + 8;
        let lines = (0..N)
            .map(|_| Line {
                buf: vec![0.0; max],
                pos: 0,
                len: 100.0,
            })
            .collect();
        let dmax = (0.02 * sr) as usize + 2;
        let ap = || std::array::from_fn(|_| Allpass::new(dmax));
        // Output taps: two orthogonal sign patterns (rows of a Hadamard matrix), so left and
        // right are decorrelated but carry the same energy. Inputs go in on two others.
        let row = |r: usize| -> [f32; N] {
            std::array::from_fn(|i| {
                if (r & i).count_ones().is_multiple_of(2) {
                    1.0
                } else {
                    -1.0
                }
            })
        };
        let norm = 1.0 / (N as f32).sqrt();
        let scale = |a: [f32; N]| a.map(|v| v * norm);
        Self {
            sr,
            lines,
            target: [100.0; N],
            g_mid: [0.0; N],
            g_low: [0.0; N],
            low_state: [0.0; N],
            hi_state: [0.0; N],
            a_low: one_pole(LOW_SPLIT_HZ, sr),
            a_hi: 1.0,
            rot: (
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ),
            diff: [ap(), ap()],
            diff_g: 0.0,
            lfo: std::array::from_fn(|i| i as f32 * 1.7),
            lfo_step: std::array::from_fn(|i| TAU * (0.11 + 0.083 * i as f32) / sr),
            mod_depth: 0.0,
            glide: 1.0 - (-1.0 / (0.08 * sr)).exp(),
            input_gain: 1.0,
            input_step: 1.0 / (0.05 * sr),
            freeze: false,
            primed: false,
            out_l: scale(row(5)),
            out_r: scale(row(10)),
            in_l: scale(row(3)),
            in_r: scale(row(12)),
        }
    }

    /// Clears everything.
    pub fn reset(&mut self) {
        for l in &mut self.lines {
            l.buf.fill(0.0);
        }
        for d in self.diff.iter_mut().flatten() {
            d.clear();
        }
        self.low_state = [0.0; N];
        self.hi_state = [0.0; N];
        self.primed = false;
        self.input_gain = 1.0;
    }

    /// Updates the coefficients (once per control block).
    pub fn set(&mut self, p: &TankParams) {
        let sr = self.sr;
        let scale = p.scale.clamp(0.05, MAX_SCALE);
        for i in 0..N {
            // Whole samples: a frozen (unmodulated) line then reads without interpolation
            // loss.
            self.target[i] = (base_ms(i) * scale * 0.001 * sr).round();
        }
        if !self.primed {
            for (l, t) in self.lines.iter_mut().zip(self.target) {
                l.len = t;
            }
        }
        let rt_mid = p.decay_s.max(0.05);
        let rt_low = rt_mid * low_ratio(p.low_damp);
        for i in 0..N {
            let len = self.target[i];
            if p.freeze {
                self.g_mid[i] = 1.0;
                self.g_low[i] = 1.0;
            } else {
                self.g_mid[i] = 10f32.powf(-3.0 * len / (rt_mid * sr)).min(0.9995);
                self.g_low[i] = 10f32.powf(-3.0 * len / (rt_low * sr)).min(0.9995);
            }
        }
        self.a_hi = if p.freeze {
            1.0
        } else {
            one_pole(damping_hz(p.high_damp), sr)
        };
        let d = p.diffusion.clamp(0.0, 1.0);
        let th = d * PI * 0.25;
        self.rot = (th.cos(), th.sin());
        self.diff_g = p.diffuser_max * d;
        let dscale = scale.clamp(0.3, 1.0);
        for side in &mut self.diff {
            for (k, ap) in side.iter_mut().enumerate() {
                let ms = [4.77, 3.59, 12.73, 9.31][k] * dscale;
                ap.set_len((ms * 0.001 * sr) as usize);
            }
        }
        self.mod_depth = if p.freeze {
            // A frozen tail keeps its pitch: modulation would smear it each pass.
            0.0
        } else {
            p.modulation.clamp(0.0, 1.0) * MAX_MOD_MS * 0.001 * sr
        };
        self.freeze = p.freeze;
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
        let mut xl = l * self.input_gain;
        let mut xr = r * self.input_gain;
        for k in 0..4 {
            xl = self.diff[0][k].tick(xl, self.diff_g);
            xr = self.diff[1][k].tick(xr, self.diff_g);
        }
        let mut y = [0.0f32; N];
        let (mut ol, mut or) = (0.0, 0.0);
        for i in 0..N {
            let line = &mut self.lines[i];
            line.len += (self.target[i] - line.len) * self.glide;
            if (self.target[i] - line.len).abs() < 1e-3 {
                line.len = self.target[i];
            }
            self.lfo[i] += self.lfo_step[i];
            if self.lfo[i] > TAU {
                self.lfo[i] -= TAU;
            }
            let d = line.read(line.len + self.mod_depth * (1.0 + self.lfo[i].sin()));
            // Three-band loop filter: lows (one-pole split), highs above the damping corner.
            self.low_state[i] += (d - self.low_state[i]) * self.a_low;
            let low = self.low_state[i];
            let v = self.g_mid[i] * (d - low) + self.g_low[i] * low;
            self.hi_state[i] += (v - self.hi_state[i]) * self.a_hi;
            let v = self.hi_state[i];
            y[i] = v;
            ol += v * self.out_l[i];
            or += v * self.out_r[i];
        }
        // Four butterfly stages of orthogonal 2x2 reflections.
        let (c, s) = self.rot;
        let mut h = 1;
        while h < N {
            let mut i = 0;
            while i < N {
                for j in i..i + h {
                    let a = y[j];
                    let b = y[j + h];
                    y[j] = c * a + s * b;
                    y[j + h] = s * a - c * b;
                }
                i += 2 * h;
            }
            h *= 2;
        }
        for i in 0..N {
            let v = y[i] + xl * self.in_l[i] + xr * self.in_r[i];
            // Flush denormals and guard against a numeric fault.
            let v = if v.abs() < 1e-20 || !v.is_finite() {
                0.0
            } else {
                v
            };
            self.lines[i].write(v);
        }
        (ol, or)
    }
}
