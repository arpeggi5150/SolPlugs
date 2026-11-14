//! The live spectrum shown while a sample previews: one FFT frame around the play position,
//! reduced to log-spaced bands from 20 Hz to 20 kHz.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use crate::audio::Audio;

/// FFT size.
pub const SIZE: usize = 4096;
/// Bands drawn.
pub const BANDS: usize = 96;
/// Floor, dBFS.
pub const FLOOR_DB: f32 = -96.0;

/// A reusable spectrum analyser.
pub struct Spectrum {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buf: Vec<Complex<f32>>,
    /// Smoothed output, dB per band.
    pub bands: Vec<f32>,
}

impl Default for Spectrum {
    fn default() -> Self {
        Self::new()
    }
}

impl Spectrum {
    /// A new analyser.
    #[must_use]
    pub fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(SIZE);
        let window = (0..SIZE)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / SIZE as f32).cos())
            .collect();
        Self {
            fft,
            window,
            buf: vec![Complex::default(); SIZE],
            bands: vec![FLOOR_DB; BANDS],
        }
    }

    /// Centre frequency of band `i`.
    #[must_use]
    pub fn band_hz(i: usize) -> f32 {
        20.0 * 1000f32.powf((i as f32 + 0.5) / BANDS as f32)
    }

    /// Analyses the frame centred on `frame` and blends it into [`Self::bands`]
    /// (fast attack, slower fall so it reads as live movement).
    pub fn update(&mut self, audio: &Audio, frame: usize) {
        let n = audio.frames();
        let start = frame.saturating_sub(SIZE / 2);
        let k = audio.channels.len().max(1) as f32;
        let wsum: f32 = self.window.iter().sum();
        for i in 0..SIZE {
            let j = start + i;
            let v = if j < n {
                audio.channels.iter().map(|c| c[j]).sum::<f32>() / k
            } else {
                0.0
            };
            self.buf[i] = Complex::new(v * self.window[i], 0.0);
        }
        self.fft.process(&mut self.buf);
        let sr = audio.sample_rate as f32;
        let bin_hz = sr / SIZE as f32;
        for b in 0..BANDS {
            let lo = 20.0 * 1000f32.powf(b as f32 / BANDS as f32);
            let hi = 20.0 * 1000f32.powf((b + 1) as f32 / BANDS as f32);
            let a = ((lo / bin_hz).floor() as usize).max(1);
            let z = ((hi / bin_hz).ceil() as usize).max(a + 1).min(SIZE / 2);
            let mut peak = 0.0f32;
            for bin in a..z {
                peak = peak.max(self.buf[bin].norm());
            }
            let mag = 2.0 * peak / wsum;
            let db = if a >= SIZE / 2 || hi > sr / 2.0 {
                FLOOR_DB
            } else {
                (20.0 * mag.max(1e-9).log10()).max(FLOOR_DB)
            };
            let cur = &mut self.bands[b];
            *cur = if db > *cur {
                db
            } else {
                *cur + (db - *cur) * 0.25
            };
        }
    }

    /// Lets the display fall to silence (when playback stops).
    pub fn decay(&mut self) {
        for v in &mut self.bands {
            *v += (FLOOR_DB - *v) * 0.15;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_peaks_in_its_band() {
        let sr = 48_000.0;
        let a = Audio::new(
            sr,
            vec![
                (0..SIZE * 2)
                    .map(|i| {
                        (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / sr).sin() as f32 * 0.5
                    })
                    .collect(),
            ],
        );
        let mut s = Spectrum::new();
        s.update(&a, SIZE);
        let (best, db) =
            s.bands.iter().enumerate().fold(
                (0, f32::MIN),
                |m, (i, v)| if *v > m.1 { (i, *v) } else { m },
            );
        let hz = Spectrum::band_hz(best);
        assert!((800.0..1250.0).contains(&hz), "{hz}");
        assert!((db - (-6.0)).abs() < 1.5, "{db}");
    }
}
