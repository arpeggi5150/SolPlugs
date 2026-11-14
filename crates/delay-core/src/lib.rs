//! SolPlugs Delay DSP. Clean-room.
//!
//! Three versions ([`Version`]):
//! - **Clean**: exact, uncoloured repeats; a soft guard only where self-oscillation would
//!   otherwise clip.
//! - **Tape**: every pass through the loop runs through a tape model (head darkening, soft
//!   saturation) with wow and flutter on the read head; up to three playback heads.
//! - **Analog**: a bucket-brigade model: the band narrows as the time gets longer, the chip
//!   clips softly and adds a little clock hiss; a slow chorus on the delay time.
//!
//! Each version has two delays fed from the same input: a stereo delay (left/right lines,
//! the right one later by the L/R offset, with width) and a ping-pong delay (the repeats
//! alternate left and right). Each has its own wet level. The repeats then pass the Low
//! Cut/High Cut knobs, LoFi and the ducker, and meet the dry signal in the shared output
//! stage ([`mixer`]). [`DelayFx`] holds a chain per version, crossfades when the version
//! changes, handles true bypass (bit-identical once the bypass fade finishes) and measures
//! what the UI shows. The dry path is never delayed: latency is zero on every version.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod curve;
pub mod line;
pub mod mixer;
pub mod version;

pub use solplugs_comp_core::AvgPeak;
pub use solplugs_comp_core::util;
pub use version::{
    Ctl, Extra, MAX_EXTRAS, MAX_TIME_MS, NOTES, Spec, Unit, Version, note_beats, note_ms,
    note_words,
};

use line::{Allpass, Line, Noise, OnePole, Svf, bbd_clip, guard, one_pole, tape_sat, warmth};
use mixer::{ChainStats, Gains, Mixer};
use solplugs_comp_core::util::{Smooth, coef, db_to_lin, lin_to_db};
use std::f32::consts::TAU;

/// Largest chunk processed in one go (internal scratch size).
pub const CHUNK: usize = 256;
/// Samples between control updates.
pub const CONTROL: usize = 16;
/// Longest reverse grain, ms (a reverse repeat reads back twice its length).
pub const MAX_REVERSE_MS: f32 = MAX_TIME_MS / 2.0;
/// Longest L/R offset, ms.
pub const MAX_OFFSET_MS: f32 = 50.0;
/// Version crossfade, ms.
const VERSION_FADE_MS: f32 = 30.0;
/// Bypass crossfade, ms.
const BYPASS_FADE_MS: f32 = 10.0;
/// Clean's time-change crossfade, ms.
const XFADE_MS: f32 = 60.0;
/// Linked stereo: how much of each side's repeat feeds the other side.
const LINK_CROSS: f32 = 0.12;

