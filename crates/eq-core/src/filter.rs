//! Filter building blocks: a band's description ([`BandSpec`]), its digital design
//! ([`Design`], a cascade of up to four sections), the exact magnitude/phase response the
//! graph draws, and a smoothed runtime band ([`Band`]) that never clicks or zippers.
//!
//! Sections are trapezoidal (zero-delay-feedback) state-variable filters and one-pole
//! filters: both are the bilinear transform of their analogue prototypes, so they stay
//! stable and quiet while their coefficients move, and their response is known exactly.

use rustfft::num_complex::Complex64;

/// The shape of a band.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Shape {
    /// Peaking bell.
    #[default]
    Bell,
    /// Low shelf.
    LowShelf,
    /// High shelf.
    HighShelf,
    /// High-pass (cuts the lows).
    LowCut,
    /// Low-pass (cuts the highs).
    HighCut,
    /// Notch.
    Notch,
    /// Band-pass (unity at the centre).
    BandPass,
    /// Tilt: a high shelf with the opposite half-gain below, pivoting at the frequency.
    Tilt,
}

impl Shape {
    /// All shapes, in parameter order.
    pub const ALL: [Shape; 8] = [
        Shape::Bell,
        Shape::LowShelf,
        Shape::HighShelf,
        Shape::LowCut,
        Shape::HighCut,
        Shape::Notch,
        Shape::BandPass,
        Shape::Tilt,
    ];

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            Shape::Bell => "Bell",
            Shape::LowShelf => "Low Shelf",
            Shape::HighShelf => "High Shelf",
            Shape::LowCut => "Low Cut",
            Shape::HighCut => "High Cut",
            Shape::Notch => "Notch",
            Shape::BandPass => "Band Pass",
            Shape::Tilt => "Tilt",
        }
    }

    /// From a parameter index.
    pub fn from_index(i: usize) -> Shape {
        Shape::ALL.get(i).copied().unwrap_or_default()
    }

    /// Parameter index.
    pub fn index(self) -> usize {
        self as usize
    }

    /// Whether the gain control does anything.
    pub fn has_gain(self) -> bool {
        matches!(
            self,
            Shape::Bell | Shape::LowShelf | Shape::HighShelf | Shape::Tilt
        )
    }

    /// Whether the slope control does anything.
    pub fn has_slope(self) -> bool {
        matches!(
            self,
            Shape::LowShelf | Shape::HighShelf | Shape::LowCut | Shape::HighCut
        )
    }
}

/// Slopes a cut or shelf can have, dB per octave.
pub const SLOPES: [u32; 6] = [6, 12, 18, 24, 36, 48];

/// What a band does, in plain units. Two specs with the same structure (shape and slope)
/// can be morphed into each other smoothly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandSpec {
    /// Shape.
    pub shape: Shape,
    /// Centre or corner frequency, Hz.
    pub freq: f64,
    /// Gain, dB (bells, shelves, tilt).
    pub gain_db: f64,
    /// Q (bandwidth; resonance for cuts and shelves).
    pub q: f64,
    /// Slope, dB/octave (cuts and shelves).
    pub slope: u32,
    /// On (an off band passes the signal untouched).
    pub on: bool,
}

impl Default for BandSpec {
    fn default() -> Self {
        Self {
            shape: Shape::Bell,
            freq: 1000.0,
            gain_db: 0.0,
            q: std::f64::consts::FRAC_1_SQRT_2,
            slope: 12,
            on: false,
        }
    }
}

impl BandSpec {
    /// A bell.
    pub fn bell(freq: f64, gain_db: f64, q: f64) -> Self {
        Self {
            shape: Shape::Bell,
            freq,
            gain_db,
            q,
            slope: 12,
            on: true,
        }
    }

    /// A shelf (`high` = high shelf) with a 12 dB/oct slope.
    pub fn shelf(high: bool, freq: f64, gain_db: f64, q: f64) -> Self {
        Self {
            shape: if high {
                Shape::HighShelf
            } else {
                Shape::LowShelf
            },
            freq,
            gain_db,
            q,
            slope: 12,
            on: true,
        }
    }

