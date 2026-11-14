//! The manager's sample tools: trim, trim silence, fades, reverse, normalize. Each works on a
//! range of frames (the selection) or the whole sample; the result is saved as an edited
//! version linked to the raw (see [`crate::edits`]).

use crate::audio::Audio;

/// Normalize target, dBFS peak.
pub const NORMALIZE_DBFS: f32 = -1.0;
/// Level below which audio counts as silence for "trim silence", dBFS.
pub const SILENCE_DBFS: f32 = -60.0;

/// A frame range, start inclusive, end exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

impl Range {
    /// The whole of `audio`.
    #[must_use]
    pub fn all(audio: &Audio) -> Self {
        Self {
            start: 0,
            end: audio.frames(),
        }
    }

    /// From seconds, clamped to `audio`.
    #[must_use]
    pub fn seconds(audio: &Audio, a: f64, b: f64) -> Self {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        Self {
            start: audio.frame_at(a),
            end: audio.frame_at(b),
        }
    }

    /// Clamped to `audio`, start <= end.
    #[must_use]
    pub fn clamp(self, audio: &Audio) -> Self {
        let end = self.end.min(audio.frames());
        Self {
            start: self.start.min(end),
            end,
        }
    }

    /// Frames in the range.
    #[must_use]
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// True if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Fade curve: equal-power-ish (sine) so fades sound smooth.
fn curve(t: f32) -> f32 {
    (t.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).sin()
}

/// Fades in over frames `start..end`; frames before `start` are silenced.
pub fn fade_in(audio: &mut Audio, start: usize, end: usize) {
    let r = Range { start, end }.clamp(audio);
    let len = r.len().max(1) as f32;
    for ch in &mut audio.channels {
        ch[..r.start].iter_mut().for_each(|v| *v = 0.0);
        for (k, v) in ch[r.start..r.end].iter_mut().enumerate() {
            *v *= curve(k as f32 / len);
        }
    }
}

/// Fades out over frames `start..end`; frames after `end` are silenced.
pub fn fade_out(audio: &mut Audio, start: usize, end: usize) {
    let r = Range { start, end }.clamp(audio);
    let len = r.len().max(1) as f32;
    for ch in &mut audio.channels {
        for (k, v) in ch[r.start..r.end].iter_mut().enumerate() {
            *v *= curve(1.0 - (k + 1) as f32 / len);
        }
        let n = ch.len();
        ch[r.end..n].iter_mut().for_each(|v| *v = 0.0);
    }
}

/// Fade in on a range: the first `len` frames of `range` rise from silence.
pub fn fade_in_range(audio: &mut Audio, range: Range, len: usize) {
    let r = range.clamp(audio);
    let end = (r.start + len.max(1)).min(r.end);
    let l = (end - r.start).max(1) as f32;
    for ch in &mut audio.channels {
        for (k, v) in ch[r.start..end].iter_mut().enumerate() {
            *v *= curve(k as f32 / l);
        }
    }
}

/// Fade out on a range: the last `len` frames of `range` fall to silence.
pub fn fade_out_range(audio: &mut Audio, range: Range, len: usize) {
    let r = range.clamp(audio);
    let start = r.end.saturating_sub(len.max(1)).max(r.start);
    let l = (r.end - start).max(1) as f32;
    for ch in &mut audio.channels {
        for (k, v) in ch[start..r.end].iter_mut().enumerate() {
            *v *= curve(1.0 - (k + 1) as f32 / l);
        }
    }
}

/// Reverses a range in place.
pub fn reverse(audio: &mut Audio, range: Range) {
    let r = range.clamp(audio);
    for ch in &mut audio.channels {
        ch[r.start..r.end].reverse();
    }
}

/// Scales so the peak sits at [`NORMALIZE_DBFS`]. Silence is left alone.
pub fn normalize(audio: &mut Audio) {
    let peak = audio.peak();
    if peak < 1e-6 {
        return;
    }
    let g = 10f32.powf(NORMALIZE_DBFS / 20.0) / peak;
    for ch in &mut audio.channels {
        ch.iter_mut().for_each(|v| *v *= g);
    }
}

/// Normalizes just a range (its own peak to [`NORMALIZE_DBFS`]).
pub fn normalize_range(audio: &mut Audio, range: Range) {
    let r = range.clamp(audio);
    let mut part = audio.slice(r.start, r.end);
    normalize(&mut part);
    for (dst, src) in audio.channels.iter_mut().zip(part.channels) {
        dst[r.start..r.end].copy_from_slice(&src);
    }
}

/// The range left after trimming leading and trailing silence (below [`SILENCE_DBFS`]).
/// Keeps a few milliseconds before the first sound so attacks aren't clipped.
#[must_use]
pub fn non_silent(audio: &Audio) -> Range {
    let thr = 10f32.powf(SILENCE_DBFS / 20.0);
    let n = audio.frames();
    let loud = |i: usize| audio.channels.iter().any(|c| c[i].abs() > thr);
    let Some(first) = (0..n).find(|&i| loud(i)) else {
        return Range { start: 0, end: 0 };
    };
    let last = (0..n).rev().find(|&i| loud(i)).unwrap_or(first);
    let pad = (audio.sample_rate * 0.002) as usize;
    Range {
        start: first.saturating_sub(pad),
        end: (last + 1 + pad).min(n),
    }
}

/// An edit operation, as the UI asks for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool {
    /// Keep only the range.
    Trim(Range),
    /// Cut leading and trailing silence.
    TrimSilence,
    /// Fade in the first `len` frames of the range.
    FadeIn { range: Range, len: usize },
    /// Fade out the last `len` frames of the range.
    FadeOut { range: Range, len: usize },
    /// Reverse the range.
    Reverse(Range),
    /// Normalize the range (the whole sample when it covers everything).
    Normalize(Range),
}

