//! The three versions and every control each one has.
//!
//! All versions share one control set ([`Ctl`]): time (ms or a note value with tempo-sync),
//! feedback and the filter in the feedback loop, the Low Cut / High Cut knobs, the LoFi and
//! Analog buttons, ducking, reverse, the L/R offset, the stereo and ping-pong stages with
//! their own wet/dry, modulation, and the output (Gain In/Out, Mix, Wet, Dry,
//! auto-makeup). On top of that each version has a knob or two of its own
//! ([`Version::extras`]). Every version keeps its own values: that is the per-version
//! memory.

/// A delay version.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Version {
    /// Modern digital: clear repeats, exact timing, no colour.
    Clean = 0,
    /// Tape echo: repeats get warmer and darker, a little wow and flutter.
    #[default]
    Tape = 1,
    /// Bucket-brigade: repeats get darker and melt away fast.
    Analog = 2,
}

impl Version {
    /// All versions, in parameter order.
    pub const ALL: [Version; 3] = [Version::Clean, Version::Tape, Version::Analog];

    /// Number of versions.
    pub const COUNT: usize = 3;

    /// Index.
    pub fn index(self) -> usize {
        self as usize
    }

    /// From an index (Tape for out of range).
    pub fn from_index(i: usize) -> Version {
        Version::ALL.get(i).copied().unwrap_or_default()
    }

    /// Display name: the type, never a brand.
    pub fn name(self) -> &'static str {
        match self {
            Version::Clean => "Clean",
            Version::Tape => "Tape",
            Version::Analog => "Analog",
        }
    }

    /// The version's own knobs.
    pub fn extras(self) -> &'static [Spec] {
        match self {
            Version::Clean => &[DIFFUSION],
            Version::Tape => &[HEADS, SATURATION],
            Version::Analog => &[DRIVE, NOISE],
        }
    }

    /// Index of extra knob `e` in this version, if it has it.
    pub fn extra(self, e: Extra) -> Option<usize> {
        self.extras().iter().position(|s| s.name == e.spec().name)
    }

    /// Whether the version has modulation (Clean stays still).
    pub fn has_mod(self) -> bool {
        self != Version::Clean
    }

    /// Default of a shared control in this version (each version starts where it sounds
    /// like itself).
    pub fn default_of(self, c: Ctl) -> f32 {
        use Version::*;
        let base = c.spec().default;
        match (self, c) {
            (Clean, Ctl::Feedback) => 30.0,
            (Clean, Ctl::Mod) => 0.0,
            (Tape, Ctl::Feedback) => 40.0,
            (Tape, Ctl::FilterHp) => 70.0,
            (Tape, Ctl::FilterLp) => 7000.0,
            (Tape, Ctl::Mod) => 30.0,
            (Tape, Ctl::ModRate) => 0.6,
            (Analog, Ctl::Feedback) => 45.0,
            (Analog, Ctl::FilterHp) => 40.0,
            (Analog, Ctl::FilterLp) => 12000.0,
            (Analog, Ctl::Mod) => 25.0,
            (Analog, Ctl::ModRate) => 0.9,
            _ => base,
        }
    }

    /// Default values of the version's own knobs.
    pub fn extra_defaults(self) -> [f32; MAX_EXTRAS] {
        let mut v = [0.0; MAX_EXTRAS];
        for (slot, s) in v.iter_mut().zip(self.extras()) {
            *slot = s.default;
        }
        v
    }
}

/// Most knobs of its own a version has.
pub const MAX_EXTRAS: usize = 2;

/// Note values for tempo-sync, in parameter order.
pub const NOTES: [&str; 15] = [
    "1/16T", "1/16", "1/16D", "1/8T", "1/8", "1/8D", "1/4T", "1/4", "1/4D", "1/2T", "1/2", "1/2D",
    "1/1T", "1/1", "1/1D",
];

/// Length of note `index` in quarter notes (beats).
pub fn note_beats(index: usize) -> f32 {
    let i = index.min(NOTES.len() - 1);
    let base = [0.25, 0.5, 1.0, 2.0, 4.0][i / 3];
    match i % 3 {
        0 => base * 2.0 / 3.0,
        1 => base,
        _ => base * 1.5,
    }
}

