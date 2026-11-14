//! SolPlugs Compressor DSP. Clean-room, no dependencies.
//!
//! Five circuits ([`Mode`]), each its own detector, gain curve, ballistics and colour:
//! - **VCA**: precise feed-forward control, optional auto release (fast + slow envelopes).
//! - **Opto**: a light-cell model: soft attack, half the reduction lets go fast and the rest
//!   slowly, and the longer it works the slower it releases.
//! - **FET**: 20-800 µs attack, peak detector, gritty saturation, and "all buttons in".
//! - **Vari-Mu**: wide soft knee, slow attack, programme-dependent release, tube colour
//!   that grows with gain reduction.
//! - **Clean**: transparent, with lookahead; no colour, no oversampling.
//!
//! [`Compressor`] holds a chain per mode, crossfades when the mode changes, handles true
//! bypass (bit-identical once the bypass fade finishes) and measures the levels the UI
//! shows. The coloured modes run their saturation 4x oversampled ([`oversample`]).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod chain;
pub mod mode;
pub mod oversample;
pub mod util;

pub use chain::{Chain, ChainStats, MAX_GR_DB, MAX_LOOKAHEAD_MS, gain_computer};
pub use mode::Mode;
use util::{Delay, coef, lin_to_db};

/// Largest chunk processed in one go (internal scratch size).
pub const CHUNK: usize = 256;
/// Mode crossfade, ms.
const MODE_FADE_MS: f32 = 20.0;
/// Bypass crossfade, ms.
const BYPASS_FADE_MS: f32 = 10.0;

/// One mode's settings, in plain units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Threshold, dBFS.
    pub threshold_db: f32,
    /// Ratio (n:1).
    pub ratio: f32,
    /// Attack, ms.
    pub attack_ms: f32,
    /// Release, ms.
    pub release_ms: f32,
    /// Knee width, dB.
    pub knee_db: f32,
    /// Gain into the compressor, dB.
    pub gain_in_db: f32,
    /// Final output level (the makeup gain), dB.
    pub gain_out_db: f32,
    /// Wet/dry blend, 0..1 (1 = all wet).
    pub mix: f32,
    /// Wet level, dB (-60 = off).
    pub wet_db: f32,
    /// Dry level, dB (-60 = off).
    pub dry_db: f32,
    /// RMS detector (else peak).
    pub rms: bool,
    /// Auto (programme-dependent) release.
    pub auto_release: bool,
    /// Sidechain high-pass, Hz (20 = off).
    pub sc_hpf_hz: f32,
    /// Harmonic colour on (coloured modes).
    pub colour: bool,
    /// Lookahead, ms (Clean).
    pub lookahead_ms: f32,
    /// FET "all buttons in".
    pub all_buttons: bool,
    /// Auto-makeup: gain out follows the measured level difference.
    pub auto_makeup: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            knee_db: 6.0,
            gain_in_db: 0.0,
            gain_out_db: 0.0,
            mix: 1.0,
            wet_db: 0.0,
            dry_db: 0.0,
            rms: false,
            auto_release: false,
            sc_hpf_hz: 20.0,
            colour: true,
            lookahead_ms: 0.0,
            all_buttons: false,
            auto_makeup: false,
        }
    }
}

/// Settings shared by all modes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Global {
    /// Active mode.
    pub mode: Mode,
    /// True bypass.
    pub bypass: bool,
    /// Stereo link (one detector for both channels); else dual mono.
    pub link: bool,
    /// Detector listens to the external sidechain input.
    pub ext_sc: bool,
}

/// Average peak: the peak of each 20 ms window, averaged over about 1.5 s. Windows where
/// the gate signal is quiet don't count, so a pause doesn't drag it down.
#[derive(Clone, Copy, Debug)]
pub struct AvgPeak {
    win: usize,
    n: usize,
    peak: f32,
    avg: f32,
    a: f32,
    primed: bool,
    /// Windows counted so far: the first ones use a plain running mean, so the first
    /// window heard doesn't bias the average for seconds.
    count: u32,
}

/// Below this a window doesn't count (dBFS).
const GATE_DB: f32 = -70.0;

impl AvgPeak {
    /// A meter at `sr`.
    pub fn new(sr: f32) -> Self {
        let win = (sr * 0.02).round().max(1.0) as usize;
        Self {
            win,
            n: 0,
            peak: 0.0,
            avg: 0.0,
            // Per-window coefficient for a 1.5 s time constant.
            a: 1.0 - (-1.0 / (1.5 / 0.02f32)).exp(),
            primed: false,
            count: 0,
        }
    }