    /// A gentle 6 dB/oct shelf.
    pub fn shelf6(high: bool, freq: f64, gain_db: f64) -> Self {
        Self {
            slope: 6,
            ..Self::shelf(high, freq, gain_db, std::f64::consts::FRAC_1_SQRT_2)
        }
    }

    /// A Butterworth cut (`high` = high cut / low-pass).
    pub fn cut(high: bool, freq: f64, slope: u32) -> Self {
        Self {
            shape: if high { Shape::HighCut } else { Shape::LowCut },
            freq,
            gain_db: 0.0,
            q: std::f64::consts::FRAC_1_SQRT_2,
            slope,
            on: true,
        }
    }

    /// The same band, switched on or off.
    pub fn with_on(self, on: bool) -> Self {
        Self { on, ..self }
    }

    /// Whether `other` can be reached by smoothing (same shape and slope).
    pub fn same_structure(&self, other: &BandSpec) -> bool {
        self.shape == other.shape && self.slope == other.slope
    }

    /// Designs the filter at sample rate `fs`.
    pub fn design(&self, fs: f64) -> Design {
        design(self, fs)
    }

    /// Complex response at `f` Hz when running at `fs` (1 when off).
    pub fn response(&self, f: f64, fs: f64) -> Complex64 {
        if !self.on {
            return Complex64::new(1.0, 0.0);
        }
        self.design(fs).response(f, fs)
    }

    /// Magnitude at `f`, dB.
    pub fn magnitude_db(&self, f: f64, fs: f64) -> f64 {
        20.0 * self.response(f, fs).norm().max(1e-12).log10()
    }
}

/// One filter section.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Sec {
    /// A wire.
    #[default]
    Off,
    /// State-variable filter: output `m0*in + m1*band + m2*low`.
    Svf {
        /// Warped cutoff, tan(pi f / fs).
        g: f64,
        /// Damping, 1/Q.
        k: f64,
        /// Input mix.
        m0: f64,
        /// Band-pass mix.
        m1: f64,
        /// Low-pass mix.
        m2: f64,
    },
    /// One-pole: output `ml*low + mh*high`.
    Pole {
        /// Warped cutoff.
        g: f64,
        /// Low-pass mix.
        ml: f64,
        /// High-pass mix.
        mh: f64,
    },
}

impl Sec {
    /// One sample through the section; `s` is its two state values.
    #[inline]
    pub fn tick(&self, s: &mut [f64; 2], x: f64) -> f64 {
        match *self {
            Sec::Off => x,
            Sec::Svf { g, k, m0, m1, m2 } => {
                let a1 = 1.0 / (1.0 + g * (g + k));
                let a2 = g * a1;
                let a3 = g * a2;
                let v3 = x - s[1];
                let v1 = a1 * s[0] + a2 * v3;
                let v2 = s[1] + a2 * s[0] + a3 * v3;
                s[0] = 2.0 * v1 - s[0];
                s[1] = 2.0 * v2 - s[1];
                m0 * x + m1 * v1 + m2 * v2
            }
            Sec::Pole { g, ml, mh } => {
                let v = (x - s[0]) * g / (1.0 + g);
                let lp = v + s[0];
                s[0] = lp + v;
                ml * lp + mh * (x - lp)
            }
        }
    }

    /// Response at the warped frequency `t` = tan(pi f / fs).
    pub fn response(&self, t: f64) -> Complex64 {
        match *self {
            Sec::Off => Complex64::new(1.0, 0.0),
            Sec::Svf { g, k, m0, m1, m2 } => {
                let s = Complex64::new(0.0, t / g);
                let d = s * s + s * k + 1.0;
                (s * m1 + m2) / d + m0
            }
            Sec::Pole { g, ml, mh } => {
                let s = Complex64::new(0.0, t / g);
                (s * mh + ml) / (s + 1.0)
            }
        }
    }
}

/// Most sections a design uses (48 dB/oct).
pub const MAX_SECS: usize = 4;

/// A band's digital filter: up to [`MAX_SECS`] sections in series, then a scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Design {
    /// Sections.
    pub secs: [Sec; MAX_SECS],
    /// Sections in use.
    pub n: usize,
    /// Output scale.
    pub scale: f64,
}