/// Note `index` in words ("1/8 dotted").
pub fn note_words(index: usize) -> String {
    let i = index.min(NOTES.len() - 1);
    let base = ["1/16", "1/8", "1/4", "1/2", "1/1"][i / 3];
    match i % 3 {
        0 => format!("{base} triplet"),
        1 => base.to_string(),
        _ => format!("{base} dotted"),
    }
}

/// Length of note `index` at `bpm`, ms.
pub fn note_ms(index: usize, bpm: f32) -> f32 {
    let bpm = if bpm.is_finite() {
        bpm.clamp(20.0, 400.0)
    } else {
        120.0
    };
    note_beats(index) * 60_000.0 / bpm
}

/// How a value reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    /// Seconds.
    Seconds,
    /// Milliseconds.
    Ms,
    /// Percent.
    Pct,
    /// dB, signed.
    Db,
    /// Hz.
    Hz,
    /// A multiplier ("1.2x").
    Mult,
    /// Ratio n:1.
    Ratio,
    /// A pick from a list (the value is the index).
    Choice(&'static [&'static str]),
    /// A switch: (off text, on text).
    Switch(&'static str, &'static str),
}

/// One control's range and text.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    /// Name ("Pre-Delay").
    pub name: &'static str,
    /// Short name for hosts.
    pub short: &'static str,
    /// Lowest value.
    pub min: f32,
    /// Highest value.
    pub max: f32,
    /// Default.
    pub default: f32,
    /// Logarithmic travel.
    pub log: bool,
    /// How it reads.
    pub unit: Unit,
    /// Help shown on hover.
    pub help: &'static str,
}

impl Spec {
    /// Number of steps (0 = continuous).
    pub fn steps(&self) -> u32 {
        match self.unit {
            Unit::Choice(c) => c.len() as u32,
            Unit::Switch(..) => 2,
            _ => 0,
        }
    }
}

#[allow(clippy::too_many_arguments)]
const fn spec(
    name: &'static str,
    short: &'static str,
    (min, max, default): (f32, f32, f32),
    log: bool,
    unit: Unit,
    help: &'static str,
) -> Spec {
    Spec {
        name,
        short,
        min,
        max,
        default,
        log,
        unit,
        help,
    }
}

/// A control every version has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ctl {
    /// Delay time when tempo-sync is off, ms.
    Time = 0,
    /// Tempo-sync on/off.
    Sync = 1,
    /// Note value when tempo-sync is on (index into [`NOTES`]).
    Note = 2,
    /// Feedback, %. Above 100 it runs away into self-oscillation.
    Feedback = 3,
    /// L/R time offset: the right side lands later, ms.
    Offset = 4,
    /// High-pass in the feedback loop, Hz.
    FilterHp = 5,
    /// Low-pass in the feedback loop, Hz.
    FilterLp = 6,
    /// Low Cut knob on the repeats, Hz.
    LowCut = 7,
    /// High Cut knob on the repeats, Hz.
    HighCut = 8,
    /// LoFi button.
    LoFi = 9,
    /// Analog button.
    Analog = 10,
    /// Ducking amount, %.
    Duck = 11,
    /// Ducking release, ms.
    DuckRelease = 12,
    /// What drives the ducking: the dry input or the external sidechain.
    DuckSource = 13,
    /// Reverse.
    Reverse = 14,
    /// Stereo width of the stereo delay, %.
    Width = 15,
    /// The stereo delay's own wet level, %.
    StereoMix = 16,
    /// The ping-pong delay's own wet level, %.
    PingPongMix = 17,
    /// Modulation depth, %.
    Mod = 18,
    /// Modulation rate, Hz.
    ModRate = 19,
    /// Gain into the delay, dB.
    GainIn = 20,
    /// Final output level (makeup), dB.
    GainOut = 21,
    /// Wet/dry blend, %.
    Mix = 22,
    /// Wet level, dB.
    Wet = 23,
    /// Dry level, dB.
    Dry = 24,
    /// Auto-makeup.
    AutoMakeup = 25,
}

