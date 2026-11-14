//! SolPlugs Samples as an embeddable app.
//!
//! - [`engine::Engine`]: the library, the preview [`player::Player`], decoded-audio cache and
//!   background worker. One per process ([`engine::Engine::shared`]).
//! - [`ui::show`]: the whole manager drawn with egui. The plugin, the standalone window and
//!   Solara all call it.
//! - [`player::Player::render`]: the audio thread mixes the preview into its output.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod dragout;
pub mod engine;
pub mod player;
pub mod ui;
pub mod worker;
