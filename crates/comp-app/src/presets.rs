//! Factory presets: a starting point per mode, plus six for common jobs.

use solplugs_comp_core::Mode;

use crate::params::K;

/// A factory preset: a mode and that mode's settings (anything not listed is the
/// mode's default).
#[derive(Debug)]
pub struct Preset {
    /// Name shown in the menu.
    pub name: &'static str,
    /// What it's for, one line.
    pub about: &'static str,
    /// Mode it switches to.
    pub mode: Mode,
    /// Settings, plain values.
    pub values: &'static [(K, f64)],
}

/// All factory presets, in menu order.
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "VCA · Bus glue",
        about: "Starting point: 4:1, 10 ms attack, auto release. Gentle punch for groups.",
        mode: Mode::Vca,
        values: &[(K::Threshold, -18.0), (K::AutoRelease, 1.0)],
    },
    Preset {
        name: "Opto · Smooth leveller",
        about: "Starting point: the light cell rides the level; 3:1, slow release.",
        mode: Mode::Opto,
        values: &[],
    },
    Preset {
        name: "FET · Bite",
        about: "Starting point: fast attack, 4:1, colour on.",
        mode: Mode::Fet,
        values: &[],
    },
    Preset {
        name: "Vari-Mu · Warm glue",
        about: "Starting point: soft knee, slow and warm.",
        mode: Mode::VariMu,
        values: &[],
    },
    Preset {
        name: "Clean · Transparent",
        about: "Starting point: precise, no colour, 2 ms lookahead.",
        mode: Mode::Clean,
        values: &[(K::Lookahead, 2.0)],
    },
    Preset {
        name: "Vocals",
        about: "Opto on a lead vocal: steady level, never grabs.",
        mode: Mode::Opto,
        values: &[
            (K::Threshold, -24.0),
            (K::Ratio, 3.0),
            (K::Attack, 10.0),
            (K::Release, 900.0),
            (K::ScHpf, 90.0),
            (K::GainOut, 4.0),
        ],
    },
    Preset {
        name: "Drums",
        about: "VCA on the drum bus: lets the hits through, glues the room.",
        mode: Mode::Vca,
        values: &[
            (K::Threshold, -20.0),
            (K::Ratio, 4.0),
            (K::Attack, 10.0),
            (K::Release, 150.0),
            (K::Knee, 2.0),
            (K::ScHpf, 60.0),
            (K::GainOut, 4.0),
        ],
    },
    Preset {
        name: "Bass",
        about: "FET on bass: slow attack keeps the pluck, holds the notes even.",
        mode: Mode::Fet,
        values: &[
            (K::Threshold, -24.0),
            (K::Ratio, 4.0),
            (K::Attack, 0.8),
            (K::Release, 120.0),
            (K::GainOut, 5.0),
        ],
    },
    Preset {
        name: "Mix bus",
        about: "VCA on the mix: 2:1, slow attack, auto release, 1-3 dB of glue.",
        mode: Mode::Vca,
        values: &[
            (K::Threshold, -14.0),
            (K::Ratio, 2.0),
            (K::Attack, 30.0),
            (K::Release, 300.0),
            (K::AutoRelease, 1.0),
            (K::ScHpf, 80.0),
            (K::GainOut, 1.5),
        ],
    },
    Preset {
        name: "Mastering",
        about: "Vari-Mu, barely working: 1.5:1, wide knee, 1-2 dB, warm.",
        mode: Mode::VariMu,
        values: &[
            (K::Threshold, -12.0),
            (K::Ratio, 1.5),
            (K::Attack, 30.0),
            (K::Release, 1500.0),
            (K::Knee, 20.0),
            (K::ScHpf, 100.0),
            (K::GainOut, 1.0),
        ],
    },
    Preset {
        name: "Parallel smash",
        about: "FET, all buttons in, blended under the dry signal.",
        mode: Mode::Fet,
        values: &[
            (K::Threshold, -30.0),
            (K::AllButtons, 1.0),
            (K::Attack, 0.1),
            (K::Release, 100.0),
            (K::GainIn, 6.0),
            (K::GainOut, 10.0),
            (K::Mix, 40.0),
        ],
    },
];
