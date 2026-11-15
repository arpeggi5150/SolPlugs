//! One mode's complete signal chain: gain in, sidechain filter, detector, gain computer,
//! ballistics, gain cell, oversampled colour, parallel mix and gain out.

use crate::mode::{Character, Mode};
use crate::oversample::Oversampler;
use crate::util::{DcBlock, Delay, HighPass, Smooth, coef, db_to_lin, lin_to_db};
use crate::{AvgPeak, Settings};

/// Most gain reduction any mode applies, dB ("nothing blows up").
pub const MAX_GR_DB: f32 = 40.0;
/// Per-window (20 ms) smoothing of the auto-makeup gain: about a 1 s time constant, so
/// the makeup rides the programme's level, not its beats.
const AUTO_SMOOTH: f32 = 0.02;
/// Longest lookahead, ms.
pub const MAX_LOOKAHEAD_MS: f32 = 10.0;

/// Static gain computer: reduction in dB (positive) for a detector level `x` dB.
///
/// Soft knee of width `knee` dB centred on the threshold. "All buttons in" (FET) is the
/// over-easy smash: the threshold drops, the knee widens and above it the curve goes
/// past infinity-to-one, so louder input comes out slightly quieter.
pub fn gain_computer(x: f32, threshold: f32, ratio: f32, knee: f32, all_buttons: bool) -> f32 {
    let (t, w, s) = if all_buttons {
        (threshold - 4.0, knee.max(8.0), 1.02)
    } else {
        (threshold, knee, 1.0 - 1.0 / ratio.max(1.0))
    };
    let d = x - t;
    let gr = if w > 0.0 && 2.0 * d.abs() <= w {
        s * (d + w / 2.0) * (d + w / 2.0) / (2.0 * w)
    } else if d > 0.0 {
        s * d
    } else {
        0.0
    };
    gr.clamp(0.0, MAX_GR_DB)
}

/// Per-block ballistics coefficients.
#[derive(Clone, Copy, Debug, Default)]
struct Coefs {
    att: f32,
    rel: f32,
    /// Auto release (single-stage modes): fast and slow envelopes.
    auto: bool,
    fast_rel: f32,
    slow_att: f32,
    slow_rel: f32,
    /// Two-stage (Opto, Vari-Mu).
    two: bool,
    share: f32,
    mem_a: f32,
    slowdown: f32,
    memory: bool,
}

/// Gain-reduction envelope of one channel.
#[derive(Clone, Copy, Debug, Default)]
struct Env {
    fast: f32,
    slow: f32,
    mem: f32,
}

#[inline]
fn branch(v: f32, target: f32, att: f32, rel: f32) -> f32 {
    v + (target - v) * if target > v { att } else { rel }
}

impl Env {
    #[inline]
    fn step(&mut self, target: f32, k: &Coefs) -> f32 {
        if k.two {
            // Slow part's release stretches with the memory of recent work.
            let rel = if k.memory {
                k.slow_rel / (1.0 + (k.slowdown - 1.0) * self.mem)
            } else {
                k.slow_rel
            };
            self.fast = branch(self.fast, target, k.att, k.fast_rel);
            self.slow = branch(self.slow, target, k.slow_att, rel);
            let gr = k.share * self.fast + (1.0 - k.share) * self.slow;
            self.mem += ((gr / 10.0).min(1.0) - self.mem) * k.mem_a;
            gr
        } else if k.auto {
            // Auto release: a fast envelope for transients and a slow one that only builds
            // under sustained compression; the larger wins.
            self.fast = branch(self.fast, target, k.att, k.fast_rel);
            self.slow = branch(self.slow, target, k.slow_att, k.slow_rel);
            self.fast.max(self.slow)
        } else {
            self.fast = branch(self.fast, target, k.att, k.rel);
            self.slow = self.fast;
            self.fast
        }
    }
}

/// Smoothed continuous settings.
#[derive(Clone, Copy, Debug)]
struct Smoothers {
    threshold: Smooth,
    ratio: Smooth,
    knee: Smooth,
    gain_in: Smooth,
    gain_out: Smooth,
    mix: Smooth,
    wet: Smooth,
    dry: Smooth,
    colour: Smooth,
}

impl Smoothers {
    fn new(sr: f32) -> Self {
        let s = Smooth::new(25.0, sr);
        Self {
            threshold: s,
            ratio: s,
            knee: s,
            gain_in: s,
            gain_out: Smooth::new(40.0, sr),
            mix: s,
            wet: s,
            dry: s,
            colour: Smooth::new(30.0, sr),
        }
    }
}