impl Default for Design {
    fn default() -> Self {
        Self {
            secs: [Sec::Off; MAX_SECS],
            n: 0,
            scale: 1.0,
        }
    }
}

/// Per-channel state of a design.
pub type DesignState = [[f64; 2]; MAX_SECS];

impl Design {
    /// One sample through the design.
    #[inline]
    pub fn tick(&self, s: &mut DesignState, x: f64) -> f64 {
        let mut y = x;
        for i in 0..self.n {
            y = self.secs[i].tick(&mut s[i], y);
        }
        y * self.scale
    }

    /// Complex response at `f` Hz when running at `fs`.
    pub fn response(&self, f: f64, fs: f64) -> Complex64 {
        let t = warp(f, fs);
        self.response_warped(t)
    }

    /// Complex response at warped frequency `t` (see [`warp`]).
    pub fn response_warped(&self, t: f64) -> Complex64 {
        let mut h = Complex64::new(self.scale, 0.0);
        for s in &self.secs[..self.n] {
            h *= s.response(t);
        }
        h
    }

    fn push(&mut self, s: Sec) {
        if self.n < MAX_SECS {
            self.secs[self.n] = s;
            self.n += 1;
        }
    }
}

/// tan(pi f / fs), with `f` kept just under Nyquist.
#[inline]
pub fn warp(f: f64, fs: f64) -> f64 {
    (std::f64::consts::PI * f.clamp(1.0, fs * 0.4999) / fs).tan()
}

/// Highest frequency a band may sit at for sample rate `fs`.
pub fn max_freq(fs: f64) -> f64 {
    fs * 0.475
}

/// Butterworth section Qs for order `n` (1..=8): the second-order sections, plus whether a
/// first-order pole is needed.
fn butterworth(n: u32) -> ([f64; 4], usize, bool) {
    let mut q = [0.0; 4];
    let pairs = (n / 2) as usize;
    let odd = n % 2 == 1;
    for (i, slot) in q.iter_mut().enumerate().take(pairs.min(4)) {
        let theta = if odd {
            (i + 1) as f64 * std::f64::consts::PI / f64::from(n)
        } else {
            (2 * i + 1) as f64 * std::f64::consts::PI / (2.0 * f64::from(n))
        };
        *slot = 1.0 / (2.0 * theta.cos());
    }
    (q, pairs.min(4), odd)
}

/// Designs `b` at sample rate `fs`. An off band designs to a wire.
pub fn design(b: &BandSpec, fs: f64) -> Design {
    let mut d = Design::default();
    if !b.on {
        return d;
    }
    let f = b.freq.clamp(5.0, max_freq(fs));
    let q = b.q.clamp(0.025, 40.0);
    let t = warp(f, fs);
    let gain = b.gain_db.clamp(-48.0, 48.0);
    match b.shape {
        Shape::Bell => {
            let a = 10f64.powf(gain / 40.0);
            let k = 1.0 / (q * a);
            d.push(Sec::Svf {
                g: t,
                k,
                m0: 1.0,
                m1: k * (a * a - 1.0),
                m2: 0.0,
            });
        }
        Shape::LowShelf | Shape::HighShelf | Shape::Tilt => {
            let high = b.shape != Shape::LowShelf;
            let order = b.slope.clamp(6, 48) / 6;
            // Steeper shelves: the gain split over cascaded sections.
            let (first, pairs) = match order {
                1 => (true, 0),
                2 => (false, 1),
                3 => (true, 1),
                4 => (false, 2),
                5 | 6 => (false, 3),
                _ => (false, 4),
            };
            let parts = (pairs + usize::from(first)) as f64;
            let gp = gain / parts;
            if first {
                d.push(shelf1(high, f, gp, fs));
            }
            for _ in 0..pairs {
                d.push(shelf2(high, f, gp, q, fs));
            }
            if b.shape == Shape::Tilt {
                d.scale = 10f64.powf(-gain / 40.0);
            }
        }
        Shape::LowCut | Shape::HighCut => {
            let high = b.shape == Shape::LowCut;
            let order = (b.slope.clamp(6, 48) / 6).min(8);
            let (qs, pairs, odd) = butterworth(order);
            if odd {
                d.push(Sec::Pole {
                    g: t,
                    ml: if high { 0.0 } else { 1.0 },
                    mh: if high { 1.0 } else { 0.0 },
                });
            }
            // The user Q sets the resonance of the sharpest section.
            let res = q / std::f64::consts::FRAC_1_SQRT_2;
            for (i, sq) in qs.iter().enumerate().take(pairs) {
                let sq = if i + 1 == pairs { sq * res } else { *sq };
                let k = 1.0 / sq.max(0.025);
                d.push(if high {
                    Sec::Svf {
                        g: t,
                        k,
                        m0: 1.0,
                        m1: -k,
                        m2: -1.0,
                    }
                } else {
                    Sec::Svf {
                        g: t,
                        k,
                        m0: 0.0,
                        m1: 0.0,
                        m2: 1.0,
                    }
                });
            }
        }
        Shape::Notch => {
            let k = 1.0 / q;
            d.push(Sec::Svf {
                g: t,
                k,
                m0: 1.0,
                m1: -k,
                m2: 0.0,
            });
        }
        Shape::BandPass => {
            let k = 1.0 / q;
            d.push(Sec::Svf {
                g: t,
                k,
                m0: 0.0,
                m1: k,
                m2: 0.0,
            });
        }
    }
    d
}

