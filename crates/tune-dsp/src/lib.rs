//! DSP building blocks for tune-ai.

#![deny(unsafe_code)]

pub mod acid303;
pub mod fx;
pub mod param;

pub use fx::{DIST_PARAMS, Distortion, DistortionMode};
pub use param::{Curve, ParamError, ParamSpec, find_spec};
