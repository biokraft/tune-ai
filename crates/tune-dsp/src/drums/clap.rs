//! 909-style clap: band-passed noise shaped by three short slaps and a tail.

use super::kick::ACCENT_GAIN_DB;
use super::noise::Noise;
use crate::acid303::math::db2amp;
use crate::acid303::{BiquadFilter, BiquadMode};
use crate::param::{Curve, ParamError, ParamSpec, find_spec};

/// User-facing parameters of [`Clap`], in plain units.
pub const CLAP_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "clap.tone",
        label: "Tone",
        unit: "Hz",
        min: 600.0,
        max: 2400.0,
        curve: Curve::Exponential,
        default: 1200.0,
        help: "Where the clap's noise sits. Low is thick and roomy, high is snappy and thin.",
    },
    ParamSpec {
        id: "clap.decay",
        label: "Decay",
        unit: "ms",
        min: 50.0,
        max: 800.0,
        curve: Curve::Exponential,
        default: 250.0,
        help: "Length of the clap's tail. Long tails sound like a big room.",
    },
    ParamSpec {
        id: "clap.spread",
        label: "Spread",
        unit: "ms",
        min: 4.0,
        max: 20.0,
        curve: Curve::Linear,
        default: 10.0,
        help: "Gap between the hand slaps that make up one clap. Wider sounds looser, more like a crowd.",
    },
];

/// Fixed makeup gain (measured default peak 0.64 at 1.0, so 0.89 here).
const MAKEUP: f64 = 1.4;
const BANDWIDTH_OCTAVES: f64 = 1.0;
const ACCENT_TONE_RATIO: f64 = 1.25;
const TAIL_LEVEL: f64 = 0.6;
/// The tail is 60 dB down at `decay`; past twice that it is below -120 dB.
const TAIL_SPAN: f64 = 2.0;

/// Synthesized clap voice.
#[derive(Clone, Debug)]
pub struct Clap {
    sample_rate: f64,
    tone: f64,
    decay_s: f64,
    spread_s: f64,
    filter: BiquadFilter,
    noise: Noise,
    active: bool,
    accent: bool,
    t: f64,
}

impl Clap {
    /// Creates a silent clap at `sample_rate` Hz with [`CLAP_PARAMS`] defaults.
    pub fn new(sample_rate: f64) -> Self {
        let mut filter = BiquadFilter::new();
        filter.set_sample_rate(sample_rate);
        filter.set_mode(BiquadMode::Bandpass);
        filter.set_bandwidth(BANDWIDTH_OCTAVES);
        let mut c = Self {
            sample_rate,
            tone: 1200.0,
            decay_s: 0.25,
            spread_s: 0.01,
            filter,
            noise: Noise::new(0),
            active: false,
            accent: false,
            t: 0.0,
        };
        for (i, spec) in CLAP_PARAMS.iter().enumerate() {
            c.set_param_at(i, spec.default);
        }
        c
    }

    /// Starts a hit: time and filter restart, the noise is reseeded with `seed`.
    pub fn trigger(&mut self, accent: bool, seed: u64) {
        self.accent = accent;
        self.t = 0.0;
        self.noise.reseed(seed);
        self.filter.reset();
        let ratio = if accent { ACCENT_TONE_RATIO } else { 1.0 };
        self.filter
            .set_frequency((self.tone * ratio).min(0.45 * self.sample_rate));
        self.active = true;
    }

    fn envelope(&self) -> f64 {
        let (t, s) = (self.t, self.spread_s);
        let fall = |age: f64, span: f64| 10f64.powf(-3.0 * age / span);
        let mut env = 0.0;
        for k in 0..3 {
            let onset = k as f64 * s;
            if t >= onset {
                env += fall(t - onset, s);
            }
        }
        if t >= 2.0 * s {
            env += TAIL_LEVEL * fall(t - 2.0 * s, self.decay_s);
        }
        env.min(1.0)
    }

    /// Next output sample; exactly 0.0 while silent.
    pub fn get_sample(&mut self) -> f64 {
        if !self.active {
            return 0.0;
        }
        if self.t > 2.0 * self.spread_s + TAIL_SPAN * self.decay_s {
            self.active = false;
            return 0.0;
        }
        let env = self.envelope();
        let mut y = self.filter.get_sample(self.noise.sample()) * env * MAKEUP;
        if self.accent {
            y *= db2amp(ACCENT_GAIN_DB);
        }
        self.t += 1.0 / self.sample_rate;
        y
    }

    /// Silences the voice until the next trigger. Parameters stay.
    pub fn reset(&mut self) {
        self.active = false;
        self.t = 0.0;
        self.filter.reset();
    }

    /// Sets a parameter in plain units (see [`CLAP_PARAMS`]).
    pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError> {
        let (index, spec) = find_spec(CLAP_PARAMS, id)?;
        spec.check(plain)?;
        self.set_param_at(index, plain);
        Ok(())
    }

    /// Sets the parameter at `index` of [`CLAP_PARAMS`] in plain units,
    /// clamped to its range (NaN is treated as the minimum).
    ///
    /// # Panics
    /// If `index` is out of range of [`CLAP_PARAMS`].
    pub fn set_param_at(&mut self, index: usize, plain: f64) {
        let spec = &CLAP_PARAMS[index];
        let plain = if plain.is_nan() {
            spec.min
        } else {
            plain.clamp(spec.min, spec.max)
        };
        match spec.id {
            "clap.tone" => self.tone = plain,
            "clap.decay" => self.decay_s = plain / 1000.0,
            "clap.spread" => self.spread_s = plain / 1000.0,
            id => unreachable!("CLAP_PARAMS id `{id}` has no setter"),
        }
    }
}
