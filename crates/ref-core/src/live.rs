//! Live analysis of a stream (the mix coming through the plugin, or a SolRoute feed).
//!
//! Runs off the audio thread: the audio thread copies samples into a ring buffer and this
//! analyzer drains it. It keeps the cumulative measurements (what DIFF compares) and the
//! fast-moving ones (what the meters show).

use crate::bands::BANDS;
use crate::loudness::LoudnessMeter;
use crate::parts::detect_parts;
use crate::peak::TruePeakMeter;
use crate::profile::{PROFILE_VERSION, Profile, TONAL_FFT, TonalAccumulator};
use crate::spectrum::BandAnalyzer;
use crate::stereo::StereoMeter;
use crate::{amp_db, power_db};

/// Seconds of audio kept for PARTS detection.
pub const PARTS_WINDOW_SECS: f64 = 30.0;
/// Tonal-balance frames kept (about ten minutes at 48 kHz).
const TONAL_FRAMES: usize = 6000;

/// Fast-moving readings for meters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveReadings {
    pub momentary_lufs: Option<f64>,
    pub short_term_lufs: Option<f64>,
    /// RMS over roughly the last 300 ms, dBFS.
    pub rms_db: f64,
    /// Peak over roughly the last 300 ms, dBFS.
    pub peak_db: f64,
    pub correlation: Option<f64>,
    pub width: Option<f64>,
    /// Spectrum of the last few seconds, dB relative to its own total (same reference as tonal balance).
    pub spectrum_db: Vec<f64>,
}

/// The analyzer.
pub struct LiveAnalyzer {
    sample_rate: f64,
    loud: LoudnessMeter,
    peak: TruePeakMeter,
    stereo_total: StereoMeter,
    stereo_live: StereoMeter,
    bands: BandAnalyzer,
    tonal: TonalAccumulator,
    fast_spectrum: [f64; BANDS],
    fast_coeff: f64,
    rms_acc: f64,
    rms_coeff: f64,
    peak_hold: f64,
    peak_decay: f64,
    sum_sq: f64,
    samples: u64,
    history_l: Vec<f32>,
    history_r: Vec<f32>,
    history_pos: usize,
    history_full: bool,
}

