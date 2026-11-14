//! SolPlugs EQ DSP. Clean-room.
//!
//! Eight styles ([`Style`]), each its own curves and colour:
//! - **Pultec**: broad tube shelves and peak, the boost-and-cut trick.
//! - **API**: three bands with proportional Q, punchy discrete colour.
//! - **SSL**: four-band console parametric with high- and low-pass, clean.
//! - **Neve**: fixed-frequency Class A bands, warm transformer colour.
//! - **Clean**: up to 24 bands of any shape, per-band solo, dynamics and mid/side,
//!   zero-latency or linear-phase. No colour.
//! - **Massive**: four passive mastering bands with tube colour.
//! - **Maag**: the Air Band plus broad low and mid bands.
//! - **Curve**: three wide, bold console bands.
//!
//! [`Eq`] holds a chain per style, crossfades when the style changes, handles true bypass
//! (bit-identical once the bypass fade finishes) and measures the levels the UI shows.
//! The hardware styles run 4x oversampled ([`hw`]); Clean runs at the base rate or through
//! the linear-phase convolver ([`linear`]).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod clean;
pub mod filter;
pub mod hw;
pub mod linear;
pub mod mixer;
pub mod style;

pub use filter::{BandSpec, Design, Shape};
pub use solplugs_comp_core::AvgPeak;
pub use solplugs_comp_core::util;
pub use style::{MAX_KNOBS, Style};

use clean::CleanChain;
use hw::HwChain;
use solplugs_comp_core::oversample::Oversampler;
use solplugs_comp_core::util::Delay;

/// Largest chunk processed in one go (internal scratch size).
pub const CHUNK: usize = 256;
/// Samples between control updates (band smoothing, dynamics).
pub const CONTROL: usize = 16;
/// Most bands in Clean.
pub const MAX_BANDS: usize = 24;
/// Style crossfade, ms.
const STYLE_FADE_MS: f32 = 20.0;
/// Bypass crossfade, ms.
const BYPASS_FADE_MS: f32 = 10.0;

/// Settings every style has, in plain units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Common {
    /// Gain into the EQ, dB (drives the colour).
    pub gain_in_db: f32,
    /// Final output level (the makeup gain), dB.
    pub gain_out_db: f32,
    /// Wet/dry blend, 0..1 (1 = all wet).
    pub mix: f32,
    /// Wet level, dB (-60 = off).
    pub wet_db: f32,
    /// Dry level, dB (-60 = off).
    pub dry_db: f32,
    /// Auto-makeup: Gain Out follows the measured level difference.
    pub auto_makeup: bool,
    /// Harmonic colour on (hardware styles).
    pub colour: bool,
}

impl Default for Common {
    fn default() -> Self {
        Self {
            gain_in_db: 0.0,
            gain_out_db: 0.0,
            mix: 1.0,
            wet_db: 0.0,
            dry_db: 0.0,
            auto_makeup: false,
            colour: true,
        }
    }
}

/// One style's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StyleSettings {
    /// Shared controls.
    pub common: Common,
    /// The faceplate's knobs (see [`Style::knobs`]).
    pub knobs: [f32; MAX_KNOBS],
}

impl StyleSettings {
    /// Defaults for `style`.
    pub fn defaults(style: Style) -> Self {
        Self {
            common: Common::default(),
            knobs: style.defaults(),
        }
    }
}

/// Which signal a Clean band works on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Place {
    /// Both channels.
    #[default]
    Stereo,
    /// The mid (L+R)/2.
    Mid,
    /// The side (L-R)/2.
    Side,
}

impl Place {
    /// All placements.
    pub const ALL: [Place; 3] = [Place::Stereo, Place::Mid, Place::Side];

    /// From a parameter index.
    pub fn from_index(i: usize) -> Place {
        Place::ALL.get(i).copied().unwrap_or_default()
    }

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            Place::Stereo => "Stereo",
            Place::Mid => "Mid",
            Place::Side => "Side",
        }
    }
}

