//! The automatable parameters. Every mode has its own full set (that is the per-mode
//! memory: switch modes and back and your knobs are where you left them), plus four that
//! all modes share: Bypass, Mode, Stereo Link and External Sidechain.
//!
//! Ids are stable (hosts save automation by id): globals are 0-3, a mode's parameter is
//! `100 + 32 * mode + key`.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU32, Ordering};

use solplugs_comp_core::{Global, Mode, Settings};

/// A parameter id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Pid(pub u32);

/// Parameters shared by all modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum G {
    /// True bypass (the host's bypass maps here).
    Bypass = 0,
    /// Active mode.
    Mode = 1,
    /// Stereo link on (else dual mono).
    Link = 2,
    /// Detector listens to the external sidechain (inputs 3/4).
    ExtSc = 3,
}

/// A per-mode parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum K {
    /// Threshold, dBFS.
    Threshold = 0,
    /// Ratio.
    Ratio = 1,
    /// Attack, ms.
    Attack = 2,
    /// Release, ms.
    Release = 3,
    /// Knee, dB.
    Knee = 4,
    /// Gain In, dB.
    GainIn = 5,
    /// Gain Out (makeup), dB.
    GainOut = 6,
    /// Wet/dry blend, %.
    Mix = 7,
    /// Wet level, dB.
    Wet = 8,
    /// Dry level, dB.
    Dry = 9,
    /// Detector: peak or RMS.
    Detector = 10,
    /// Auto release.
    AutoRelease = 11,
    /// Sidechain high-pass, Hz.
    ScHpf = 12,
    /// Harmonic colour.
    Colour = 13,
    /// Auto-makeup.
    AutoMakeup = 14,
    /// Lookahead, ms (Clean only).
    Lookahead = 15,
    /// All buttons in (FET only).
    AllButtons = 16,
}

impl K {
    /// All keys, by number.
    pub const ALL: [K; 17] = [
        K::Threshold,
        K::Ratio,
        K::Attack,
        K::Release,
        K::Knee,
        K::GainIn,
        K::GainOut,
        K::Mix,
        K::Wet,
        K::Dry,
        K::Detector,
        K::AutoRelease,
        K::ScHpf,
        K::Colour,
        K::AutoMakeup,
        K::Lookahead,
        K::AllButtons,
    ];

    /// Whether `mode` has this parameter.
    pub fn in_mode(self, mode: Mode) -> bool {
        match self {
            K::Lookahead => mode == Mode::Clean,
            K::AllButtons => mode == Mode::Fet,
            K::Colour => mode != Mode::Clean,
            _ => true,
        }
    }

    fn label(self) -> (&'static str, &'static str) {
        match self {
            K::Threshold => ("Threshold", "Thresh"),
            K::Ratio => ("Ratio", "Ratio"),
            K::Attack => ("Attack", "Attack"),
            K::Release => ("Release", "Release"),
            K::Knee => ("Knee", "Knee"),
            K::GainIn => ("Gain In", "In"),
            K::GainOut => ("Gain Out", "Out"),
            K::Mix => ("Mix", "Mix"),
            K::Wet => ("Wet", "Wet"),
            K::Dry => ("Dry", "Dry"),
            K::Detector => ("Detector", "Det"),
            K::AutoRelease => ("Auto Release", "AutoRel"),
            K::ScHpf => ("Sidechain HPF", "SC HPF"),
            K::Colour => ("Colour", "Colour"),
            K::AutoMakeup => ("Auto Makeup", "AutoMk"),
            K::Lookahead => ("Lookahead", "Look"),
            K::AllButtons => ("All Buttons", "All"),
        }
    }
}

impl Pid {
    /// A shared parameter.
    pub const fn g(g: G) -> Pid {
        Pid(g as u32)
    }

    /// A mode's parameter.
    pub const fn m(mode: Mode, k: K) -> Pid {
        Pid(100 + 32 * mode as u32 + k as u32)
    }

    /// What it is.
    pub fn decode(self) -> Option<Which> {
        match self.0 {
            0 => Some(Which::G(G::Bypass)),
            1 => Some(Which::G(G::Mode)),
            2 => Some(Which::G(G::Link)),
            3 => Some(Which::G(G::ExtSc)),
            100..260 => {
                let m = ((self.0 - 100) / 32) as usize;
                let k = ((self.0 - 100) % 32) as usize;
                let mode = *Mode::ALL.get(m)?;
                let key = *K::ALL.get(k)?;
                key.in_mode(mode).then_some(Which::M(mode, key))
            }
            _ => None,
        }
    }

    /// Static description, or None for an unknown id.
    pub fn info(self) -> Option<&'static Info> {
        index_of(self).map(|i| &TABLE[i])
    }
}

