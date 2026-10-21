//! DSP building blocks for tune-ai.

#![deny(unsafe_code)]

pub mod acid303;
pub mod drums;
pub mod fx;
pub mod param;

pub use acid303::math::db2amp;
pub use drums::{
    ACCENT_GAIN_DB, CLAP_PARAMS, Clap, DrumHit, DrumPattern, DrumPatternError, HAT_CLOSED_PARAMS,
    HAT_OPEN_PARAMS, Hat, HatKind, KICK_PARAMS, Kick, Noise,
};
pub use fx::{Biquad, BiquadKind, DIST_PARAMS, Distortion, DistortionMode, EQ_PARAMS, Equalizer};
pub use param::{Curve, ParamError, ParamSpec, find_spec};
