//! The plugin's automatable parameters: one table both plugin formats and the UI read.

use std::sync::atomic::{AtomicU32, Ordering};

use solplugs_pitch_core::realtime::Range;
use solplugs_pitch_core::scale::{NOTE_NAMES, ScaleKind};

/// Parameter ids (stable: hosts save automation by id).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum P {
    /// Bypass (host bypass maps here).
    Bypass = 0,
    /// Key root, 0 = C.
    Key = 1,
    /// Scale, index into [`ScaleKind::ALL`].
    Scale = 2,
    /// Retune speed, ms.
    Retune = 3,
    /// Humanize, %.
    Humanize = 4,
    /// Correction amount, %.
    Amount = 5,
    /// Vibrato, %.
    Vibrato = 6,
    /// Formant preservation on/off.
    Formant = 7,
    /// Formant shift, semitones.
    FormantShift = 8,
    /// Transpose, semitones.
    Transpose = 9,
    /// Singer range, index into [`Range::ALL`].
    Range = 10,
    /// Mode: 0 automatic, 1 graph.
    Mode = 11,
}

/// Number of parameters.
pub const COUNT: usize = 12;

impl P {
    /// All, by id.
    pub const ALL: [P; COUNT] = [
        P::Bypass,
        P::Key,
        P::Scale,
        P::Retune,
        P::Humanize,
        P::Amount,
        P::Vibrato,
        P::Formant,
        P::FormantShift,
        P::Transpose,
        P::Range,
        P::Mode,
    ];

    /// From a host id.
    pub fn from_id(id: u32) -> Option<P> {
        Self::ALL.get(id as usize).copied()
    }

    /// Host id.
    pub fn id(self) -> u32 {
        self as u32
    }

    /// Static description.
    pub fn info(self) -> &'static Info {
        &INFO[self as usize]
    }
}

/// A parameter's description.
#[derive(Debug)]
pub struct Info {
    /// Display name.
    pub name: &'static str,
    /// Short name for small hosts.
    pub short: &'static str,
    /// Lowest plain value.
    pub min: f64,
    /// Highest plain value.
    pub max: f64,
    /// Default plain value.
    pub default: f64,
    /// 0 = continuous, otherwise the number of discrete values.
    pub steps: u32,
    /// Unit suffix for continuous values.
    pub unit: &'static str,
}

const fn info(
    name: &'static str,
    short: &'static str,
    min: f64,
    max: f64,
    default: f64,
    steps: u32,
    unit: &'static str,
) -> Info {
    Info {
        name,
        short,
        min,
        max,
        default,
        steps,
        unit,
    }
}

static INFO: [Info; COUNT] = [
    info("Bypass", "Byp", 0.0, 1.0, 0.0, 2, ""),
    info("Key", "Key", 0.0, 11.0, 0.0, 12, ""),
    info("Scale", "Scale", 0.0, 7.0, 0.0, 8, ""),
    info("Retune Speed", "Retune", 0.0, 400.0, 50.0, 0, "ms"),
    info("Humanize", "Human", 0.0, 100.0, 0.0, 0, "%"),
    info("Amount", "Amount", 0.0, 100.0, 100.0, 0, "%"),
    info("Vibrato", "Vibrato", 0.0, 200.0, 100.0, 0, "%"),
    info("Formant Preserve", "Formant", 0.0, 1.0, 1.0, 2, ""),
    info("Formant Shift", "FmtShift", -12.0, 12.0, 0.0, 0, "st"),
    info("Transpose", "Transp", -12.0, 12.0, 0.0, 25, "st"),
    info("Range", "Range", 0.0, 2.0, 1.0, 3, ""),
    info("Mode", "Mode", 0.0, 1.0, 0.0, 2, ""),
];

impl Info {
    /// Clamps and, for stepped parameters, rounds.
    pub fn sanitize(&self, v: f64) -> f64 {
        let v = if v.is_finite() { v } else { self.default };
        let v = v.clamp(self.min, self.max);
        if self.steps > 0 { v.round() } else { v }
    }

    /// Plain → 0..1.
    pub fn normalize(&self, v: f64) -> f64 {
        ((self.sanitize(v) - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    /// 0..1 → plain.
    pub fn denormalize(&self, n: f64) -> f64 {
        self.sanitize(self.min + n.clamp(0.0, 1.0) * (self.max - self.min))
    }
}

/// Display text for a plain value.
pub fn format(p: P, v: f64) -> String {
    let i = p.info().sanitize(v);
    match p {
        P::Bypass => if i >= 0.5 { "Bypassed" } else { "Active" }.into(),
        P::Formant => if i >= 0.5 { "On" } else { "Off" }.into(),
        P::Key => NOTE_NAMES[i as usize % 12].into(),
        P::Scale => ScaleKind::from_index(i as usize).name().into(),
        P::Range => Range::from_index(i as usize).name().into(),
        P::Mode => if i >= 0.5 { "Graph" } else { "Automatic" }.into(),
        P::Retune => format!("{i:.0} ms"),
        P::Humanize | P::Amount | P::Vibrato => format!("{i:.0}%"),
        P::FormantShift => format!("{i:+.1} st"),
        P::Transpose => format!("{i:+.0} st"),
    }
}

/// Parses display text back to a plain value (what [`format`] wrote, or a bare number).
pub fn parse(p: P, s: &str) -> Option<f64> {
    let s = s.trim();
    let info = p.info();
    let by_name = |names: &[&str]| {
        names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(s))
            .map(|i| i as f64)
    };
    let named = match p {
        P::Bypass => by_name(&["Active", "Bypassed"]),
        P::Formant => by_name(&["Off", "On"]),
        P::Key => by_name(&NOTE_NAMES),
        P::Scale => by_name(&ScaleKind::ALL.map(|k| k.name())),
        P::Range => by_name(&Range::ALL.map(|r| r.name())),
        P::Mode => by_name(&["Automatic", "Graph"]),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    let num: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
        .collect();
    num.parse::<f64>().ok().map(|v| info.sanitize(v))
}

/// The live values, shared lock-free between the audio thread, the UI and the host glue.
pub struct Values {
    v: [AtomicU32; COUNT],
}

impl Default for Values {
    fn default() -> Self {
        Self {
            v: std::array::from_fn(|i| AtomicU32::new((INFO[i].default as f32).to_bits())),
        }
    }
}

impl Values {
    /// Current plain value.
    pub fn get(&self, p: P) -> f64 {
        f64::from(f32::from_bits(self.v[p as usize].load(Ordering::Relaxed)))
    }

    /// Sets a plain value (sanitized). Returns the stored value.
    pub fn set(&self, p: P, v: f64) -> f64 {
        let v = p.info().sanitize(v);
        self.v[p as usize].store((v as f32).to_bits(), Ordering::Relaxed);
        v
    }

    /// As a bool.
    pub fn on(&self, p: P) -> bool {
        self.get(p) >= 0.5
    }

    /// As an index.
    pub fn index(&self, p: P) -> usize {
        self.get(p).round().max(0.0) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for p in P::ALL {
            let i = p.info();
            assert_eq!(P::from_id(p.id()), Some(p));
            for n in [0.0, 0.25, 0.5, 1.0] {
                let plain = i.denormalize(n);
                let text = format(p, plain);
                let back = parse(p, &text).unwrap_or(f64::NAN);
                assert!(
                    (back - plain).abs() < 0.051 * (i.max - i.min).max(1.0) / 10.0 + 0.051,
                    "{p:?} {plain} -> {text} -> {back}"
                );
            }
        }
    }
}