impl Ctl {
    /// All, in id order.
    pub const ALL: [Ctl; 26] = [
        Ctl::Time,
        Ctl::Sync,
        Ctl::Note,
        Ctl::Feedback,
        Ctl::Offset,
        Ctl::FilterHp,
        Ctl::FilterLp,
        Ctl::LowCut,
        Ctl::HighCut,
        Ctl::LoFi,
        Ctl::Analog,
        Ctl::Duck,
        Ctl::DuckRelease,
        Ctl::DuckSource,
        Ctl::Reverse,
        Ctl::Width,
        Ctl::StereoMix,
        Ctl::PingPongMix,
        Ctl::Mod,
        Ctl::ModRate,
        Ctl::GainIn,
        Ctl::GainOut,
        Ctl::Mix,
        Ctl::Wet,
        Ctl::Dry,
        Ctl::AutoMakeup,
    ];

    /// The control's range and text.
    pub fn spec(self) -> Spec {
        use Unit::*;
        match self {
            Ctl::Time => spec(
                "Time",
                "Time",
                (1.0, MAX_TIME_MS, 375.0),
                true,
                Ms,
                "Time between repeats, when tempo-sync is off. Tap tempo sets it by hand.",
            ),
            Ctl::Sync => spec(
                "Sync",
                "Sync",
                (0.0, 1.0, 1.0),
                false,
                Switch("Off", "On"),
                "Tempo-sync: lock the repeats to the song's tempo as a note value.",
            ),
            Ctl::Note => spec(
                "Note",
                "Note",
                (0.0, 14.0, 5.0),
                false,
                Choice(&NOTES),
                "Note value of the repeats when tempo-sync is on. D = dotted, T = triplet.",
            ),
            Ctl::Feedback => spec(
                "Feedback",
                "Fdbk",
                (0.0, 120.0, 35.0),
                false,
                Pct,
                "How many repeats. Past 100% it runs away into self-oscillation, on purpose.",
            ),
            Ctl::Offset => spec(
                "L/R Offset",
                "Offset",
                (0.0, 50.0, 0.0),
                false,
                Ms,
                "Shifts the right side later than the left, for width.",
            ),
            Ctl::FilterHp => spec(
                "Filter HP",
                "FiltHP",
                (20.0, 2000.0, 20.0),
                true,
                Hz,
                "High-pass in the feedback loop: each repeat gets thinner. 20 Hz = off.",
            ),
            Ctl::FilterLp => spec(
                "Filter LP",
                "FiltLP",
                (500.0, 20000.0, 20000.0),
                true,
                Hz,
                "Low-pass in the feedback loop: each repeat gets darker. 20 kHz = off.",
            ),
            Ctl::LowCut => spec(
                "Low Cut",
                "LoCut",
                (20.0, 1000.0, 20.0),
                true,
                Hz,
                "Rolls the low end off the repeats (the dry stays untouched). 20 Hz = off.",
            ),
            Ctl::HighCut => spec(
                "High Cut",
                "HiCut",
                (1000.0, 20000.0, 20000.0),
                true,
                Hz,
                "Rolls the top end off the repeats (the dry stays untouched). 20 kHz = off.",
            ),
            Ctl::LoFi => spec(
                "LoFi",
                "LoFi",
                (0.0, 1.0, 0.0),
                false,
                Switch("Off", "On"),
                "Lo-fi grit on the repeats: lower sample rate and fewer bits.",
            ),
            Ctl::Analog => spec(
                "Analog",
                "Analog",
                (0.0, 1.0, 0.0),
                false,
                Switch("Off", "On"),
                "Analog warmth on the repeats: soft drive and a rounder top, every time round.",
            ),
            Ctl::Duck => spec(
                "Ducking",
                "Duck",
                (0.0, 100.0, 0.0),
                false,
                Pct,
                "The repeats turn down while the dry signal plays and come back up when it stops.",
            ),
            Ctl::DuckRelease => spec(
                "Duck Release",
                "DuckRel",
                (20.0, 2000.0, 300.0),
                true,
                Ms,
                "How fast the repeats come back up after the dry signal stops.",
            ),
            Ctl::DuckSource => spec(
                "Duck Source",
                "DuckSrc",
                (0.0, 1.0, 0.0),
                false,
                Choice(&["Dry", "Sidechain"]),
                "What ducks the repeats: the dry signal, or the external sidechain on inputs 3/4.",
            ),
            Ctl::Reverse => spec(
                "Reverse",
                "Rev",
                (0.0, 1.0, 0.0),
                false,
                Switch("Off", "On"),
                "The repeats play backwards.",
            ),
            Ctl::Width => spec(
                "Width",
                "Width",
                (0.0, 200.0, 100.0),
                false,
                Pct,
                "Stereo width of the stereo delay. 0 = mono, 100 = natural, 200 = extra wide.",
            ),
            Ctl::StereoMix => spec(
                "Stereo Mix",
                "StMix",
                (0.0, 100.0, 100.0),
                false,
                Pct,
                "The stereo delay's own wet level. Blend it against the ping-pong delay.",
            ),
            Ctl::PingPongMix => spec(
                "Ping-Pong Mix",
                "PPMix",
                (0.0, 100.0, 0.0),
                false,
                Pct,
                "The ping-pong delay's own wet level: repeats that bounce left and right. 0 = off.",
            ),
            Ctl::Mod => spec(
                "Modulation",
                "Mod",
                (0.0, 100.0, 30.0),
                false,
                Pct,
                "Wow and flutter on Tape, a little chorus on Analog. Clean stays still.",
            ),
            Ctl::ModRate => spec(
                "Mod Rate",
                "ModRt",
                (0.1, 5.0, 0.8),
                true,
                Hz,
                "Speed of the wow (Tape) or the chorus (Analog).",
            ),
            Ctl::GainIn => spec(
                "Gain In",
                "In",
                (-24.0, 24.0, 0.0),
                false,
                Db,
                "Level into the delay (the dry path is untouched).",
            ),
            Ctl::GainOut => spec(
                "Gain Out",
                "Out",
                (-24.0, 24.0, 0.0),
                false,
                Db,
                "Final output level: the makeup gain. MATCH and auto-makeup set it.",
            ),
            Ctl::Mix => spec(
                "Mix",
                "Mix",
                (0.0, 100.0, 30.0),
                false,
                Pct,
                "Blend of the repeats (wet) and the original (dry).",
            ),
            Ctl::Wet => spec(
                "Wet",
                "Wet",
                (-60.0, 12.0, 0.0),
                false,
                Db,
                "Level of the repeats.",
            ),
            Ctl::Dry => spec(
                "Dry",
                "Dry",
                (-60.0, 12.0, 0.0),
                false,
                Db,
                "Level of the original signal.",
            ),
            Ctl::AutoMakeup => spec(
                "Auto Makeup",
                "AutoMk",
                (0.0, 1.0, 0.0),
                false,
                Switch("Off", "On"),
                "Gain Out tracks live so the output stays as loud as the input.",
            ),
        }
    }
}

