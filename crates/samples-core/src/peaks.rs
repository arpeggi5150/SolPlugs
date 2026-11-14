//! The waveform overview stored with each sample, so cards draw without decoding.

use serde::{Deserialize, Serialize};

use crate::audio::Audio;

/// Points in a stored overview.
pub const POINTS: usize = 256;

/// Min/max per column, quantised to i8 (-127..127 = -1..1).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Peaks {
    pub min: Vec<i8>,
    pub max: Vec<i8>,
}

impl Peaks {
    /// The overview of `audio` (left and right folded together).
    #[must_use]
    pub fn of(audio: &Audio) -> Self {
        let (min, max) = columns(audio, POINTS);
        let q = |v: f32| (v.clamp(-1.0, 1.0) * 127.0).round() as i8;
        Self {
            min: min.into_iter().map(q).collect(),
            max: max.into_iter().map(q).collect(),
        }
    }

    /// Column `i` as floats (min, max).
    #[must_use]
    pub fn get(&self, i: usize) -> (f32, f32) {
        let f = |v: i8| f32::from(v) / 127.0;
        (
            self.min.get(i).copied().map_or(0.0, f),
            self.max.get(i).copied().map_or(0.0, f),
        )
    }

    /// Number of columns.
    #[must_use]
    pub fn len(&self) -> usize {
        self.min.len().min(self.max.len())
    }

    /// True if there's nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Min/max of `audio` in `n` columns, at full precision (for the detail waveform).
#[must_use]
pub fn columns(audio: &Audio, n: usize) -> (Vec<f32>, Vec<f32>) {
    columns_range(audio, 0, audio.frames(), n)
}

/// Min/max of frames `start..end` in `n` columns.
#[must_use]
pub fn columns_range(audio: &Audio, start: usize, end: usize, n: usize) -> (Vec<f32>, Vec<f32>) {
    let end = end.min(audio.frames());
    let start = start.min(end);
    let len = end - start;
    let mut mn = vec![0.0f32; n];
    let mut mx = vec![0.0f32; n];
    if len == 0 || n == 0 {
        return (mn, mx);
    }
    for c in 0..n {
        let a = start + c * len / n;
        let b = (start + (c + 1) * len / n).max(a + 1).min(end);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for i in a..b {
            for ch in &audio.channels {
                let v = ch[i];
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        if lo <= hi {
            mn[c] = lo;
            mx[c] = hi;
        }
    }
    (mn, mx)
}
