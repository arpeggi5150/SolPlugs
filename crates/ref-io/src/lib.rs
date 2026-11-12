//! Decoding, the reference library, and online preview search for the SolPlugs reference analyzer.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod decode;
#[cfg(feature = "deezer")]
pub mod deezer;
pub mod library;
