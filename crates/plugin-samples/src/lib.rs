//! SolPlugs Samples as a VST3 and CLAP plugin (built as two binaries, one per format).
//!
//! The plugin passes its input through untouched and mixes the manager's previews into its
//! output, with zero latency. Everything else happens in [`solplugs_samples_app`]; this crate
//! is format glue and the editor window.
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
pub const NAME: &str = "SolPlugs Samples";
/// Vendor.
pub const VENDOR: &str = "SolPlugs";
/// Project URL.
pub const URL: &str = "https://github.com/arpeggi5150/SolPlugs";
/// Version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// CLAP plugin id.
pub const CLAP_ID: &str = "com.solplugs.samples";

/// What a project stores for an instance: the library is a separate data file.
pub const STATE: &[u8] = br#"{"solplugs_samples":1}"#;