/// A decoded id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    /// Shared.
    G(G),
    /// Per mode.
    M(Mode, K),
}

/// How a continuous parameter maps to the host's 0..1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    /// Even steps.
    Linear,
    /// Even ratios (times, frequencies, ratio).
    Log,
}

/// A parameter's description.
#[derive(Debug)]
pub struct Info {
    /// Id.
    pub id: Pid,
    /// Display name ("FET Attack").
    pub name: String,
    /// Short name.
    pub short: String,
    /// Group (mode name or "Global"), for hosts that show modules.
    pub group: &'static str,
    /// Lowest plain value.
    pub min: f64,
    /// Highest plain value.
    pub max: f64,
    /// Default plain value.
    pub default: f64,
    /// 0 = continuous, else the number of values.
    pub steps: u32,
    /// Mapping.
    pub scale: Scale,
}

impl Info {
    /// Clamps (and rounds stepped values).
    pub fn sanitize(&self, v: f64) -> f64 {
        let v = if v.is_finite() { v } else { self.default };
        let v = v.clamp(self.min, self.max);
        if self.steps > 0 { v.round() } else { v }
    }

    /// Plain to 0..1.
    pub fn normalize(&self, v: f64) -> f64 {
        let v = self.sanitize(v);
        let n = match self.scale {
            Scale::Linear => (v - self.min) / (self.max - self.min),
            Scale::Log => (v / self.min).ln() / (self.max / self.min).ln(),
        };
        n.clamp(0.0, 1.0)
    }

