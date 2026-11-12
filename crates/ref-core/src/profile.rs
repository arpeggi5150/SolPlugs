//! A track's analysis profile: the numbers and curves that get compared and saved.
//!
//! A profile holds no audio. It is what the reference library stores, so a reference that came
//! from a streamed preview keeps its analysis without keeping the preview.

use serde::{Deserialize, Serialize};

use crate::bands::BANDS;
use crate::loudness::LoudnessMeter;
use crate::parts::{Parts, detect_parts};
use crate::peak::TruePeakMeter;
use crate::spectrum::{BandAnalyzer, BandFrame};
use crate::stereo::StereoMeter;
use crate::{amp_db, power_db};

/// Current profile format version.
pub const PROFILE_VERSION: u32 = 1;
/// FFT size for tonal balance.
pub const TONAL_FFT: usize = 8192;
/// Frames whose level is this far below the track's mean are ignored for tonal balance.
const RELATIVE_GATE_DB: f64 = 20.0;
/// Frames below this absolute level are ignored for tonal balance.
const ABSOLUTE_GATE_DB: f64 = -70.0;

/// Tonal balance: per-band levels relative to the track's own total, so tracks at different
/// loudness compare by shape.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TonalBalance {
    /// Mean level of each band, dB relative to the total.
    pub mean_db: Vec<f64>,
    /// 10th percentile of each band's level over time (dB, same reference).
    pub low_db: Vec<f64>,
    /// 90th percentile of each band's level over time (dB, same reference).
    pub high_db: Vec<f64>,
    /// Stereo width of each band (0 = mono, 1 = uncorrelated).
    pub width: Vec<f64>,
}

impl TonalBalance {
    /// Makes every curve exactly [`BANDS`] long (profiles from disk are hostile input).
    pub fn normalise(&mut self) {
        for v in [&mut self.mean_db, &mut self.low_db, &mut self.high_db] {
            v.resize(BANDS, -120.0);
            for x in v.iter_mut() {
                if !x.is_finite() {
                    *x = -120.0;
                }
                *x = x.clamp(-200.0, 50.0);
            }
        }
        self.width.resize(BANDS, 0.0);
        for x in &mut self.width {
            if !x.is_finite() {
                *x = 0.0;
            }
            *x = x.clamp(0.0, 2.0);
        }
    }

    /// Whether the curves carry data.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mean_db.len() != BANDS
    }
}

/// Collects band frames and reduces them to a [`TonalBalance`].
#[derive(Clone, Default)]
pub struct TonalAccumulator {
    frames: Vec<BandFrame>,
    cap: usize,
    next: usize,
}

impl TonalAccumulator {
    /// Keeps at most `cap` frames (the oldest are overwritten).
    #[must_use]
    pub fn with_capacity(cap: usize) -> Self {
        let cap = cap.max(1);
        Self {
            frames: Vec::with_capacity(cap),
            cap,
            next: 0,
        }
    }

    /// Adds a frame.
    pub fn push(&mut self, f: &BandFrame) {
        if self.frames.len() < self.cap {
            self.frames.push(*f);
        } else if let Some(slot) = self.frames.get_mut(self.next) {
            *slot = *f;
            self.next = (self.next + 1) % self.cap;
        }
    }

    /// Drops every frame.
    pub fn clear(&mut self) {
        self.frames.clear();
        self.next = 0;
    }