/// Per-channel state.
#[derive(Clone, Debug)]
struct Channel {
    hpf: HighPass,
    ms: f32,
    /// Peak detector level and hold countdown.
    pk: f32,
    hold: usize,
    /// Delays the gain to line up with the oversampled path when colour comes first.
    gr_delay: Delay,
    env: Env,
    lookahead: Delay,
    dry: Delay,
    os: Oversampler,
    dc: DcBlock,
}

impl Channel {
    /// Updates the RMS and held-peak detectors with sidechain sample `d`; returns the level
    /// the gain computer sees.
    #[inline]
    fn detect(&mut self, d: f32, rms_a: f32, hold_n: usize, pk_fall: f32, rms: bool) -> f32 {
        self.ms += (d * d - self.ms) * rms_a;
        let a = d.abs();
        if a >= self.pk {
            self.pk = a;
            self.hold = hold_n;
        } else if self.hold > 0 {
            self.hold -= 1;
        } else {
            self.pk = a.max(self.pk * pk_fall);
        }
        if rms {
            self.ms.max(0.0).sqrt()
        } else {
            self.pk
        }
    }
}

/// What one chunk measured.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChainStats {
    /// Largest gain reduction per channel, dB (positive).
    pub gr: [f32; 2],
    /// Peak of the wet part of the output.
    pub wet_peak: f32,
    /// Peak of the dry part of the output.
    pub dry_peak: f32,
}

/// One mode's chain.
#[derive(Clone, Debug)]
pub struct Chain {
    mode: Mode,
    ch: Character,
    sr: f32,
    chans: [Channel; 2],
    sm: Smoothers,
    /// Auto-makeup measurement: raw input vs wet+dry before gain out.
    before: AvgPeak,
    after: AvgPeak,
    auto_db: f32,
}

impl Chain {
    /// A chain for `mode` at `sr`.
    pub fn new(mode: Mode, sr: f32) -> Self {
        let max_la = (MAX_LOOKAHEAD_MS * 0.001 * sr).ceil() as usize + 1;
        let chan = Channel {
            hpf: HighPass::default(),
            ms: 0.0,
            pk: 0.0,
            hold: 0,
            gr_delay: {
                let mut d = Delay::new(Oversampler::LATENCY);
                d.set_len(Oversampler::LATENCY);
                d
            },
            env: Env::default(),
            lookahead: Delay::new(max_la),
            dry: Delay::new(max_la.max(Oversampler::LATENCY)),
            os: Oversampler::new(),
            dc: DcBlock::new(sr),
        };
        Self {
            mode,
            ch: mode.character(),
            sr,
            chans: [chan.clone(), chan],
            sm: Smoothers::new(sr),
            before: AvgPeak::new(sr),
            after: AvgPeak::new(sr),
            auto_db: 0.0,
        }
    }

    /// Mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Latency for these settings, samples.
    pub fn latency_for(mode: Mode, s: &Settings, sr: f32) -> usize {
        if mode.coloured() {
            Oversampler::LATENCY
        } else {
            (s.lookahead_ms.clamp(0.0, MAX_LOOKAHEAD_MS) * 0.001 * sr).round() as usize
        }
    }

    /// Auto-makeup gain the chain measures, dB.
    pub fn auto_makeup_db(&self) -> f32 {
        self.auto_db
    }

    /// Clears all state (used when a mode becomes active again, and after a numeric fault).
    pub fn reset(&mut self) {
        *self = Self::new(self.mode, self.sr);
    }

    fn coefs(&self, s: &Settings) -> Coefs {
        let sr = self.sr;
        let (mut att_ms, mut rel_ms) = (s.attack_ms.max(0.01), s.release_ms.max(1.0));
        if s.all_buttons && self.mode == Mode::Fet {
            att_ms *= 0.6;
            rel_ms *= 0.7;
        }
        let mut k = Coefs {
            att: coef(att_ms, sr),
            rel: coef(rel_ms, sr),
            ..Coefs::default()
        };
        if let Some(t) = self.ch.two_stage {
            k.two = true;
            k.fast_rel = coef(t.fast_release_ms.min(rel_ms), sr);
            k.slow_att = coef(att_ms * t.slow_attack_mul, sr);
            k.slow_rel = coef(rel_ms, sr);
            k.share = t.fast_share;
            k.mem_a = coef(t.memory_ms, sr);
            k.slowdown = t.memory_slowdown;
            k.memory = s.auto_release;
        } else if s.auto_release {
            k.auto = true;
            k.fast_rel = coef((rel_ms * 0.25).clamp(30.0, 120.0), sr);
            k.slow_att = coef(att_ms * 10.0 + 150.0, sr);
            k.slow_rel = coef(rel_ms.max(600.0) * 2.0, sr);
        }
        k
    }