    /// 0..1 to plain.
    pub fn denormalize(&self, n: f64) -> f64 {
        let n = if n.is_finite() {
            n.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.sanitize(match self.scale {
            Scale::Linear => self.min + n * (self.max - self.min),
            Scale::Log => self.min * (self.max / self.min).powf(n),
        })
    }
}

/// (min, max, default, steps, scale) of a mode's parameter.
fn range(mode: Mode, k: K) -> (f64, f64, f64, u32, Scale) {
    use Mode::*;
    use Scale::*;
    let by_mode = |v: [f64; 5]| v[mode.index()];
    match k {
        K::Threshold => (
            -60.0,
            0.0,
            by_mode([-18.0, -20.0, -20.0, -20.0, -18.0]),
            0,
            Linear,
        ),
        K::Ratio => (1.0, 20.0, by_mode([4.0, 3.0, 4.0, 2.5, 4.0]), 0, Log),
        K::Attack => match mode {
            Vca => (0.1, 30.0, 10.0, 0, Log),
            Opto => (1.0, 100.0, 10.0, 0, Log),
            Fet => (0.02, 0.8, 0.3, 0, Log),
            VariMu => (0.5, 100.0, 25.0, 0, Log),
            Clean => (0.01, 200.0, 5.0, 0, Log),
        },
        K::Release => match mode {
            Vca => (50.0, 2000.0, 300.0, 0, Log),
            Opto => (100.0, 5000.0, 1200.0, 0, Log),
            Fet => (50.0, 1100.0, 250.0, 0, Log),
            VariMu => (100.0, 5000.0, 1500.0, 0, Log),
            Clean => (5.0, 3000.0, 120.0, 0, Log),
        },
        K::Knee => (0.0, 24.0, by_mode([3.0, 8.0, 3.0, 18.0, 6.0]), 0, Linear),
        K::GainIn | K::GainOut => (-24.0, 24.0, 0.0, 0, Linear),
        K::Mix => (0.0, 100.0, 100.0, 0, Linear),
        K::Wet | K::Dry => (-60.0, 12.0, 0.0, 0, Linear),
        K::Detector => (0.0, 1.0, by_mode([0.0, 1.0, 0.0, 1.0, 0.0]), 2, Linear),
        K::AutoRelease => (0.0, 1.0, by_mode([0.0, 1.0, 0.0, 1.0, 0.0]), 2, Linear),
        K::ScHpf => (20.0, 500.0, 20.0, 0, Log),
        K::Colour => (0.0, 1.0, 1.0, 2, Linear),
        K::AutoMakeup => (0.0, 1.0, 0.0, 2, Linear),
        K::Lookahead => (0.0, 10.0, 0.0, 0, Linear),
        K::AllButtons => (0.0, 1.0, 0.0, 2, Linear),
    }
}

fn global_info(g: G) -> Info {
    let (name, short, steps, default, max) = match g {
        G::Bypass => ("Bypass", "Byp", 2, 0.0, 1.0),
        G::Mode => ("Mode", "Mode", 5, 0.0, 4.0),
        G::Link => ("Stereo Link", "Link", 2, 1.0, 1.0),
        G::ExtSc => ("External Sidechain", "Ext SC", 2, 0.0, 1.0),
    };
    Info {
        id: Pid::g(g),
        name: name.into(),
        short: short.into(),
        group: "Global",
        min: 0.0,
        max,
        default,
        steps,
        scale: Scale::Linear,
    }
}

/// Every parameter, in host order.
pub static TABLE: LazyLock<Vec<Info>> = LazyLock::new(|| {
    let mut v: Vec<Info> = [G::Bypass, G::Mode, G::Link, G::ExtSc]
        .into_iter()
        .map(global_info)
        .collect();
    for mode in Mode::ALL {
        for k in K::ALL {
            if !k.in_mode(mode) {
                continue;
            }
            let (min, max, default, steps, scale) = range(mode, k);
            let (name, short) = k.label();
            v.push(Info {
                id: Pid::m(mode, k),
                name: format!("{} {name}", mode.name()),
                short: short.into(),
                group: mode.name(),
                min,
                max,
                default,
                steps,
                scale,
            });
        }
    }
    v
});

/// Number of parameters.
pub fn count() -> usize {
    TABLE.len()
}

/// Position of `id` in [`TABLE`].
pub fn index_of(id: Pid) -> Option<usize> {
    // The table is sorted by id.
    TABLE.binary_search_by_key(&id, |i| i.id).ok()
}

/// Display text for a plain value.
pub fn format(id: Pid, v: f64) -> String {
    let Some(info) = id.info() else {
        return String::new();
    };
    let v = info.sanitize(v);
    let on = v >= 0.5;
    match id.decode() {
        Some(Which::G(G::Bypass)) => if on { "Bypassed" } else { "Active" }.into(),
        Some(Which::G(G::Mode)) => Mode::from_index(v as usize).name().into(),
        Some(Which::G(G::Link)) => if on { "Linked" } else { "Dual Mono" }.into(),
        Some(Which::G(G::ExtSc)) => if on { "External" } else { "Internal" }.into(),
        Some(Which::M(_, k)) => match k {
            K::Threshold | K::Knee => format!("{v:.1} dB"),
            K::GainIn | K::GainOut => format!("{v:+.1} dB"),
            K::Ratio => {
                if v < 9.95 {
                    format!("{v:.1}:1")
                } else {
                    format!("{v:.0}:1")
                }
            }
            K::Attack | K::Release => ms_text(v),
            K::Mix => format!("{v:.0}%"),
            K::Wet | K::Dry => {
                if v <= -59.9 {
                    "Off".into()
                } else {
                    format!("{v:+.1} dB")
                }
            }
            K::Detector => if on { "RMS" } else { "Peak" }.into(),
            K::ScHpf => {
                if v <= 20.5 {
                    "Off".into()
                } else {
                    format!("{v:.0} Hz")
                }
            }
            K::Lookahead => format!("{v:.1} ms"),
            K::AutoRelease | K::Colour | K::AutoMakeup | K::AllButtons => {
                if on { "On" } else { "Off" }.into()
            }
        },
        None => String::new(),
    }
}

/// Milliseconds as text: µs below 1 ms, seconds from 1 s.
pub fn ms_text(v: f64) -> String {
    if v < 1.0 {
        format!("{:.0} µs", v * 1000.0)
    } else if v < 10.0 {
        format!("{v:.1} ms")
    } else if v < 1000.0 {
        format!("{v:.0} ms")
    } else {
        format!("{:.2} s", v / 1000.0)
    }
}

/// Parses display text back to a plain value.
pub fn parse(id: Pid, s: &str) -> Option<f64> {
    let info = id.info()?;
    let t = s.trim().to_ascii_lowercase();
    let named: Option<f64> = match id.decode()? {
        Which::G(G::Mode) => Mode::ALL
            .iter()
            .position(|m| m.name().eq_ignore_ascii_case(&t))
            .map(|i| i as f64),
        _ => match t.as_str() {
            "off" | "active" | "peak" | "internal" | "dual mono" => {
                if matches!(id.decode(), Some(Which::M(_, K::Wet | K::Dry))) {
                    Some(-60.0)
                } else if matches!(id.decode(), Some(Which::M(_, K::ScHpf))) {
                    Some(20.0)
                } else {
                    Some(0.0)
                }
            }
            "on" | "bypassed" | "rms" | "external" | "linked" => Some(1.0),
            _ => None,
        },
    };
    if let Some(v) = named {
        return Some(info.sanitize(v));
    }
    let num: String = t
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
        .collect();
    let mut v: f64 = num.parse().ok()?;
    if t.ends_with("µs") || t.ends_with("us") {
        v /= 1000.0;
    } else if t.ends_with(" s") && matches!(id.decode(), Some(Which::M(_, K::Attack | K::Release)))
    {
        v *= 1000.0;
    }
    Some(info.sanitize(v))
}

/// The live values, lock-free between the audio thread, the UI and the host glue.
pub struct Values {
    v: Vec<AtomicU32>,
}

impl Default for Values {
    fn default() -> Self {
        Self {
            v: TABLE
                .iter()
                .map(|i| AtomicU32::new((i.default as f32).to_bits()))
                .collect(),
        }
    }
}

impl Values {
    /// Plain value (the default for an unknown id).
    pub fn get(&self, id: Pid) -> f64 {
        index_of(id).map_or(0.0, |i| {
            f64::from(f32::from_bits(self.v[i].load(Ordering::Relaxed)))
        })
    }

