//! Factory presets: a starting point per style, plus seven for common jobs (vocals,
//! drums, bass, mix bus, mastering, air, low-end clean-up). Anything a preset doesn't
//! list is the style's default; a Clean preset replaces all bands.

use solplugs_eq_core::style::{api, curve, maag, massive, neve, pultec, ssl};
use solplugs_eq_core::{Place, Shape, Style};

use crate::params::C;

/// One Clean band in a preset.
#[derive(Clone, Copy, Debug)]
pub struct PBand {
    /// Shape.
    pub shape: Shape,
    /// Hz.
    pub freq: f64,
    /// dB.
    pub gain: f64,
    /// Q.
    pub q: f64,
    /// Slope index (0 = 6 dB/oct ... 5 = 48 dB/oct).
    pub slope: f64,
    /// Placement.
    pub place: Place,
    /// Dynamics: (threshold dBFS, range dB).
    pub dynamic: Option<(f64, f64)>,
}

const fn band(shape: Shape, freq: f64, gain: f64, q: f64) -> PBand {
    PBand {
        shape,
        freq,
        gain,
        q,
        slope: 1.0,
        place: Place::Stereo,
        dynamic: None,
    }
}

const fn cut(low: bool, freq: f64, slope: f64) -> PBand {
    PBand {
        slope,
        ..band(
            if low { Shape::LowCut } else { Shape::HighCut },
            freq,
            0.0,
            0.707,
        )
    }
}

/// A factory preset.
#[derive(Debug)]
pub struct Preset {
    /// Name shown in the menu.
    pub name: &'static str,
    /// What it's for, one line.
    pub about: &'static str,
    /// Style it switches to.
    pub style: Style,
    /// Faceplate knobs (index, plain value).
    pub knobs: &'static [(usize, f64)],
    /// Shared controls.
    pub common: &'static [(C, f64)],
    /// Clean bands.
    pub bands: &'static [PBand],
    /// Clean linear phase.
    pub linear: bool,
}

const P: Preset = Preset {
    name: "",
    about: "",
    style: Style::Pultec,
    knobs: &[],
    common: &[],
    bands: &[],
    linear: false,
};