    /// Auto-makeup: matches the long-term peak of the output (before gain out) to the
    /// input. Silence holds the last value.
    #[inline]
    fn track_makeup(&mut self, before: f32, after: f32) {
        let gate = self.before.push(before);
        self.after.push_gated(after, gate);
        if gate {
            let a_db = self.before.db();
            let b_db = self.after.db();
            if a_db > -90.0 && b_db > -90.0 {
                let target = (a_db - b_db).clamp(-24.0, 24.0);
                self.auto_db += (target - self.auto_db) * AUTO_SMOOTH;
            }
        }
    }

    /// Processes `n` samples of `input` into `out` (same channel count, 1 or 2).
    /// `sc` is the detector source when an external sidechain is on.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        input: [&[f32]; 2],
        out: [&mut [f32]; 2],
        channels: usize,
        sc: Option<[&[f32]; 2]>,
        s: &Settings,
        link: bool,
    ) -> ChainStats {
        let n = input[0].len();
        let sr = self.sr;
        let k = self.coefs(s);
        let rms_a = coef(self.ch.rms_ms, sr);
        let hold_n = (self.ch.hold_ms * 0.001 * sr) as usize;
        let pk_fall = 1.0 - coef(3.0, sr);
        let coloured = self.mode.coloured();
        let la = if coloured {
            0
        } else {
            (s.lookahead_ms.clamp(0.0, MAX_LOOKAHEAD_MS) * 0.001 * sr).round() as usize
        };
        let lat = Self::latency_for(self.mode, s, sr);
        for c in &mut self.chans {
            c.hpf.set(s.sc_hpf_hz, sr);
            c.lookahead.set_len(la);
            c.dry.set_len(lat);
        }
        let all = s.all_buttons && self.mode == Mode::Fet;
        let base_drive = self.ch.drive * if all { 2.6 } else { 1.0 };
        let bias = self.ch.bias * if all { 1.6 } else { 1.0 };
        let colour_target = if s.colour && coloured { 1.0 } else { 0.0 };
        let gain_out_target = if s.auto_makeup {
            self.auto_db
        } else {
            s.gain_out_db
        };
        let mut st = ChainStats::default();
        let [o0, o1] = out;
        // The detector's view of the input stage uses the colour setting without smoothing.
        let colour_det = colour_target;
        for i in 0..n {
            let thr = self.sm.threshold.next(s.threshold_db);
            let ratio = self.sm.ratio.next(s.ratio);
            let knee = self.sm.knee.next(s.knee_db);
            let gin = db_to_lin(self.sm.gain_in.next(s.gain_in_db));
            let gout = db_to_lin(self.sm.gain_out.next(gain_out_target));
            let mix = self.sm.mix.next(s.mix.clamp(0.0, 1.0));
            let wet_g = level(self.sm.wet.next(s.wet_db));
            let dry_g = level(self.sm.dry.next(s.dry_db));
            let colour = self.sm.colour.next(colour_target);

            // Detector levels.
            let mut lvl = [0.0f32; 2];
            for c in 0..channels {
                let src = match sc {
                    Some(sc) => sc[c][i],
                    // A pushed input stage clips before the detector hears it.
                    None if coloured && self.ch.pre_colour => {
                        saturate(input[c][i] * gin, base_drive * colour_det, bias)
                    }
                    None => input[c][i] * gin,
                };
                let ch = &mut self.chans[c];
                let d = ch.hpf.process(src);
                lvl[c] = ch.detect(d, rms_a, hold_n, pk_fall, s.rms);
            }
            if link && channels == 2 {
                let l = if s.rms {
                    (0.5 * (self.chans[0].ms + self.chans[1].ms))
                        .max(0.0)
                        .sqrt()
                } else {
                    lvl[0].max(lvl[1])
                };
                lvl = [l, l];
            }
            let mut gr = [0.0f32; 2];
            if link && channels == 2 {
                let t = gain_computer(lin_to_db(lvl[0]), thr, ratio, knee, all);
                gr[0] = self.chans[0].env.step(t, &k);
                // Keep the right envelope in step so switching to dual mono is seamless.
                self.chans[1].env = self.chans[0].env;
                gr[1] = gr[0];
            } else {
                for c in 0..channels {
                    let t = gain_computer(lin_to_db(lvl[c]), thr, ratio, knee, all);
                    gr[c] = self.chans[c].env.step(t, &k);
                }
            }

            let mut before = 0.0f32;
            let mut after = 0.0f32;
            for c in 0..channels {
                let x = input[c][i];
                let ch = &mut self.chans[c];
                let drive = base_drive * colour * (1.0 + self.ch.drive_per_db_gr * gr[c]);
                let a = if !coloured {
                    ch.lookahead.process(x * gin) * db_to_lin(-gr[c])
                } else if self.ch.pre_colour {
                    // Input stage first (pushed by Gain In), then the gain cell, its gain
                    // delayed to line up with the oversampled audio.
                    let pre = ch.os.process(x * gin, |v| saturate(v, drive, bias));
                    ch.dc.process(pre) * db_to_lin(-ch.gr_delay.process(gr[c]))
                } else {
                    let cell = x * gin * db_to_lin(-gr[c]);
                    ch.dc
                        .process(ch.os.process(cell, |v| saturate(v, drive, bias)))
                };
                let dry = ch.dry.process(x);
                let w = mix * wet_g * a;
                let d = (1.0 - mix) * dry_g * dry;
                let y = w + d;
                before = before.max(x.abs());
                after = after.max(y.abs());
                st.wet_peak = st.wet_peak.max((w * gout).abs());
                st.dry_peak = st.dry_peak.max((d * gout).abs());
                let y = y * gout;
                if c == 0 {
                    o0[i] = y;
                } else {
                    o1[i] = y;
                }
                st.gr[c] = st.gr[c].max(gr[c]);
            }
            if channels == 1 {
                st.gr[1] = st.gr[0];
            }
            // Auto-makeup: match the long-term peak of the output (before gain out) to the
            // input. Silence holds the last value.
            self.track_makeup(before, after);
        }
        if !st.gr[0].is_finite() || !st.gr[1].is_finite() || !self.chans[0].env.fast.is_finite() {
            // A numeric fault (should never happen): start clean rather than ring forever.
            self.reset();
        }
        st
    }
}

