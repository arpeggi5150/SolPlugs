//! The SolPlugs reference analyzer as an embeddable app.
//!
//! - [`bridge::Bridge`] is the only state the audio thread touches (atomics and a lock-free
//!   swap), shared by the [`processor::Processor`] (audio thread) and the [`engine::Engine`].
//! - [`engine::Engine`] owns a background worker that drains the mix, measures, decodes,
//!   searches and keeps [`state::AppState`] current.
//! - [`ui::show`] draws everything with egui; Solara embeds it as-is, the plugin hosts it in
//!   its own window.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod bridge;
pub mod engine;
pub mod processor;
pub mod state;
pub mod ui;
mod worker;
