//! The wet-only chain, run on the reverb before it meets the dry signal:
//! saturation (4x oversampled) -> EQ (low cut, low shelf, mid bell, high shelf, high cut)
//! -> compressor -> stereo stage (width, with its own wet/dry) -> ping-pong stage (with its
//! own wet/dry). The dry signal never passes through any of it.

use solplugs_comp_core::oversample::Oversampler;
use solplugs_comp_core::util::Smooth;
use solplugs_comp_core::{Chain, Mode, Settings as CompSettings};
use solplugs_eq_core::filter::{BAND_FADE_MS, BAND_SMOOTH_MS, Band, BandSpec};

use crate::{CHUNK, CONTROL, VersionSettings};

/// Low shelf corner, Hz.
pub const LOW_SHELF_HZ: f64 = 250.0;
/// High shelf corner, Hz.
pub const HIGH_SHELF_HZ: f64 = 5000.0;
/// Mid bell Q.
pub const MID_Q: f64 = 0.9;
/// Ping-pong feedback.
const PP_FEEDBACK: f32 = 0.45;
/// Longest ping-pong time, ms.
const PP_MAX_MS: f32 = 1000.0;

/// The saturation curve for `amount` 0..1: a soft, slightly asymmetric clip with unity
/// gain for small signals. Pushed hard it rounds over smoothly, never a hard edge.
#[inline]
pub fn saturate(x: f32, amount: f32) -> f32 {
    if amount <= 0.0 {
        return x;
    }
    let k = 1.0 + amount * 5.0;
    let bias = 0.12 * amount;
    let v = k * x + bias;
    (v.tanh() - bias.tanh()) / k * (1.0 + 0.5 * amount)
}

/// The wet EQ's five bands for these settings.
pub fn eq_bands(s: &VersionSettings) -> [BandSpec; 5] {
    [
        BandSpec::cut(false, f64::from(s.low_cut_hz), 12).with_on(s.low_cut_hz > 20.5),
        BandSpec::shelf(
            false,
            LOW_SHELF_HZ,
            f64::from(s.low_db),
            std::f64::consts::FRAC_1_SQRT_2,
        )
        .with_on(s.low_db.abs() > 0.01),
        BandSpec::bell(f64::from(s.mid_hz), f64::from(s.mid_db), MID_Q)
            .with_on(s.mid_db.abs() > 0.01),
        BandSpec::shelf(
            true,
            HIGH_SHELF_HZ,
            f64::from(s.high_db),
            std::f64::consts::FRAC_1_SQRT_2,
        )
        .with_on(s.high_db.abs() > 0.01),
        BandSpec::cut(true, f64::from(s.high_cut_hz), 12).with_on(s.high_cut_hz < 19_900.0),
    ]
}

/// The compressor settings for these controls.
pub fn comp_settings(s: &VersionSettings) -> CompSettings {
    CompSettings {
        threshold_db: s.threshold_db,
        ratio: if s.threshold_db >= -0.05 {
            1.0
        } else {
            s.ratio
        },
        attack_ms: s.attack_ms,
        release_ms: s.release_ms,
        knee_db: 6.0,
        lookahead_ms: 0.0,
        colour: false,
        // Program-dependent release: no pumping on a tail.
        auto_release: true,
        ..CompSettings::default()
    }
}

/// The wet chain.
pub struct Wet {
    sr: f32,
    os: [Oversampler; 2],
    sat: Smooth,
    bands: [Band; 5],
    comp: Box<Chain>,
    width: Smooth,
    stereo_mix: Smooth,
    pp_mix: Smooth,
    pp_time: Smooth,
    pp: [Vec<f32>; 2],
    pp_pos: usize,
    scratch: [[f32; CHUNK]; 2],
    /// Gain reduction of the wet compressor in the last call, dB.
    pub gr_db: f32,
}

impl Wet {
    /// A chain at `sr`.
    pub fn new(sr: f32) -> Self {
        let pp_len = (PP_MAX_MS * 0.001 * sr) as usize + 4;
        let s = Smooth::new(20.0, sr);
        Self {
            sr,
            os: [Oversampler::new(), Oversampler::new()],
            sat: s,
            bands: std::array::from_fn(|_| Band::default()),
            comp: Box::new(Chain::new(Mode::Clean, sr)),
            width: s,
            stereo_mix: s,
            pp_mix: s,
            pp_time: Smooth::new(120.0, sr),
            pp: [vec![0.0; pp_len], vec![0.0; pp_len]],
            pp_pos: 0,
            scratch: [[0.0; CHUNK]; 2],
            gr_db: 0.0,
        }
    }

