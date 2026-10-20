//! Faithful Rust port of Open303 (a TB-303 emulation, MIT).
//!
//! Each module mirrors one C++ file of `third_party/open303` and keeps its
//! structure, member names (snake_case) and operation order.

pub mod analog_env;
pub mod biquad;
pub mod decay_env;
pub mod elliptic_qb;
pub mod leaky_integrator;
pub mod math;
pub mod one_pole;

pub use analog_env::AnalogEnvelope;
pub use biquad::{BiquadFilter, BiquadMode};
pub use decay_env::DecayEnvelope;
pub use elliptic_qb::EllipticQuarterBandFilter;
pub use leaky_integrator::LeakyIntegrator;
pub use one_pole::{OnePoleFilter, OnePoleMode};
