//! SolPlugs EQ as a VST3 and CLAP plugin. Each shipped binary carries one format's
//! entry point; `cargo xtask bundle` builds them separately.
//!
//! Everything that touches audio lives in [`solplugs_eq_app`]; this crate is format glue
//! and the editor window.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

#[cfg(feature = "clap")]
pub mod clap;
pub mod editor;
#[cfg(feature = "vst3")]
pub mod vst3;

/// Display name.
pub const NAME: &str = "SolPlugs EQ";
/// Vendor.
pub const VENDOR: &str = "SolPlugs";
/// Project URL.
pub const URL: &str = "https://github.com/arpeggi5150/SolPlugs";
/// Version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// CLAP plugin id.
pub const CLAP_ID: &str = "com.solplugs.eq";