    /// Sets a plain value (sanitized). Returns the stored value.
    pub fn set(&self, id: Pid, v: f64) -> f64 {
        let Some(i) = index_of(id) else {
            return 0.0;
        };
        let v = TABLE[i].sanitize(v);
        self.v[i].store((v as f32).to_bits(), Ordering::Relaxed);
        v
    }

    /// As a bool.
    pub fn on(&self, id: Pid) -> bool {
        self.get(id) >= 0.5
    }

    /// Shared parameter shortcut.
    pub fn g(&self, g: G) -> f64 {
        self.get(Pid::g(g))
    }

    /// Mode parameter shortcut.
    pub fn m(&self, mode: Mode, k: K) -> f64 {
        self.get(Pid::m(mode, k))
    }

    /// Active mode.
    pub fn mode(&self) -> Mode {
        Mode::from_index(self.g(G::Mode).round().max(0.0) as usize)
    }

    /// The shared settings.
    pub fn global(&self) -> Global {
        Global {
            mode: self.mode(),
            bypass: self.g(G::Bypass) >= 0.5,
            link: self.g(G::Link) >= 0.5,
            ext_sc: self.g(G::ExtSc) >= 0.5,
        }
    }

    /// One mode's settings for the DSP.
    pub fn settings(&self, mode: Mode) -> Settings {
        let f = |k: K| self.m(mode, k) as f32;
        let b = |k: K| k.in_mode(mode) && self.m(mode, k) >= 0.5;
        Settings {
            threshold_db: f(K::Threshold),
            ratio: f(K::Ratio),
            attack_ms: f(K::Attack),
            release_ms: f(K::Release),
            knee_db: f(K::Knee),
            gain_in_db: f(K::GainIn),
            gain_out_db: f(K::GainOut),
            mix: f(K::Mix) / 100.0,
            wet_db: f(K::Wet),
            dry_db: f(K::Dry),
            rms: b(K::Detector),
            auto_release: b(K::AutoRelease),
            sc_hpf_hz: f(K::ScHpf),
            colour: b(K::Colour),
            lookahead_ms: if mode == Mode::Clean {
                f(K::Lookahead)
            } else {
                0.0
            },
            all_buttons: b(K::AllButtons),
            auto_makeup: b(K::AutoMakeup),
        }
    }

    /// Every mode's settings.
    pub fn all_settings(&self) -> [Settings; 5] {
        Mode::ALL.map(|m| self.settings(m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_decode_and_index() {
        for (i, info) in TABLE.iter().enumerate() {
            assert_eq!(index_of(info.id), Some(i));
            assert!(info.id.decode().is_some());
            assert!(
                info.default >= info.min && info.default <= info.max,
                "{}",
                info.name
            );
        }
        assert_eq!(Pid(99).decode(), None);
        assert_eq!(Pid::m(Mode::Vca, K::Lookahead).decode(), None);
        assert!(Pid::m(Mode::Clean, K::Lookahead).decode().is_some());
    }

    #[test]
    fn values_round_trip_through_text_and_normalized() {
        for info in TABLE.iter() {
            for n in [0.0, 0.3, 0.5, 1.0] {
                let plain = info.denormalize(n);
                assert!((info.normalize(plain) - n).abs() < 1e-6 || info.steps > 0);
                let text = format(info.id, plain);
                let back = parse(info.id, &text).unwrap_or(f64::NAN);
                let tol = 0.051 * (plain.abs().max(1.0));
                assert!(
                    (back - plain).abs() <= tol,
                    "{}: {plain} -> {text} -> {back}",
                    info.name
                );
            }
        }
    }

    #[test]
    fn defaults_match_the_dsp_starting_points() {
        let v = Values::default();
        let s = v.settings(Mode::Opto);
        assert!(s.rms && s.auto_release);
        assert_eq!(s.ratio, 3.0);
        assert_eq!(v.global().mode, Mode::Vca);
        assert!(v.global().link);
    }
}