/// One version's settings, in plain units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VersionSettings {
    /// Time when tempo-sync is off, ms.
    pub time_ms: f32,
    /// Tempo-sync.
    pub sync: bool,
    /// Note value index (see [`NOTES`]).
    pub note: usize,
    /// Feedback 0..1.2.
    pub feedback: f32,
    /// L/R offset, ms.
    pub offset_ms: f32,
    /// Loop high-pass, Hz (20 = off).
    pub filter_hp: f32,
    /// Loop low-pass, Hz (20 k = off).
    pub filter_lp: f32,
    /// Low Cut knob, Hz (20 = off).
    pub low_cut_hz: f32,
    /// High Cut knob, Hz (20 k = off).
    pub high_cut_hz: f32,
    /// LoFi button.
    pub lofi: bool,
    /// Analog button.
    pub analog: bool,
    /// Ducking 0..1.
    pub duck: f32,
    /// Ducking release, ms.
    pub duck_release_ms: f32,
    /// Ducking from the external sidechain (else the dry input).
    pub duck_ext: bool,
    /// Reverse.
    pub reverse: bool,
    /// Width 0..2.
    pub width: f32,
    /// Stereo delay level 0..1.
    pub stereo_mix: f32,
    /// Ping-pong delay level 0..1.
    pub pingpong_mix: f32,
    /// Modulation depth 0..1.
    pub mod_depth: f32,
    /// Modulation rate, Hz.
    pub mod_rate: f32,
    /// The version's own knobs (see [`Version::extras`]), plain values.
    pub extras: [f32; MAX_EXTRAS],
    /// Gain into the delay, dB.
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
        let mut s = Self::from_fn(|c| v.default_of(c));
        s.extras = v.extra_defaults();
        s
    }

    /// Builds settings from each control's plain value (as the parameters store them).
    /// The extras start at zero; set them from the version's own knobs.
    pub fn from_fn(f: impl Fn(Ctl) -> f32) -> Self {
        let pct = |c| f(c) / 100.0;
        let on = |c| f(c) >= 0.5;
        Self {
            time_ms: f(Ctl::Time),
            sync: on(Ctl::Sync),
            note: f(Ctl::Note).round().max(0.0) as usize,
            feedback: pct(Ctl::Feedback),
            offset_ms: f(Ctl::Offset),
            filter_hp: f(Ctl::FilterHp),
            filter_lp: f(Ctl::FilterLp),
            low_cut_hz: f(Ctl::LowCut),
            high_cut_hz: f(Ctl::HighCut),
            lofi: on(Ctl::LoFi),
            analog: on(Ctl::Analog),
            duck: pct(Ctl::Duck),
            duck_release_ms: f(Ctl::DuckRelease),
            duck_ext: on(Ctl::DuckSource),
            reverse: on(Ctl::Reverse),
            width: pct(Ctl::Width),
            stereo_mix: pct(Ctl::StereoMix),
            pingpong_mix: pct(Ctl::PingPongMix),
            mod_depth: pct(Ctl::Mod),
            mod_rate: f(Ctl::ModRate),
            extras: [0.0; MAX_EXTRAS],
            gain_in_db: f(Ctl::GainIn),
            gain_out_db: f(Ctl::GainOut),
            mix: pct(Ctl::Mix),
            wet_db: f(Ctl::Wet),
            dry_db: f(Ctl::Dry),
            auto_makeup: on(Ctl::AutoMakeup),
        }
    }

    /// The value of one of the version's own knobs (its default when the version doesn't
    /// have it).
    pub fn extra(&self, v: Version, e: Extra) -> f32 {
        v.extra(e)
            .and_then(|i| self.extras.get(i).copied())
            .unwrap_or(e.spec().default)
    }

    /// The delay time at `bpm`, ms: the note value when synced, else the Time knob.
    pub fn delay_ms(&self, bpm: f32) -> f32 {
        let t = if self.sync {
            note_ms(self.note, bpm)
        } else {
            self.time_ms
        };
        t.clamp(1.0, MAX_TIME_MS)
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Global {
    /// Active version.
    pub version: Version,
    /// True bypass.
    pub bypass: bool,
    /// Stereo link: one delay drives both channels; else dual mono.
    pub link: bool,
    /// Tempo, BPM (the host's, or the plugin's own when the host gives none).
    pub bpm: f32,
}

impl Default for Global {
    fn default() -> Self {
        Self {
            version: Version::default(),
            bypass: false,
            link: true,
            bpm: 120.0,
        }
    }
}

/// What a call to [`DelayFx::process`] measured.
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
    /// Peak of what is circulating in the feedback loop, linear (the feedback meter).
    pub loop_peak: f32,
    /// Ducking gain reduction, dB (positive).
    pub duck_db: f32,
    /// The delay time in use, ms.
    pub delay_ms: f32,
}

/// Tap tempo: the time follows the gaps between taps.
#[derive(Clone, Copy, Debug, Default)]
pub struct TapTempo {
    last: Option<f64>,
    gaps: [f64; 4],
    n: usize,
}

impl TapTempo {
    /// Longest gap between two taps that still counts, s.
    pub const MAX_GAP_S: f64 = 2.5;

    /// A tap at `t` seconds. Returns the new time, ms, once there are two taps (the
    /// average of up to the last four gaps).
    pub fn tap(&mut self, t: f64) -> Option<f32> {
        let out = match self.last {
            Some(l) if t - l > 0.03 && t - l <= Self::MAX_GAP_S => {
                self.gaps.rotate_right(1);
                self.gaps[0] = t - l;
                self.n = (self.n + 1).min(self.gaps.len());
                let avg = self.gaps[..self.n].iter().sum::<f64>() / self.n as f64;
                Some((avg * 1000.0) as f32)
            }
            Some(l) if t - l <= 0.03 => return None,
            _ => {
                self.n = 0;
                None
            }
        };
        self.last = Some(t);
        out
    }
}

