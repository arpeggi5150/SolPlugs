//! Automatic correction: turns the detected pitch track into a target pitch, frame by frame.
//!
//! The detected pitch `m` (MIDI, fractional) is split into a slow centre (a two-pole
//! low-pass, ~1.5 Hz, which follows fast when the singer moves to another note) and the vibrato around it, `v = m - centre`. The note is chosen from
//! the centre with hysteresis, so vibrato across a note boundary never flips notes. The
//! correction `note - centre` is smoothed with the retune time constant and applied to the
//! centre, so vibrato survives correction untouched (scaled by the vibrato control). At
//! retune speed 0 the vibrato is flattened too, which is the hard-tune effect.

use crate::scale::Scale;

/// Correction controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CorrectParams {
    /// The allowed notes.
    pub scale: Scale,
    /// Retune time constant in milliseconds: 0 = instant snap.
    pub retune_ms: f32,
    /// 0..1: on sustained notes, slows the retune so small natural drift stays.
    pub humanize: f32,
    /// 0..1: how much of the correction is applied.
    pub amount: f32,
    /// 0..2: vibrato depth in the output, 1 = as sung.
    pub vibrato: f32,
}

impl Default for CorrectParams {
    fn default() -> Self {
        Self {
            scale: Scale::chromatic(),
            retune_ms: 50.0,
            humanize: 0.0,
            amount: 1.0,
            vibrato: 1.0,
        }
    }
}

/// Below this retune time the vibrato is progressively flattened (hard tune).
const VIBRATO_KEEP_MS: f32 = 20.0;
/// How far (semitones) past the midpoint the centre must move before the note switches.
const HYSTERESIS: f32 = 0.15;
/// Centre low-pass cutoff, Hz.
const CENTRE_HZ: f32 = 1.5;
/// Vibrato swings through the centre at least every half cycle (about 60-100 ms at
/// 5-8 Hz). Pitch that stays on one side longer than this, or strays further than
/// [`MOVE_ST`], is the singer moving to another note, and the centre follows it fast.
const ONE_SIDE_MS: f32 = 90.0;
const MOVE_ST: f32 = 1.0;
/// How fast the centre follows a note move.
const FOLLOW_HZ: f32 = 15.0;
/// Humanize: extra time constant at full humanize on a fully sustained note.
const HUMANIZE_MS: f32 = 350.0;
/// Humanize: how long a note must be held to count as fully sustained.
const SUSTAIN_MS: f32 = 300.0;

/// One frame of correction output.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Corrected {
    /// Detected pitch (MIDI), 0 when unvoiced.
    pub input: f32,
    /// The pitch the output should have (MIDI), 0 when unvoiced.
    pub output: f32,
    /// The target note, or -1.
    pub note: i32,
    /// `output - input`, semitones.
    pub shift: f32,
}

/// The per-frame corrector state.
#[derive(Clone, Debug)]
pub struct Corrector {
    dt_ms: f32,
    centre_a: f32,
    follow_a: f32,
    side: f32,
    side_ms: f32,
    c1: f32,
    c2: f32,
    note: Option<i32>,
    held_ms: f32,
    smoothed: f32,
    voiced: bool,
}

impl Corrector {
    /// A corrector updated once every `frame_ms` milliseconds.
    pub fn new(frame_ms: f32) -> Self {
        let a = 1.0 - (-std::f32::consts::TAU * CENTRE_HZ * frame_ms / 1000.0).exp();
        Self {
            dt_ms: frame_ms,
            centre_a: a,
            follow_a: 1.0 - (-std::f32::consts::TAU * FOLLOW_HZ * frame_ms / 1000.0).exp(),
            side: 0.0,
            side_ms: 0.0,
            c1: 0.0,
            c2: 0.0,
            note: None,
            held_ms: 0.0,
            smoothed: 0.0,
            voiced: false,
        }
    }

    /// Forgets the current note (e.g. on transport jumps).
    pub fn reset(&mut self) {
        self.voiced = false;
        self.note = None;
        self.smoothed = 0.0;
    }