    /// Frames held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether no frames are held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Reduces the frames to a tonal balance. Empty when nothing passed the gates.
    #[must_use]
    pub fn reduce(&self) -> TonalBalance {
        let levels: Vec<f64> = self.frames.iter().map(|f| power_db(f.sum())).collect();
        let loud: Vec<usize> = (0..self.frames.len())
            .filter(|&i| levels[i] > ABSOLUTE_GATE_DB)
            .collect();
        if loud.is_empty() {
            return TonalBalance::default();
        }
        let mean_p = loud.iter().map(|&i| self.frames[i].sum()).sum::<f64>() / loud.len() as f64;
        let gate = power_db(mean_p) - RELATIVE_GATE_DB;
        let kept: Vec<&BandFrame> = loud
            .iter()
            .filter(|&&i| levels[i] > gate)
            .map(|&i| &self.frames[i])
            .collect();
        if kept.is_empty() {
            return TonalBalance::default();
        }
        let n = kept.len() as f64;
        let mut mid = [0.0; BANDS];
        let mut side = [0.0; BANDS];
        for f in &kept {
            for b in 0..BANDS {
                mid[b] += f.mid[b] / n;
                side[b] += f.side[b] / n;
            }
        }
        let total: f64 = (0..BANDS).map(|b| mid[b] + side[b]).sum();
        let reference = power_db(total);
        let mut out = TonalBalance {
            mean_db: (0..BANDS)
                .map(|b| power_db(mid[b] + side[b]) - reference)
                .collect(),
            low_db: Vec::with_capacity(BANDS),
            high_db: Vec::with_capacity(BANDS),
            width: (0..BANDS)
                .map(|b| {
                    let t = mid[b] + side[b];
                    if t > 1e-20 {
                        (2.0 * side[b] / t).clamp(0.0, 2.0)
                    } else {
                        0.0
                    }
                })
                .collect(),
        };
        let mut col: Vec<f64> = Vec::with_capacity(kept.len());
        for b in 0..BANDS {
            col.clear();
            col.extend(kept.iter().map(|f| power_db(f.total(b)) - reference));
            col.sort_by(f64::total_cmp);
            out.low_db.push(percentile(&col, 0.10));
            out.high_db.push(percentile(&col, 0.90));
        }
        out.normalise();
        out
    }
}

