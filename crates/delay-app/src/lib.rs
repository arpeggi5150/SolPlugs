//! SolPlugs Delay engine and UI, shared by the plugin, the standalone app and Solara.
//!
//! - [`params`]: the automatable parameters (a full set per version: per-version memory).
//! - [`engine`]: shared state, the audio-thread processor, presets, A/B, MATCH, auto-makeup.
//! - [`presets`]: factory presets.
//! - [`ui`]: the egui editor: digital-first (big decay curve, slim hardware strip) or
//!   stacked (hardware on top, digital below).
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
