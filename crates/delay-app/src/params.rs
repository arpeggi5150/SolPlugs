//! The automatable parameters. Every version has its own full set (that is the per-version
//! memory: switch versions and back and your knobs are where you left them), and four
//! are shared: Bypass, Version, Stereo Link and Tempo.
//!
//! Ids are stable (hosts save automation by id): globals are 0-3, and a version's
//! parameter is `100 + 64 * version + key`: keys 0-31 are the shared controls
//! ([`Ctl`]), 32 on the version's own knobs.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU32, Ordering};

use solplugs_delay_core::{
    Ctl, Global, MAX_EXTRAS, Settings, Spec, Unit, Version, VersionSettings,
};

/// A parameter id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Pid(pub u32);

/// Parameters shared by all versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum G {
    /// True bypass (the host's bypass maps here).
    Bypass = 0,
    /// Active version.
    Version = 1,
    /// Stereo link (else dual mono).
    Link = 2,
    /// The delay's own tempo, used when the host gives none (the standalone app).
    Tempo = 3,
}

/// First extra-knob key of a version.
pub const EXTRA_BASE: u32 = 32;

const VERSION_NAMES: &[&str] = &["Clean", "Tape", "Analog"];

impl Pid {
    /// A shared parameter.
    pub const fn g(g: G) -> Pid {
        Pid(g as u32)
    }

    /// A version's control.
    pub const fn c(v: Version, c: Ctl) -> Pid {
        Pid(100 + 64 * v as u32 + c as u32)
    }

    /// Extra knob `i` of a version.
    pub const fn x(v: Version, i: usize) -> Pid {
        Pid(100 + 64 * v as u32 + EXTRA_BASE + i as u32)
    }

    /// What it is.
    pub fn decode(self) -> Option<Which> {
        match self.0 {
            0 => Some(Which::G(G::Bypass)),
            1 => Some(Which::G(G::Version)),
            2 => Some(Which::G(G::Link)),
            3 => Some(Which::G(G::Tempo)),
            100..292 => {
                let v = *Version::ALL.get(((self.0 - 100) / 64) as usize)?;
                let key = (self.0 - 100) % 64;
                if key < EXTRA_BASE {
                    Some(Which::C(v, *Ctl::ALL.get(key as usize)?))
                } else {
                    let i = (key - EXTRA_BASE) as usize;
                    (i < v.extras().len()).then_some(Which::X(v, i))
                }
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
    /// A version's shared control.
    C(Version, Ctl),
    /// A version's own knob.
    X(Version, usize),
}

/// How a continuous parameter maps to the host's 0..1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    /// Even steps.
    Linear,
    /// Even ratios.
    Log,
}

/// How a value reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Signed dB.
    Db,
    /// A level in dB where the bottom is "Off".
    Level,
    /// Seconds.
    Seconds,
    /// Milliseconds.
    Ms,
    /// Hz.
    Hz,
    /// Percent.
    Pct,
    /// Multiplier.
    Mult,
    /// Ratio n:1.
    Ratio,
    /// Beats per minute.
    Bpm,
    /// A list.
    Choice(&'static [&'static str]),
    /// Off/on texts.
    Switch(&'static str, &'static str),
}