/// Second-order shelf, `gain` dB, at `f`.
fn shelf2(high: bool, f: f64, gain: f64, q: f64, fs: f64) -> Sec {
    let a = 10f64.powf(gain / 40.0);
    let k = 1.0 / q.clamp(0.025, 40.0);
    let t = warp(f, fs);
    if high {
        Sec::Svf {
            g: t * a.sqrt(),
            k,
            m0: a * a,
            m1: k * (1.0 - a) * a,
            m2: 1.0 - a * a,
        }
    } else {
        Sec::Svf {
            g: t / a.sqrt(),
            k,
            m0: 1.0,
            m1: k * (a - 1.0),
            m2: a * a - 1.0,
        }
    }
}

/// First-order shelf, `gain` dB, with its midpoint at `f`.
fn shelf1(high: bool, f: f64, gain: f64, fs: f64) -> Sec {
    let a2 = 10f64.powf(gain / 20.0);
    // Corner placed so the geometric midpoint of the transition is `f` (in the warped
    // domain, so the midpoint lands exactly).
    let t = warp(f, fs);
    if high {
        Sec::Pole {
            g: t * a2.sqrt(),
            ml: 1.0,
            mh: a2,
        }
    } else {
        Sec::Pole {
            g: t / a2.sqrt(),
            ml: a2,
            mh: 1.0,
        }
    }
}

/// Combined magnitude of several designs at `f`, dB.
pub fn combined_db(designs: &[Design], f: f64, fs: f64) -> f64 {
    let t = warp(f, fs);
    let mut h = Complex64::new(1.0, 0.0);
    for d in designs {
        h *= d.response_warped(t);
    }
    20.0 * h.norm().max(1e-12).log10()
}

/// Smoothing time for band moves, ms.
pub const BAND_SMOOTH_MS: f64 = 25.0;
/// Fade time for structural changes (shape, slope, on/off), ms.
pub const BAND_FADE_MS: f64 = 12.0;

/// A runtime band: the target spec is reached by smoothing frequency (in octaves), gain
/// (dB) and Q (in octaves); structural changes fade the band out, switch, and fade back in.
/// Each of two channels has its own design so a dynamic band can move them apart.
#[derive(Clone, Debug)]
pub struct Band {
    cur: BandSpec,
    lf: f64,
    gain: f64,
    lq: f64,
    dynamic: [f64; 2],
    /// Wet amount of the band, 0..1.
    on: f64,
    designs: [Design; 2],
    state: [DesignState; 2],
    primed: bool,
}