/// What the loop of a version does each time round.
#[derive(Clone, Copy, Debug)]
struct LoopParams {
    version: Version,
    hp_on: bool,
    lp_on: bool,
    diffusion: f32,
    saturation: f32,
    drive: f32,
    analog: bool,
}

/// One delay line and what its loop does to each repeat.
struct Voice {
    line: Line,
    hp: Svf,
    lp: Svf,
    tape: OnePole,
    bbd: [Svf; 2],
    ap: [Allpass; 2],
    warm: OnePole,
    dc: OnePole,
}

impl Voice {
    fn new(max: usize, sr: f32) -> Self {
        let ms = |m: f32| (m * 0.001 * sr) as usize;
        Self {
            line: Line::new(max),
            hp: Svf::default(),
            lp: Svf::default(),
            tape: OnePole::default(),
            bbd: [Svf::default(); 2],
            ap: [Allpass::new(ms(3.1)), Allpass::new(ms(7.9))],
            warm: OnePole::default(),
            dc: OnePole::new(one_pole(8.0, sr)),
        }
    }

    /// Sets the loop filters: `hz` = [loop HP, loop LP, bucket-brigade band].
    fn control(&mut self, hz: &[f32; 5], sr: f32) {
        self.hp.set(hz[0], sr);
        self.lp.set(hz[1], sr);
        self.tape.a = one_pole(5200.0, sr);
        for f in &mut self.bbd {
            f.set(hz[2], sr);
        }
        self.warm.a = one_pole(7500.0, sr);
    }

    /// What gets written: the input plus feedback, through the loop.
    #[inline]
    fn shape(&mut self, x: f32, p: &LoopParams) -> f32 {
        let mut y = x;
        if p.hp_on {
            y = self.hp.hp(y);
        }
        if p.lp_on {
            y = self.lp.lp(y);
        }
        match p.version {
            Version::Clean => {
                if p.diffusion > 0.001 {
                    let g = 0.6 * p.diffusion;
                    let a = self.ap[0].tick(y, g);
                    let d = self.ap[1].tick(a, g);
                    y += (d - y) * p.diffusion;
                }
            }
            Version::Tape => {
                y = tape_sat(self.tape.lp(y), p.saturation);
            }
            Version::Analog => {
                y = self.bbd[0].lp(y);
                y = bbd_clip(y, p.drive);
                y = self.bbd[1].lp(y);
            }
        }
        if p.analog {
            y = self.warm.lp(warmth(y));
        }
        if p.version != Version::Clean || p.analog {
            y = self.dc.hp(y);
        }
        guard(y)
    }
}

/// Reverse playback: two overlapping grains, each reading backwards.
#[derive(Clone, Copy, Debug)]
struct Reverse {
    age: [f32; 2],
    len: [f32; 2],
}

impl Reverse {
    fn new() -> Self {
        Self {
            age: [0.0, 0.5],
            len: [1.0, 1.0],
        }
    }

    /// Reads one reversed sample from `line` for grains of `len` samples.
    #[inline]
    fn read(&mut self, line: &Line, len: f32) -> f32 {
        let mut y = 0.0;
        for g in 0..2 {
            if self.age[g] >= self.len[g] {
                self.age[g] -= self.len[g];
                self.len[g] = len.max(64.0);
                self.age[g] = self.age[g].min(self.len[g] - 1.0);
            }
            // Read position walks back in time from where the grain started.
            let phase = self.age[g] / self.len[g];
            let w = (std::f32::consts::PI * phase).sin();
            y += line.read(2.0 + 2.0 * self.age[g]) * w * w;
            self.age[g] += 1.0;
        }
        y
    }

    fn start(&mut self, len: f32) {
        let len = len.max(64.0);
        self.len = [len, len];
        self.age = [0.0, len * 0.5];
    }
}

/// The delay time as it reaches the read head: Clean crossfades to a new time, Tape and
/// Analog glide (the pitch bends like a real machine). Never a click.
#[derive(Clone, Copy, Debug)]
struct TimeHead {
    cur: f32,
    next: f32,
    xf: f32,
    vel: f32,
    glide_a: f32,
    init: bool,
}

