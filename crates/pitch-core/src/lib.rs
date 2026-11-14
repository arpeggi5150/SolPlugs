//! SolPlugs Pitch DSP core: everything that touches samples, no UI and no plugin glue.
//!
//! - [`detect`]: McLeod pitch method (YIN-family NSDF detector), clean-room from the paper.
//! - [`ola`]: TD-PSOLA grain overlap-add, formant preserving.
//! - [`correct`]: scale correction with retune speed, humanize and vibrato control.
//! - [`realtime`]: the live corrector (detection + correction + shifting at fixed latency).
//! - [`notes`]: offline analysis of a captured take into notes, and rendering note edits.
//! - [`scale`]: keys, scales, note names.
//! - [`metrics`]: objective quality measures (pitch accuracy, formant envelope, clicks).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod correct;
pub mod detect;
pub mod metrics;
pub mod notes;
pub mod ola;
pub mod realtime;
pub mod scale;