    /// Advances one frame. `midi` is the detected pitch, or `None` when unvoiced.
    pub fn step(&mut self, midi: Option<f32>, p: &CorrectParams) -> Corrected {
        let Some(m) = midi else {
            self.voiced = false;
            return Corrected {
                note: -1,
                ..Corrected::default()
            };
        };
        if !self.voiced {
            // Voiced onset: start the centre at the pitch and pick a fresh note.
            self.c1 = m;
            self.c2 = m;
            self.note = None;
            self.smoothed = 0.0;
            self.voiced = true;
            self.side = 0.0;
            self.side_ms = 0.0;
        }
        // Which side of the centre the pitch is on, and for how long.
        let d = m - self.c2;
        let side = if d.abs() < 0.02 {
            self.side
        } else {
            d.signum()
        };
        if side == self.side {
            self.side_ms += self.dt_ms;
        } else {
            self.side = side;
            self.side_ms = 0.0;
        }
        let moving = self.side_ms > ONE_SIDE_MS || d.abs() > MOVE_ST;
        let a = if moving { self.follow_a } else { self.centre_a };
        self.c1 += a * (m - self.c1);
        self.c2 += a * (self.c1 - self.c2);
        let centre = self.c2;
        let vib = m - centre;

        let cand = p.scale.nearest(centre);
        let note = match self.note {
            Some(cur) if cur != cand && p.scale.contains(cur) => {
                let d_cur = (centre - cur as f32).abs();
                let d_new = (centre - cand as f32).abs();
                if d_cur - d_new > 2.0 * HYSTERESIS || d_cur > 1.5 {
                    cand
                } else {
                    cur
                }
            }
            _ => cand,
        };
        if Some(note) != self.note {
            self.held_ms = 0.0;
        } else {
            self.held_ms += self.dt_ms;
        }
        let first = self.note.is_none();
        self.note = Some(note);

        let want = note as f32 - centre;
        let sustain = (self.held_ms / SUSTAIN_MS).clamp(0.0, 1.0);
        let tau = p.retune_ms.max(0.0) + p.humanize.clamp(0.0, 1.0) * HUMANIZE_MS * sustain;
        if tau <= 0.0 || (first && p.retune_ms <= 0.0) {
            self.smoothed = want;
        } else {
            let a = 1.0 - (-self.dt_ms / tau).exp();
            self.smoothed += a * (want - self.smoothed);
        }
        let keep = (p.retune_ms / VIBRATO_KEEP_MS).clamp(0.0, 1.0);
        let vib_gain = p.vibrato.clamp(0.0, 2.0) * keep;
        let amount = p.amount.clamp(0.0, 1.0);
        // output = m + amount·(correction + (vibrato gain - 1)·vibrato)
        let output = m + amount * (self.smoothed + (vib_gain - 1.0) * vib);
        Corrected {
            input: m,
            output,
            note,
            shift: output - m,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scale::ScaleKind;

    fn run(p: &CorrectParams, track: impl Iterator<Item = f32>) -> Vec<Corrected> {
        let mut c = Corrector::new(2.0);
        track.map(|m| c.step(Some(m), p)).collect()
    }

    #[test]
    fn hard_tune_snaps_and_flattens_vibrato() {
        let p = CorrectParams {
            retune_ms: 0.0,
            ..CorrectParams::default()
        };
        // 60.3 with ±0.4 st vibrato at 6 Hz.
        let out = run(
            &p,
            (0..1000).map(|i| 60.3 + 0.4 * (i as f32 * 0.002 * 6.0 * std::f32::consts::TAU).sin()),
        );
        for c in &out[100..] {
            assert!((c.output - 60.0).abs() < 1e-3, "{c:?}");
        }
    }

    #[test]
    fn slow_retune_keeps_vibrato_and_fixes_the_centre() {
        let p = CorrectParams {
            retune_ms: 120.0,
            ..CorrectParams::default()
        };
        let vib = |i: usize| 0.4 * (i as f32 * 0.002 * 5.5 * std::f32::consts::TAU).sin();
        let out = run(&p, (0..3000).map(|i| 60.3 + vib(i)));
        let tail = &out[1500..];
        let mean = tail.iter().map(|c| c.output).sum::<f32>() / tail.len() as f32;
        assert!((mean - 60.0).abs() < 0.05, "centre {mean}");
        let depth = tail.iter().map(|c| c.output).fold(f32::MIN, f32::max)
            - tail.iter().map(|c| c.output).fold(f32::MAX, f32::min);
        assert!(depth > 0.7, "vibrato depth {depth}");
        assert!(tail.iter().all(|c| c.note == 60));
    }

    #[test]
    fn amount_zero_is_identity() {
        let p = CorrectParams {
            amount: 0.0,
            scale: Scale::new(0, ScaleKind::Major, 0),
            ..CorrectParams::default()
        };
        for c in run(&p, (0..500).map(|i| 61.0 + 0.3 * (i as f32 * 0.07).sin())) {
            assert!(c.shift.abs() < 1e-4);
        }
    }

    #[test]
    fn scale_picks_scale_notes() {
        let p = CorrectParams {
            retune_ms: 0.0,
            scale: Scale::new(0, ScaleKind::Major, 0),
            ..CorrectParams::default()
        };
        let out = run(&p, std::iter::repeat_n(61.3, 200));
        assert_eq!(out.last().map(|c| c.note), Some(62));
    }
}
