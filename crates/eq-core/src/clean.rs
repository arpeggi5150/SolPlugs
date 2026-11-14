//! Clean: the transparent modern EQ. Up to 24 bands of any shape, each on stereo, mid or
//! side, each with optional dynamics (the band's gain moves when the level in its own
//! frequency range passes a threshold) and solo (hear only what the band touches).
//! Zero-latency (minimum-phase IIR) or linear-phase (FIR, see [`crate::linear`]); a mode
//! change crossfades.

use solplugs_comp_core::util::{Delay, Smooth};

use crate::filter::{
    BAND_FADE_MS, BAND_SMOOTH_MS, Band, BandSpec, Design, DesignState, MAX_SECS, Shape, design,
};
use crate::linear::{self, LinearPhase};
use crate::mixer::{ChainStats, Mixer};
use crate::{CONTROL, CleanBand, CleanSettings, MAX_BANDS, Place, StyleSettings};

/// The level a dynamic band reacts to: its own frequency range of the input.
#[derive(Clone, Debug, Default)]
struct Detector {
    design: Design,
    state: [DesignState; 2],
    env: [f64; 2],
    att: f64,
    rel: f64,
}

impl Detector {
    fn set(&mut self, b: &CleanBand, sr: f64) {
        let f = f64::from(b.freq);
        let q = f64::from(b.q).clamp(0.3, 20.0);
        let spec = match b.shape {
            Shape::Bell | Shape::Notch | Shape::BandPass => BandSpec {
                shape: Shape::BandPass,
                freq: f,
                q,
                ..BandSpec::default().with_on(true)
            },
            Shape::LowShelf | Shape::LowCut => BandSpec::cut(true, f, 12),
            Shape::HighShelf | Shape::HighCut | Shape::Tilt => BandSpec::cut(false, f, 12),
        };
        self.design = design(&spec, sr);
        let c = |ms: f32| 1.0 - (-1.0 / (f64::from(ms).max(0.1) * 0.001 * sr)).exp();
        self.att = c(b.attack_ms);
        self.rel = c(b.release_ms);
    }

    #[inline]
    fn push(&mut self, c: usize, x: f64) {
        let y = self.design.tick(&mut self.state[c], x).abs();
        let e = &mut self.env[c];
        let a = if y > *e { self.att } else { self.rel };
        *e += (y - *e) * a;
    }

    fn level_db(&self, c: usize) -> f64 {
        20.0 * self.env[c].max(1e-9).log10()
    }
}

/// Gain change of a dynamic band at detector level `level` dB: nothing below the
/// threshold, then the gain moves toward `range` over 1.5x its size in dB (about a 3:1
/// curve), and stops at `range`.
pub fn dynamic_gain(level: f64, threshold: f64, range: f64) -> f64 {
    let over = level - threshold;
    if over <= 0.0 || range.abs() < 1e-6 {
        return 0.0;
    }
    range * (over / (1.5 * range.abs())).min(1.0)
}

/// What a band's solo plays: the part of the spectrum it works on.
fn solo_spec(b: &CleanBand) -> BandSpec {
    let f = f64::from(b.freq);
    match b.shape {
        Shape::Bell | Shape::Notch | Shape::BandPass => BandSpec {
            shape: Shape::BandPass,
            freq: f,
            q: f64::from(b.q).clamp(0.3, 20.0),
            ..BandSpec::default().with_on(true)
        },
        // Shelves and cuts: the side of the corner they change.
        Shape::LowShelf | Shape::LowCut => BandSpec::cut(true, f, 12),
        Shape::HighShelf | Shape::HighCut | Shape::Tilt => BandSpec::cut(false, f, 12),
    }
}

