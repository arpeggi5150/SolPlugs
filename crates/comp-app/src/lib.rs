//! SolPlugs Compressor engine and UI, shared by the plugin, the standalone app and Solara.
//!
//! - [`params`]: the automatable parameters (a full set per mode: per-mode memory).
//! - [`engine`]: shared state, the audio-thread processor, presets, A/B, MATCH, auto-makeup.
//! - [`presets`]: factory presets.
//! - [`ui`]: the egui editor: the mode's hardware faceplate and the digital side view.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod engine;
pub mod params;
pub mod presets;
pub mod ui;
