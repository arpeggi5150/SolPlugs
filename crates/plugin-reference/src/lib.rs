//! SolPlugs Reference as a VST3 and CLAP plugin. One DLL carries both entry points;
//! the bundler names it `.vst3` and `.clap`.
//!
//! The plugin is pure passthrough with zero latency: everything happens in
//! [`solplugs_ref_app`]; this crate is format glue and the editor window.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod clap;
pub mod editor;
pub mod vst3;

/// Display name.
pub const NAME: &str = "SolPlugs Reference";
/// Vendor.
pub const VENDOR: &str = "SolPlugs";
/// Project URL.
pub const URL: &str = "https://github.com/arpeggi5150/SolPlugs";
/// Version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// CLAP plugin id.
pub const CLAP_ID: &str = "com.solplugs.reference";
