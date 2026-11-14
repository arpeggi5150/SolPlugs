//! Factory presets: seven for common jobs (vocals, drums, guitar, mix bus, ambient, small
//! room, big hall) plus a starting point per version. Anything a preset doesn't list is
//! the version's default.

use solplugs_reverb_core::{Ctl, Version};

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
        name: "Vocals",
        about: "Plate with a pre-delay that keeps the words clear, the lows and the splash cut from the tail.",
        version: Version::Plate,
        ctl: &[
            (Decay, 1.6),
            (Size, 55.0),
            (PreDelay, 60.0),
            (Damping, 45.0),
            (LowDamping, 35.0),
            (Diffusion, 85.0),
            (LowCut, 220.0),
            (HighCut, 9000.0),
            (Mix, 22.0),
            (Threshold, -24.0),
            (Ratio, 2.5),
        ],
        extras: &[(0, 25.0), (1, 9000.0)],
    },
    Preset {
        name: "Drums",
        about: "A tight, bright room that makes a kit sound like it's in a real space, compressed for snap.",
        version: Version::Room,
        ctl: &[
            (Decay, 0.9),
            (Size, 55.0),
            (PreDelay, 6.0),
            (Damping, 35.0),
            (LowDamping, 40.0),
            (Diffusion, 75.0),
            (LowCut, 160.0),
            (Mix, 25.0),
            (Threshold, -18.0),
            (Ratio, 4.0),
            (Attack, 8.0),
            (Release, 120.0),
            (Saturation, 25.0),
        ],
        extras: &[(0, 70.0), (1, 60.0)],
    },
    Preset {
        name: "Guitar",
        about: "Splashy spring tank with a little tube drive: the amp-in-a-room sound.",
        version: Version::Spring,
        ctl: &[
            (Decay, 2.4),
            (Size, 55.0),
            (Damping, 50.0),
            (LowDamping, 45.0),
            (Diffusion, 65.0),
            (Mix, 28.0),
        ],
        extras: &[(0, 2.0), (1, 35.0)],
    },
    Preset {
        name: "Mix Bus",
        about: "Clean, short and dark: glue for the whole mix without washing it out.",
        version: Version::Clean,
        ctl: &[
            (Decay, 1.2),
            (Size, 50.0),
            (PreDelay, 15.0),
            (Damping, 60.0),
            (LowDamping, 50.0),
            (Diffusion, 85.0),
            (LowCut, 300.0),
            (HighCut, 8000.0),
            (Mix, 10.0),
        ],
        extras: &[(0, 15.0), (1, 30.0)],
    },
    Preset {
        name: "Ambient",
        about: "A huge modulated hall with a long tail and gentle ping-pong: pads out of anything. Try Freeze.",
        version: Version::Hall,
        ctl: &[
            (Decay, 9.0),
            (Size, 95.0),
            (PreDelay, 40.0),
            (Damping, 55.0),
            (LowDamping, 20.0),
            (Diffusion, 90.0),
            (Mix, 45.0),
            (Width, 140.0),
            (PingPongMix, 25.0),
            (PingPongTime, 375.0),
            (Saturation, 15.0),
        ],
        extras: &[(0, 70.0), (1, 9000.0)],
    },
    Preset {
        name: "Small Room",
        about: "A booth-sized room: natural and short, adds space without sounding like reverb.",
        version: Version::Room,
        ctl: &[
            (Decay, 0.45),
            (Size, 20.0),
            (PreDelay, 2.0),
            (Damping, 55.0),
            (LowDamping, 30.0),
            (Diffusion, 65.0),
            (Mix, 20.0),
        ],
        extras: &[(0, 60.0), (1, 25.0)],
    },
    Preset {
        name: "Big Hall",
        about: "A concert hall: long, smooth, dark tail with lush movement.",
        version: Version::Hall,
        ctl: &[
            (Decay, 3.8),
            (Size, 85.0),
            (PreDelay, 28.0),
            (Damping, 62.0),
            (LowDamping, 15.0),
            (Diffusion, 80.0),
            (Mix, 30.0),
        ],
        extras: &[(0, 45.0), (1, 10000.0)],
    },
    Preset {
        name: "Clean Start",
        about: "Clean's defaults: clear, honest, no colour.",
        version: Version::Clean,
        ctl: &[],
        extras: &[],
    },
    Preset {
        name: "Hall Start",
        about: "Hall's defaults: the dark 80s rack hall.",
        version: Version::Hall,
        ctl: &[],
        extras: &[],
    },
    Preset {
        name: "Room Start",
        about: "Room's defaults: a studio room.",
        version: Version::Room,
        ctl: &[],
        extras: &[],
    },
    Preset {
        name: "Plate Start",
        about: "Plate's defaults: bright and dense.",
        version: Version::Plate,
        ctl: &[],
        extras: &[],
    },
    Preset {
        name: "Spring Start",
        about: "Spring's defaults: the tube spring tank.",
        version: Version::Spring,
        ctl: &[],
        extras: &[],
    },
];