/// The value at `q` (0..=1) of sorted data.
#[must_use]
pub fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((sorted.len() - 1) as f64 * q.clamp(0.0, 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Everything measured about one track.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub version: u32,
    pub sample_rate: f64,
    pub duration_secs: f64,
    /// Gated integrated loudness, LUFS.
    pub integrated_lufs: Option<f64>,
    /// Loudness range, LU.
    pub loudness_range: Option<f64>,
    /// Loudest short-term (3 s) value, LUFS.
    pub short_term_max: Option<f64>,
    /// Loudest momentary (400 ms) value, LUFS.
    pub momentary_max: Option<f64>,
    /// Sample peak, dBFS.
    pub sample_peak_db: f64,
    /// True peak, dBTP.
    pub true_peak_db: f64,
    /// Unweighted RMS, dBFS (channel average).
    pub rms_db: f64,
    /// Peak to RMS, dB.
    pub crest_db: f64,
    /// Peak to loudness ratio (true peak minus integrated), dB.
    pub plr_db: Option<f64>,
    /// Phase correlation, -1..1.
    pub correlation: Option<f64>,
    /// Stereo width, 0 = mono, 1 = uncorrelated.
    pub width: Option<f64>,
    pub tonal: TonalBalance,
    pub parts: Parts,
}

impl Profile {
    /// Repairs a profile read from disk: fixed curve lengths, finite numbers.
    pub fn normalise(&mut self) {
        self.tonal.normalise();
        self.parts.normalise();
        for v in [
            &mut self.integrated_lufs,
            &mut self.loudness_range,
            &mut self.short_term_max,
            &mut self.momentary_max,
            &mut self.plr_db,
            &mut self.correlation,
            &mut self.width,
        ] {
            if v.is_some_and(|x| !x.is_finite()) {
                *v = None;
            }
        }
        for x in [
            &mut self.sample_peak_db,
            &mut self.true_peak_db,
            &mut self.rms_db,
            &mut self.crest_db,
            &mut self.sample_rate,
            &mut self.duration_secs,
        ] {
            if !x.is_finite() {
                *x = 0.0;
            }
        }
    }
}

/// Analyzes a whole stereo track offline. `right` may equal `left` for mono material.
#[must_use]
pub fn analyze(left: &[f32], right: &[f32], sample_rate: f64) -> Profile {
    let n = left.len().min(right.len());
    let (left, right) = (&left[..n], &right[..n]);
    let mut loud = LoudnessMeter::new(sample_rate);
    let mut peak = TruePeakMeter::new();
    let mut stereo = StereoMeter::cumulative();
    let mut bands = BandAnalyzer::new(sample_rate, TONAL_FFT, TONAL_FFT / 2);
    let mut tonal = TonalAccumulator::with_capacity(n / (TONAL_FFT / 2) + 1);
    let (mut sum_l, mut sum_r) = (0.0f64, 0.0f64);
    for (l, r) in left.chunks(4096).zip(right.chunks(4096)) {
        loud.process(l, Some(r));
        peak.process(l, Some(r));
        stereo.process(l, r);
        bands.process(l, r, |f| tonal.push(f));
        sum_l += l.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>();
        sum_r += r.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>();
    }
    let rms = if n > 0 {
        ((sum_l + sum_r) / (2.0 * n as f64)).sqrt()
    } else {
        0.0
    };
    let integrated = loud.integrated();
    let true_peak_db = amp_db(peak.true_peak());
    let sample_peak_db = amp_db(peak.sample_peak());
    let rms_db = amp_db(rms);
    let mut p = Profile {
        version: PROFILE_VERSION,
        sample_rate,
        duration_secs: n as f64 / sample_rate.max(1.0),
        integrated_lufs: integrated,
        loudness_range: loud.loudness_range(),
        short_term_max: loud.short_term_max(),
        momentary_max: loud.momentary_max(),
        sample_peak_db,
        true_peak_db,
        rms_db,
        crest_db: if rms > 0.0 {
            sample_peak_db - rms_db
        } else {
            0.0
        },
        plr_db: integrated.map(|i| true_peak_db - i),
        correlation: stereo.correlation(),
        width: stereo.width(),
        tonal: tonal.reduce(),
        parts: detect_parts(left, right, sample_rate),
    };
    p.normalise();
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_noise_has_a_rising_tonal_balance_and_survives_json() {
        // White noise: equal power per Hz, so each 1/6-octave band holds more than the last.
        let sr = 48_000.0;
        let mut x: u32 = 1;
        let mut noise = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32 - 0.5) * 0.5
        };
        let l: Vec<f32> = (0..sr as usize * 10).map(|_| noise()).collect();
        let r: Vec<f32> = (0..sr as usize * 10).map(|_| noise()).collect();
        let p = analyze(&l, &r, sr);
        assert_eq!(p.tonal.mean_db.len(), BANDS);
        assert!(p.tonal.mean_db[50] > p.tonal.mean_db[20] + 10.0);
        // Independent channels: wide.
        assert!(p.width.unwrap_or(0.0) > 0.9);
        assert!(p.tonal.width[40] > 0.8);
        let json = serde_json::to_string(&p).unwrap_or_default();
        let back: Profile = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(back.tonal.mean_db.len(), BANDS);
        assert!(
            (back.integrated_lufs.unwrap_or(0.0) - p.integrated_lufs.unwrap_or(1.0)).abs() < 1e-9
        );
    }

    #[test]
    fn hostile_profile_is_repaired() {
        let mut p: Profile = serde_json::from_str(
            r#"{"version":1,"sample_rate":48000,"duration_secs":1,"integrated_lufs":null,"loudness_range":null,
                "short_term_max":null,"momentary_max":null,"sample_peak_db":0,"true_peak_db":0,"rms_db":0,
                "crest_db":0,"plr_db":null,"correlation":null,"width":null,
                "tonal":{"mean_db":[1,2],"low_db":[],"high_db":[],"width":[9]},
                "parts":{"elements":[]}}"#,
        )
        .unwrap_or_default();
        p.normalise();
        assert_eq!(p.tonal.mean_db.len(), BANDS);
        assert!((p.tonal.width[0] - 2.0).abs() < 1e-9);
    }
}