/// One Clean band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CleanBand {
    /// The band exists (shown on the graph).
    pub used: bool,
    /// The band is switched in.
    pub enabled: bool,
    /// Shape.
    pub shape: Shape,
    /// Frequency, Hz.
    pub freq: f32,
    /// Gain, dB.
    pub gain_db: f32,
    /// Q.
    pub q: f32,
    /// Slope, dB/oct.
    pub slope: u32,
    /// Stereo, mid or side.
    pub place: Place,
    /// Solo: hear only what this band touches.
    pub solo: bool,
    /// Dynamic EQ on.
    pub dynamic: bool,
    /// Dynamic threshold, dBFS.
    pub threshold_db: f32,
    /// Dynamic range: how far the gain moves past the threshold, dB (negative = cut).
    pub range_db: f32,
    /// Dynamic attack, ms.
    pub attack_ms: f32,
    /// Dynamic release, ms.
    pub release_ms: f32,
}

impl Default for CleanBand {
    fn default() -> Self {
        Self {
            used: false,
            enabled: true,
            shape: Shape::Bell,
            freq: 1000.0,
            gain_db: 0.0,
            q: 1.0,
            slope: 12,
            place: Place::Stereo,
            solo: false,
            dynamic: false,
            threshold_db: -24.0,
            range_db: -6.0,
            attack_ms: 10.0,
            release_ms: 150.0,
        }
    }
}

impl CleanBand {
    /// The static filter (no dynamics), on only when used and enabled.
    pub fn spec(&self) -> BandSpec {
        BandSpec {
            shape: self.shape,
            freq: f64::from(self.freq),
            gain_db: f64::from(self.gain_db),
            q: f64::from(self.q),
            slope: self.slope,
            on: self.used && self.enabled,
        }
    }
}

/// Clean's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CleanSettings {
    /// The bands.
    pub bands: [CleanBand; MAX_BANDS],
    /// Linear-phase mode (else zero latency).
    pub linear: bool,
}

impl Default for CleanSettings {
    fn default() -> Self {
        Self {
            bands: [CleanBand::default(); MAX_BANDS],
            linear: false,
        }
    }
}

/// Every style's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Per style (Clean's knobs are unused; its common controls are used).
    pub styles: [StyleSettings; Style::COUNT],
    /// Clean's bands.
    pub clean: CleanSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            styles: Style::ALL.map(StyleSettings::defaults),
            clean: CleanSettings::default(),
        }
    }
}

/// Settings shared by all styles.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Global {
    /// Active style.
    pub style: Style,
    /// True bypass.
    pub bypass: bool,
    /// Stereo link: dynamic bands use one detector for both channels; else dual mono.
    pub link: bool,
}

/// What a call to [`Eq::process`] measured.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// Input peak (dry, before gain in), linear.
    pub in_peak: f32,
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
    /// Gain Out the active style's auto-makeup measures, dB.
    pub auto_makeup_db: f32,
    /// Dynamic gain of each Clean band now, dB (for the graph).
    pub dynamic_db: [f32; MAX_BANDS],
}

/// A style's chain.
enum Chain {
    Hw(Box<HwChain>),
    Clean(Box<CleanChain>),
}

impl Chain {
    fn process(
        &mut self,
        input: [&[f32]; 2],
        out: [&mut [f32]; 2],
        channels: usize,
        s: &Settings,
        style: Style,
        link: bool,
    ) -> mixer::ChainStats {
        match self {
            Chain::Hw(c) => c.process(input, out, channels, &s.styles[style.index()]),
            Chain::Clean(c) => c.process(
                input,
                out,
                channels,
                &s.styles[style.index()],
                &s.clean,
                link,
            ),
        }
    }

    fn reset(&mut self) {
        match self {
            Chain::Hw(c) => c.reset(),
            Chain::Clean(c) => c.reset(),
        }
    }