impl TimeHead {
    fn new(glide_ms: f32, sr: f32) -> Self {
        Self {
            cur: 0.0,
            next: 0.0,
            xf: 1.0,
            vel: 0.0,
            glide_a: coef(glide_ms, sr),
            init: false,
        }
    }
}

/// Modulation of the read heads.
#[derive(Clone, Copy, Debug, Default)]
struct Lfo {
    wow: f32,
    flutter: f32,
    drift: f32,
    drift_target: f32,
    noise: Noise,
    count: u32,
}

/// One version's processing.
struct Chain {
    version: Version,
    sr: f32,
    /// Stereo delay: left, right.
    st: [Voice; 2],
    /// Ping-pong delay: a (left), b (right).
    pp: [Voice; 2],
    heads: [TimeHead; 2],
    rev: [Reverse; 4],
    rev_on: Smooth,
    was_reverse: bool,
    lfo: Lfo,
    fb: Smooth,
    width: Smooth,
    st_mix: Smooth,
    pp_mix: Smooth,
    cut: [[Svf; 2]; 2],
    cut_on: [Smooth; 2],
    lofi: [(u32, f32); 2],
    lofi_lp: [OnePole; 2],
    lofi_on: Smooth,
    hiss: Noise,
    env: [f32; 2],
    mixer: Mixer,
    buf: [[f32; CHUNK]; 2],
    gains: [Gains; CHUNK],
    countdown: usize,
    loop_peak: f32,
    duck_db: f32,
    delay_ms: f32,
    peak: f32,
    max_slew: f32,
    inertia: f32,
    corners: [f32; 5],
    duck_red: [Smooth; 2],
    duck_now: f32,
}

/// Where the read heads are for one sample.
#[derive(Clone, Copy, Debug, Default)]
struct Taps {
    a: [f32; 2],
    b: [f32; 2],
    w: [f32; 2],
    m: [f32; 2],
    rev: f32,
    rev_len: f32,
    heads: usize,
    cross: bool,
}

impl Chain {
    fn new(version: Version, sr: f32) -> Self {
        let max = ((MAX_TIME_MS + MAX_OFFSET_MS + 40.0) * 0.001 * sr) as usize;
        let glide = match version {
            Version::Clean => 1.0,
            Version::Tape => 220.0,
            Version::Analog => 90.0,
        };
        let s = Smooth::new(20.0, sr);
        Self {
            version,
            sr,
            st: [Voice::new(max, sr), Voice::new(max, sr)],
            pp: [Voice::new(max, sr), Voice::new(max, sr)],
            heads: [TimeHead::new(glide, sr); 2],
            rev: [Reverse::new(); 4],
            rev_on: Smooth::new(15.0, sr),
            was_reverse: false,
            lfo: Lfo {
                noise: Noise::seeded(0x51A7 + version as u32),
                ..Lfo::default()
            },
            fb: s,
            width: s,
            st_mix: s,
            pp_mix: s,
            cut: [[Svf::default(); 2]; 2],
            cut_on: [s; 2],
            lofi: [(0, 0.0); 2],
            lofi_lp: [OnePole::default(); 2],
            lofi_on: Smooth::new(10.0, sr),
            hiss: Noise::seeded(0xB0B),
            env: [0.0; 2],
            duck_red: [Smooth::new(5.0, sr); 2],
            duck_now: 0.0,
            peak: 0.0,
            corners: [f32::NAN; 5],
            max_slew: if version == Version::Tape { 0.5 } else { 0.7 },
            inertia: coef(if version == Version::Tape { 60.0 } else { 25.0 }, sr),
            mixer: Mixer::new(sr, 1),
            buf: [[0.0; CHUNK]; 2],
            gains: [Gains {
                gin: 1.0,
                gout: 1.0,
                wet: 0.0,
                dry: 1.0,
            }; CHUNK],
            countdown: 0,
            loop_peak: 0.0,
            duck_db: 0.0,
            delay_ms: 0.0,
        }
    }

    fn reset(&mut self) {
        *self = Self::new(self.version, self.sr);
    }

