//! Song Sense's BPM and key detector, shared with Song Sense itself.
//!
//! The detector is Song Sense's portable C++ core, compiled from a private checkout at build
//! time (see `build.rs`). When that checkout isn't present the crate builds without it,
//! [`available`] is false and [`detect`] returns `None`.

/// What Song Sense found.
#[derive(Clone, Debug, PartialEq)]
pub struct Detection {
    /// Overall tempo, if Song Sense found a beat.
    pub bpm: Option<f64>,
    /// Overall key in Song Sense's spelling ("C major", "F# minor"), if it found one.
    pub key: Option<String>,
    /// Key confidence, 0..1.
    pub key_confidence: f64,
}

/// Longest stretch handed to the detector, seconds. Samples are short; a long file is
/// judged by its first minute so an import stays quick.
pub const MAX_SECONDS: f64 = 60.0;
/// Shortest audio worth detecting, seconds. One-shots have no tempo or key to find.
pub const MIN_SECONDS: f64 = 1.5;

/// True when the detector was linked into this build.
#[must_use]
pub fn available() -> bool {
    cfg!(songsense_linked)
}

/// Song Sense's version, when linked.
#[must_use]
pub fn version() -> Option<String> {
    imp::version()
}

/// Detects BPM and key of mono audio at `sample_rate`.
#[must_use]
pub fn detect(mono: &[f32], sample_rate: f64) -> Option<Detection> {
    if sample_rate.is_nan() || sample_rate <= 0.0 || (mono.len() as f64) < sample_rate * MIN_SECONDS
    {
        return None;
    }
    let n = mono.len().min((sample_rate * MAX_SECONDS) as usize);
    imp::detect(&mono[..n], sample_rate)
}

#[cfg(songsense_linked)]
mod imp {
    use super::Detection;
    use std::ffi::{CStr, c_char, c_int};

    #[repr(C)]
    struct ShimResult {
        has_bpm: c_int,
        bpm: f64,
        key_confidence: f64,
        key: [c_char; 32],
    }

    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn ss_shim_analyse(
            mono: *const f32,
            frames: usize,
            sample_rate: f64,
            out: *mut ShimResult,
        ) -> c_int;
        fn ss_shim_version() -> *const c_char;
    }

    pub fn version() -> Option<String> {
        // SAFETY: returns a pointer to a static NUL-terminated string literal.
        #[allow(unsafe_code)]
        let s = unsafe { CStr::from_ptr(ss_shim_version()) };
        Some(s.to_string_lossy().into_owned())
    }

    pub fn detect(mono: &[f32], sample_rate: f64) -> Option<Detection> {
        let mut out = ShimResult {
            has_bpm: 0,
            bpm: 0.0,
            key_confidence: 0.0,
            key: [0; 32],
        };
        // SAFETY: `mono` is a valid slice for `len` floats and outlives the call; the shim
        // copies it before analysing, writes only into `out`, NUL-terminates `key` and
        // catches every C++ exception so nothing unwinds across the boundary.
        #[allow(unsafe_code)]
        let code = unsafe { ss_shim_analyse(mono.as_ptr(), mono.len(), sample_rate, &mut out) };
        if code != 0 {
            log::debug!("Song Sense declined the audio (code {code})");
            return None;
        }
        // SAFETY: the shim zeroes the struct and copies at most 31 bytes, so `key` ends in NUL.
        #[allow(unsafe_code)]
        let key = unsafe { CStr::from_ptr(out.key.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let bpm = (out.has_bpm != 0 && out.bpm.is_finite() && out.bpm > 0.0).then_some(out.bpm);
        let key = (!key.is_empty() && key != "unknown").then_some(key);
        Some(Detection {
            bpm,
            key,
            key_confidence: out.key_confidence,
        })
    }
}

#[cfg(not(songsense_linked))]
mod imp {
    use super::Detection;

    pub fn version() -> Option<String> {
        None
    }

    pub fn detect(_mono: &[f32], _sample_rate: f64) -> Option<Detection> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kick on every beat at 120 BPM over a C major triad.
    fn groove(sr: f64, secs: f64) -> Vec<f32> {
        let n = (sr * secs) as usize;
        (0..n)
            .map(|i| {
                let t = i as f64 / sr;
                let beat = t % 0.5;
                let kick =
                    0.6 * (-beat * 30.0).exp() * (2.0 * std::f64::consts::PI * 60.0 * t).sin();
                let chord: f64 = [261.63, 329.63, 392.0]
                    .iter()
                    .map(|f| (2.0 * std::f64::consts::PI * f * t).sin())
                    .sum();
                (kick + 0.2 * chord) as f32
            })
            .collect()
    }

    #[test]
    fn too_short_is_skipped() {
        assert_eq!(detect(&[0.0; 100], 44_100.0), None);
    }

    #[test]
    fn finds_tempo_and_key_when_linked() {
        let d = detect(&groove(44_100.0, 8.0), 44_100.0);
        if !available() {
            assert!(d.is_none());
            return;
        }
        let d = d.unwrap_or_else(|| panic!("no detection"));
        let bpm = d.bpm.unwrap_or_default();
        assert!((bpm - 120.0).abs() < 1.0, "bpm {bpm}");
        assert_eq!(d.key.as_deref(), Some("C major"));
    }
}
