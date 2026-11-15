//! The five versions and every control each one has.
//!
//! All versions share one control set ([`Ctl`]): the five tank knobs on the hardware strip
//! (Decay, Size, Pre-Delay, Damping, Diffusion), high and low damping, freeze, the wet-only chain (saturation, EQ,
//! compressor), the stereo and ping-pong stages, and the output (Gain In/Out, Mix, Wet,
//! Dry, auto-makeup). On top of that each version has a few knobs of its own
//! ([`Version::extras`]). Every version keeps its own values: that is the per-version
//! memory.

/// A reverb version.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Version {
    /// Transparent modern digital: a dense, smooth 16-line network.
    Clean = 0,
    /// Dark 80s rack hall: lush modulation, a band-limited input.
    #[default]
    Hall = 1,
    /// Studio room: strong early reflections, short dense tail.
    Room = 2,
    /// Steel plate: instant density, bright, no early reflections.
    Plate = 3,
    /// Tube-driven spring tank: dispersive chirps ("drip").
    Spring = 4,
}

impl Version {
    /// All versions, in parameter order.
    pub const ALL: [Version; 5] = [
        Version::Clean,
        Version::Hall,
        Version::Room,
        Version::Plate,
        Version::Spring,
    ];

    /// Number of versions.
    pub const COUNT: usize = 5;

    /// Index.
    pub fn index(self) -> usize {
        self as usize
    }

    /// From an index (Hall for out of range).
    pub fn from_index(i: usize) -> Version {
        Version::ALL.get(i).copied().unwrap_or_default()
    }

