//! The song engine: transport clock, instrument and effect traits, and the
//! track wrappers around the `tune-dsp` voices.

#![forbid(unsafe_code)]

pub mod clock;
pub mod instrument;
pub mod tracks;

pub use clock::{Clock, hit_seed, track_seed};
pub use instrument::{Cell, Effect, Instrument, StepCtx};
pub use tracks::{AcidTrack, make_effect, make_instrument};
