//! SolPlugs Verb DSP. Clean-room.
//!
//! Five versions ([`Version`]), each its own space and colour:
//! - **Clean**: a dense 16-line network, smooth and honest, optional early reflections.
//! - **Hall**: a big network with lush modulation and a band-limited input, like an 80s
//!   rack hall.
//! - **Room**: strong early reflections into a short, dense tail.
//! - **Plate**: short, tightly packed lines with heavy input diffusion: instant density,
//!   bright, no early reflections.
//! - **Spring**: a tube driver into dispersive spring loops (the "drip").
//!
//! Every version then runs the wet-only chain ([`wet`]: saturation, EQ, compressor,
//! stereo and ping-pong stages) and the shared output stage ([`mixer`]: Gain In, Wet, Dry,
//! Mix, auto-makeup, Gain Out). [`Reverb`] holds a chain per version, crossfades when the
//! version changes, handles true bypass (bit-identical once the bypass fade finishes) and
//! measures the levels the UI shows. The dry path has no delay, so latency is zero on
//! every version.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod curve;
pub mod early;
pub mod mixer;
pub mod spring;
pub mod tank;
pub mod version;
pub mod wet;

pub use solplugs_comp_core::AvgPeak;
pub use solplugs_comp_core::util;
pub use version::{Ctl, Extra, MAX_EXTRAS, Spec, Unit, Version};

use early::Early;
use mixer::{ChainStats, Gains, Mixer};
use solplugs_comp_core::util::{Delay, Smooth};
use spring::{SpringParams, SpringTank};
use tank::{Tank, TankParams, one_pole};
use wet::Wet;

/// Largest chunk processed in one go (internal scratch size).
pub const CHUNK: usize = 256;
/// Samples between control updates.
pub const CONTROL: usize = 16;
/// Longest pre-delay, ms.
pub const MAX_PREDELAY_MS: f32 = 500.0;
/// Version crossfade, ms.
const VERSION_FADE_MS: f32 = 30.0;
/// Bypass crossfade, ms.
const BYPASS_FADE_MS: f32 = 10.0;

/// One version's settings, in plain units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VersionSettings {
    /// RT60 of the mids, s.
    pub decay_s: f32,
    /// Size 0..1.
    pub size: f32,
    /// Pre-delay, ms.
    pub predelay_ms: f32,
    /// High damping 0..1.
    pub high_damp: f32,
    /// Low damping 0..1.
    pub low_damp: f32,
    /// Diffusion 0..1.
    pub diffusion: f32,
    /// Freeze.
    pub freeze: bool,
    /// The version's own knobs (see [`Version::extras`]), plain values.
    pub extras: [f32; MAX_EXTRAS],
    /// Wet saturation 0..1.
    pub saturation: f32,
    /// Wet low cut, Hz (20 = off).
    pub low_cut_hz: f32,
    /// Wet high cut, Hz (20 k = off).
    pub high_cut_hz: f32,
    /// Wet low shelf, dB.
    pub low_db: f32,
    /// Wet mid bell, dB.
    pub mid_db: f32,
    /// Wet mid frequency, Hz.
    pub mid_hz: f32,
    /// Wet high shelf, dB.
    pub high_db: f32,
    /// Wet compressor threshold, dB (0 = off).
    pub threshold_db: f32,
    /// Wet compressor ratio.
    pub ratio: f32,
    /// Wet compressor attack, ms.
    pub attack_ms: f32,
    /// Wet compressor release, ms.
    pub release_ms: f32,
    /// Width 0..2.
    pub width: f32,
    /// Stereo stage wet/dry 0..1.
    pub stereo_mix: f32,
    /// Ping-pong stage wet/dry 0..1.
    pub pingpong_mix: f32,
    /// Ping-pong time, ms.
    pub pingpong_ms: f32,
    /// Gain into the reverb, dB.
    pub gain_in_db: f32,
    /// Final output level (makeup), dB.
    pub gain_out_db: f32,
    /// Wet/dry blend 0..1 (1 = all wet).
    pub mix: f32,
    /// Wet level, dB (-60 = off).
    pub wet_db: f32,
    /// Dry level, dB (-60 = off).
    pub dry_db: f32,
    /// Auto-makeup.
    pub auto_makeup: bool,
}

