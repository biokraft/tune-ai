//! The song engine: transport clock, instrument and effect traits, the
//! track wrappers around the `tune-dsp` voices and song compilation.

#![forbid(unsafe_code)]

pub mod clock;
pub mod compile;
pub mod instrument;
pub mod mixer;
pub mod tracks;

pub use clock::{Clock, hit_seed, track_seed};
pub use compile::{
    CompiledFx, CompiledLane, CompiledSection, CompiledSong, CompiledTrack, ParamTarget,
};
pub use instrument::{Cell, Effect, Instrument, StepCtx};
pub use mixer::pan_gains;
pub use tracks::{AcidTrack, make_effect, make_instrument};
