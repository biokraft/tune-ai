//! Effects that sit after an instrument voice.

pub mod biquad;
pub mod delay;
pub mod distortion;
pub mod duck;
pub mod eq;
pub mod reverb;
pub mod strip;

pub use biquad::{Biquad, BiquadKind};
pub use delay::{DELAY_MAX_STEPS, DELAY_PARAMS, Delay};
pub use distortion::{DIST_PARAMS, Distortion, DistortionMode};
pub use duck::{DUCK_ATTACK_MS, Ducker};
pub use eq::{EQ_PARAMS, Equalizer};
pub use reverb::{REVERB_PARAMS, Reverb};
pub use strip::{
    SEND_OFF_DB, STRIP_DUCK_DEPTH, STRIP_DUCK_RELEASE, STRIP_PARAMS, STRIP_SEND_DELAY,
    STRIP_SEND_REVERB, send_gain,
};