    /// Adds one sample's peak. At a window boundary returns whether the window counted
    /// (it was louder than the gate); otherwise false.
    pub fn push(&mut self, x: f32) -> bool {
        self.step(x, None)
    }

    /// As [`AvgPeak::push`], but the window counts when `gate` is true at its boundary:
    /// the after-meter counts exactly the windows the before-meter counted, even where its
    /// own output is quiet. Both must be pushed in lockstep.
    pub fn push_gated(&mut self, x: f32, gate: bool) -> bool {
        self.step(x, Some(gate))
    }

    fn step(&mut self, x: f32, gate: Option<bool>) -> bool {
        self.peak = self.peak.max(x);
        self.n += 1;
        if self.n < self.win {
            return false;
        }
        self.n = 0;
        let p = std::mem::take(&mut self.peak);
        let counts = gate.unwrap_or_else(|| lin_to_db(p) > GATE_DB);
        if counts {
            self.count = self.count.saturating_add(1);
            self.primed = true;
            let a = self.a.max(1.0 / self.count as f32);
            self.avg += (p - self.avg) * a;
        }
        counts
    }

    /// Current average, dBFS (very low until anything was heard).
    pub fn db(&self) -> f32 {
        if self.primed {
            lin_to_db(self.avg)
        } else {
            -120.0
        }
    }

    /// Forgets the history.
    pub fn reset(&mut self) {
        self.avg = 0.0;
        self.primed = false;
        self.count = 0;
        self.peak = 0.0;
        self.n = 0;
    }
}

/// What a call to [`Compressor::process`] measured.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// Input peak (dry, before gain in), linear.
    pub in_peak: f32,
    /// Output peak, linear.
    pub out_peak: f32,
    /// Largest gain reduction per channel, dB (positive).
    pub gr: [f32; 2],
    /// Peak of the wet part of the output, linear.
    pub wet_peak: f32,
    /// Peak of the dry part of the output, linear.
    pub dry_peak: f32,
    /// Average peak of the input, dBFS (the post meter's BEFORE).
    pub avg_before_db: f32,
    /// Average peak of the output, dBFS (the post meter's AFTER).
    pub avg_after_db: f32,
    /// Gain out the active mode's auto-makeup measures, dB.
    pub auto_makeup_db: f32,
}

/// The compressor: one chain per mode, mode crossfade, true bypass and meters.
pub struct Compressor {
    sr: f32,
    chains: Vec<Chain>,
    active: Mode,
    /// Mode being faded out, and the fade position (0 = all old, 1 = all new).
    fade_from: Option<Mode>,
    fade: f32,
    fade_step: f32,
    /// Bypass amount, 0 = processing, 1 = bypassed (exact).
    bypass: f32,
    bypass_step: f32,
    bypass_delay: [Delay; 2],
    before: AvgPeak,
    after: AvgPeak,
    scratch: [[f32; CHUNK]; 4],
    started: bool,
}