impl Default for Band {
    fn default() -> Self {
        Self {
            cur: BandSpec::default(),
            lf: 1000f64.ln(),
            gain: 0.0,
            lq: std::f64::consts::FRAC_1_SQRT_2.ln(),
            dynamic: [0.0; 2],
            on: 0.0,
            designs: [Design::default(); 2],
            state: [[[0.0; 2]; MAX_SECS]; 2],
            primed: false,
        }
    }
}

impl Band {
    /// Moves toward `target` by one control step. `dyn_db` is extra gain per channel
    /// (dynamic EQ). `a` is the smoothing fraction for this step, `fade` the fade step.
    pub fn update(&mut self, target: &BandSpec, dyn_db: [f64; 2], fs: f64, a: f64, fade: f64) {
        if !self.primed {
            // First update: start in place, no glide.
            self.primed = true;
            self.cur = *target;
            self.lf = target.freq.max(1.0).ln();
            self.gain = target.gain_db;
            self.lq = target.q.max(0.01).ln();
            self.dynamic = dyn_db;
            self.on = if target.on { 1.0 } else { 0.0 };
            self.redesign(fs);
            return;
        }
        let structural = !self.cur.same_structure(target);
        let want_on = if structural || !target.on { 0.0 } else { 1.0 };
        if self.on > want_on {
            self.on = (self.on - fade).max(want_on);
        } else if self.on < want_on {
            self.on = (self.on + fade).min(want_on);
        }
        if structural && self.on <= 0.0 {
            // Silent now: switch shape and jump to the new place.
            self.cur.shape = target.shape;
            self.cur.slope = target.slope;
            self.lf = target.freq.max(1.0).ln();
            self.gain = target.gain_db;
            self.lq = target.q.max(0.01).ln();
            self.state = [[[0.0; 2]; MAX_SECS]; 2];
        } else if self.on <= 0.0 && !target.on {
            // Off and silent: follow instantly so it comes back where it is set.
            self.lf = target.freq.max(1.0).ln();
            self.gain = target.gain_db;
            self.lq = target.q.max(0.01).ln();
        } else {
            self.lf += (target.freq.max(1.0).ln() - self.lf) * a;
            self.gain += (target.gain_db - self.gain) * a;
            self.lq += (target.q.max(0.01).ln() - self.lq) * a;
        }
        for (d, t) in self.dynamic.iter_mut().zip(dyn_db) {
            *d = t;
        }
        self.redesign(fs);
    }

    fn redesign(&mut self, fs: f64) {
        let base = BandSpec {
            shape: self.cur.shape,
            slope: self.cur.slope,
            freq: self.lf.exp(),
            gain_db: self.gain,
            q: self.lq.exp(),
            on: true,
        };
        for c in 0..2 {
            let spec = BandSpec {
                gain_db: base.gain_db + self.dynamic[c],
                ..base
            };
            self.designs[c] = design(&spec, fs);
        }
    }

    /// Whether the band does anything right now.
    pub fn active(&self) -> bool {
        self.on > 0.0
    }

    /// One sample of channel `c`.
    #[inline]
    pub fn tick(&mut self, c: usize, x: f64) -> f64 {
        if self.on <= 0.0 {
            return x;
        }
        let y = self.designs[c].tick(&mut self.state[c], x);
        if self.on >= 1.0 {
            y
        } else {
            x + (y - x) * self.on
        }
    }

    /// The current (smoothed) design of channel `c`.
    pub fn design(&self, c: usize) -> &Design {
        &self.designs[c]
    }