impl VersionSettings {
    /// Defaults for `v`.
    pub fn defaults(v: Version) -> Self {
        let mut s = Self::from_fn(v, |c| v.default_of(c));
        s.extras = v.extra_defaults();
        s
    }

    /// Builds settings from each control's plain value (as the parameters store them:
    /// percentages in %, etc.).
    pub fn from_fn(v: Version, f: impl Fn(Ctl) -> f32) -> Self {
        let pct = |c| f(c) / 100.0;
        Self {
            decay_s: f(Ctl::Decay),
            size: pct(Ctl::Size),
            predelay_ms: f(Ctl::PreDelay),
            high_damp: pct(Ctl::Damping),
            low_damp: pct(Ctl::LowDamping),
            diffusion: pct(Ctl::Diffusion),
            freeze: f(Ctl::Freeze) >= 0.5,
            extras: v.extra_defaults(),
            saturation: pct(Ctl::Saturation),
            low_cut_hz: f(Ctl::LowCut),
            high_cut_hz: f(Ctl::HighCut),
            low_db: f(Ctl::Low),
            mid_db: f(Ctl::Mid),
            mid_hz: f(Ctl::MidFreq),
            high_db: f(Ctl::High),
            threshold_db: f(Ctl::Threshold),
            ratio: f(Ctl::Ratio),
            attack_ms: f(Ctl::Attack),
            release_ms: f(Ctl::Release),
            width: pct(Ctl::Width),
            stereo_mix: pct(Ctl::StereoMix),
            pingpong_mix: pct(Ctl::PingPongMix),
            pingpong_ms: f(Ctl::PingPongTime),
            gain_in_db: f(Ctl::GainIn),
            gain_out_db: f(Ctl::GainOut),
            mix: pct(Ctl::Mix),
            wet_db: f(Ctl::Wet),
            dry_db: f(Ctl::Dry),
            auto_makeup: f(Ctl::AutoMakeup) >= 0.5,
        }
    }

    /// The value of one of the version's own knobs (its default when the version doesn't
    /// have it).
    pub fn extra(&self, v: Version, e: Extra) -> f32 {
        v.extra(e)
            .and_then(|i| self.extras.get(i).copied())
            .unwrap_or(e.spec().default)
    }
}

/// Every version's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Per version.
    pub versions: [VersionSettings; Version::COUNT],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            versions: Version::ALL.map(VersionSettings::defaults),
        }
    }
}

/// Settings shared by all versions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Global {
    /// Active version.
    pub version: Version,
    /// True bypass.
    pub bypass: bool,
    /// Stereo link: one reverb for both channels (true stereo); else dual mono.
    pub link: bool,
}

/// What a call to [`Reverb::process`] measured.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// Input peak (before gain in), linear.
    pub in_peak: f32,
    /// Input peak per channel, linear.
    pub in_lr: [f32; 2],
    /// Output peak, linear.
    pub out_peak: f32,
    /// Output peak per channel, linear.
    pub out_lr: [f32; 2],
    /// Peak of the wet part of the output, linear.
    pub wet_peak: f32,
    /// Peak of the dry part of the output, linear.
    pub dry_peak: f32,
    /// Average peak of the input, dBFS (the post meter's BEFORE).
    pub avg_before_db: f32,
    /// Average peak of the output, dBFS (the post meter's AFTER).
    pub avg_after_db: f32,
    /// Gain Out the active version's auto-makeup measures, dB.
    pub auto_makeup_db: f32,
    /// Wet compressor gain reduction, dB.
    pub comp_gr_db: f32,
}