    fn loop_params(&self, s: &VersionSettings) -> LoopParams {
        let v = self.version;
        LoopParams {
            version: v,
            hp_on: s.filter_hp > 20.5,
            lp_on: s.filter_lp < 19_900.0,
            diffusion: s.extra(v, Extra::Diffusion) / 100.0,
            saturation: s.extra(v, Extra::Saturation) / 100.0,
            drive: s.extra(v, Extra::Drive) / 100.0,
            analog: s.analog,
        }
    }

    /// Updates the filters. Every corner glides (in octaves), so a moving knob or a
    /// changing time never steps a filter: no clicks, no zipper noise.
    fn control(&mut self, s: &VersionSettings, delay_ms: f32) {
        let sr = self.sr;
        // A bucket-brigade's band narrows as the clock slows (longer time = darker); it
        // follows the read head as it glides, like a real clock.
        let clock_ms = if self.heads[0].init {
            self.heads[0].cur / sr * 1000.0
        } else {
            delay_ms
        };
        let bbd_hz = (3800.0 * (250.0 / clock_ms.max(1.0)).sqrt()).clamp(1200.0, 7000.0);
        let target = [
            s.filter_hp,
            s.filter_lp,
            bbd_hz,
            s.low_cut_hz,
            s.high_cut_hz,
        ];
        let a = coef(25.0, sr / CONTROL as f32);
        for (c, t) in self.corners.iter_mut().zip(target) {
            let t = t.max(1.0).ln();
            *c = if c.is_nan() { t } else { *c + (t - *c) * a };
        }
        let hz = self.corners.map(f32::exp);
        for v in self.st.iter_mut().chain(self.pp.iter_mut()) {
            v.control(&hz, sr);
        }
        for c in 0..2 {
            self.cut[0][c].set(hz[3], sr);
            self.cut[1][c].set(hz[4], sr);
            self.lofi_lp[c].a = one_pole(5500.0, sr);
        }
    }

    /// Advances a read head toward `target` samples; returns (tap a, tap b, crossfade
    /// position 0..1 toward b).
    #[inline]
    fn head(&mut self, i: usize, target: f32) -> (f32, f32, f32) {
        let h = &mut self.heads[i];
        if !h.init {
            h.init = true;
            h.cur = target;
            h.next = target;
            h.vel = 0.0;
        }
        if self.version == Version::Clean {
            if h.xf >= 1.0 && (target - h.cur).abs() > 0.01 {
                h.next = target;
                h.xf = 0.0;
            }
            if h.xf < 1.0 {
                h.xf += 1.0 / (XFADE_MS * 0.001 * self.sr);
                if h.xf >= 1.0 {
                    h.xf = 1.0;
                    h.cur = h.next;
                }
            }
            (h.cur, h.next, h.xf)
        } else {
            // A motor (or a clock) has inertia: the read speed eases toward where the
            // glide wants it and never past the slew limit, so a big jump bends the
            // pitch smoothly, never a click or a shriek.
            let want = ((target - h.cur) * h.glide_a).clamp(-self.max_slew, self.max_slew);
            h.vel += (want - h.vel) * self.inertia;
            h.cur += h.vel;
            let d = h.cur;
            (d, d, 0.0)
        }
    }

    /// Modulation offsets, samples, for (left, right).
    #[inline]
    fn modulation(&mut self, s: &VersionSettings, link: bool) -> (f32, f32) {
        let depth = if self.version.has_mod() {
            s.mod_depth
        } else {
            0.0
        };
        if depth <= 0.0 {
            return (0.0, 0.0);
        }
        let sr = self.sr;
        let rate = s.mod_rate.clamp(0.05, 10.0);
        let l = &mut self.lfo;
        l.wow = (l.wow + rate / sr).fract();
        let quad = if link { 0.25 } else { 0.0 };
        match self.version {
            Version::Tape => {
                l.flutter = (l.flutter + 6.3 / sr).fract();
                l.count += 1;
                if l.count >= (sr * 0.25) as u32 {
                    l.count = 0;
                    l.drift_target = l.noise.sample();
                }
                l.drift += (l.drift_target - l.drift) * (1.0 / (sr * 0.4));
                // Peak pitch deviation about 0.35 % (wow) and 0.12 % (flutter) at 100 %.
                let aw = depth * 0.0035 * sr / (TAU * rate);
                let af = depth * 0.0012 * sr / (TAU * 6.3);
                let ad = depth * 0.0008 * sr;
                let fl = af * (TAU * l.flutter).sin();
                let a = aw * (TAU * l.wow).sin() + fl + ad * l.drift;
                let b = aw * (TAU * (l.wow + quad)).sin() + fl + ad * l.drift;
                (a, b)
            }
            Version::Analog => {
                // A slow chorus: up to 2.5 ms of sweep.
                let a = depth * 0.0025 * sr;
                let tri = |p: f32| 1.0 - 4.0 * (p - 0.5).abs().min(0.5) + 1.0;
                let x = tri(l.wow) * 0.5;
                let y = tri((l.wow + quad).fract()) * 0.5;
                (a * x, a * y)
            }
            Version::Clean => (0.0, 0.0),
        }
    }

