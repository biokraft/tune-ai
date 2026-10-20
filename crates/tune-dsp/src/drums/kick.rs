//! 909-style synthesized kick: pitch-swept sine, exponential decay, noise
//! click, tanh drive.

use super::noise::Noise;
use crate::acid303::math::db2amp;
use crate::param::{Curve, ParamError, ParamSpec, find_spec};
use std::f64::consts::TAU;

/// Extra gain on accented hits, in dB.
pub const ACCENT_GAIN_DB: f64 = 4.0;

/// User-facing parameters of [`Kick`], in plain units.
pub const KICK_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "kick.tune",
        label: "Tune",
        unit: "Hz",
        min: 40.0,
        max: 80.0,
        curve: Curve::Linear,
        default: 50.0,
        help: "Pitch the kick body settles on. Tune it to the track's root note so kick and bassline agree.",
    },
    ParamSpec {
        id: "kick.decay",
        label: "Decay",
        unit: "ms",
        min: 100.0,
        max: 1500.0,
        curve: Curve::Exponential,
        default: 450.0,
        help: "How long the kick booms. Short is tight and punchy; long turns into a rumbling sub.",
    },
    ParamSpec {
        id: "kick.punch",
        label: "Punch",
        unit: "%",
        min: 0.0,
        max: 100.0,
        curve: Curve::Linear,
        default: 50.0,
        help: "How far the pitch drops at the start of each hit. More punch, more thud in the chest.",
    },
    ParamSpec {
        id: "kick.click",
        label: "Click",
        unit: "%",
        min: 0.0,
        max: 100.0,
        curve: Curve::Linear,
        default: 30.0,
        help: "The short tick at the front of the kick that helps it cut through on small speakers.",
    },
    ParamSpec {
        id: "kick.drive",
        label: "Drive",
        unit: "dB",
        min: 0.0,
        max: 24.0,
        curve: Curve::Linear,
        default: 6.0,
        help: "Saturates the kick for a harder, rave-style knock. Too much turns the low end muddy.",
    },
];

const PITCH_SWEEP_RATIO: f64 = 6.0;
const PITCH_SWEEP_TAU: f64 = 0.012;
const CLICK_TAU: f64 = 0.0015;
const ACCENT_CLICK_BOOST: f64 = 1.5;
/// Stop rendering once the envelope is below this (-120 dB).
const SILENCE_AMP: f64 = 1e-6;

/// Synthesized kick drum voice.
#[derive(Clone, Debug)]
pub struct Kick {
    sample_rate: f64,
    tune: f64,
    decay_s: f64,
    punch: f64,
    click: f64,
    drive_gain: f64,
    noise: Noise,
    active: bool,
    accent: bool,
    phase: f64,
    t: f64,
}

impl Kick {
    /// Creates a silent kick at `sample_rate` Hz with [`KICK_PARAMS`] defaults.
    pub fn new(sample_rate: f64) -> Self {
        let mut k = Self {
            sample_rate,
            tune: 50.0,
            decay_s: 0.45,
            punch: 0.5,
            click: 0.3,
            drive_gain: 1.0,
            noise: Noise::new(0),
            active: false,
            accent: false,
            phase: 0.0,
            t: 0.0,
        };
        for (i, spec) in KICK_PARAMS.iter().enumerate() {
            k.set_param_at(i, spec.default);
        }
        k
    }

    /// Starts a hit: phase and time restart, the noise is reseeded with `seed`.
    pub fn trigger(&mut self, accent: bool, seed: u64) {
        self.accent = accent;
        self.phase = 0.0;
        self.t = 0.0;
        self.noise.reseed(seed);
        self.active = true;
    }

    /// Next output sample; exactly 0.0 while silent.
    pub fn get_sample(&mut self) -> f64 {
        if !self.active {
            return 0.0;
        }
        let t = self.t;
        let amp = (-t * 1000f64.ln() / self.decay_s).exp();
        if amp < SILENCE_AMP {
            self.active = false;
            return 0.0;
        }
        let freq =
            self.tune * (1.0 + PITCH_SWEEP_RATIO * self.punch * (-t / PITCH_SWEEP_TAU).exp());
        let body = (TAU * self.phase).sin();
        let boost = if self.accent { ACCENT_CLICK_BOOST } else { 1.0 };
        let click = self.click * boost * self.noise.sample() * (-t / CLICK_TAU).exp();
        let x = body * amp + click;
        let mut y = (self.drive_gain * x).tanh() / self.drive_gain.tanh();
        if self.accent {
            y *= db2amp(ACCENT_GAIN_DB);
        }
        self.phase = (self.phase + freq / self.sample_rate).fract();
        self.t += 1.0 / self.sample_rate;
        y
    }

    /// Silences the voice until the next trigger. Parameters stay.
    pub fn reset(&mut self) {
        self.active = false;
        self.phase = 0.0;
        self.t = 0.0;
    }

    /// Sets a parameter in plain units (see [`KICK_PARAMS`]).
    pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError> {
        let (index, spec) = find_spec(KICK_PARAMS, id)?;
        spec.check(plain)?;
        self.set_param_at(index, plain);
        Ok(())
    }

    /// Sets the parameter at `index` of [`KICK_PARAMS`] in plain units,
    /// clamped to its range (NaN is treated as the minimum).
    ///
    /// # Panics
    /// If `index` is out of range of [`KICK_PARAMS`].
    pub fn set_param_at(&mut self, index: usize, plain: f64) {
        let spec = &KICK_PARAMS[index];
        let plain = if plain.is_nan() {
            spec.min
        } else {
            plain.clamp(spec.min, spec.max)
        };
        match spec.id {
            "kick.tune" => self.tune = plain,
            "kick.decay" => self.decay_s = plain / 1000.0,
            "kick.punch" => self.punch = plain / 100.0,
            "kick.click" => self.click = plain / 100.0,
            "kick.drive" => self.drive_gain = db2amp(plain),
            id => unreachable!("KICK_PARAMS id `{id}` has no setter"),
        }
    }
}
