//! Effects that sit after an instrument voice.

pub mod biquad;
pub mod distortion;
pub mod eq;

pub use biquad::{Biquad, BiquadKind};
pub use distortion::{DIST_PARAMS, Distortion, DistortionMode};
pub use eq::{EQ_PARAMS, Equalizer};