/// How a version maps its controls onto the engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// Line-length scale for Size 0 and 100%.
    pub scale: (f32, f32),
    /// Most input-diffuser gain.
    pub diffuser_max: f32,
    /// Level of the late tail.
    pub late: f32,
    /// Fixed early-reflection level (when the version has no Early knob).
    pub early: f32,
}

impl Version {
    /// How this version maps Size, Diffusion and levels.
    pub fn shape(self) -> Shape {
        match self {
            Version::Clean => Shape {
                scale: (0.35, 1.6),
                diffuser_max: 0.72,
                late: 2.0,
                early: 0.0,
            },
            Version::Hall => Shape {
                scale: (0.7, 2.2),
                diffuser_max: 0.7,
                late: 3.6,
                early: 0.35,
            },
            Version::Room => Shape {
                scale: (0.12, 0.6),
                diffuser_max: 0.65,
                late: 1.4,
                early: 0.0,
            },
            Version::Plate => Shape {
                scale: (0.18, 0.7),
                diffuser_max: 0.8,
                late: 2.4,
                early: 0.0,
            },
            Version::Spring => Shape {
                scale: (0.6, 1.6),
                diffuser_max: 0.0,
                late: 1.9,
                early: 0.0,
            },
        }
    }

    /// Line-length scale for `size` 0..1.
    pub fn scale(self, size: f32) -> f32 {
        let (a, b) = self.shape().scale;
        a + (b - a) * size.clamp(0.0, 1.0)
    }
}

/// The tank parameters a version's settings give.
pub fn tank_params(v: Version, s: &VersionSettings) -> TankParams {
    let modulation = match v {
        Version::Room => 0.12,
        Version::Spring => 0.0,
        _ => s.extra(v, Extra::Mod) / 100.0,
    };
    TankParams {
        decay_s: s.decay_s,
        scale: v.scale(s.size),
        high_damp: s.high_damp,
        low_damp: s.low_damp,
        diffusion: s.diffusion,
        diffuser_max: v.shape().diffuser_max,
        modulation,
        freeze: s.freeze,
    }
}

/// The spring parameters a version's settings give.
pub fn spring_params(s: &VersionSettings) -> SpringParams {
    let v = Version::Spring;
    SpringParams {
        decay_s: s.decay_s,
        scale: v.scale(s.size),
        top_hz: 7500.0 * (2200.0f32 / 7500.0).powf(s.high_damp.clamp(0.0, 1.0)),
        bottom_hz: 50.0 * (450.0f32 / 50.0).powf(s.low_damp.clamp(0.0, 1.0)),
        dispersion: s.diffusion,
        springs: s.extra(v, Extra::Springs).round() as usize + 1,
        tube: s.extra(v, Extra::Tube) / 100.0,
        freeze: s.freeze,
    }
}

enum Engine {
    Tank(Box<[Tank; 2]>),
    Spring(Box<[SpringTank; 2]>),
}

/// One version's processing.
struct Chain {
    version: Version,
    sr: f32,
    pre: [Delay; 2],
    pre_len: Smooth,
    bw: [[f32; 2]; 2],
    a_bw: f32,
    engine: Engine,
    early: Early,
    early_gain: Smooth,
    freeze_in: f32,
    wet: Wet,
    mixer: Mixer,
    buf: [[f32; CHUNK]; 2],
    gains: [Gains; CHUNK],
    countdown: usize,
}