    #[inline]
    fn tap(line: &Line, a: f32, b: f32, wb: f32) -> f32 {
        if wb <= 0.0 {
            line.read(a)
        } else {
            // Equal-power: the two taps hold different audio.
            let t = wb.min(1.0) * std::f32::consts::FRAC_PI_2;
            line.read(a) * t.cos() + line.read(b) * t.sin()
        }
    }

    /// One sample of the stereo delay; returns its output (with the extra tape heads).
    #[inline]
    fn stereo(&mut self, x: [f32; 2], fb: f32, t: &Taps, p: &LoopParams) -> [f32; 2] {
        let mut y = [0.0f32; 2];
        let mut out = [0.0f32; 2];
        for c in 0..2 {
            let line = &self.st[c].line;
            let mut v = Self::tap(line, t.a[c] + t.m[c], t.b[c] + t.m[c], t.w[c]);
            if t.rev > 0.0 {
                let r = self.rev[c].read(line, t.rev_len);
                v += (r - v) * t.rev;
            }
            y[c] = v;
            // Extra tape heads: earlier echoes off the same tape.
            let mut h = v;
            if t.heads >= 1 {
                h += 0.7 * line.read(t.a[c] * 0.5 + t.m[c]);
            }
            if t.heads >= 2 {
                h += 0.55 * line.read(t.a[c] * 0.75 + t.m[c]);
            }
            out[c] = h;
        }
        let k = if t.cross { LINK_CROSS } else { 0.0 };
        let fbk = [y[0] * (1.0 - k) + y[1] * k, y[1] * (1.0 - k) + y[0] * k];
        for c in 0..2 {
            let w = self.st[c].shape(x[c] + fb * fbk[c], p);
            self.st[c].line.write(w);
        }
        self.peak = self.peak.max(y[0].abs()).max(y[1].abs());
        out
    }

    /// One sample of the ping-pong delay: a mono feed into the left line, the left feeds
    /// the right, the right feeds back into the left.
    #[inline]
    fn pingpong(&mut self, m: f32, fb: f32, t: &Taps, p: &LoopParams) -> [f32; 2] {
        let mut y = [0.0f32; 2];
        for (c, y) in y.iter_mut().enumerate() {
            let line = &self.pp[c].line;
            let mut v = Self::tap(line, t.a[c] + t.m[c], t.b[c] + t.m[c], t.w[c]);
            if t.rev > 0.0 {
                let r = self.rev[2 + c].read(line, t.rev_len);
                v += (r - v) * t.rev;
            }
            *y = v;
        }
        let wa = self.pp[0].shape(m + fb * y[1], p);
        let wb = self.pp[1].shape(fb * y[0], p);
        self.pp[0].line.write(wa);
        self.pp[1].line.write(wb);
        y
    }

