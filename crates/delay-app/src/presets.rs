//! Factory presets: the classic delay jobs (slapback, dotted eighth, ping-pong, tape echo,
//! dub, vocal echo) plus a starting point per version. Anything a preset doesn't list is
//! the version's default.

use solplugs_delay_core::{Ctl, Version};

/// A factory preset.
#[derive(Debug)]
pub struct Preset {
    /// Name shown in the menu.
    pub name: &'static str,
    /// What it's for, one line.
    pub about: &'static str,
    /// Version it switches to.
    pub version: Version,
    /// Shared controls (plain values, as the parameters store them).
    pub ctl: &'static [(Ctl, f64)],
    /// The version's own knobs (index, plain value).
    pub extras: &'static [(usize, f64)],
}

use Ctl::*;

/// Every factory preset, in menu order.
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Slapback",
        about: "One quick repeat, the 50s rockabilly vocal and guitar sound.",
        version: Version::Tape,
        ctl: &[
            (Sync, 0.0),
            (Time, 110.0),
            (Feedback, 8.0),
            (FilterLp, 6000.0),
            (Mix, 28.0),
            (Mod, 15.0),
        ],
        extras: &[(1, 30.0)],
    },
    Preset {
        name: "Dotted Eighth",
        about: "The rhythmic guitar delay: dotted eighths that fill the gaps between notes.",
        version: Version::Clean,
        ctl: &[
            (Sync, 1.0),
            (Note, 5.0),
            (Feedback, 40.0),
            (FilterLp, 9000.0),
            (FilterHp, 120.0),
            (Mix, 32.0),
            (Offset, 6.0),
        ],
        extras: &[],
    },
    Preset {
        name: "Ping-Pong",
        about: "Quarter-note repeats that bounce left and right.",
        version: Version::Clean,
        ctl: &[
            (Sync, 1.0),
            (Note, 7.0),
            (Feedback, 45.0),
            (StereoMix, 0.0),
            (PingPongMix, 100.0),
            (FilterLp, 8000.0),
            (Mix, 30.0),
        ],
        extras: &[],
    },
    Preset {
        name: "Tape Echo",
        about: "The warm tape-echo sound: repeats that darken and wobble, two heads for a galloping rhythm.",
        version: Version::Tape,
        ctl: &[
            (Sync, 1.0),
            (Note, 7.0),
            (Feedback, 55.0),
            (FilterLp, 5000.0),
            (FilterHp, 90.0),
            (Mod, 40.0),
            (Mix, 32.0),
        ],
        extras: &[(0, 1.0), (1, 45.0)],
    },
    Preset {
        name: "Dub",
        about: "Long, dark, high-feedback echoes that nearly run away: ride the Feedback for dub throws.",
        version: Version::Tape,
        ctl: &[
            (Sync, 1.0),
            (Note, 8.0),
            (Feedback, 82.0),
            (FilterLp, 3200.0),
            (FilterHp, 160.0),
            (Mod, 35.0),
            (Mix, 38.0),
            (PingPongMix, 40.0),
            (StereoMix, 80.0),
        ],
        extras: &[(1, 60.0)],
    },
    Preset {
        name: "Vocal Echo",
        about: "Quarter-note echoes that duck under the singer and bloom in the gaps, lows and splash cut.",
        version: Version::Clean,
        ctl: &[
            (Sync, 1.0),
            (Note, 7.0),
            (Feedback, 30.0),
            (LowCut, 250.0),
            (HighCut, 7000.0),
            (Duck, 60.0),
            (DuckRelease, 350.0),
            (Offset, 8.0),
            (Mix, 25.0),
        ],
        extras: &[(0, 20.0)],
    },
    Preset {
        name: "Clean Start",
        about: "Clean's starting point: exact eighth-note repeats, no colour.",
        version: Version::Clean,
        ctl: &[(Sync, 1.0), (Note, 4.0)],
        extras: &[],
    },
    Preset {
        name: "Tape Start",
        about: "Tape's starting point: dotted eighths off one head, gentle wow and flutter.",
        version: Version::Tape,
        ctl: &[],
        extras: &[],
    },
    Preset {
        name: "Analog Start",
        about: "Analog's starting point: dark repeats that melt away, a little chorus.",
        version: Version::Analog,
        ctl: &[(Sync, 1.0), (Note, 7.0)],
        extras: &[],
    },
];