impl Chain {
    fn new(version: Version, sr: f32) -> Self {
        let max_pre = (MAX_PREDELAY_MS * 0.001 * sr) as usize + 4;
        let engine = if version == Version::Spring {
            Engine::Spring(Box::new([SpringTank::new(sr), SpringTank::new(sr)]))
        } else {
            Engine::Tank(Box::new([Tank::new(sr), Tank::new(sr)]))
        };
        Self {
            version,
            sr,
            pre: [Delay::new(max_pre), Delay::new(max_pre)],
            pre_len: Smooth::new(60.0, sr),
            bw: [[0.0; 2]; 2],
            a_bw: 1.0,
            engine,
            early: Early::new(sr),
            early_gain: Smooth::new(20.0, sr),
            freeze_in: 1.0,
            wet: Wet::new(sr),
            mixer: Mixer::new(sr, 1),
            buf: [[0.0; CHUNK]; 2],
            gains: [Gains {
                gin: 1.0,
                gout: 1.0,
                wet: 0.0,
                dry: 1.0,
            }; CHUNK],
            countdown: 0,
        }
    }

    fn reset(&mut self) {
        *self = Self::new(self.version, self.sr);
    }

    fn auto_db(&self) -> f32 {
        self.mixer.auto_db()
    }

    fn early_level(&self, s: &VersionSettings) -> f32 {
        match self.version {
            Version::Clean | Version::Room => s.extra(self.version, Extra::Early) / 100.0,
            v => v.shape().early,
        }
    }

    fn control(&mut self, s: &VersionSettings) {
        let v = self.version;
        match &mut self.engine {
            Engine::Tank(t) => {
                let p = tank_params(v, s);
                for t in t.iter_mut() {
                    t.set(&p);
                }
            }
            Engine::Spring(t) => {
                let p = spring_params(s);
                for t in t.iter_mut() {
                    t.set(&p);
                }
            }
        }
        let spread = if v == Version::Room {
            s.extra(v, Extra::Shape) / 100.0
        } else {
            0.5
        };
        self.early.set(v.scale(s.size), spread, s.high_damp);
        self.a_bw = match v {
            Version::Hall | Version::Plate => one_pole(s.extra(v, Extra::Bandwidth), self.sr),
            _ => 1.0,
        };
    }

