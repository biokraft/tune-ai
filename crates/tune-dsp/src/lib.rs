//! DSP building blocks for tune-ai.

#![deny(unsafe_code)]

pub mod acid303;
pub mod drums;
pub mod fx;
pub mod param;

pub use acid303::math::db2amp;
pub use drums::{
    ACCENT_GAIN_DB, CLAP_PARAMS, Clap, DrumHit, DrumPattern, DrumPatternError, HAT_CLOSED_PARAMS,
    HAT_OPEN_PARAMS, Hat, HatKind, KICK_PARAMS, Kick, Noise, RIDE_PARAMS,
};
pub use fx::{
    Biquad, BiquadKind, DELAY_MAX_STEPS, DELAY_PARAMS, DETECTOR_DELAY, DIST_PARAMS, DUCK_ATTACK_MS,
    Delay, Distortion, DistortionMode, Ducker, EQ_PARAMS, Equalizer, LIMITER_LOOKAHEAD_MS, Limiter,
    MASTER_CEILING, MASTER_GAIN, MASTER_PARAMS, MASTER_RELEASE, REVERB_PARAMS, Reverb, SEND_OFF_DB,
    STRIP_DUCK_DEPTH, STRIP_DUCK_RELEASE, STRIP_PARAMS, STRIP_SEND_DELAY, STRIP_SEND_REVERB,
    send_gain,
};
pub use param::{Curve, ParamError, ParamSpec, find_spec};