/// A parameter's description.
#[derive(Debug)]
pub struct Info {
    /// Id.
    pub id: Pid,
    /// Display name ("Hall Decay").
    pub name: String,
    /// Short name.
    pub short: String,
    /// Group (version name or "Global").
    pub group: String,
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
    /// How it reads.
    pub kind: Kind,
    /// Help line.
    pub help: &'static str,
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

fn kind_of(unit: Unit, c: Option<Ctl>) -> Kind {
    match unit {
        Unit::Db if matches!(c, Some(Ctl::Wet | Ctl::Dry)) => Kind::Level,
        Unit::Db => Kind::Db,
        Unit::Seconds => Kind::Seconds,
        Unit::Ms => Kind::Ms,
        Unit::Hz => Kind::Hz,
        Unit::Pct => Kind::Pct,
        Unit::Mult => Kind::Mult,
        Unit::Ratio => Kind::Ratio,
        Unit::Choice(l) => Kind::Choice(l),
        Unit::Switch(a, b) => Kind::Switch(a, b),
    }
}

fn spec_info(id: Pid, v: Version, s: &Spec, default: f32, c: Option<Ctl>) -> Info {
    Info {
        id,
        name: format!("{} {}", v.name(), s.name),
        short: s.short.into(),
        group: v.name().into(),
        min: f64::from(s.min),
        max: f64::from(s.max),
        default: f64::from(default),
        steps: s.steps(),
        scale: if s.log { Scale::Log } else { Scale::Linear },
        kind: kind_of(s.unit, c),
        help: s.help,
    }
}

fn global_info(g: G) -> Info {
    let (name, short, steps, default, max, kind, help) = match g {
        G::Tempo => {
            return Info {
                id: Pid::g(g),
                name: "Tempo".into(),
                short: "BPM".into(),
                group: "Global".into(),
                min: 20.0,
                max: 300.0,
                default: 120.0,
                steps: 0,
                scale: Scale::Linear,
                kind: Kind::Bpm,
                help: "Tempo for tempo-sync when the host doesn't send one (the standalone app). In a DAW the song's tempo is used.",
            };
        }
        G::Bypass => (
            "Bypass",
            "Byp",
            2,
            0.0,
            1.0,
            Kind::Switch("Active", "Bypassed"),
            "True bypass: the output is exactly the input.",
        ),
        G::Version => (
            "Version",
            "Version",
            3,
            1.0,
            2.0,
            Kind::Choice(VERSION_NAMES),
            "Which delay: Clean, Tape or Analog.",
        ),
        G::Link => (
            "Stereo Link",
            "Link",
            2,
            1.0,
            1.0,
            Kind::Switch("Dual Mono", "Linked"),
            "Linked: one delay drives both channels (wide). Dual mono: each side its own delay.",
        ),
    };
    Info {
        id: Pid::g(g),
        name: name.into(),
        short: short.into(),
        group: "Global".into(),
        min: 0.0,
        max,
        default,
        steps,
        scale: Scale::Linear,
        kind,
        help,
    }
}

/// Every parameter, in host order (sorted by id).
pub static TABLE: LazyLock<Vec<Info>> = LazyLock::new(|| {
    let mut t: Vec<Info> = [G::Bypass, G::Version, G::Link, G::Tempo]
        .into_iter()
        .map(global_info)
        .collect();
    for v in Version::ALL {
        for c in Ctl::ALL {
            t.push(spec_info(
                Pid::c(v, c),
                v,
                &c.spec(),
                v.default_of(c),
                Some(c),
            ));
        }
        for (i, s) in v.extras().iter().enumerate() {
            t.push(spec_info(Pid::x(v, i), v, s, s.default, None));
        }
    }
    t
});

/// Number of parameters.
pub fn count() -> usize {
    TABLE.len()
}

/// Position of `id` in [`TABLE`].
pub fn index_of(id: Pid) -> Option<usize> {
    TABLE.binary_search_by_key(&id, |i| i.id).ok()
}

/// Hz as text ("850 Hz", "1.52 kHz").
pub fn hz_text(v: f64) -> String {
    if v >= 10_000.0 {
        format!("{:.1} kHz", v / 1000.0)
    } else if v >= 1000.0 {
        format!("{:.2} kHz", v / 1000.0)
    } else if v >= 100.0 {
        format!("{v:.0} Hz")
    } else {
        format!("{v:.1} Hz")
    }
}

/// Milliseconds as text.
pub fn ms_text(v: f64) -> String {
    if v < 10.0 {
        format!("{v:.1} ms")
    } else if v < 1000.0 {
        format!("{v:.0} ms")
    } else {
        format!("{:.2} s", v / 1000.0)
    }
}

/// Display text for a plain value.
pub fn format(id: Pid, v: f64) -> String {
    let Some(info) = id.info() else {
        return String::new();
    };
    let v = info.sanitize(v);
    let off_at_end = |v: f64| match id.decode() {
        Some(Which::C(_, Ctl::LowCut)) => v <= 20.5,
        Some(Which::C(_, Ctl::HighCut)) => v >= 19_900.0,
        Some(Which::C(_, Ctl::FilterHp)) => v <= 20.5,
        Some(Which::C(_, Ctl::FilterLp)) => v >= 19_900.0,
        Some(Which::C(_, Ctl::PingPongMix | Ctl::Duck | Ctl::Offset)) => v <= 0.0,
        Some(Which::C(_, Ctl::Mod)) => v <= 0.0,
        _ => false,
    };
    if off_at_end(v) {
        return "Off".into();
    }
    match info.kind {
        Kind::Db => format!("{v:+.1} dB"),
        Kind::Level => {
            if v <= -59.9 {
                "Off".into()
            } else {
                format!("{v:+.1} dB")
            }
        }
        Kind::Seconds => {
            if v < 10.0 {
                format!("{v:.2} s")
            } else {
                format!("{v:.1} s")
            }
        }
        Kind::Ms => ms_text(v),
        Kind::Bpm => format!("{v:.1} BPM"),
        Kind::Hz => hz_text(v),
        Kind::Pct => format!("{v:.0}%"),
        Kind::Mult => format!("{v:.2}x"),
        Kind::Ratio => format!("{v:.1}:1"),
        Kind::Choice(list) => list.get(v as usize).copied().unwrap_or("").into(),
        Kind::Switch(off, on) => if v >= 0.5 { on } else { off }.into(),
    }
}

/// Parses display text back to a plain value.
pub fn parse(id: Pid, s: &str) -> Option<f64> {
    let info = id.info()?;
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    match info.kind {
        Kind::Choice(list) => {
            if let Some(i) = list.iter().position(|l| l.eq_ignore_ascii_case(t)) {
                return Some(i as f64);
            }
        }
        Kind::Switch(off, on) => {
            if lower == on.to_ascii_lowercase() || lower == "on" {
                return Some(1.0);
            }
            if lower == off.to_ascii_lowercase() || lower == "off" {
                return Some(0.0);
            }
        }
        _ => {}
    }
    if lower == "off" {
        return Some(match id.decode() {
            Some(Which::C(_, Ctl::HighCut | Ctl::FilterLp)) => info.max,
            _ => info.min,
        });
    }
    let num: String = lower
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
        .collect();
    let mut v: f64 = num.parse().ok()?;
    let rest = lower[num.len()..].trim();
    if matches!(info.kind, Kind::Hz) && rest.starts_with('k') {
        v *= 1000.0;
    }
    if matches!(info.kind, Kind::Ms) && rest == "s" {
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
    /// Plain value (0 for an unknown id).
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

    /// Active version.
    pub fn version(&self) -> Version {
        Version::from_index(self.get(Pid::g(G::Version)).round().max(0.0) as usize)
    }

    /// The shared settings.
    pub fn global(&self) -> Global {
        Global {
            version: self.version(),
            bypass: self.on(Pid::g(G::Bypass)),
            link: self.on(Pid::g(G::Link)),
            bpm: self.get(Pid::g(G::Tempo)) as f32,
        }
    }

    /// A version's control.
    pub fn c(&self, v: Version, c: Ctl) -> f64 {
        self.get(Pid::c(v, c))
    }

    /// One version's settings for the DSP.
    pub fn version_settings(&self, v: Version) -> VersionSettings {
        let mut s = VersionSettings::from_fn(|c| self.c(v, c) as f32);
        let mut extras = [0.0f32; MAX_EXTRAS];
        for (i, slot) in extras.iter_mut().enumerate().take(v.extras().len()) {
            *slot = self.get(Pid::x(v, i)) as f32;
        }
        s.extras = extras;
        s
    }

    /// Everything the DSP needs.
    pub fn settings(&self) -> Settings {
        Settings {
            versions: Version::ALL.map(|v| self.version_settings(v)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_decode_and_index() {
        for (i, info) in TABLE.iter().enumerate() {
            assert_eq!(index_of(info.id), Some(i));
            assert!(info.id.decode().is_some(), "{}", info.name);
            assert!(
                info.default >= info.min && info.default <= info.max,
                "{}",
                info.name
            );
        }
        assert_eq!(Pid(99).decode(), None);
        assert_eq!(Pid::x(Version::Clean, 1).decode(), None);
        assert!(Pid::c(Version::Analog, Ctl::Reverse).decode().is_some());
        assert_eq!(Pid(292).decode(), None);
    }

    #[test]
    fn values_round_trip_through_text_and_normalized() {
        for info in TABLE.iter() {
            for n in [0.0, 0.3, 0.5, 1.0] {
                let plain = info.denormalize(n);
                assert!((info.normalize(plain) - n).abs() < 1e-6 || info.steps > 0);
                let text = format(info.id, plain);
                let back = parse(info.id, &text).unwrap_or(f64::NAN);
                let tol = 0.051 * plain.abs().max(1.0);
                assert!(
                    (back - plain).abs() <= tol,
                    "{}: {plain} -> {text} -> {back}",
                    info.name
                );
            }
        }
    }

    #[test]
    fn defaults_are_the_dsp_defaults() {
        let v = Values::default();
        let s = v.settings();
        assert_eq!(s, Settings::default());
        assert!(v.global().link);
        assert_eq!(v.version(), Version::Tape);
        assert_eq!(v.global().bpm, 120.0);
    }
}
