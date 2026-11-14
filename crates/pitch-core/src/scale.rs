//! Keys, scales and note names.

use serde::{Deserialize, Serialize};

/// Note names, sharps.
pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Built-in scales. `Custom` uses the user's 12-note mask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaleKind {
    /// All 12 notes.
    #[default]
    Chromatic,
    /// Ionian.
    Major,
    /// Aeolian.
    Minor,
    /// Minor with a raised 7th.
    HarmonicMinor,
    /// Five-note major.
    MajorPentatonic,
    /// Five-note minor.
    MinorPentatonic,
    /// Minor pentatonic plus the flat 5th.
    Blues,
    /// The user's own notes.
    Custom,
}

impl ScaleKind {
    /// All scales, in UI order.
    pub const ALL: [ScaleKind; 8] = [
        ScaleKind::Chromatic,
        ScaleKind::Major,
        ScaleKind::Minor,
        ScaleKind::HarmonicMinor,
        ScaleKind::MajorPentatonic,
        ScaleKind::MinorPentatonic,
        ScaleKind::Blues,
        ScaleKind::Custom,
    ];

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            ScaleKind::Chromatic => "Chromatic",
            ScaleKind::Major => "Major",
            ScaleKind::Minor => "Minor",
            ScaleKind::HarmonicMinor => "Harmonic minor",
            ScaleKind::MajorPentatonic => "Major pentatonic",
            ScaleKind::MinorPentatonic => "Minor pentatonic",
            ScaleKind::Blues => "Blues",
            ScaleKind::Custom => "Custom",
        }
    }

    /// Index in [`ScaleKind::ALL`].
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    /// From an index in [`ScaleKind::ALL`], clamped.
    pub fn from_index(i: usize) -> Self {
        Self::ALL[i.min(Self::ALL.len() - 1)]
    }

    /// Intervals above the root, as a 12-bit mask (bit 0 = root).
    fn intervals(self) -> u16 {
        fn mask(steps: &[u8]) -> u16 {
            steps.iter().fold(0, |m, s| m | (1 << s))
        }
        match self {
            ScaleKind::Chromatic | ScaleKind::Custom => 0xfff,
            ScaleKind::Major => mask(&[0, 2, 4, 5, 7, 9, 11]),
            ScaleKind::Minor => mask(&[0, 2, 3, 5, 7, 8, 10]),
            ScaleKind::HarmonicMinor => mask(&[0, 2, 3, 5, 7, 8, 11]),
            ScaleKind::MajorPentatonic => mask(&[0, 2, 4, 7, 9]),
            ScaleKind::MinorPentatonic => mask(&[0, 3, 5, 7, 10]),
            ScaleKind::Blues => mask(&[0, 3, 5, 6, 7, 10]),
        }
    }
}

/// A key and scale: which of the 12 pitch classes are allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scale {
    /// Allowed pitch classes, bit 0 = C. Never empty.
    pub mask: u16,
}

impl Scale {
    /// The scale `kind` on `root` (0 = C). `custom` is the absolute pitch-class mask used for
    /// [`ScaleKind::Custom`].
    pub fn new(root: u8, kind: ScaleKind, custom: u16) -> Self {
        let mask = if kind == ScaleKind::Custom {
            custom & 0xfff
        } else {
            let rel = kind.intervals();
            let r = u32::from(root % 12);
            (((u32::from(rel) << r) | (u32::from(rel) >> (12 - r))) & 0xfff) as u16
        };
        Self {
            mask: if mask == 0 { 0xfff } else { mask },
        }
    }

    /// Chromatic.
    pub fn chromatic() -> Self {
        Self { mask: 0xfff }
    }

    /// Whether MIDI note `note` is in the scale.
    pub fn contains(&self, note: i32) -> bool {
        self.mask & (1 << note.rem_euclid(12)) != 0
    }

    /// The allowed MIDI note nearest to `midi` (fractional). Ties go down.
    pub fn nearest(&self, midi: f32) -> i32 {
        let base = midi.round() as i32;
        let mut best = base;
        let mut best_d = f32::MAX;
        for off in -6..=6 {
            let n = base + off;
            if self.contains(n) {
                let d = (midi - n as f32).abs();
                if d < best_d - 1e-6 {
                    best = n;
                    best_d = d;
                }
            }
        }
        best
    }
}

/// MIDI note number (fractional) of `freq` Hz, A4 = 440 Hz = 69.
pub fn freq_to_midi(freq: f32) -> f32 {
    69.0 + 12.0 * (freq / 440.0).log2()
}

/// Frequency in Hz of MIDI note `midi`.
pub fn midi_to_freq(midi: f32) -> f32 {
    440.0 * ((midi - 69.0) / 12.0).exp2()
}

/// "A4", "C#3", ...
pub fn note_name(note: i32) -> String {
    format!(
        "{}{}",
        NOTE_NAMES[note.rem_euclid(12) as usize],
        note.div_euclid(12) - 1
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_major_and_a_minor_share_notes() {
        assert_eq!(
            Scale::new(0, ScaleKind::Major, 0),
            Scale::new(9, ScaleKind::Minor, 0)
        );
    }

    #[test]
    fn nearest_snaps_into_the_scale() {
        let c_major = Scale::new(0, ScaleKind::Major, 0);
        assert_eq!(c_major.nearest(61.2), 62); // C#+ -> D
        assert_eq!(c_major.nearest(60.9), 60); // nearer C than D
        assert_eq!(c_major.nearest(65.4), 65); // F
        assert_eq!(Scale::chromatic().nearest(61.4), 61);
    }

    #[test]
    fn names() {
        assert_eq!(note_name(69), "A4");
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(61), "C#4");
        assert!((freq_to_midi(440.0) - 69.0).abs() < 1e-5);
        assert!((midi_to_freq(57.0) - 220.0).abs() < 1e-3);
    }
}