    /// Clears the history.
    pub fn reset(&mut self) {
        *self = Self::new(self.sr);
    }

    /// Processes `buf` (the reverb, both channels, up to [`CHUNK`] samples) in place.
    pub fn process(
        &mut self,
        buf: &mut [[f32; CHUNK]; 2],
        n: usize,
        s: &VersionSettings,
        link: bool,
    ) {
        let n = n.min(CHUNK);
        let fs = f64::from(self.sr);
        let specs = eq_bands(s);
        let a = 1.0 - (-(CONTROL as f64) / (BAND_SMOOTH_MS * 0.001 * fs)).exp();
        let fade = CONTROL as f64 / (BAND_FADE_MS * 0.001 * fs);
        let sat_target = s.saturation;
        let mut i = 0;
        while i < n {
            let m = (n - i).min(CONTROL);
            for (b, spec) in self.bands.iter_mut().zip(&specs) {
                b.update(spec, [0.0; 2], fs, a, fade);
            }
            for j in i..i + m {
                let amt = self.sat.next(sat_target);
                for c in 0..2 {
                    let mut v = buf[c][j];
                    if amt > 1e-4 {
                        v = self.os[c].process(v, |x| saturate(x, amt));
                    }
                    let mut w = f64::from(v);
                    for b in &mut self.bands {
                        if b.active() {
                            w = b.tick(c, w);
                        }
                    }
                    buf[c][j] = w as f32;
                }
            }
            i += m;
        }

        // Compressor (the chain's own detector; linked or per side).
        let cs = comp_settings(s);
        if cs.ratio > 1.0 {
            let [s0, s1] = &mut self.scratch;
            s0[..n].copy_from_slice(&buf[0][..n]);
            s1[..n].copy_from_slice(&buf[1][..n]);
            let [b0, b1] = buf;
            let st = self.comp.process(
                [&s0[..n], &s1[..n]],
                [&mut b0[..n], &mut b1[..n]],
                2,
                None,
                &cs,
                link,
            );
            self.gr_db = st.gr[0].max(st.gr[1]);
        } else {
            self.gr_db = 0.0;
        }

        // Stereo stage, then ping-pong, each with its own wet/dry.
        let len = self.pp[0].len();
        for j in 0..n {
            let w = self.width.next(s.width);
            let sm = self.stereo_mix.next(s.stereo_mix);
            let (l, r) = (buf[0][j], buf[1][j]);
            let mid = (l + r) * 0.5;
            let side = (l - r) * 0.5 * w;
            let (wl, wr) = (mid + side, mid - side);
            let l = l + (wl - l) * sm;
            let r = r + (wr - r) * sm;

            let pm = self.pp_mix.next(s.pingpong_mix);
            let t = self.pp_time.next(s.pingpong_ms.clamp(20.0, PP_MAX_MS)) * 0.001 * self.sr;
            self.pp_pos = (self.pp_pos + 1) % len;
            let read = |b: &Vec<f32>| {
                let d = t.clamp(1.0, (len - 2) as f32);
                let k = d.floor();
                let f = d - k;
                let k = k as usize;
                let x0 = b[(self.pp_pos + len - k) % len];
                let x1 = b[(self.pp_pos + len - k - 1) % len];
                x0 + (x1 - x0) * f
            };
            let a_out = read(&self.pp[0]);
            let b_out = read(&self.pp[1]);
            // Mono into the left line, left feeds right, right feeds back into left.
            let ina = (l + r) * 0.5 + b_out * PP_FEEDBACK;
            self.pp[0][self.pp_pos] = if ina.is_finite() { ina } else { 0.0 };
            self.pp[1][self.pp_pos] = a_out;
            buf[0][j] = l + (a_out * 1.3 - l) * pm;
            buf[1][j] = r + (b_out * 1.3 - r) * pm;
        }
    }
}
