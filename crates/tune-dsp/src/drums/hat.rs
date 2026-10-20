//! 909-style closed and open hats: square-oscillator metal bank blended with
//! noise, high-passed, with an exponential envelope.

use super::kick::ACCENT_GAIN_DB;
use super::noise::Noise;
use crate::acid303::math::db2amp;
use crate::acid303::{BiquadFilter, BiquadMode};
use crate::param::{Curve, ParamError, ParamSpec, find_spec};

/// User-facing parameters of the closed [`Hat`].
pub const HAT_CLOSED_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "hat_closed.tone",
        label: "Tone",
        unit: "Hz",
        min: 4000.0,
        max: 12000.0,
        curve: Curve::Exponential,
        default: 7000.0,
        help: "Brightness of the hat. Higher is crisper and thinner.",
    },
    ParamSpec {
        id: "hat_closed.decay",
        label: "Decay",
        unit: "ms",
        min: 10.0,
        max: 200.0,
        curve: Curve::Exponential,
        default: 45.0,
        help: "How long the hat rings. Keep it short so fast 16ths stay tight.",
    },
    ParamSpec {
        id: "hat_closed.metal",
        label: "Metal",
        unit: "%",
        min: 0.0,
        max: 100.0,
        curve: Curve::Linear,
        default: 40.0,
        help: "Blend of metallic ring and plain noise. More metal sounds like a real cymbal.",
    },
];

/// User-facing parameters of the open [`Hat`].
pub const HAT_OPEN_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "hat_open.tone",
        label: "Tone",
        unit: "Hz",
        min: 4000.0,
        max: 12000.0,
        curve: Curve::Exponential,
        default: 6500.0,
        help: "Brightness of the open hat. Higher is crisper and thinner.",
    },
    ParamSpec {
        id: "hat_open.decay",
        label: "Decay",
        unit: "ms",
        min: 100.0,
        max: 1500.0,
        curve: Curve::Exponential,
        default: 400.0,
        help: "How long the open hat hisses. Long tails fill the gap between kicks.",
    },
    ParamSpec {
        id: "hat_open.metal",
        label: "Metal",
        unit: "%",
        min: 0.0,
        max: 100.0,
        curve: Curve::Linear,
        default: 50.0,
        help: "Blend of metallic ring and plain noise. More metal sounds like a real cymbal.",
    },
];

/// Which hat a [`Hat`] voice is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HatKind {
    Closed,
    Open,
}

const METAL_FREQS: [f64; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
/// Fixed makeup gains. Measured default peaks (50 seeds): closed 0.84, open 0.82 at 1.0.
const CLOSED_MAKEUP: f64 = 1.0;
const OPEN_MAKEUP: f64 = 1.0;
const ACCENT_TONE_RATIO: f64 = 1.25;
/// Stop rendering once the envelope is below this (-120 dB).
const SILENCE_AMP: f64 = 1e-6;

/// Synthesized closed or open hi-hat voice.
#[derive(Clone, Debug)]
pub struct Hat {
    kind: HatKind,
    sample_rate: f64,
    tone: f64,
    decay_s: f64,
    metal: f64,
    filter: BiquadFilter,
    noise: Noise,
    phases: [f64; 6],
    active: bool,
    accent: bool,
    t: f64,
}

impl Hat {
    /// Creates a silent hat at `sample_rate` Hz with the kind's defaults.
    pub fn new(kind: HatKind, sample_rate: f64) -> Self {
        let mut filter = BiquadFilter::new();
        filter.set_sample_rate(sample_rate);
        filter.set_mode(BiquadMode::Highpass12);
        let mut h = Self {
            kind,
            sample_rate,
            tone: 7000.0,
            decay_s: 0.045,
            metal: 0.4,
            filter,
            noise: Noise::new(0),
            phases: [0.0; 6],
            active: false,
            accent: false,
            t: 0.0,
        };
        for (i, spec) in h.params().iter().enumerate() {
            h.set_param_at(i, spec.default);
        }
        h
    }

    /// Parameter table of this hat's kind.
    pub fn params(&self) -> &'static [ParamSpec] {
        match self.kind {
            HatKind::Closed => HAT_CLOSED_PARAMS,
            HatKind::Open => HAT_OPEN_PARAMS,
        }
    }

    /// Starts a hit: oscillator phases and time restart, noise reseeded with `seed`.
    pub fn trigger(&mut self, accent: bool, seed: u64) {
        self.accent = accent;
        self.t = 0.0;
        self.phases = [0.0; 6];
        self.noise.reseed(seed);
        self.filter.reset();
        self.apply_tone();
        self.active = true;
    }

    /// Next output sample; exactly 0.0 while silent.
    pub fn get_sample(&mut self) -> f64 {
        if !self.active {
            return 0.0;
        }
        let amp = 10f64.powf(-3.0 * self.t / self.decay_s);
        if amp < SILENCE_AMP {
            self.active = false;
            return 0.0;
        }
        let mut bank = 0.0;
        for (phase, freq) in self.phases.iter_mut().zip(METAL_FREQS) {
            bank += if *phase < 0.5 { 1.0 } else { -1.0 };
            *phase = (*phase + freq / self.sample_rate).fract();
        }
        bank /= METAL_FREQS.len() as f64;
        let src = self.metal * bank + (1.0 - self.metal) * self.noise.sample();
        let makeup = match self.kind {
            HatKind::Closed => CLOSED_MAKEUP,
            HatKind::Open => OPEN_MAKEUP,
        };
        let mut y = self.filter.get_sample(src) * amp * makeup;
        if self.accent {
            y *= db2amp(ACCENT_GAIN_DB);
        }
        self.t += 1.0 / self.sample_rate;
        y
    }

    /// Points the live filter at `tone` (x1.25 when accented, capped at 0.45 sr).
    /// Recomputes coefficients only when the frequency changes.
    fn apply_tone(&mut self) {
        let ratio = if self.accent { ACCENT_TONE_RATIO } else { 1.0 };
        let hz = (self.tone * ratio).min(0.45 * self.sample_rate);
        if hz != self.filter.get_frequency() {
            self.filter.set_frequency(hz);
        }
    }

    /// Silences the voice until the next trigger. Parameters stay.
    pub fn reset(&mut self) {
        self.active = false;
        self.t = 0.0;
        self.phases = [0.0; 6];
        self.filter.reset();
    }

    /// Sets a parameter in plain units (see [`Hat::params`]).
    pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError> {
        let (index, spec) = find_spec(self.params(), id)?;
        spec.check(plain)?;
        self.set_param_at(index, plain);
        Ok(())
    }

    /// Sets the parameter at `index` of [`Hat::params`] in plain units,
    /// clamped to its range (NaN is treated as the minimum).
    ///
    /// # Panics
    /// If `index` is out of range of [`Hat::params`].
    pub fn set_param_at(&mut self, index: usize, plain: f64) {
        let spec = &self.params()[index];
        let plain = if plain.is_nan() {
            spec.min
        } else {
            plain.clamp(spec.min, spec.max)
        };
        match spec.id {
            "hat_closed.tone" | "hat_open.tone" => {
                self.tone = plain;
                self.apply_tone();
            }
            "hat_closed.decay" | "hat_open.decay" => self.decay_s = plain / 1000.0,
            "hat_closed.metal" | "hat_open.metal" => self.metal = plain / 100.0,
            id => unreachable!("hat param id `{id}` has no setter"),
        }
    }
}