    /// Display name: the type, never a brand.
    pub fn name(self) -> &'static str {
        match self {
            Version::Clean => "Clean",
            Version::Hall => "Hall",
            Version::Room => "Room",
            Version::Plate => "Plate",
            Version::Spring => "Spring",
        }
    }

    /// The version's own knobs.
    pub fn extras(self) -> &'static [Spec] {
        match self {
            Version::Clean => &[MOD, EARLY],
            Version::Hall => &[MOD, BANDWIDTH],
            Version::Room => &[EARLY, SHAPE],
            Version::Plate => &[MOD, BANDWIDTH],
            Version::Spring => &[SPRINGS, TUBE],
        }
    }

    /// Index of extra knob `e` in this version, if it has it.
    pub fn extra(self, e: Extra) -> Option<usize> {
        self.extras().iter().position(|s| s.name == e.spec().name)
    }

    /// Default of a shared control in this version (each version starts where it sounds
    /// like itself).
    pub fn default_of(self, c: Ctl) -> f32 {
        use Version::*;
        let base = c.spec().default;
        match (self, c) {
            (Clean, Ctl::Decay) => 1.8,
            (Clean, Ctl::Size) => 55.0,
            (Clean, Ctl::Damping) => 40.0,
            (Clean, Ctl::Diffusion) => 80.0,
            (Hall, Ctl::Decay) => 2.4,
            (Hall, Ctl::Size) => 48.0,
            (Hall, Ctl::PreDelay) => 12.0,
            (Hall, Ctl::Damping) => 65.0,
            (Hall, Ctl::Diffusion) => 72.0,
            (Room, Ctl::Decay) => 0.7,
            (Room, Ctl::Size) => 40.0,
            (Room, Ctl::PreDelay) => 4.0,
            (Room, Ctl::Damping) => 50.0,
            (Room, Ctl::LowDamping) => 25.0,
            (Room, Ctl::Diffusion) => 70.0,
            (Plate, Ctl::Decay) => 1.9,
            (Plate, Ctl::Size) => 50.0,
            (Plate, Ctl::PreDelay) => 0.0,
            (Plate, Ctl::Damping) => 30.0,
            (Plate, Ctl::LowDamping) => 30.0,
            (Plate, Ctl::Diffusion) => 85.0,
            (Spring, Ctl::Decay) => 2.2,
            (Spring, Ctl::Size) => 50.0,
            (Spring, Ctl::PreDelay) => 0.0,
            (Spring, Ctl::Damping) => 55.0,
            (Spring, Ctl::LowDamping) => 40.0,
            (Spring, Ctl::Diffusion) => 60.0,
            (Spring, Ctl::LowCut) => 120.0,
            (Spring, Ctl::HighCut) => 7000.0,
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
    /// Reverb time (RT60), s.
    Decay = 0,
    /// Room size, %.
    Size = 1,
    /// Pre-delay, ms.
    PreDelay = 2,
    /// High-frequency damping, %.
    Damping = 3,
    /// Diffusion (echo density), %.
    Diffusion = 4,
    /// Saturation on the wet signal, %.
    Saturation = 5,
    /// Wet EQ low cut, Hz.
    LowCut = 6,
    /// Wet EQ high cut, Hz.
    HighCut = 7,
    /// Wet EQ low band (shelf at 250 Hz), dB.
    Low = 8,
    /// Wet EQ mid band gain, dB.
    Mid = 9,
    /// Wet EQ mid band frequency, Hz.
    MidFreq = 10,
    /// Wet EQ high band (shelf at 5 kHz), dB.
    High = 11,
    /// Wet compressor threshold, dB.
    Threshold = 12,
    /// Wet compressor ratio.
    Ratio = 13,
    /// Wet compressor attack, ms.
    Attack = 14,
    /// Wet compressor release, ms.
    Release = 15,
    /// Stereo width of the wet signal, %.
    Width = 16,
    /// Stereo stage's own wet/dry, %.
    StereoMix = 17,
    /// Ping-pong stage's own wet/dry, %.
    PingPongMix = 18,
    /// Ping-pong time, ms.
    PingPongTime = 19,
    /// Gain into the reverb, dB.
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
    /// Low-frequency damping, %.
    LowDamping = 26,
    /// Freeze: hold the tail forever.
    Freeze = 27,
}

impl Ctl {
    /// All, in id order.
    pub const ALL: [Ctl; 28] = [
        Ctl::Decay,
        Ctl::Size,
        Ctl::PreDelay,
        Ctl::Damping,
        Ctl::Diffusion,
        Ctl::Saturation,
        Ctl::LowCut,
        Ctl::HighCut,
        Ctl::Low,
        Ctl::Mid,
        Ctl::MidFreq,
        Ctl::High,
        Ctl::Threshold,
        Ctl::Ratio,
        Ctl::Attack,
        Ctl::Release,
        Ctl::Width,
        Ctl::StereoMix,
        Ctl::PingPongMix,
        Ctl::PingPongTime,
        Ctl::GainIn,
        Ctl::GainOut,
        Ctl::Mix,
        Ctl::Wet,
        Ctl::Dry,
        Ctl::AutoMakeup,
        Ctl::LowDamping,
        Ctl::Freeze,
    ];

    /// The control's range and text.
    pub fn spec(self) -> Spec {
        use Unit::*;
        match self {
            Ctl::Decay => spec(
                "Decay",
                "Decay",
                (0.1, 20.0, 2.0),
                true,
                Seconds,
                "How long the tail rings: the time it takes to fall 60 dB (RT60).",
            ),
            Ctl::Size => spec(
                "Size",
                "Size",
                (0.0, 100.0, 50.0),
                false,
                Pct,
                "How big the space is: longer paths between the walls, slower echo build-up.",
            ),
            Ctl::PreDelay => spec(
                "Pre-Delay",
                "PreDly",
                (0.0, 500.0, 10.0),
                false,
                Ms,
                "Gap before the reverb starts. Keeps a vocal clear of its own tail.",
            ),
            Ctl::Damping => spec(
                "High Damping",
                "HiDamp",
                (0.0, 100.0, 50.0),
                false,
                Pct,
                "Rolls the top off the tail: the highs die away faster than the mids, so it never gets harsh.",
            ),
            Ctl::LowDamping => spec(
                "Low Damping",
                "LoDamp",
                (0.0, 100.0, 10.0),
                false,
                Pct,
                "Rolls the bottom off the tail: the lows die away faster than the mids, so it never gets muddy.",
            ),
            Ctl::Freeze => spec(
                "Freeze",
                "Freeze",
                (0.0, 1.0, 0.0),
                false,
                Switch("Off", "On"),
                "Holds the tail forever: a pad you can play over. New input stops going in while it is on.",
            ),
            Ctl::Diffusion => spec(
                "Diffusion",
                "Diff",
                (0.0, 100.0, 70.0),
                false,
                Pct,
                "Echo density. Low = you hear separate early echoes, high = a smooth wash.",
            ),
            Ctl::Saturation => spec(
                "Saturation",
                "Sat",
                (0.0, 100.0, 0.0),
                false,
                Pct,
                "Warm drive on the wet signal only (the dry stays clean). 4x oversampled.",
            ),
            Ctl::LowCut => spec(
                "Low Cut",
                "LoCut",
                (20.0, 1000.0, 20.0),
                true,
                Hz,
                "Cuts lows from the wet signal only: a reverb that doesn't muddy the mix. 20 Hz = off.",
            ),
            Ctl::HighCut => spec(
                "High Cut",
                "HiCut",
                (1000.0, 20000.0, 20000.0),
                true,
                Hz,
                "Cuts highs from the wet signal only: softer, less splashy tail. 20 kHz = off.",
            ),
            Ctl::Low => spec(
                "Low",
                "Low",
                (-12.0, 12.0, 0.0),
                false,
                Db,
                "Wet EQ low shelf at 250 Hz.",
            ),
            Ctl::Mid => spec(
                "Mid",
                "Mid",
                (-12.0, 12.0, 0.0),
                false,
                Db,
                "Wet EQ mid bell.",
            ),
            Ctl::MidFreq => spec(
                "Mid Freq",
                "MidF",
                (200.0, 8000.0, 1500.0),
                true,
                Hz,
                "Where the wet EQ's mid bell sits.",
            ),
            Ctl::High => spec(
                "High",
                "High",
                (-12.0, 12.0, 0.0),
                false,
                Db,
                "Wet EQ high shelf at 5 kHz.",
            ),
            Ctl::Threshold => spec(
                "Comp Threshold",
                "Thresh",
                (-60.0, 0.0, 0.0),
                false,
                Db,
                "Wet compressor: level where it starts to work. 0 dB = off.",
            ),
            Ctl::Ratio => spec(
                "Comp Ratio",
                "Ratio",
                (1.0, 20.0, 3.0),
                true,
                Ratio,
                "Wet compressor: how hard it squeezes the tail.",
            ),
            Ctl::Attack => spec(
                "Comp Attack",
                "Att",
                (0.1, 200.0, 20.0),
                true,
                Ms,
                "Wet compressor: how fast it grabs.",
            ),
            Ctl::Release => spec(
                "Comp Release",
                "Rel",
                (10.0, 2000.0, 250.0),
                true,
                Ms,
                "Wet compressor: how fast it lets go.",
            ),
            Ctl::Width => spec(
                "Width",
                "Width",
                (0.0, 200.0, 100.0),
                false,
                Pct,
                "Stereo width of the reverb. 0 = mono, 100 = natural, 200 = extra wide.",
            ),
            Ctl::StereoMix => spec(
                "Stereo Mix",
                "StMix",
                (0.0, 100.0, 100.0),
                false,
                Pct,
                "The stereo stage's own wet/dry: how much of the widened reverb replaces the plain one.",
            ),
            Ctl::PingPongMix => spec(
                "Ping-Pong Mix",
                "PPMix",
                (0.0, 100.0, 0.0),
                false,
                Pct,
                "The ping-pong stage's own wet/dry: how much of the reverb bounces left-right. 0 = off.",
            ),
            Ctl::PingPongTime => spec(
                "Ping-Pong Time",
                "PPTime",
                (20.0, 1000.0, 250.0),
                true,
                Ms,
                "Time between the left and right bounces.",
            ),
            Ctl::GainIn => spec(
                "Gain In",
                "In",
                (-24.0, 24.0, 0.0),
                false,
                Db,
                "Level into the reverb (the dry path is untouched).",
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
                "Blend of reverb (wet) and the original (dry).",
            ),
            Ctl::Wet => spec(
                "Wet",
                "Wet",
                (-60.0, 12.0, 0.0),
                false,
                Db,
                "Level of the reverb.",
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

/// A version's own knob, by meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extra {
    /// Modulation depth.
    Mod,
    /// Early reflections level.
    Early,
    /// Input bandwidth.
    Bandwidth,
    /// Early reflection spread.
    Shape,
    /// Number of springs.
    Springs,
    /// Tube drive into the springs.
    Tube,
}

impl Extra {
    /// The knob's spec.
    pub fn spec(self) -> Spec {
        match self {
            Extra::Mod => MOD,
            Extra::Early => EARLY,
            Extra::Bandwidth => BANDWIDTH,
            Extra::Shape => SHAPE,
            Extra::Springs => SPRINGS,
            Extra::Tube => TUBE,
        }
    }
}

const MOD: Spec = spec(
    "Modulation",
    "Mod",
    (0.0, 100.0, 30.0),
    false,
    Unit::Pct,
    "Slow movement inside the tail: lush and chorused, never metallic.",
);
const EARLY: Spec = spec(
    "Early",
    "Early",
    (0.0, 100.0, 50.0),
    false,
    Unit::Pct,
    "Level of the early reflections: the first bounces off the walls that tell the ear how big the room is.",
);
const BANDWIDTH: Spec = spec(
    "Bandwidth",
    "BW",
    (2000.0, 20000.0, 10000.0),
    true,
    Unit::Hz,
    "Top of the band that goes into the tank, like the converters of an 80s rack unit. Lower = darker, vintage.",
);
const SHAPE: Spec = spec(
    "Shape",
    "Shape",
    (0.0, 100.0, 50.0),
    false,
    Unit::Pct,
    "How the early reflections spread: low = a tight booth, high = a wide live room.",
);
const SPRINGS: Spec = spec(
    "Springs",
    "Springs",
    (0.0, 2.0, 2.0),
    false,
    Unit::Choice(&["1", "2", "3"]),
    "How many springs in the tank. More springs = denser, smoother.",
);
const TUBE: Spec = spec(
    "Tube Drive",
    "Tube",
    (0.0, 100.0, 25.0),
    false,
    Unit::Pct,
    "The tube driver in front of the springs: more = grit and bloom.",
);