impl Compressor {
    /// A compressor at `sample_rate`.
    pub fn new(sample_rate: f32) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate > 1000.0 {
            sample_rate
        } else {
            48_000.0
        };
        let max_lat =
            (MAX_LOOKAHEAD_MS * 0.001 * sr).ceil() as usize + oversample::Oversampler::LATENCY;
        Self {
            sr,
            chains: Mode::ALL.iter().map(|m| Chain::new(*m, sr)).collect(),
            active: Mode::Vca,
            fade_from: None,
            fade: 1.0,
            fade_step: 1.0 / (MODE_FADE_MS * 0.001 * sr),
            bypass: 0.0,
            bypass_step: 1.0 / (BYPASS_FADE_MS * 0.001 * sr),
            bypass_delay: [Delay::new(max_lat), Delay::new(max_lat)],
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

    /// Latency of `mode` with `s`, samples.
    pub fn latency(&self, mode: Mode, s: &Settings) -> usize {
        Chain::latency_for(mode, s, self.sr)
    }

    /// Forgets the post meter's history (after MATCH, so the new reading settles fresh).
    pub fn reset_meters(&mut self) {
        self.after.reset();
    }

    /// Processes `io` in place (1 or 2 channels, any length). `sc` is the external sidechain
    /// (1 or 2 channels), used when `g.ext_sc` is on. `settings` holds every mode's
    /// settings; only the active mode (and the one fading out) runs.
    pub fn process(
        &mut self,
        io: &mut [&mut [f32]],
        sc: Option<&[&[f32]]>,
        g: &Global,
        settings: &[Settings; 5],
    ) -> Stats {
        let channels = io.len().min(2);
        let mut stats = Stats::default();
        if channels == 0 {
            return stats;
        }
        if !self.started {
            // First call: start in the requested state with no fades.
            self.started = true;
            self.active = g.mode;
            self.bypass = if g.bypass { 1.0 } else { 0.0 };
        }
        if g.mode != self.active {
            // A fade already running is cut short: the newest mode fades in from the old.
            self.fade_from = Some(self.active);
            self.active = g.mode;
            self.fade = 0.0;
            self.chains[g.mode.index()].reset();
        }
        let n_total = io.iter().map(|c| c.len()).min().unwrap_or(0);
        let mut start = 0;
        while start < n_total {
            let n = (n_total - start).min(CHUNK);
            self.chunk(io, sc, g, settings, start, n, &mut stats);
            start += n;
        }
        stats.avg_before_db = self.before.db();
        stats.avg_after_db = self.after.db();
        stats.auto_makeup_db = self.chains[self.active.index()].auto_makeup_db();
        stats
    }

    #[allow(clippy::too_many_arguments)]
    fn chunk(
        &mut self,
        io: &mut [&mut [f32]],
        sc: Option<&[&[f32]]>,
        g: &Global,
        settings: &[Settings; 5],
        start: usize,
        n: usize,
        stats: &mut Stats,
    ) {
        let channels = io.len().min(2);
        let range = start..start + n;
        // Copy the input aside: chains read it, the output is written over `io`.
        let [in0, in1, f0, f1] = &mut self.scratch;
        in0[..n].copy_from_slice(&io[0][range.clone()]);
        if channels > 1 {
            in1[..n].copy_from_slice(&io[1][range.clone()]);
        }
        let sc_slices: Option<[&[f32]; 2]> = match sc {
            Some(sc) if g.ext_sc && !sc.is_empty() && sc.iter().all(|c| c.len() >= start + n) => {
                let a = &sc[0][range.clone()];
                let b = sc.get(1).map_or(a, |c| &c[range.clone()]);
                Some([a, b])
            }
            _ => None,
        };
        let input: [&[f32]; 2] = [&in0[..n], if channels > 1 { &in1[..n] } else { &in0[..n] }];
        let active = self.active.index();
        let lat = Chain::latency_for(self.active, &settings[active], self.sr);
        let fade_start = self.fade;
        let lat_old = self.fade_from.map_or(lat, |m| {
            Chain::latency_for(m, &settings[m.index()], self.sr)
        });
        let (o0, rest) = io.split_at_mut(1);
        let out0 = &mut o0[0][range.clone()];
        let mut dummy = [0.0f32; CHUNK];
        let out1: &mut [f32] = if channels > 1 {
            &mut rest[0][range.clone()]
        } else {
            &mut dummy[..n]
        };
        let cs = self.chains[active].process(
            input,
            [out0, out1],
            channels,
            sc_slices,
            &settings[active],
            g.link,
        );
        stats.gr[0] = stats.gr[0].max(cs.gr[0]);
        stats.gr[1] = stats.gr[1].max(cs.gr[1]);
        stats.wet_peak = stats.wet_peak.max(cs.wet_peak);
        stats.dry_peak = stats.dry_peak.max(cs.dry_peak);

        // Mode crossfade: run the old chain too and blend.
        if let Some(old) = self.fade_from {
            let oi = old.index();
            self.chains[oi].process(
                input,
                [&mut f0[..n], &mut f1[..n]],
                channels,
                sc_slices,
                &settings[oi],
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

        // True bypass: the input, delayed by the active mode's latency so the host's
        // compensation still lines up. Exact once the fade is done.
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
                let x = if c == 0 { input[0][i] } else { input[1][i] };
                // While the mode fades, the bypass delay fades between the two latencies.
                let bd = &mut self.bypass_delay[c];
                bd.write(x);
                let d = if lat_old == lat {
                    bd.read(lat)
                } else {
                    let t = (fade_start + self.fade_step * i as f32).min(1.0);
                    bd.read(lat) * t + bd.read(lat_old) * (1.0 - t)
                };
                let y = &mut io[c][start + i];
                if b >= 1.0 {
                    *y = d;
                } else if b > 0.0 {
                    *y = *y * (1.0 - b) + d * b;
                }
                before = before.max(x.abs());
                after = after.max(y.abs());
            }
            stats.in_peak = stats.in_peak.max(before);
            stats.out_peak = stats.out_peak.max(after);
            let gate = self.before.push(before);
            self.after.push_gated(after, gate);
        }
        self.bypass = b;
    }
}

/// One-pole coefficient (re-exported for meters in the app).
pub fn smoothing_coef(ms: f32, sr: f32) -> f32 {
    coef(ms, sr)
}