/// Wet/dry level in dB; -60 and below is off.
#[inline]
fn level(db: f32) -> f32 {
    if db <= -59.9 { 0.0 } else { db_to_lin(db) }
}

/// Asymmetric soft saturation with unity small-signal gain: `drive` sets how hard it bends,
/// `bias` how lopsided (even harmonics). Zero drive is a wire.
#[inline]
pub fn saturate(v: f32, drive: f32, bias: f32) -> f32 {
    if drive < 1e-4 {
        return v;
    }
    let tb = (drive * bias).tanh();
    let slope = drive * (1.0 - tb * tb);
    ((drive * (v + bias)).tanh() - tb) / slope
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_knee_ratio() {
        // 4:1, 10 dB over: 7.5 dB reduction.
        assert!((gain_computer(-10.0, -20.0, 4.0, 0.0, false) - 7.5).abs() < 1e-4);
        assert_eq!(gain_computer(-30.0, -20.0, 4.0, 0.0, false), 0.0);
    }

    #[test]
    fn soft_knee_is_continuous() {
        let mut prev = 0.0;
        for i in 0..400 {
            let x = -40.0 + i as f32 * 0.1;
            let g = gain_computer(x, -20.0, 4.0, 10.0, false);
            assert!(g >= prev - 1e-5 && g - prev < 0.1, "{x}: {prev} -> {g}");
            prev = g;
        }
    }

    #[test]
    fn all_buttons_goes_past_infinity() {
        let a = gain_computer(-10.0, -20.0, 4.0, 0.0, true);
        let b = gain_computer(0.0, -20.0, 4.0, 0.0, true);
        // 10 dB more input, more than 10 dB more reduction.
        assert!(b - a > 10.0);
        assert!(b <= MAX_GR_DB);
    }

    #[test]
    fn saturation_has_unity_small_signal_gain() {
        for (d, b) in [(0.5, 0.0), (0.7, 0.06), (1.5, 0.3)] {
            let y = saturate(1e-4, d, b) / 1e-4;
            assert!((y - 1.0).abs() < 1e-2, "{d} {b}: {y}");
        }
        assert_eq!(saturate(0.3, 0.0, 0.2), 0.3);
    }
}
