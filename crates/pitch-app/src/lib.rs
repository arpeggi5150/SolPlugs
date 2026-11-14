//! SolPlugs Pitch engine and UI, shared by the plugin and Solara.
//!
//! - [`params`]: the automatable parameters.
//! - [`engine`]: shared state, the audio-thread processor, capture and the worker.
//! - [`ui`]: the egui editor (automatic and graph pages).
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
pub mod ui;