/// Longest delay time, ms.
pub const MAX_TIME_MS: f32 = 4000.0;

/// A version's own knob, by meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extra {
    /// Smear of the repeats (Clean).
    Diffusion,
    /// Which playback heads (Tape).
    Heads,
    /// Tape saturation (Tape).
    Saturation,
    /// Bucket-brigade drive (Analog).
    Drive,
    /// Clock noise (Analog).
    Noise,
}

impl Extra {
    /// The knob's spec.
    pub fn spec(self) -> Spec {
        match self {
            Extra::Diffusion => DIFFUSION,
            Extra::Heads => HEADS,
            Extra::Saturation => SATURATION,
            Extra::Drive => DRIVE,
            Extra::Noise => NOISE,
        }
    }
}

const DIFFUSION: Spec = spec(
    "Diffusion",
    "Diff",
    (0.0, 100.0, 0.0),
    false,
    Unit::Pct,
    "Smears each repeat a little more: from sharp echoes to soft clouds.",
);
const HEADS: Spec = spec(
    "Heads",
    "Heads",
    (0.0, 2.0, 0.0),
    false,
    Unit::Choice(&["1", "1+2", "1+2+3"]),
    "Which playback heads read the tape. Extra heads add earlier echoes for a galloping rhythm.",
);
const SATURATION: Spec = spec(
    "Saturation",
    "Sat",
    (0.0, 100.0, 35.0),
    false,
    Unit::Pct,
    "How hard the repeats hit the tape: more = thicker, rounder, squashed.",
);
const DRIVE: Spec = spec(
    "Drive",
    "Drive",
    (0.0, 100.0, 30.0),
    false,
    Unit::Pct,
    "How hard the repeats hit the bucket-brigade chip: more = fuzzier, melts faster.",
);
const NOISE: Spec = spec(
    "Noise",
    "Noise",
    (0.0, 100.0, 20.0),
    false,
    Unit::Pct,
    "The faint hiss of the clock and the chip.",
);