/// The Clean chain.
pub struct CleanChain {
    sr: f32,
    bands: Box<[Band; MAX_BANDS]>,
    det: Box<[Detector; MAX_BANDS]>,
    dyn_db: [[f64; 2]; MAX_BANDS],
    place: [Place; MAX_BANDS],
    solo: Band,
    solo_amt: Smooth,
    solo_delay: [Delay; 2],
    lin: Box<LinearPhase>,
    /// 0 = zero-latency path, 1 = linear-phase path.
    lin_amt: f32,
    lin_step: f32,
    mixer: Mixer,
    countdown: usize,
    lin_designs: [[Design; MAX_BANDS]; 2],
}

impl CleanChain {
    /// A Clean chain at `sr`.
    pub fn new(sr: f32) -> Self {
        let max_lat = linear::latency(sr);
        Self {
            sr,
            bands: Box::new(std::array::from_fn(|_| Band::default())),
            det: Box::new(std::array::from_fn(|_| Detector::default())),
            dyn_db: [[0.0; 2]; MAX_BANDS],
            place: [Place::Stereo; MAX_BANDS],
            solo: Band::default(),
            solo_amt: Smooth::new(15.0, sr),
            solo_delay: [Delay::new(max_lat), Delay::new(max_lat)],
            lin: Box::new(LinearPhase::new(sr)),
            lin_amt: -1.0,
            lin_step: 1.0 / (0.02 * sr),
            mixer: Mixer::new(sr, max_lat + 1),
            countdown: 0,
            lin_designs: [[Design::default(); MAX_BANDS]; 2],
        }
    }

    /// Auto-makeup gain, dB.
    pub fn auto_db(&self) -> f32 {
        self.mixer.auto_db()
    }

    /// Dynamic gain per band now, dB (largest move of the two channels).
    pub fn dynamic_db(&self) -> [f32; MAX_BANDS] {
        let mut out = [0.0; MAX_BANDS];
        for (o, d) in out.iter_mut().zip(&self.dyn_db) {
            *o = if d[0].abs() >= d[1].abs() { d[0] } else { d[1] } as f32;
        }
        out
    }

    /// Clears all state.
    /// Doesn't allocate (it runs on the audio thread when the style changes).
    pub fn reset(&mut self) {
        for b in self.bands.iter_mut() {
            b.reset();
        }
        for d in self.det.iter_mut() {
            d.state = [[[0.0; 2]; MAX_SECS]; 2];
            d.env = [0.0; 2];
        }
        self.dyn_db = [[0.0; 2]; MAX_BANDS];
        self.solo.reset();
        self.solo_amt = Smooth::new(15.0, self.sr);
        for d in &mut self.solo_delay {
            d.clear();
        }
        self.lin.reset();
        // Start on whichever path the settings ask for, no fade.
        self.lin_amt = -1.0;
        self.mixer.reset();
        self.countdown = 0;
    }

