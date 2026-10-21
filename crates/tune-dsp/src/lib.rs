//! DSP building blocks for tune-ai.

#![deny(unsafe_code)]

pub mod acid303;
pub mod drums;
pub mod fx;
pub mod param;

pub use drums::{
    ACCENT_GAIN_DB, CLAP_PARAMS, Clap, DrumHit, DrumPattern, DrumPatternError, HAT_CLOSED_PARAMS,
    HAT_OPEN_PARAMS, Hat, HatKind, KICK_PARAMS, Kick, Noise,
};
pub use fx::{DIST_PARAMS, Distortion, DistortionMode};
pub use param::{Curve, ParamError, ParamSpec, find_spec};
