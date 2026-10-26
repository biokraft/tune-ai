//! Instrument and effect modules: the traits they implement, today's
//! built-in modules and the registry that names them.

#![forbid(unsafe_code)]

pub mod builtin;
pub mod descriptor;
pub mod instrument;
pub mod registry;

pub use builtin::{ACID303, AcidTrack, CLAP, DISTORTION, EQ, HAT_CLOSED, HAT_OPEN, KICK, RIDE};
pub use descriptor::{ClipKind, MakeEffect, MakeInstrument, ModuleDescriptor, Role, SettingSpec};
pub use instrument::{Cell, Effect, Instrument, StepCtx};
pub use registry::{BUILTIN, Registry};
