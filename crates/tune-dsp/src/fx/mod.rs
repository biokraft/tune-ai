//! Effects that sit after an instrument voice.

pub mod biquad;
pub mod delay;
pub mod distortion;
pub mod duck;
pub mod eq;
pub mod limiter;
pub mod reverb;
pub mod strip;

pub use biquad::{Biquad, BiquadKind};
pub use delay::{DELAY_MAX_STEPS, DELAY_PARAMS, Delay};
pub use distortion::{DIST_PARAMS, Distortion, DistortionMode};
pub use duck::{DUCK_ATTACK_MS, Ducker};
pub use eq::{EQ_PARAMS, Equalizer};
pub use limiter::{
    DETECTOR_DELAY, LIMITER_LOOKAHEAD_MS, Limiter, MASTER_CEILING, MASTER_GAIN, MASTER_PARAMS,
    MASTER_RELEASE,
};
pub use reverb::{REVERB_PARAMS, Reverb};
pub use strip::{
    SEND_OFF_DB, STRIP_DUCK_DEPTH, STRIP_DUCK_RELEASE, STRIP_PARAMS, STRIP_SEND_DELAY,
    STRIP_SEND_REVERB, send_gain,
};