    fn process(
        &mut self,
        input: [&[f32]; 2],
        out: [&mut [f32]; 2],
        channels: usize,
        s: &VersionSettings,
        link: bool,
    ) -> (ChainStats, f32) {
        let n = input[0].len().min(CHUNK);
        let late = self.version.shape().late;
        let early_target = self.early_level(s);
        let pre_target = s.predelay_ms.clamp(0.0, MAX_PREDELAY_MS) * 0.001 * self.sr;
        let freeze_step = 1.0 / (0.05 * self.sr);
        let stereo = channels > 1;
        for i in 0..n {
            if self.countdown == 0 {
                self.control(s);
                self.countdown = CONTROL;
            }
            self.countdown -= 1;
            let g = self.mixer.step(s);
            self.gains[i] = g;
            let xl = input[0][i] * g.gin;
            let xr = if stereo { input[1][i] * g.gin } else { xl };
            // Pre-delay (whole samples; the length glides so a change never clicks).
            let d = self.pre_len.next(pre_target).round() as usize;
            let mut p = [xl, xr];
            for c in 0..2 {
                self.pre[c].set_len(d);
                p[c] = self.pre[c].process(p[c]);
                // Input bandwidth (Hall, Plate): two one-pole stages, 12 dB/oct.
                if self.a_bw < 1.0 {
                    self.bw[c][0] += (p[c] - self.bw[c][0]) * self.a_bw;
                    self.bw[c][1] += (self.bw[c][0] - self.bw[c][1]) * self.a_bw;
                    p[c] = self.bw[c][1];
                }
            }
            let ft = if s.freeze { 0.0 } else { 1.0 };
            if self.freeze_in < ft {
                self.freeze_in = (self.freeze_in + freeze_step).min(ft);
            } else if self.freeze_in > ft {
                self.freeze_in = (self.freeze_in - freeze_step).max(ft);
            }
            let eg = self.early_gain.next(early_target);
            let (el, er) = if eg > 1e-4 || self.early_gain.value() > 1e-4 {
                self.early.tick(
                    p[0] * self.freeze_in,
                    p[1] * self.freeze_in,
                    link || !stereo,
                )
            } else {
                (0.0, 0.0)
            };
            let (tl, tr) = match &mut self.engine {
                Engine::Tank(t) => {
                    if link || !stereo {
                        t[0].tick(p[0], p[1])
                    } else {
                        let (a, _) = t[0].tick(p[0], p[0]);
                        let (b, _) = t[1].tick(p[1], p[1]);
                        (a, b)
                    }
                }
                Engine::Spring(t) => {
                    if link || !stereo {
                        t[0].tick(p[0], p[1])
                    } else {
                        let (a, b0) = t[0].tick(p[0], p[0]);
                        let (a1, b) = t[1].tick(p[1], p[1]);
                        ((a + b0) * 0.5, (a1 + b) * 0.5)
                    }
                }
            };
            let mut wl = tl * late + el * eg;
            let mut wr = tr * late + er * eg;
            if !stereo {
                wl = (wl + wr) * 0.5;
                wr = wl;
            }
            self.buf[0][i] = wl;
            self.buf[1][i] = wr;
        }
        self.wet.process(&mut self.buf, n, s, link || !stereo);
        if !stereo {
            for i in 0..n {
                self.buf[0][i] = (self.buf[0][i] + self.buf[1][i]) * 0.5;
            }
        }
        let mut st = ChainStats::default();
        let [o0, o1] = out;
        for i in 0..n {
            let g = self.gains[i];
            let mut before = 0.0f32;
            let mut after = 0.0f32;
            for c in 0..channels {
                let x = input[c][i];
                let (pre, y) = self.mixer.blend(c, x, self.buf[c][i], &g, &mut st);
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
        (st, self.wet.gr_db)
    }
}

/// Latency of a version, samples: zero on every version (the dry path is never delayed;
/// the wet path's oversampling only adds a fraction of a millisecond to the pre-delay).
pub fn latency_for(_version: Version, _s: &Settings, _sr: f32) -> usize {
    0
}

/// The reverb: one chain per version, version crossfade, true bypass and meters.
pub struct Reverb {
    sr: f32,
    chains: Vec<Chain>,
    active: Version,
    fade_from: Option<Version>,
    fade: f32,
    fade_step: f32,
    bypass: f32,
    bypass_step: f32,
    before: AvgPeak,
    after: AvgPeak,
    scratch: [[f32; CHUNK]; 4],
    started: bool,
}

impl Reverb {
    /// A reverb at `sample_rate`.
    pub fn new(sample_rate: f32) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate > 1000.0 {
            sample_rate
        } else {
            48_000.0
        };
        Self {
            sr,
            chains: Version::ALL.iter().map(|v| Chain::new(*v, sr)).collect(),
            active: Version::Hall,
            fade_from: None,
            fade: 1.0,
            fade_step: 1.0 / (VERSION_FADE_MS * 0.001 * sr),
            bypass: 0.0,
            bypass_step: 1.0 / (BYPASS_FADE_MS * 0.001 * sr),
            before: AvgPeak::new(sr),
            after: AvgPeak::new(sr),
            scratch: [[0.0; CHUNK]; 4],
            started: false,
        }
    }

    /// Sample rate.
    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    /// Forgets the post meter's AFTER history (after MATCH, so it settles fresh).
    pub fn reset_meters(&mut self) {
        self.after.reset();
    }

    /// Processes `io` in place (1 or 2 channels, any length).
    pub fn process(&mut self, io: &mut [&mut [f32]], g: &Global, s: &Settings) -> Stats {
        let channels = io.len().min(2);
        let mut stats = Stats::default();
        if channels == 0 {
            return stats;
        }
        if !self.started {
            self.started = true;
            self.active = g.version;
            self.bypass = if g.bypass { 1.0 } else { 0.0 };
        }
        if g.version != self.active {
            self.fade_from = Some(self.active);
            self.active = g.version;
            self.fade = 0.0;
            self.chains[g.version.index()].reset();
        }
        let n_total = io.iter().take(channels).map(|c| c.len()).min().unwrap_or(0);
        let mut start = 0;
        while start < n_total {
            let n = (n_total - start).min(CHUNK);
            self.chunk(io, g, s, start, n, &mut stats);
            start += n;
        }
        stats.avg_before_db = self.before.db();
        stats.avg_after_db = self.after.db();
        stats.auto_makeup_db = self.chains[self.active.index()].auto_db();
        stats
    }

    fn chunk(
        &mut self,
        io: &mut [&mut [f32]],
        g: &Global,
        s: &Settings,
        start: usize,
        n: usize,
        stats: &mut Stats,
    ) {
        let channels = io.len().min(2);
        let range = start..start + n;
        let [in0, in1, f0, f1] = &mut self.scratch;
        in0[..n].copy_from_slice(&io[0][range.clone()]);
        if channels > 1 {
            in1[..n].copy_from_slice(&io[1][range.clone()]);
        }
        let input: [&[f32]; 2] = [&in0[..n], if channels > 1 { &in1[..n] } else { &in0[..n] }];
        let active = self.active;
        let (cs, gr) = {
            let (o0, rest) = io.split_at_mut(1);
            let out0 = &mut o0[0][range.clone()];
            let mut dummy = [0.0f32; CHUNK];
            let out1: &mut [f32] = if channels > 1 {
                &mut rest[0][range.clone()]
            } else {
                &mut dummy[..n]
            };
            self.chains[active.index()].process(
                input,
                [out0, out1],
                channels,
                &s.versions[active.index()],
                g.link,
            )
        };
        stats.wet_peak = stats.wet_peak.max(cs.wet_peak);
        stats.dry_peak = stats.dry_peak.max(cs.dry_peak);
        stats.comp_gr_db = stats.comp_gr_db.max(gr);

        if let Some(old) = self.fade_from {
            self.chains[old.index()].process(
                input,
                [&mut f0[..n], &mut f1[..n]],
                channels,
                &s.versions[old.index()],
                g.link,
            );
            for i in 0..n {
                let t = (self.fade + self.fade_step * i as f32).min(1.0);
                io[0][start + i] = io[0][start + i] * t + f0[i] * (1.0 - t);
                if channels > 1 {
                    io[1][start + i] = io[1][start + i] * t + f1[i] * (1.0 - t);
                }
            }
            self.fade += self.fade_step * n as f32;
            if self.fade >= 1.0 {
                self.fade = 1.0;
                self.fade_from = None;
            }
        }

        // True bypass: the input itself (latency is zero). Exact once the fade is done.
        let target = if g.bypass { 1.0 } else { 0.0 };
        let mut b = self.bypass;
        for i in 0..n {
            if target > b {
                b = (b + self.bypass_step).min(1.0);
            } else if target < b {
                b = (b - self.bypass_step).max(0.0);
            }
            let mut before = 0.0f32;
            let mut after = 0.0f32;
            for c in 0..channels {
                let x = input[c][i];
                let y = &mut io[c][start + i];
                if b >= 1.0 {
                    *y = x;
                } else if b > 0.0 {
                    *y = *y * (1.0 - b) + x * b;
                }
                before = before.max(x.abs());
                after = after.max(y.abs());
                stats.in_lr[c] = stats.in_lr[c].max(x.abs());
                stats.out_lr[c] = stats.out_lr[c].max(y.abs());
            }
            if channels == 1 {
                stats.out_lr[1] = stats.out_lr[0];
                stats.in_lr[1] = stats.in_lr[0];
            }
            stats.in_peak = stats.in_peak.max(before);
            stats.out_peak = stats.out_peak.max(after);
            let gate = self.before.push(before);
            self.after.push_gated(after, gate);
        }
        self.bypass = b;
    }
}
