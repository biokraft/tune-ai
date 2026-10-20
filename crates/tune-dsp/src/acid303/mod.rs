//! Faithful Rust port of Open303 (a TB-303 emulation, MIT).
//!
//! Each module mirrors one C++ file of `third_party/open303` and keeps its
//! structure, member names (snake_case) and operation order.

pub mod analog_env;
pub mod biquad;
pub mod blend_osc;
pub mod decay_env;
pub mod elliptic_qb;
pub mod fft;
pub mod leaky_integrator;
pub mod math;
pub mod one_pole;
pub mod wavetable;

pub use analog_env::AnalogEnvelope;
pub use biquad::{BiquadFilter, BiquadMode};
pub use blend_osc::BlendOscillator;
pub use decay_env::DecayEnvelope;
pub use elliptic_qb::EllipticQuarterBandFilter;
pub use leaky_integrator::LeakyIntegrator;
pub use one_pole::{OnePoleFilter, OnePoleMode};
pub use wavetable::{WaveTable, Waveform};