    fn control(&mut self, cs: &CleanSettings, link: bool) {
        let sr = f64::from(self.sr);
        let n = CONTROL as f64;
        let a = 1.0 - (-n / (BAND_SMOOTH_MS * 0.001 * sr)).exp();
        let fade = n / (BAND_FADE_MS * 0.001 * sr);
        for i in 0..MAX_BANDS {
            let b = &cs.bands[i];
            let spec = b.spec();
            // Dynamics.
            let dynamic = spec.on && b.dynamic;
            if dynamic {
                self.det[i].set(b, sr);
                let lv = [self.det[i].level_db(0), self.det[i].level_db(1)];
                let lv = if link || self.place[i] != Place::Stereo {
                    let m = lv[0].max(lv[1]);
                    [m, m]
                } else {
                    lv
                };
                for c in 0..2 {
                    self.dyn_db[i][c] =
                        dynamic_gain(lv[c], f64::from(b.threshold_db), f64::from(b.range_db));
                }
            } else {
                self.dyn_db[i] = [0.0; 2];
            }
            // A placement change fades the band out, switches, and fades it back in.
            let moving = b.place != self.place[i];
            let target = if moving { spec.with_on(false) } else { spec };
            self.bands[i].update(&target, self.dyn_db[i], sr, a, fade);
            if moving && !self.bands[i].active() {
                self.place[i] = b.place;
                self.bands[i].reset();
            }
        }
        // Solo: the first soloed band.
        let soloed = cs
            .bands
            .iter()
            .find(|b| b.used && b.enabled && b.solo)
            .map(solo_spec);
        let solo_target = soloed.unwrap_or(BandSpec::default());
        self.solo
            .update(&solo_target.with_on(true), [0.0; 2], sr, a, fade);
        // Linear-phase kernels: mid gets stereo + mid bands, side gets stereo + side.
        // Kept current even in zero-latency mode, so switching to linear phase has a
        // kernel and a full history ready.
        {
            let rate = self.lin.design_rate();
            let mut counts = [0usize; 2];
            for (i, b) in cs.bands.iter().enumerate() {
                let spec = b.spec();
                if !spec.on {
                    continue;
                }
                // One kernel per signal: the dynamic gain moves both sides together,
                // quantised so a settled band doesn't rebuild the kernel.
                let [d0, d1] = self.dyn_db[i];
                let d = if d0.abs() >= d1.abs() { d0 } else { d1 };
                let d = (d * 10.0).round() / 10.0;
                let des = BandSpec {
                    gain_db: spec.gain_db + d,
                    ..spec
                }
                .design(rate);
                for (ch, ok) in [(0, b.place != Place::Side), (1, b.place != Place::Mid)] {
                    if ok {
                        self.lin_designs[ch][counts[ch]] = des;
                        counts[ch] += 1;
                    }
                }
            }
            for ch in 0..2 {
                let k = counts[ch];
                let designs = self.lin_designs[ch];
                self.lin.set_bands(ch, &designs[..k]);
            }
        }
    }

    /// The zero-latency path for one sample frame (in place).
    #[inline]
    fn iir(&mut self, l: &mut f64, r: &mut f64, channels: usize) {
        for i in 0..MAX_BANDS {
            let band = &mut self.bands[i];
            if !band.active() {
                continue;
            }
            match self.place[i] {
                Place::Stereo => {
                    *l = band.tick(0, *l);
                    if channels > 1 {
                        *r = band.tick(1, *r);
                    }
                }
                Place::Mid => {
                    if channels > 1 {
                        let m = 0.5 * (*l + *r);
                        let s = 0.5 * (*l - *r);
                        let m = band.tick(0, m);
                        *l = m + s;
                        *r = m - s;
                    } else {
                        *l = band.tick(0, *l);
                    }
                }
                Place::Side => {
                    if channels > 1 {
                        let m = 0.5 * (*l + *r);
                        let s = 0.5 * (*l - *r);
                        let s = band.tick(0, s);
                        *l = m + s;
                        *r = m - s;
                    }
                }
            }
        }
    }

    /// Feeds each dynamic band's detector with the input, in the band's own signal.
    #[inline]
    fn detect(&mut self, cs: &CleanSettings, l: f64, r: f64, channels: usize) {
        for i in 0..MAX_BANDS {
            let b = &cs.bands[i];
            if !(b.used && b.enabled && b.dynamic) {
                continue;
            }
            let d = &mut self.det[i];
            if channels < 2 {
                d.push(0, l);
                continue;
            }
            match self.place[i] {
                Place::Stereo => {
                    d.push(0, l);
                    d.push(1, r);
                }
                Place::Mid => d.push(0, 0.5 * (l + r)),
                Place::Side => d.push(0, 0.5 * (l - r)),
            }
        }
    }

