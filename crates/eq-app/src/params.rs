//! The automatable parameters. Every style has its own full set (that is the per-style
//! memory: switch styles and back and your knobs are where you left them), Clean has 24
//! bands of 14 parameters, and three are shared: Bypass, Style and Stereo Link.
//!
//! Ids are stable (hosts save automation by id): globals are 0-2, a style's parameter is
//! `100 + 64 * style + key` (keys 0-7 are the shared controls, 8 on the faceplate's
//! knobs), and Clean band `b`'s parameter is `1000 + 16 * b + key`.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU32, Ordering};

use solplugs_eq_core::filter::SLOPES;
use solplugs_eq_core::style::Unit;
use solplugs_eq_core::{
    CleanBand, CleanSettings, Common, Global, MAX_BANDS, MAX_KNOBS, Place, Settings, Shape,
    Style, StyleSettings,
};

/// A parameter id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Pid(pub u32);

/// Parameters shared by all styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum G {
    /// True bypass (the host's bypass maps here).
    Bypass = 0,
    /// Active style.
    Style = 1,
    /// Stereo link (else dual mono).
    Link = 2,
}

/// A per-style shared control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum C {
    /// Gain In, dB.
    GainIn = 0,
    /// Gain Out (makeup), dB.
    GainOut = 1,
    /// Wet/dry blend, %.
    Mix = 2,
    /// Wet level, dB.
    Wet = 3,
    /// Dry level, dB.
    Dry = 4,
    /// Auto-makeup.
    AutoMakeup = 5,
    /// Harmonic colour (hardware styles).
    Colour = 6,
    /// Phase mode (Clean): zero latency or linear phase.
    Phase = 7,
}

impl C {
    /// All, by number.
    pub const ALL: [C; 8] = [
        C::GainIn,
        C::GainOut,
        C::Mix,
        C::Wet,
        C::Dry,
        C::AutoMakeup,
        C::Colour,
        C::Phase,
    ];

    /// Whether `style` has this control.
    pub fn in_style(self, style: Style) -> bool {
        match self {
            C::Colour => style.coloured(),
            C::Phase => style == Style::Clean,
            _ => true,
        }
    }
}

/// First knob key of a style.
pub const KNOB_BASE: u32 = 8;

/// A Clean band parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum B {
    /// The band exists.
    Used = 0,
    /// Switched in.
    On = 1,
    /// Shape.
    Shape = 2,
    /// Frequency.
    Freq = 3,
    /// Gain.
    Gain = 4,
    /// Q.
    Q = 5,
    /// Slope (index into the slope list).
    Slope = 6,
    /// Stereo / mid / side.
    Place = 7,
    /// Solo.
    Solo = 8,
    /// Dynamic on.
    Dynamic = 9,
    /// Dynamic threshold.
    Threshold = 10,
    /// Dynamic range.
    Range = 11,
    /// Dynamic attack.
    Attack = 12,
    /// Dynamic release.
    Release = 13,
}

impl B {
    /// All, by number.
    pub const ALL: [B; 14] = [
        B::Used,
        B::On,
        B::Shape,
        B::Freq,
        B::Gain,
        B::Q,
        B::Slope,
        B::Place,
        B::Solo,
        B::Dynamic,
        B::Threshold,
        B::Range,
        B::Attack,
        B::Release,
    ];
}

const SHAPE_NAMES: &[&str] = &[
    "Bell",
    "Low Shelf",
    "High Shelf",
    "Low Cut",
    "High Cut",
    "Notch",
    "Band Pass",
    "Tilt",
];
const SLOPE_NAMES: &[&str] = &["6 dB", "12 dB", "18 dB", "24 dB", "36 dB", "48 dB"];
const PLACE_NAMES: &[&str] = &["Stereo", "Mid", "Side"];
const STYLE_NAMES: &[&str] = &[
    "Pultec", "API", "SSL", "Neve", "Clean", "Massive", "Maag", "Curve",
];

impl Pid {
    /// A shared parameter.
    pub const fn g(g: G) -> Pid {
        Pid(g as u32)
    }

    /// A style's shared control.
    pub const fn c(style: Style, c: C) -> Pid {
        Pid(100 + 64 * style as u32 + c as u32)
    }

