//! Synthesized drum voices, seeded noise and the drum step notation.

pub mod clap;
pub mod hat;
pub mod kick;
pub mod noise;
pub mod pattern;

pub use clap::{CLAP_PARAMS, Clap};
pub use hat::{HAT_CLOSED_PARAMS, HAT_OPEN_PARAMS, Hat, HatKind, RIDE_PARAMS};
pub use kick::{ACCENT_GAIN_DB, KICK_PARAMS, Kick};
pub use noise::Noise;
pub use pattern::{DrumHit, DrumPattern, DrumPatternError};
