//! Synthesized drum voices, seeded noise and the drum step notation.

pub mod noise;
pub mod pattern;

pub use noise::Noise;
pub use pattern::{DrumHit, DrumPattern, DrumPatternError};