impl LiveAnalyzer {
    /// An analyzer for a stream at `sample_rate`.
    #[must_use]
    pub fn new(sample_rate: f64) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate >= 8000.0 {
            sample_rate
        } else {
            48_000.0
        };
        let hop = TONAL_FFT / 2;
        let hist = (sr * PARTS_WINDOW_SECS) as usize;
        Self {
            sample_rate: sr,
            loud: LoudnessMeter::new(sr),
            peak: TruePeakMeter::new(),
            stereo_total: StereoMeter::cumulative(),
            stereo_live: StereoMeter::live(sr, 0.3),
            bands: BandAnalyzer::new(sr, TONAL_FFT, hop),
            tonal: TonalAccumulator::with_capacity(TONAL_FRAMES),
            fast_spectrum: [0.0; BANDS],
            // About 3 s of frames.
            fast_coeff: 1.0 - (-(hop as f64 / sr) / 3.0).exp(),
            rms_acc: 0.0,
            rms_coeff: 1.0 - (-1.0 / (sr * 0.3)).exp(),
            peak_hold: 0.0,
            peak_decay: (-1.0 / (sr * 0.3)).exp(),
            sum_sq: 0.0,
            samples: 0,
            history_l: vec![0.0; hist],
            history_r: vec![0.0; hist],
            history_pos: 0,
            history_full: false,
        }
    }

    /// The stream's sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Seconds analysed since the last reset.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        self.samples as f64 / self.sample_rate
    }

    /// Starts over (new song section, new take).
    pub fn reset(&mut self) {
        self.loud.reset();
        self.peak.reset();
        self.stereo_total.reset();
        self.stereo_live.reset();
        self.bands.reset();
        self.tonal.clear();
        self.fast_spectrum = [0.0; BANDS];
        self.rms_acc = 0.0;
        self.peak_hold = 0.0;
        self.sum_sq = 0.0;
        self.samples = 0;
        self.history_l.fill(0.0);
        self.history_r.fill(0.0);
        self.history_pos = 0;
        self.history_full = false;
    }

    /// Feeds stereo audio.
    pub fn process(&mut self, left: &[f32], right: &[f32]) {
        let n = left.len().min(right.len());
        let (left, right) = (&left[..n], &right[..n]);
        self.loud.process(left, Some(right));
        self.peak.process(left, Some(right));
        self.stereo_total.process(left, right);
        self.stereo_live.process(left, right);
        let (tonal, fast, c) = (&mut self.tonal, &mut self.fast_spectrum, self.fast_coeff);
        self.bands.process(left, right, |f| {
            tonal.push(f);
            for (b, x) in fast.iter_mut().enumerate() {
                *x += (f.total(b) - *x) * c;
            }
        });
        for (&l, &r) in left.iter().zip(right) {
            let (l, r) = (f64::from(l), f64::from(r));
            let sq = 0.5 * (l * l + r * r);
            self.sum_sq += sq;
            self.rms_acc += (sq - self.rms_acc) * self.rms_coeff;
            self.peak_hold = (self.peak_hold * self.peak_decay).max(l.abs()).max(r.abs());
        }
        self.samples += n as u64;
        let cap = self.history_l.len();
        if cap > 0 {
            for (&l, &r) in left.iter().zip(right) {
                self.history_l[self.history_pos] = l;
                self.history_r[self.history_pos] = r;
                self.history_pos += 1;
                if self.history_pos == cap {
                    self.history_pos = 0;
                    self.history_full = true;
                }
            }
        }
    }

    /// Meter readings.
    #[must_use]
    pub fn readings(&self) -> LiveReadings {
        let total: f64 = self.fast_spectrum.iter().sum();
        let reference = power_db(total);
        LiveReadings {
            momentary_lufs: self.loud.momentary(),
            short_term_lufs: self.loud.short_term(),
            rms_db: amp_db(self.rms_acc.max(0.0).sqrt()),
            peak_db: amp_db(self.peak_hold),
            correlation: self.stereo_live.correlation(),
            width: self.stereo_live.width(),
            spectrum_db: self
                .fast_spectrum
                .iter()
                .map(|&p| {
                    if total > 0.0 {
                        power_db(p) - reference
                    } else {
                        -120.0
                    }
                })
                .collect(),
        }
    }

    /// Integrated loudness since reset.
    #[must_use]
    pub fn integrated_lufs(&self) -> Option<f64> {
        self.loud.integrated()
    }

    /// The last [`PARTS_WINDOW_SECS`] of audio, oldest first.
    fn history(&self) -> (Vec<f32>, Vec<f32>) {
        if !self.history_full {
            return (
                self.history_l[..self.history_pos].to_vec(),
                self.history_r[..self.history_pos].to_vec(),
            );
        }
        let mut l = Vec::with_capacity(self.history_l.len());
        let mut r = Vec::with_capacity(self.history_r.len());
        l.extend_from_slice(&self.history_l[self.history_pos..]);
        l.extend_from_slice(&self.history_l[..self.history_pos]);
        r.extend_from_slice(&self.history_r[self.history_pos..]);
        r.extend_from_slice(&self.history_r[..self.history_pos]);
        (l, r)
    }

    /// Everything measured since reset as a [`Profile`]. With `with_parts`, also runs PARTS on
    /// the recent audio (a few milliseconds of work; call it a few times a second at most).
    #[must_use]
    pub fn profile(&self, with_parts: bool) -> Profile {
        let integrated = self.loud.integrated();
        let rms = if self.samples > 0 {
            (self.sum_sq / self.samples as f64).sqrt()
        } else {
            0.0
        };
        let true_peak_db = amp_db(self.peak.true_peak());
        let sample_peak_db = amp_db(self.peak.sample_peak());
        let rms_db = amp_db(rms);
        let parts = if with_parts {
            let (l, r) = self.history();
            detect_parts(&l, &r, self.sample_rate)
        } else {
            crate::parts::Parts::default()
        };
        let mut p = Profile {
            version: PROFILE_VERSION,
            sample_rate: self.sample_rate,
            duration_secs: self.seconds(),
            integrated_lufs: integrated,
            loudness_range: self.loud.loudness_range(),
            short_term_max: self.loud.short_term_max(),
            momentary_max: self.loud.momentary_max(),
            sample_peak_db,
            true_peak_db,
            rms_db,
            crest_db: if rms > 0.0 {
                sample_peak_db - rms_db
            } else {
                0.0
            },
            plr_db: integrated.map(|i| true_peak_db - i),
            correlation: self.stereo_total.correlation(),
            width: self.stereo_total.width(),
            tonal: self.tonal.reduce(),
            parts,
        };
        p.normalise();
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::analyze;

    #[test]
    fn live_profile_matches_offline_analysis() {
        let sr = 48_000.0;
        let mut x: u32 = 3;
        let sig: Vec<f32> = (0..sr as usize * 8)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32 - 0.5) * 0.3
            })
            .collect();
        let offline = analyze(&sig, &sig, sr);
        let mut live = LiveAnalyzer::new(sr);
        for chunk in sig.chunks(512) {
            live.process(chunk, chunk);
        }
        let p = live.profile(false);
        let (a, b) = (
            p.integrated_lufs.unwrap_or(0.0),
            offline.integrated_lufs.unwrap_or(1.0),
        );
        assert!((a - b).abs() < 1e-6, "{a} vs {b}");
        assert!((p.tonal.mean_db[30] - offline.tonal.mean_db[30]).abs() < 1e-6);
        let r = live.readings();
        assert!(r.short_term_lufs.is_some());
        assert_eq!(r.spectrum_db.len(), BANDS);
    }
}
