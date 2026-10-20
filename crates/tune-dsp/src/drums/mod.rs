//! Synthesized drum voices, seeded noise and the drum step notation.

pub mod clap;
pub mod kick;
pub mod noise;
pub mod pattern;

pub use clap::{CLAP_PARAMS, Clap};
pub use kick::{ACCENT_GAIN_DB, KICK_PARAMS, Kick};
pub use noise::Noise;
pub use pattern::{DrumHit, DrumPattern, DrumPatternError};