    /// Knob `i` of a style's faceplate.
    pub const fn k(style: Style, i: usize) -> Pid {
        Pid(100 + 64 * style as u32 + KNOB_BASE + i as u32)
    }

    /// A Clean band's parameter.
    pub const fn b(band: usize, b: B) -> Pid {
        Pid(1000 + 16 * band as u32 + b as u32)
    }

    /// What it is.
    pub fn decode(self) -> Option<Which> {
        match self.0 {
            0 => Some(Which::G(G::Bypass)),
            1 => Some(Which::G(G::Style)),
            2 => Some(Which::G(G::Link)),
            100..612 => {
                let s = ((self.0 - 100) / 64) as usize;
                let key = (self.0 - 100) % 64;
                let style = *Style::ALL.get(s)?;
                if key < KNOB_BASE {
                    let c = *C::ALL.get(key as usize)?;
                    c.in_style(style).then_some(Which::C(style, c))
                } else {
                    let i = (key - KNOB_BASE) as usize;
                    (i < style.knobs().len()).then_some(Which::K(style, i))
                }
            }
            1000..1384 => {
                let band = ((self.0 - 1000) / 16) as usize;
                let key = ((self.0 - 1000) % 16) as usize;
                Some(Which::B(band, *B::ALL.get(key)?))
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
    /// A style's shared control.
    C(Style, C),
    /// A style's faceplate knob.
    K(Style, usize),
    /// A Clean band parameter.
    B(usize, B),
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
    /// Hz.
    Hz,
    /// Percent.
    Pct,
    /// Q.
    Q,
    /// Milliseconds.
    Ms,
    /// 0-10 scale.
    Number,
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
    /// Display name ("Pultec Low Boost").
    pub name: String,
    /// Short name.
    pub short: String,
    /// Group (style name, "Clean Band 3" or "Global").
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

#[allow(clippy::too_many_arguments)]
fn info(
    id: Pid,
    name: String,
    short: &str,
    group: &str,
    (min, max, default): (f64, f64, f64),
    steps: u32,
    scale: Scale,
    kind: Kind,
) -> Info {
    Info {
        id,
        name,
        short: short.into(),
        group: group.into(),
        min,
        max,
        default,
        steps,
        scale,
        kind,
    }
}

fn global_info(g: G) -> Info {
    let (name, short, steps, default, max, kind) = match g {
        G::Bypass => ("Bypass", "Byp", 2, 0.0, 1.0, Kind::Switch("Active", "Bypassed")),
        G::Style => ("Style", "Style", 8, 0.0, 7.0, Kind::Choice(STYLE_NAMES)),
        G::Link => (
            "Stereo Link",
            "Link",
            2,
            1.0,
            1.0,
            Kind::Switch("Dual Mono", "Linked"),
        ),
    };
    info(
        Pid::g(g),
        name.into(),
        short,
        "Global",
        (0.0, max, default),
        steps,
        Scale::Linear,
        kind,
    )
}

fn common_info(style: Style, c: C) -> Info {
    use Scale::*;
    let (name, short, range, steps, kind) = match c {
        C::GainIn => ("Gain In", "In", (-24.0, 24.0, 0.0), 0, Kind::Db),
        C::GainOut => ("Gain Out", "Out", (-24.0, 24.0, 0.0), 0, Kind::Db),
        C::Mix => ("Mix", "Mix", (0.0, 100.0, 100.0), 0, Kind::Pct),
        C::Wet => ("Wet", "Wet", (-60.0, 12.0, 0.0), 0, Kind::Level),
        C::Dry => ("Dry", "Dry", (-60.0, 12.0, 0.0), 0, Kind::Level),
        C::AutoMakeup => (
            "Auto Makeup",
            "AutoMk",
            (0.0, 1.0, 0.0),
            2,
            Kind::Switch("Off", "On"),
        ),
        C::Colour => (
            "Colour",
            "Colour",
            (0.0, 1.0, 1.0),
            2,
            Kind::Switch("Off", "On"),
        ),
        C::Phase => (
            "Phase Mode",
            "Phase",
            (0.0, 1.0, 0.0),
            2,
            Kind::Switch("Zero Latency", "Linear Phase"),
        ),
    };
    info(
        Pid::c(style, c),
        format!("{} {name}", style.name()),
        short,
        style.name(),
        range,
        steps,
        Linear,
        kind,
    )
}

fn knob_info(style: Style, i: usize) -> Info {
    let k = &style.knobs()[i];
    let kind = match k.unit {
        Unit::Db => Kind::Db,
        Unit::Hz => Kind::Hz,
        Unit::Number => Kind::Number,
        Unit::Q => Kind::Q,
        Unit::Choice(c) => Kind::Choice(c),
        Unit::Switch(a, b) => Kind::Switch(a, b),
    };
    info(
        Pid::k(style, i),
        format!("{} {}", style.name(), k.name),
        k.short,
        style.name(),
        (f64::from(k.min), f64::from(k.max), f64::from(k.default)),
        k.steps(),
        if k.log { Scale::Log } else { Scale::Linear },
        kind,
    )
}

fn band_info(band: usize, b: B) -> Info {
    use Scale::*;
    let onoff = Kind::Switch("Off", "On");
    let (name, short, range, steps, scale, kind) = match b {
        B::Used => ("Used", "Used", (0.0, 1.0, 0.0), 2, Linear, onoff),
        B::On => ("On", "On", (0.0, 1.0, 1.0), 2, Linear, onoff),
        B::Shape => (
            "Shape",
            "Shape",
            (0.0, 7.0, 0.0),
            8,
            Linear,
            Kind::Choice(SHAPE_NAMES),
        ),
        B::Freq => ("Frequency", "Freq", (10.0, 30_000.0, 1000.0), 0, Log, Kind::Hz),
        B::Gain => ("Gain", "Gain", (-30.0, 30.0, 0.0), 0, Linear, Kind::Db),
        B::Q => ("Q", "Q", (0.1, 40.0, 1.0), 0, Log, Kind::Q),
        B::Slope => (
            "Slope",
            "Slope",
            (0.0, 5.0, 1.0),
            6,
            Linear,
            Kind::Choice(SLOPE_NAMES),
        ),
        B::Place => (
            "Placement",
            "Place",
            (0.0, 2.0, 0.0),
            3,
            Linear,
            Kind::Choice(PLACE_NAMES),
        ),
        B::Solo => ("Solo", "Solo", (0.0, 1.0, 0.0), 2, Linear, onoff),
        B::Dynamic => ("Dynamic", "Dyn", (0.0, 1.0, 0.0), 2, Linear, onoff),
        B::Threshold => (
            "Threshold",
            "Thresh",
            (-60.0, 0.0, -24.0),
            0,
            Linear,
            Kind::Db,
        ),
        B::Range => ("Range", "Range", (-24.0, 24.0, -6.0), 0, Linear, Kind::Db),
        B::Attack => ("Attack", "Att", (0.5, 200.0, 10.0), 0, Log, Kind::Ms),
        B::Release => ("Release", "Rel", (10.0, 2000.0, 150.0), 0, Log, Kind::Ms),
    };
    let group = format!("Clean Band {}", band + 1);
    info(
        Pid::b(band, b),
        format!("Band {} {name}", band + 1),
        short,
        &group,
        range,
        steps,
        scale,
        kind,
    )
}

/// Every parameter, in host order (sorted by id).
pub static TABLE: LazyLock<Vec<Info>> = LazyLock::new(|| {
    let mut v: Vec<Info> = [G::Bypass, G::Style, G::Link]
        .into_iter()
        .map(global_info)
        .collect();
    for style in Style::ALL {
        for c in C::ALL {
            if c.in_style(style) {
                v.push(common_info(style, c));
            }
        }
        for i in 0..style.knobs().len() {
            v.push(knob_info(style, i));
        }
    }
    for band in 0..MAX_BANDS {
        for b in B::ALL {
            v.push(band_info(band, b));
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
    match info.kind {
        Kind::Db => format!("{v:+.1} dB"),
        Kind::Level => {
            if v <= -59.9 {
                "Off".into()
            } else {
                format!("{v:+.1} dB")
            }
        }
        Kind::Hz => {
            // The SSL filters read "Off" at their ends.
            match id.decode() {
                Some(Which::K(Style::Ssl, i))
                    if (i == 0 && v <= 16.5) || (i == 1 && v >= 21_999.0) =>
                {
                    "Off".into()
                }
                _ => hz_text(v),
            }
        }
        Kind::Pct => format!("{v:.0}%"),
        Kind::Q => format!("{v:.2}"),
        Kind::Ms => ms_text(v),
        Kind::Number => format!("{v:.1}"),
        Kind::Choice(list) => {
            let label = list.get(v as usize).copied().unwrap_or("");
            match id.decode() {
                Some(Which::K(..)) if label != "Off" => {
                    if label.ends_with('k') {
                        format!("{label}Hz")
                    } else {
                        format!("{label} Hz")
                    }
                }
                _ => label.into(),
            }
        }
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
            let bare = lower.trim_end_matches("hz").trim();
            if let Some(i) = list
                .iter()
                .position(|l| l.eq_ignore_ascii_case(bare) || l.eq_ignore_ascii_case(t))
            {
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
        Kind::Level if lower == "off" => return Some(info.min),
        Kind::Hz if lower == "off" => {
            return Some(if info.default >= info.max {
                info.max
            } else {
                info.min
            });
        }
        _ => {}
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

    /// Active style.
    pub fn style(&self) -> Style {
        Style::from_index(self.get(Pid::g(G::Style)).round().max(0.0) as usize)
    }

    /// The shared settings.
    pub fn global(&self) -> Global {
        Global {
            style: self.style(),
            bypass: self.on(Pid::g(G::Bypass)),
            link: self.on(Pid::g(G::Link)),
        }
    }

    /// A style's shared control.
    pub fn c(&self, style: Style, c: C) -> f64 {
        self.get(Pid::c(style, c))
    }

    /// One style's settings for the DSP.
    pub fn style_settings(&self, style: Style) -> StyleSettings {
        let f = |c: C| self.c(style, c) as f32;
        let mut knobs = [0.0f32; MAX_KNOBS];
        for (i, slot) in knobs.iter_mut().enumerate().take(style.knobs().len()) {
            *slot = self.get(Pid::k(style, i)) as f32;
        }
        StyleSettings {
            common: Common {
                gain_in_db: f(C::GainIn),
                gain_out_db: f(C::GainOut),
                mix: f(C::Mix) / 100.0,
                wet_db: f(C::Wet),
                dry_db: f(C::Dry),
                auto_makeup: f(C::AutoMakeup) >= 0.5,
                colour: style.coloured() && f(C::Colour) >= 0.5,
            },
            knobs,
        }
    }

    /// One Clean band.
    pub fn band(&self, band: usize) -> CleanBand {
        let f = |b: B| self.get(Pid::b(band, b));
        CleanBand {
            used: f(B::Used) >= 0.5,
            enabled: f(B::On) >= 0.5,
            shape: Shape::from_index(f(B::Shape) as usize),
            freq: f(B::Freq) as f32,
            gain_db: f(B::Gain) as f32,
            q: f(B::Q) as f32,
            slope: SLOPES
                .get(f(B::Slope) as usize)
                .copied()
                .unwrap_or(12),
            place: Place::from_index(f(B::Place) as usize),
            solo: f(B::Solo) >= 0.5,
            dynamic: f(B::Dynamic) >= 0.5,
            threshold_db: f(B::Threshold) as f32,
            range_db: f(B::Range) as f32,
            attack_ms: f(B::Attack) as f32,
            release_ms: f(B::Release) as f32,
        }
    }

    /// Clean's settings.
    pub fn clean(&self) -> CleanSettings {
        CleanSettings {
            bands: std::array::from_fn(|b| self.band(b)),
            linear: self.c(Style::Clean, C::Phase) >= 0.5,
        }
    }

    /// Everything the DSP needs.
    pub fn settings(&self) -> Settings {
        Settings {
            styles: Style::ALL.map(|s| self.style_settings(s)),
            clean: self.clean(),
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
        assert_eq!(Pid::c(Style::Pultec, C::Phase).decode(), None);
        assert_eq!(Pid::c(Style::Clean, C::Colour).decode(), None);
        assert!(Pid::k(Style::Ssl, 13).decode().is_some());
        assert_eq!(Pid::k(Style::Maag, 7).decode(), None);
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
    fn defaults_are_the_dsp_defaults() {
        let v = Values::default();
        let s = v.settings();
        let d = Settings::default();
        assert_eq!(s.styles, d.styles);
        assert!(s.clean.bands.iter().all(|b| !b.used));
        assert!(v.global().link);
        assert_eq!(v.style(), Style::Pultec);
    }
}
