//! Faithful Rust port of Open303 (a TB-303 emulation, MIT).
//!
//! Each module mirrors one C++ file of `third_party/open303` and keeps its
//! structure, member names (snake_case) and operation order.

use crate::param::{Curve, ParamSpec};

pub mod analog_env;
pub mod biquad;
pub mod blend_osc;
pub mod decay_env;
pub mod elliptic_qb;
pub mod fft;
pub mod leaky_integrator;
pub mod math;
pub mod one_pole;
pub mod pattern;
pub mod teebee;
pub mod voice;
pub mod wavetable;

pub use analog_env::AnalogEnvelope;
pub use biquad::{BiquadFilter, BiquadMode};
pub use blend_osc::BlendOscillator;
pub use decay_env::DecayEnvelope;
pub use elliptic_qb::EllipticQuarterBandFilter;
pub use leaky_integrator::LeakyIntegrator;
pub use one_pole::{OnePoleFilter, OnePoleMode};
pub use pattern::{AcidPattern, AcidStep, PatternError};
pub use teebee::{FilterMode, TeeBeeFilter};
pub use voice::Acid303;
pub use wavetable::{WaveTable, Waveform};

/// User-facing parameters of [`Acid303`], in plain units. Ranges are the
/// Open303 knob ranges; defaults are the values the C++ constructor leaves
/// (the waveform blend is uninitialised there, so 0.0 = saw is used).
pub const ACID303_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "acid303.waveform",
        label: "Waveform",
        unit: "",
        min: 0.0,
        max: 1.0,
        curve: Curve::Linear,
        default: 0.0,
        help: "Blends saw (0) into square (1). Saw is brighter and buzzier; square is hollower.",
    },
    ParamSpec {
        id: "acid303.tuning",
        label: "Tuning",
        unit: "Hz",
        min: 400.0,
        max: 480.0,
        curve: Curve::Linear,
        default: 440.0,
        help: "Master pitch. 440 Hz is standard tuning.",
    },
    ParamSpec {
        id: "acid303.cutoff",
        label: "Cutoff",
        unit: "Hz",
        min: 314.0,
        max: 2394.0,
        curve: Curve::Exponential,
        default: 1000.0,
        help: "How bright or muffled the sound is. Slowly opening this over many bars is the classic acid build.",
    },
    ParamSpec {
        id: "acid303.resonance",
        label: "Resonance",
        unit: "%",
        min: 0.0,
        max: 100.0,
        curve: Curve::Linear,
        default: 0.0,
        help: "A whistling peak at the cutoff. High values make the line squeal and turn rubbery.",
    },
    ParamSpec {
        id: "acid303.env_mod",
        label: "Env Mod",
        unit: "%",
        min: 0.0,
        max: 100.0,
        curve: Curve::Linear,
        default: 25.0,
        help: "How far the filter jumps open on each note. This is the 'wow' on every step.",
    },
    ParamSpec {
        id: "acid303.decay",
        label: "Decay",
        unit: "ms",
        min: 200.0,
        max: 2000.0,
        curve: Curve::Exponential,
        default: 1000.0,
        help: "How fast the filter closes again after each note. Short is plucky, long is smeared.",
    },
    ParamSpec {
        id: "acid303.accent",
        label: "Accent",
        unit: "%",
        min: 0.0,
        max: 100.0,
        curve: Curve::Linear,
        default: 0.0,
        help: "How much harder and brighter accented steps hit. Back-to-back accents stack into a rising squelch.",
    },
    ParamSpec {
        id: "acid303.volume",
        label: "Volume",
        unit: "dB",
        min: -60.0,
        max: 0.0,
        curve: Curve::Linear,
        default: -12.0,
        help: "Output level of the 303.",
    },
    ParamSpec {
        id: "acid303.slide_time",
        label: "Slide Time",
        unit: "ms",
        min: 0.0,
        max: 200.0,
        curve: Curve::Linear,
        default: 60.0,
        help: "How long a slide takes to glide into the next note. This is the wobble.",
    },
];