impl Tool {
    /// Applies the tool, returning the new audio. `None` if the result would be empty.
    #[must_use]
    pub fn apply(&self, audio: &Audio) -> Option<Audio> {
        let mut a = audio.clone();
        match *self {
            Self::Trim(r) => a = audio.slice(r.start, r.end),
            Self::TrimSilence => {
                let r = non_silent(audio);
                a = audio.slice(r.start, r.end);
            }
            Self::FadeIn { range, len } => fade_in_range(&mut a, range, len),
            Self::FadeOut { range, len } => fade_out_range(&mut a, range, len),
            Self::Reverse(r) => reverse(&mut a, r),
            Self::Normalize(r) => {
                if r.clamp(audio) == Range::all(audio) {
                    normalize(&mut a);
                } else {
                    normalize_range(&mut a, r);
                }
            }
        }
        (a.frames() > 0).then_some(a)
    }

    /// Plain-words description for the edit history.
    #[must_use]
    pub fn describe(&self, sr: f64) -> String {
        let s = |f: usize| f as f64 / sr.max(1.0);
        let span = |r: &Range| format!("{:.2}-{:.2} s", s(r.start), s(r.end));
        match self {
            Self::Trim(r) => format!("Trim to {}", span(r)),
            Self::TrimSilence => "Trim silence".into(),
            Self::FadeIn { range, len } => {
                format!("Fade in {:.0} ms at {}", s(*len) * 1000.0, span(range))
            }
            Self::FadeOut { range, len } => {
                format!("Fade out {:.0} ms at {}", s(*len) * 1000.0, span(range))
            }
            Self::Reverse(r) => format!("Reverse {}", span(r)),
            Self::Normalize(_) => format!("Normalize to {NORMALIZE_DBFS} dBFS"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize) -> Audio {
        Audio::new(1000.0, vec![(0..n).map(|i| i as f32 / n as f32).collect()])
    }

    #[test]
    fn trim_keeps_the_range() {
        let a = ramp(100);
        let out = Tool::Trim(Range { start: 10, end: 20 })
            .apply(&a)
            .unwrap_or_default();
        assert_eq!(out.channels[0], a.channels[0][10..20].to_vec());
    }

    #[test]
    fn trim_silence_cuts_both_ends() {
        let mut v = vec![0.0f32; 1000];
        v[300..600].iter_mut().for_each(|x| *x = 0.5);
        let a = Audio::new(1000.0, vec![v]);
        let r = non_silent(&a);
        assert_eq!((r.start, r.end), (298, 602));
        assert!(
            Tool::TrimSilence
                .apply(&Audio::new(1000.0, vec![vec![0.0; 10]]))
                .is_none()
        );
    }

    #[test]
    fn reverse_flips_only_the_range() {
        let a = ramp(10);
        let out = Tool::Reverse(Range { start: 2, end: 5 })
            .apply(&a)
            .unwrap_or_default();
        assert_eq!(out.channels[0][2], a.channels[0][4]);
        assert_eq!(out.channels[0][4], a.channels[0][2]);
        assert_eq!(out.channels[0][7], a.channels[0][7]);
    }

    #[test]
    fn fades_reach_silence_at_the_edges() {
        let a = Audio::new(1000.0, vec![vec![1.0; 100]]);
        let all = Range::all(&a);
        let fin = Tool::FadeIn {
            range: all,
            len: 20,
        }
        .apply(&a)
        .unwrap_or_default();
        assert_eq!(fin.channels[0][0], 0.0);
        assert_eq!(fin.channels[0][50], 1.0);
        let fout = Tool::FadeOut {
            range: all,
            len: 20,
        }
        .apply(&a)
        .unwrap_or_default();
        assert!(fout.channels[0][99].abs() < 1e-6);
        assert_eq!(fout.channels[0][50], 1.0);
    }

    #[test]
    fn normalize_whole_and_range() {
        let a = Audio::new(1000.0, vec![vec![0.1, -0.25, 0.2, 0.05]]);
        let out = Tool::Normalize(Range::all(&a))
            .apply(&a)
            .unwrap_or_default();
        assert!((out.peak() - 10f32.powf(-1.0 / 20.0)).abs() < 1e-6);
        let part = Tool::Normalize(Range { start: 2, end: 4 })
            .apply(&a)
            .unwrap_or_default();
        assert_eq!(part.channels[0][1], -0.25);
        assert!((part.channels[0][2] - 10f32.powf(-1.0 / 20.0)).abs() < 1e-6);
    }
}
