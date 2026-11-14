//! SolPlugs Samples: the sample library behind the manager.
//!
//! - [`model`]: samples (raw and edited versions), packs, genres, settings.
//! - [`store`]: the library data file (separate from code) and its edits/art folders.
//! - [`scan`]: import a folder, read tags ([`tags`]), names ([`names`]) and Song Sense BPM/key.
//! - [`audio`] / [`aiff`]: decoding WAV, AIFF, FLAC, MP3; writing WAV.
//! - [`tools`], [`fx`], [`edits`]: sample tools and saving their results as linked versions.
//! - [`filter`]: search and filter. [`export`]: pack export. [`spectrum`]: the live spectrum.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod aiff;
pub mod art;
pub mod audio;
pub mod edits;
pub mod export;
pub mod filter;
pub mod fx;
pub mod genres;
pub mod keys;
pub mod model;
pub mod names;
pub mod peaks;
pub mod scan;
pub mod spectrum;
pub mod store;
pub mod tags;
pub mod tools;
