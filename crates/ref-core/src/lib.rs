//! Analysis core for the SolPlugs reference analyzer.
//!
//! Everything here is plain, allocation-aware DSP with no UI or plugin-format code, so the
//! same core runs inside Solara, inside the VST3/CLAP plugin and in tests.
//!
//! Realtime rules: types used on the audio thread ([`abswitch::AbSwitch`],
//! [`loudness::LoudnessMeter`], [`peak::TruePeakMeter`], [`stereo::StereoMeter`]) allocate only
//! in their constructors.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod abswitch;
pub mod bands;
pub mod biquad;
pub mod diff;
pub mod group;
pub mod live;
pub mod loudness;
pub mod parts;
pub mod peak;
pub mod profile;
pub mod resample;
pub mod spectrum;
pub mod stereo;

/// Converts a linear power ratio to decibels, flooring silence at -200 dB.
#[must_use]
pub fn power_db(p: f64) -> f64 {
    if p > 1e-20 { 10.0 * p.log10() } else { -200.0 }
}

/// Converts a linear amplitude to decibels, flooring silence at -200 dB.
#[must_use]
pub fn amp_db(a: f64) -> f64 {
    if a > 1e-10 { 20.0 * a.log10() } else { -200.0 }
}

/// Converts decibels to a linear amplitude.
#[must_use]
pub fn db_to_amp(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}