    /// Clears the filter state.
    pub fn reset(&mut self) {
        self.state = [[[0.0; 2]; MAX_SECS]; 2];
        self.primed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f64 = 48_000.0;

    /// Measures the steady-state gain of a band at `f` by running a sine through it.
    fn measured_db(spec: &BandSpec, f: f64) -> f64 {
        let d = spec.design(FS);
        let mut s = [[0.0; 2]; MAX_SECS];
        let n = 48_000;
        // Amplitude by correlation (the sampled peak of a sine misses its true peak).
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for i in 0..n {
            let ph = std::f64::consts::TAU * f * i as f64 / FS;
            let y = d.tick(&mut s, ph.sin());
            if i >= n / 2 {
                re += y * ph.sin();
                im += y * ph.cos();
            }
        }
        20.0 * ((re * re + im * im).sqrt() * 2.0 / (n / 2) as f64).log10()
    }

    #[test]
    fn response_matches_what_the_filter_does() {
        let specs = [
            BandSpec::bell(1000.0, 9.0, 1.4),
            BandSpec::bell(120.0, -6.0, 0.7),
            BandSpec::shelf(false, 200.0, 6.0, 0.707),
            BandSpec::shelf(true, 6000.0, -8.0, 1.2),
            BandSpec::shelf6(true, 4000.0, 5.0),
            BandSpec::cut(false, 100.0, 24),
            BandSpec::cut(true, 5000.0, 18),
            BandSpec {
                shape: Shape::Tilt,
                ..BandSpec::shelf(true, 1000.0, 6.0, 0.5)
            },
        ];
        for s in specs {
            for f in [50.0, 200.0, 1000.0, 3000.0, 10_000.0] {
                let want = s.magnitude_db(f, FS);
                let got = measured_db(&s, f);
                assert!(
                    (want - got).abs() < 0.05 || want < -40.0,
                    "{s:?} @ {f}: response {want:.2}, measured {got:.2}"
                );
            }
        }
    }

    #[test]
    fn bells_and_shelves_hit_their_gain() {
        let b = BandSpec::bell(1000.0, 6.0, 1.0);
        assert!((b.magnitude_db(1000.0, FS) - 6.0).abs() < 0.01);
        let ls = BandSpec::shelf(false, 200.0, 6.0, 0.707);
        assert!((ls.magnitude_db(20.0, FS) - 6.0).abs() < 0.1);
        assert!((ls.magnitude_db(200.0, FS) - 3.0).abs() < 0.1);
        assert!(ls.magnitude_db(5000.0, FS).abs() < 0.05);
        let hs = BandSpec::shelf6(true, 2000.0, -6.0);
        assert!((hs.magnitude_db(2000.0, FS) + 3.0).abs() < 0.1);
        let lc = BandSpec::cut(false, 100.0, 12);
        assert!((lc.magnitude_db(100.0, FS) + 3.01).abs() < 0.05);
        let lc48 = BandSpec::cut(false, 100.0, 48);
        assert!((lc48.magnitude_db(100.0, FS) + 3.01).abs() < 0.05);
        assert!(lc48.magnitude_db(50.0, FS) < -45.0);
    }

    #[test]
    fn zero_gain_bell_is_a_wire() {
        let d = BandSpec::bell(1000.0, 0.0, 1.0).design(FS);
        let mut s = [[0.0; 2]; MAX_SECS];
        for i in 0..1000 {
            let x = ((i * 7919) % 1000) as f64 / 1000.0 - 0.5;
            assert_eq!(d.tick(&mut s, x), x);
        }
    }

    #[test]
    fn moving_a_band_does_not_click() {
        // Sweep a +12 dB bell from 100 Hz to 10 kHz in 50 ms over a sine: the output must
        // not jump more than the sine itself could.
        let mut b = Band::default();
        let fs = FS;
        let step = 16;
        let a = 1.0 - (-(step as f64) / (BAND_SMOOTH_MS * 0.001 * fs)).exp();
        let fade = step as f64 / (BAND_FADE_MS * 0.001 * fs);
        let mut prev = 0.0;
        let mut worst = 0.0f64;
        for blk in 0..400 {
            let t = (blk as f64 / 150.0).min(1.0);
            let spec = BandSpec::bell(100.0 * 100f64.powf(t), 12.0, 2.0);
            b.update(&spec, [0.0; 2], fs, a, fade);
            for i in 0..step {
                let n = blk * step + i;
                let x = 0.25 * (std::f64::consts::TAU * 440.0 * n as f64 / fs).sin();
                let y = b.tick(0, x);
                if n > 0 {
                    worst = worst.max((y - prev).abs());
                }
                prev = y;
            }
        }
        // A 440 Hz sine at +12 dB moves at most ~0.058 per sample.
        assert!(worst < 0.08, "jump {worst}");
    }
}