/// All factory presets, in menu order.
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Pultec · Silk",
        about: "Starting point: a little low boost-and-cut and a broad 10 kHz lift.",
        style: Style::Pultec,
        knobs: &[
            (pultec::LOW_FREQ, 3.0),
            (pultec::LOW_BOOST, 3.0),
            (pultec::LOW_ATTEN, 2.0),
            (pultec::HIGH_FREQ, 4.0),
            (pultec::HIGH_BOOST, 3.5),
            (pultec::BANDWIDTH, 7.0),
        ],
        ..P
    },
    Preset {
        name: "API · Punch",
        about: "Starting point: low shelf, a forward upper mid and a high shelf, tight Q.",
        style: Style::Api,
        knobs: &[
            (api::LOW_FREQ, 3.0),
            (api::LOW_GAIN, 3.0),
            (api::MID_FREQ, 4.0),
            (api::MID_GAIN, 3.0),
            (api::HIGH_GAIN, 2.0),
        ],
        ..P
    },
    Preset {
        name: "SSL · Clean carve",
        about: "Starting point: rumble filter, a small mud cut and a gentle top.",
        style: Style::Ssl,
        knobs: &[
            (ssl::HPF, 30.0),
            (ssl::LMF_GAIN, -2.0),
            (ssl::LMF_FREQ, 300.0),
            (ssl::HF_GAIN, 1.5),
            (ssl::HF_FREQ, 10_000.0),
        ],
        ..P
    },
    Preset {
        name: "Neve · Warm",
        about: "Starting point: 80 Hz filter, body at 110 Hz, presence and sweet top; pushed a little for colour.",
        style: Style::Neve,
        knobs: &[
            (neve::HPF, 2.0),
            (neve::LOW_GAIN, 2.0),
            (neve::MID_FREQ, 3.0),
            (neve::MID_GAIN, 2.0),
            (neve::HIGH_GAIN, 3.0),
        ],
        common: &[(C::GainIn, 4.0), (C::GainOut, -4.0)],
        ..P
    },
    Preset {
        name: "Clean · Start",
        about: "Starting point: a 30 Hz cut, a small 250 Hz dip and a high shelf.",
        style: Style::Clean,
        bands: &[
            cut(true, 30.0, 3.0),
            band(Shape::Bell, 250.0, -2.0, 1.0),
            band(Shape::HighShelf, 10_000.0, 2.0, 0.7),
        ],
        ..P
    },
    Preset {
        name: "Massive · Glow",
        about: "Starting point: wide low and high shelves, a touch of mid.",
        style: Style::Massive,
        knobs: &[
            (massive::gain(0), 1.5),
            (massive::bandwidth(0), 7.0),
            (massive::gain(2), 1.0),
            (massive::freq(2), 3300.0),
            (massive::gain(3), 2.0),
            (massive::freq(3), 16_000.0),
            (massive::bandwidth(3), 6.0),
        ],
        ..P
    },
    Preset {
        name: "Maag · Air",
        about: "Starting point: the 40 kHz Air Band at +5 dB.",
        style: Style::Maag,
        knobs: &[(maag::AIR_GAIN, 5.0)],
        ..P
    },
    Preset {
        name: "Curve · Vibe",
        about: "Starting point: bold bass and treble curves.",
        style: Style::Curve,
        knobs: &[
            (curve::BASS_FREQ, 2.0),
            (curve::BASS_GAIN, 4.0),
            (curve::MID_FREQ, 6.0),
            (curve::MID_GAIN, 1.5),
            (curve::TREBLE_GAIN, 3.0),
        ],
        ..P
    },
    Preset {
        name: "Vocals",
        about: "Clean: 80 Hz cut, mud out, presence and air, plus a dynamic de-esser band at 6.5 kHz.",
        style: Style::Clean,
        bands: &[
            cut(true, 80.0, 3.0),
            band(Shape::Bell, 300.0, -2.0, 1.0),
            band(Shape::Bell, 3000.0, 2.0, 0.8),
            band(Shape::HighShelf, 10_000.0, 3.0, 0.7),
            PBand {
                dynamic: Some((-30.0, -6.0)),
                ..band(Shape::Bell, 6500.0, 0.0, 3.0)
            },
        ],
        ..P
    },
    Preset {
        name: "Drums",
        about: "API: low thump, boxiness out, snap on top.",
        style: Style::Api,
        knobs: &[
            (api::LOW_FREQ, 2.0),
            (api::LOW_GAIN, 4.0),
            (api::MID_FREQ, 1.0),
            (api::MID_GAIN, -4.0),
            (api::HIGH_FREQ, 1.0),
            (api::HIGH_GAIN, 4.0),
            (api::HIGH_SHELF, 0.0),
        ],
        ..P
    },
    Preset {
        name: "Bass",
        about: "Pultec: the 60 Hz boost-and-cut trick for weight without mud, plus some string definition.",
        style: Style::Pultec,
        knobs: &[
            (pultec::LOW_FREQ, 2.0),
            (pultec::LOW_BOOST, 5.0),
            (pultec::LOW_ATTEN, 4.0),
            (pultec::HIGH_FREQ, 0.0),
            (pultec::HIGH_BOOST, 2.0),
            (pultec::BANDWIDTH, 3.0),
        ],
        ..P
    },
    Preset {
        name: "Mix bus",
        about: "SSL: 20 Hz filter, a little low shelf, a hair of mud out, a gentle high shelf.",
        style: Style::Ssl,
        knobs: &[
            (ssl::HPF, 20.0),
            (ssl::LF_GAIN, 1.5),
            (ssl::LF_FREQ, 60.0),
            (ssl::LMF_GAIN, -1.0),
            (ssl::LMF_FREQ, 350.0),
            (ssl::LMF_Q, 0.8),
            (ssl::HF_GAIN, 1.5),
            (ssl::HF_FREQ, 12_000.0),
        ],
        ..P
    },
    Preset {
        name: "Mastering",
        about: "Massive: wide shelves, a soft mid scoop, tube glow.",
        style: Style::Massive,
        knobs: &[
            (massive::freq(0), 40.0),
            (massive::gain(0), 1.0),
            (massive::bandwidth(0), 8.0),
            (massive::freq(1), 300.0),
            (massive::gain(1), -0.8),
            (massive::bandwidth(1), 8.0),
            (massive::freq(3), 18_000.0),
            (massive::gain(3), 1.5),
            (massive::bandwidth(3), 8.0),
        ],
        ..P
    },
    Preset {
        name: "Air / sparkle",
        about: "Maag: Air Band at 20 kHz, a touch of 5 kHz presence.",
        style: Style::Maag,
        knobs: &[
            (maag::AIR_FREQ, 3.0),
            (maag::AIR_GAIN, 6.0),
            (maag::MID_FREQ, 5000.0),
            (maag::MID_GAIN, 1.0),
        ],
        ..P
    },
    Preset {
        name: "Low-end clean-up",
        about: "Clean: steep 30 Hz cut, mono below 120 Hz (side cut), and a dynamic 200 Hz band that dips only when the low mids pile up.",
        style: Style::Clean,
        bands: &[
            cut(true, 30.0, 5.0),
            PBand {
                place: Place::Side,
                ..cut(true, 120.0, 3.0)
            },
            PBand {
                dynamic: Some((-24.0, -4.0)),
                ..band(Shape::Bell, 200.0, 0.0, 1.2)
            },
        ],
        ..P
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_has_a_starting_point_and_knobs_exist() {
        for s in Style::ALL {
            assert!(PRESETS.iter().any(|p| p.style == s), "{}", s.name());
        }
        for p in PRESETS {
            for (k, _) in p.knobs {
                assert!(*k < p.style.knobs().len(), "{}", p.name);
            }
            assert!(p.bands.len() <= solplugs_eq_core::MAX_BANDS);
        }
        assert_eq!(PRESETS.len(), 15);
        let _ = neve::HPFS;
    }
}
