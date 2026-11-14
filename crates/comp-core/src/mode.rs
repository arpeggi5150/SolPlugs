//! The five compressor circuits and their fixed character: detector ballistics, curve shape,
//! harmonic colour.

/// A compressor circuit. Named by circuit type, never by product.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Mode {
    /// Fast, punchy, clean: the bus glue comp.
    #[default]
    Vca,
    /// Smooth, slow, musical optical levelling.
    Opto,
    /// Aggressive, fast, gritty field-effect transistor comp.
    Fet,
    /// Warm, slow, soft tube glue.
    VariMu,
    /// Transparent modern digital.
    Clean,
}

impl Mode {
    /// All modes, in UI order.
    pub const ALL: [Mode; 5] = [Mode::Vca, Mode::Opto, Mode::Fet, Mode::VariMu, Mode::Clean];

    /// From an index (clamped).
    pub fn from_index(i: usize) -> Mode {
        Self::ALL[i.min(4)]
    }

    /// Index in [`Mode::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Vca => "VCA",
            Mode::Opto => "Opto",
            Mode::Fet => "FET",
            Mode::VariMu => "Vari-Mu",
            Mode::Clean => "Clean",
        }
    }

    /// Whether this mode adds harmonic colour (and so runs oversampled).
    pub fn coloured(self) -> bool {
        self != Mode::Clean
    }

    /// Fixed character of the mode.
    pub fn character(self) -> Character {
        match self {
            Mode::Vca => Character {
                rms_ms: 8.0,
                drive: 0.25,
                bias: 0.0,
                drive_per_db_gr: 0.0,
                two_stage: None,
                hold_ms: 5.0,
                pre_colour: false,
            },
            Mode::Opto => Character {
                rms_ms: 18.0,
                drive: 0.45,
                bias: 0.18,
                drive_per_db_gr: 0.01,
                // The light cell: half the reduction lets go in ~60 ms, the rest slowly.
                // Its attack is softer than the knob says.
                two_stage: Some(TwoStage {
                    fast_release_ms: 60.0,
                    slow_attack_mul: 4.0,
                    fast_share: 0.4,
                    memory_ms: 2500.0,
                    memory_slowdown: 2.0,
                }),
                hold_ms: 5.0,
                pre_colour: false,
            },
            Mode::Fet => Character {
                rms_ms: 5.0,
                drive: 0.7,
                bias: 0.06,
                drive_per_db_gr: 0.015,
                two_stage: None,
                hold_ms: 2.5,
                pre_colour: true,
            },
            Mode::VariMu => Character {
                rms_ms: 25.0,
                drive: 0.55,
                bias: 0.3,
                drive_per_db_gr: 0.05,
                // Programme-dependent release: the longer it works, the slower it lets go.
                two_stage: Some(TwoStage {
                    fast_release_ms: 150.0,
                    slow_attack_mul: 3.0,
                    fast_share: 0.35,
                    memory_ms: 4000.0,
                    memory_slowdown: 3.0,
                }),
                hold_ms: 8.0,
                pre_colour: true,
            },
            Mode::Clean => Character {
                rms_ms: 10.0,
                drive: 0.0,
                bias: 0.0,
                drive_per_db_gr: 0.0,
                two_stage: None,
                hold_ms: 5.0,
                pre_colour: false,
            },
        }
    }
}

/// A mode's fixed character.
#[derive(Clone, Copy, Debug)]
pub struct Character {
    /// RMS detector window, ms.
    pub rms_ms: f32,
    /// Saturation drive at 0 dB gain reduction (0 = clean).
    pub drive: f32,
    /// Saturation asymmetry (even harmonics).
    pub bias: f32,
    /// Extra drive per dB of gain reduction (tube stages distort more as they work).
    pub drive_per_db_gr: f32,
    /// Two-stage, programme-dependent release (Opto and Vari-Mu).
    pub two_stage: Option<TwoStage>,
    /// Peak detector hold, ms: the level holds this long before it falls, so the gain
    /// doesn't ripple on low notes.
    pub hold_ms: f32,
    /// Colour sits before the gain cell (an input stage pushed by Gain In) rather than
    /// after it.
    pub pre_colour: bool,
}

/// Two-stage release with memory.
#[derive(Clone, Copy, Debug)]
pub struct TwoStage {
    /// Release of the fast part, ms.
    pub fast_release_ms: f32,
    /// The slow part attacks this many times slower than the knob.
    pub slow_attack_mul: f32,
    /// Share of the reduction the fast part carries.
    pub fast_share: f32,
    /// Time constant of the memory (how long the circuit "remembers" working), ms.
    pub memory_ms: f32,
    /// At full memory the slow release is this many times longer.
    pub memory_slowdown: f32,
}