    fn auto_db(&self) -> f32 {
        match self {
            Chain::Hw(c) => c.auto_db(),
            Chain::Clean(c) => c.auto_db(),
        }
    }
}

/// Latency of `style` with settings `s` at `sr`, samples.
pub fn latency_for(style: Style, s: &Settings, sr: f32) -> usize {
    if style.coloured() {
        Oversampler::LATENCY
    } else if s.clean.linear {
        linear::latency(sr)
    } else {
        0
    }
}

/// The EQ: one chain per style, style crossfade, true bypass and meters.
pub struct Eq {
    sr: f32,
    chains: Vec<Chain>,
    active: Style,
    fade_from: Option<Style>,
    fade: f32,
    fade_step: f32,
    bypass: f32,
    bypass_step: f32,
    bypass_delay: [Delay; 2],
    before: AvgPeak,
    after: AvgPeak,
    scratch: [[f32; CHUNK]; 4],
    started: bool,
}

impl Eq {
    /// An EQ at `sample_rate`.
    pub fn new(sample_rate: f32) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate > 1000.0 {
            sample_rate
        } else {
            48_000.0
        };
        let max_lat = linear::latency(sr).max(Oversampler::LATENCY);
        Self {
            sr,
            chains: Style::ALL
                .iter()
                .map(|s| {
                    if *s == Style::Clean {
                        Chain::Clean(Box::new(CleanChain::new(sr)))
                    } else {
                        Chain::Hw(Box::new(HwChain::new(*s, sr)))
                    }
                })
                .collect(),
            active: Style::Pultec,
            fade_from: None,
            fade: 1.0,
            fade_step: 1.0 / (STYLE_FADE_MS * 0.001 * sr),
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
            self.active = g.style;
            self.bypass = if g.bypass { 1.0 } else { 0.0 };
        }
        if g.style != self.active {
            self.fade_from = Some(self.active);
            self.active = g.style;
            self.fade = 0.0;
            self.chains[g.style.index()].reset();
        }
        let n_total = io.iter().map(|c| c.len()).min().unwrap_or(0);
        let mut start = 0;
        while start < n_total {
            let n = (n_total - start).min(CHUNK);
            self.chunk(io, g, s, start, n, &mut stats);
            start += n;
        }
        stats.avg_before_db = self.before.db();
        stats.avg_after_db = self.after.db();
        stats.auto_makeup_db = self.chains[self.active.index()].auto_db();
        if let Chain::Clean(c) = &self.chains[Style::Clean.index()] {
            stats.dynamic_db = c.dynamic_db();
        }
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
        let lat = latency_for(active, s, self.sr);
        let fade_start = self.fade;
        let lat_old = self.fade_from.map_or(lat, |st| latency_for(st, s, self.sr));
        let cs = {
            let (o0, rest) = io.split_at_mut(1);
            let out0 = &mut o0[0][range.clone()];
            let mut dummy = [0.0f32; CHUNK];
            let out1: &mut [f32] = if channels > 1 {
                &mut rest[0][range.clone()]
            } else {
                &mut dummy[..n]
            };
            self.chains[active.index()].process(input, [out0, out1], channels, s, active, g.link)
        };
        stats.wet_peak = stats.wet_peak.max(cs.wet_peak);
        stats.dry_peak = stats.dry_peak.max(cs.dry_peak);

        if let Some(old) = self.fade_from {
            self.chains[old.index()].process(
                input,
                [&mut f0[..n], &mut f1[..n]],
                channels,
                s,
                old,
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

        // True bypass: the input, delayed by the active style's latency so the host's
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
                let x = input[c][i];
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
                stats.out_lr[c] = stats.out_lr[c].max(y.abs());
            }
            if channels == 1 {
                stats.out_lr[1] = stats.out_lr[0];
            }
            stats.in_peak = stats.in_peak.max(before);
            stats.out_peak = stats.out_peak.max(after);
            let gate = self.before.push(before);
            self.after.push_gated(after, gate);
        }
        self.bypass = b;
    }
}