    /// Processes `input` into `out` (1 or 2 channels).
    pub fn process(
        &mut self,
        input: [&[f32]; 2],
        out: [&mut [f32]; 2],
        channels: usize,
        s: &StyleSettings,
        cs: &CleanSettings,
        link: bool,
    ) -> ChainStats {
        let n = input[0].len();
        let mut st = ChainStats::default();
        let lin_target = if cs.linear { 1.0 } else { 0.0 };
        if self.lin_amt < 0.0 {
            // First call: start on the chosen path, no fade.
            self.lin_amt = lin_target;
        }
        let any_solo = cs.bands.iter().any(|b| b.used && b.enabled && b.solo);
        let [o0, o1] = out;
        for i in 0..n {
            if self.countdown == 0 {
                self.control(cs, link);
                self.countdown = CONTROL;
                let lat = if self.lin_amt >= 0.5 {
                    self.lin.latency()
                } else {
                    0
                };
                self.mixer.set_latency(lat);
            }
            self.countdown -= 1;
            if self.lin_amt < lin_target {
                self.lin_amt = (self.lin_amt + self.lin_step).min(1.0);
            } else if self.lin_amt > lin_target {
                self.lin_amt = (self.lin_amt - self.lin_step).max(0.0);
                if self.lin_amt >= 1.0 - self.lin_step {
                    for b in self.bands.iter_mut() {
                        b.reset();
                    }
                }
            }
            let g = self.mixer.step(&s.common);
            let x0 = input[0][i];
            let x1 = if channels > 1 { input[1][i] } else { x0 };
            let (l_in, r_in) = (f64::from(x0 * g.gin), f64::from(x1 * g.gin));
            self.detect(cs, l_in, r_in, channels);

            // Zero-latency path.
            let (mut l, mut r) = (l_in, r_in);
            if self.lin_amt < 1.0 {
                self.iir(&mut l, &mut r, channels);
            }
            // Linear-phase path: always fed, so it is primed when the mode switches.
            let (lm, ls) = if channels > 1 {
                (0.5 * (l_in + r_in), 0.5 * (l_in - r_in))
            } else {
                (l_in, 0.0)
            };
            let (m, s) = self.lin.process(lm, ls);
            if self.lin_amt > 0.0 {
                let (ll, lr) = if channels > 1 { (m + s, m - s) } else { (m, m) };
                let t = f64::from(self.lin_amt);
                l = l + (ll - l) * t;
                r = r + (lr - r) * t;
            }

            // Solo: hear only what the band touches (aligned with the active path).
            let sa = self.solo_amt.next(if any_solo { 1.0 } else { 0.0 });
            let lat = if self.lin_amt >= 0.5 {
                self.lin.latency()
            } else {
                0
            };
            let mut before = 0.0f32;
            let mut after = 0.0f32;
            for (c, wet) in [l, r].into_iter().enumerate().take(channels) {
                let x = if c == 0 { x0 } else { x1 };
                let (pre, mut y) = self.mixer.blend(c, x, wet as f32, &g, &mut st);
                let sd = &mut self.solo_delay[c];
                sd.write(x * g.gin);
                if sa > 0.0 {
                    let solo = self.solo.tick(c, f64::from(sd.read(lat))) as f32;
                    y = y * (1.0 - sa) + solo * sa;
                }
                before = before.max(x.abs());
                after = after.max(pre.abs());
                if c == 0 {
                    o0[i] = y;
                } else {
                    o1[i] = y;
                }
            }
            self.mixer.track(before, after);
        }
        st
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_curve() {
        assert_eq!(dynamic_gain(-40.0, -30.0, -12.0), 0.0);
        assert!((dynamic_gain(-21.0, -30.0, -12.0) + 6.0).abs() < 1e-9);
        assert!((dynamic_gain(0.0, -30.0, -12.0) + 12.0).abs() < 1e-9);
        assert!(dynamic_gain(-20.0, -30.0, 6.0) > 0.0);
    }

    #[test]
    fn solo_band_passes() {
        let b = CleanBand {
            used: true,
            ..CleanBand::default()
        };
        let s = solo_spec(&b);
        assert!(s.magnitude_db(1000.0, 48_000.0).abs() < 0.01);
        assert!(s.magnitude_db(100.0, 48_000.0) < -15.0);
        let _ = MAX_SECS;
    }
}