    /// Low Cut and High Cut knobs, LoFi and hiss on the repeats.
    #[inline]
    fn post(&mut self, mut wet: [f32; 2], s: &VersionSettings, hold: u32, hiss: f32) -> [f32; 2] {
        let lc = self.cut_on[0].next(if s.low_cut_hz > 20.5 { 1.0 } else { 0.0 });
        let hc = self.cut_on[1].next(if s.high_cut_hz < 19_900.0 { 1.0 } else { 0.0 });
        let lo = self.lofi_on.next(if s.lofi { 1.0 } else { 0.0 });
        let noise = if hiss > 0.0 {
            self.hiss.sample() * hiss
        } else {
            0.0
        };
        for (c, v) in wet.iter_mut().enumerate() {
            if lc > 0.0 {
                let f = self.cut[0][c].hp(*v);
                *v += (f - *v) * lc;
            }
            if hc > 0.0 {
                let f = self.cut[1][c].lp(*v);
                *v += (f - *v) * hc;
            }
            if lo > 0.0 {
                let (cnt, held) = &mut self.lofi[c];
                if *cnt == 0 {
                    // Fewer bits, lower rate.
                    *held = (*v * 256.0).round() / 256.0;
                }
                *cnt = (*cnt + 1) % hold;
                let f = self.lofi_lp[c].lp(*held);
                *v += (f - *v) * lo;
            }
            *v += noise;
        }
        wet
    }

    /// Ducking: the repeats turn down while the key plays. Returns the gain per side.
    #[inline]
    fn duck(
        &mut self,
        key: [f32; 2],
        s: &VersionSettings,
        link: bool,
        att: f32,
        rel: f32,
    ) -> [f32; 2] {
        let mut red = [0.0f32; 2];
        for (c, r) in red.iter_mut().enumerate() {
            let k = key[c];
            let e = &mut self.env[c];
            *e += (k - *e) * if k > *e { att } else { rel };
            *r = s.duck * 30.0 * ((lin_to_db(*e) + 50.0) / 35.0).clamp(0.0, 1.0);
        }
        if link {
            let m = red[0].max(red[1]);
            red = [m, m];
        }
        for c in 0..2 {
            self.duck_red[c].next(red[c]);
        }
        let r = [self.duck_red[0].value(), self.duck_red[1].value()];
        self.duck_now = self.duck_now.max(r[0].max(r[1]));
        if r[0] <= 0.0 && r[1] <= 0.0 {
            [1.0, 1.0]
        } else {
            [db_to_lin(-r[0]), db_to_lin(-r[1])]
        }
    }

    fn process(
        &mut self,
        input: [&[f32]; 2],
        sc: Option<[&[f32]; 2]>,
        out: [&mut [f32]; 2],
        channels: usize,
        s: &VersionSettings,
        g: &Global,
    ) -> ChainStats {
        let n = input[0].len().min(CHUNK);
        let sr = self.sr;
        let stereo = channels > 1;
        let link = g.link || !stereo;
        let delay_ms = s.delay_ms(g.bpm);
        self.delay_ms = delay_ms;
        let base = delay_ms * 0.001 * sr;
        let offset = s.offset_ms.clamp(0.0, MAX_OFFSET_MS) * 0.001 * sr;
        let p = self.loop_params(s);
        let rev_len = delay_ms.min(MAX_REVERSE_MS) * 0.001 * sr;
        if s.reverse && !self.was_reverse {
            for r in &mut self.rev {
                r.start(rev_len);
            }
        }
        self.was_reverse = s.reverse;
        let fb_target = s.feedback.clamp(0.0, 1.2);
        let rel = coef(s.duck_release_ms.max(1.0), sr);
        let att = coef(3.0, sr);
        let hiss = if self.version == Version::Analog {
            s.extra(Version::Analog, Extra::Noise) / 100.0 * db_to_lin(-72.0)
        } else {
            0.0
        };
        let hold = ((sr / 12_000.0).round() as u32).max(2);
        let mut t = Taps {
            heads: if self.version == Version::Tape {
                s.extra(Version::Tape, Extra::Heads).round() as usize
            } else {
                0
            },
            rev_len,
            cross: link && stereo,
            ..Taps::default()
        };
        self.peak = 0.0;
        self.duck_now = 0.0;
        for i in 0..n {
            if self.countdown == 0 {
                self.control(s, delay_ms);
                self.countdown = CONTROL;
            }
            self.countdown -= 1;
            let gm = self.mixer.step(s);
            self.gains[i] = gm;
            let xl = input[0][i] * gm.gin;
            let xr = if stereo { input[1][i] * gm.gin } else { xl };
            let fb = self.fb.next(fb_target);
            let (ml, mr) = self.modulation(s, link);
            let (al, bl, wl) = self.head(0, base);
            let (ar, br, wr) = self.head(1, base + offset);
            t.a = [al, ar];
            t.b = [bl, br];
            t.w = [wl, wr];
            t.m = [ml, mr];
            t.rev = self.rev_on.next(if s.reverse { 1.0 } else { 0.0 });

            let y = self.stereo([xl, xr], fb, &t, &p);
            let pp = self.pingpong((xl + xr) * 0.5, fb, &t, &p);

            // Stereo stage (width), then the two delays' own levels.
            let w = self.width.next(s.width);
            let mid = (y[0] + y[1]) * 0.5;
            let side = (y[0] - y[1]) * 0.5 * w;
            let sm = self.st_mix.next(s.stereo_mix);
            let pm = self.pp_mix.next(s.pingpong_mix);
            let mut wet = [
                (mid + side) * sm + pp[0] * pm,
                (mid - side) * sm + pp[1] * pm,
            ];
            if !stereo {
                wet[0] = y[0] * sm + (pp[0] + pp[1]) * pm;
            }
            wet = self.post(wet, s, hold, hiss);

            let key = |c: usize| match (s.duck_ext, sc) {
                (true, Some(k)) => k[c].get(i).copied().unwrap_or(0.0).abs(),
                (true, None) => 0.0,
                _ => input[c.min(channels - 1)][i].abs(),
            };
            let dg = self.duck([key(0), key(1)], s, link, att, rel);
            self.buf[0][i] = wet[0] * dg[0];
            self.buf[1][i] = wet[1] * dg[1];
        }
        let loop_peak = self.peak;
        let duck_db = self.duck_now;
        self.loop_peak = loop_peak;
        self.duck_db = duck_db;

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
        st
    }
}

