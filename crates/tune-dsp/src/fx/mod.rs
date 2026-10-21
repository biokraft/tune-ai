//! Effects that sit after an instrument voice.

pub mod biquad;
pub mod delay;
pub mod distortion;
pub mod eq;
pub mod reverb;

pub use biquad::{Biquad, BiquadKind};
pub use delay::{DELAY_MAX_STEPS, DELAY_PARAMS, Delay};
pub use distortion::{DIST_PARAMS, Distortion, DistortionMode};
pub use eq::{EQ_PARAMS, Equalizer};
pub use reverb::{REVERB_PARAMS, Reverb};