/// Latency of a version, samples: zero on every version (the dry path is never delayed).
pub fn latency_for(_version: Version, _s: &Settings, _sr: f32) -> usize {
    0
}

/// The delay: one chain per version, version crossfade, true bypass and meters.
pub struct DelayFx {
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

impl DelayFx {
    /// A delay at `sample_rate`.
    pub fn new(sample_rate: f32) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate > 1000.0 {
            sample_rate
        } else {
            48_000.0
        };
        Self {
            sr,
            chains: Version::ALL.iter().map(|v| Chain::new(*v, sr)).collect(),
            active: Version::default(),
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

    /// Processes `io` in place (1 or 2 channels, any length). `sc` is the external
    /// sidechain (inputs 3/4), when the host gives one; it must be at least as long.
    pub fn process(
        &mut self,
        io: &mut [&mut [f32]],
        sc: Option<[&[f32]; 2]>,
        g: &Global,
        s: &Settings,
    ) -> Stats {
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
            let sc_part = sc.and_then(|k| {
                (k[0].len() >= start + n && k[1].len() >= start + n)
                    .then(|| [&k[0][start..start + n], &k[1][start..start + n]])
            });
            self.chunk(io, sc_part, g, s, start, n, &mut stats);
            start += n;
        }
        let chain = &self.chains[self.active.index()];
        stats.avg_before_db = self.before.db();
        stats.avg_after_db = self.after.db();
        stats.auto_makeup_db = chain.mixer.auto_db();
        stats.delay_ms = s.versions[self.active.index()].delay_ms(g.bpm);
        stats
    }

    #[allow(clippy::too_many_arguments)]
    fn chunk(
        &mut self,
        io: &mut [&mut [f32]],
        sc: Option<[&[f32]; 2]>,
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
        let cs = {
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
                sc,
                [out0, out1],
                channels,
                &s.versions[active.index()],
                g,
            )
        };
        let chain = &self.chains[active.index()];
        stats.wet_peak = stats.wet_peak.max(cs.wet_peak);
        stats.dry_peak = stats.dry_peak.max(cs.dry_peak);
        stats.loop_peak = stats.loop_peak.max(chain.loop_peak);
        stats.duck_db = stats.duck_db.max(chain.duck_db);

        if let Some(old) = self.fade_from {
            self.chains[old.index()].process(
                input,
                sc,
                [&mut f0[..n], &mut f1[..n]],
                channels,
                &s.versions[old.index()],
                g,
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
