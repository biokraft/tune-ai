//! Four-stage track EQ: low cut, mid peak, high shelf, high cut.

use super::biquad::{Biquad, BiquadKind};
use crate::param::{Curve, ParamError, ParamSpec, find_spec};

/// User-facing parameters of [`Equalizer`], in plain units.
pub const EQ_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "eq.low_cut",
        label: "Low cut",
        unit: "Hz",
        min: 20.0,
        max: 1000.0,
        curve: Curve::Exponential,
        default: 20.0,
        help: "Removes everything below this (24 dB/oct). 20 Hz is off. On the 303 try 85 Hz so the kick owns the sub.",
    },
    ParamSpec {
        id: "eq.mid_freq",
        label: "Mid freq",
        unit: "Hz",
        min: 100.0,
        max: 8000.0,
        curve: Curve::Exponential,
        default: 1000.0,
        help: "Centre of the mid band.",
    },
    ParamSpec {
        id: "eq.mid_gain",
        label: "Mid",
        unit: "dB",
        min: -18.0,
        max: 18.0,
        curve: Curve::Linear,
        default: 0.0,
        help: "Boost or cut around Mid freq. Cut 200-300 Hz to clear mud; boost near 1 kHz to bring the squelch forward. 0 is off.",
    },
    ParamSpec {
        id: "eq.mid_q",
        label: "Mid Q",
        unit: "",
        min: 0.3,
        max: 8.0,
        curve: Curve::Exponential,
        default: 1.0,
        help: "Width of the mid band. Higher is narrower.",
    },
    ParamSpec {
        id: "eq.high_gain",
        label: "High",
        unit: "dB",
        min: -18.0,
        max: 18.0,
        curve: Curve::Linear,
        default: 0.0,
        help: "Shelf above 8 kHz: air and sizzle up, or harshness down. 0 is off.",
    },
    ParamSpec {
        id: "eq.high_cut",
        label: "High cut",
        unit: "Hz",
        min: 1000.0,
        max: 20000.0,
        curve: Curve::Exponential,
        default: 20000.0,
        help: "Removes everything above this (12 dB/oct). 20 kHz is off. Tames distortion fizz.",
    },
];

const LOW_CUT: usize = 0;
const MID_FREQ: usize = 1;
const MID_GAIN: usize = 2;
const MID_Q: usize = 3;
const HIGH_GAIN: usize = 4;
const HIGH_CUT: usize = 5;

const LOW_CUT_OFF: f64 = 20.0;
const HIGH_CUT_OFF: f64 = 20000.0;
const SHELF_HZ: f64 = 8000.0;
const BUTTERWORTH_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// Q of the two highpass sections of a 4th-order Butterworth.
const LOW_CUT_Q: [f64; 2] = [0.541196, 1.306563];

/// Low cut (24 dB/oct), mid peak, high shelf, high cut (12 dB/oct), in that
/// order. A stage at its off value is skipped, so the defaults pass the input
/// through bit for bit. Coefficients are recomputed only when a stage's inputs
/// change.
#[derive(Clone, Debug)]
pub struct Equalizer {
    sample_rate: f64,
    values: [f64; 6],
    /// Stage needs its coefficients recomputed: low cut, mid, high shelf, high cut.
    dirty: [bool; 4],
    active: [bool; 4],
    low: [Biquad; 2],
    mid: Biquad,
    shelf: Biquad,
    high: Biquad,
}

impl Equalizer {
    /// Creates an EQ at `sample_rate` Hz with every stage bypassed.
    pub fn new(sample_rate: f64) -> Self {
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(EQ_PARAMS) {
            *v = spec.default;
        }
        Self {
            sample_rate,
            values,
            dirty: [false; 4],
            active: [false; 4],
            low: [Biquad::new(), Biquad::new()],
            mid: Biquad::new(),
            shelf: Biquad::new(),
            high: Biquad::new(),
        }
    }

    /// Sets a parameter in plain units (see [`EQ_PARAMS`]).
    pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError> {
        let (index, spec) = find_spec(EQ_PARAMS, id)?;
        spec.check(plain)?;
        self.set_param_at(index, plain);
        Ok(())
    }

    /// Sets the parameter at `index` of [`EQ_PARAMS`] in plain units, clamped
    /// to its range (NaN is treated as the minimum).
    ///
    /// # Panics
    /// If `index` is out of range of [`EQ_PARAMS`].
    pub fn set_param_at(&mut self, index: usize, plain: f64) {
        let spec = &EQ_PARAMS[index];
        let plain = if plain.is_nan() {
            spec.min
        } else {
            plain.clamp(spec.min, spec.max)
        };
        if self.values[index] == plain {
            return;
        }
        self.values[index] = plain;
        let stage = match index {
            LOW_CUT => 0,
            MID_FREQ | MID_GAIN | MID_Q => 1,
            HIGH_GAIN => 2,
            _ => 3,
        };
        self.dirty[stage] = true;
    }

    /// Clears the filter states. Parameters stay.
    pub fn reset(&mut self) {
        for b in &mut self.low {
            b.reset();
        }
        self.mid.reset();
        self.shelf.reset();
        self.high.reset();
    }

    fn update(&mut self) {
        let sr = self.sample_rate;
        let v = self.values;
        if self.dirty[0] {
            let on = v[LOW_CUT] > LOW_CUT_OFF;
            if on {
                if !self.active[0] {
                    self.low.iter_mut().for_each(Biquad::reset);
                }
                for (b, q) in self.low.iter_mut().zip(LOW_CUT_Q) {
                    b.set(BiquadKind::Highpass, v[LOW_CUT], q, sr);
                }
            }
            self.active[0] = on;
        }
        if self.dirty[1] {
            let on = v[MID_GAIN] != 0.0;
            if on {
                if !self.active[1] {
                    self.mid.reset();
                }
                let kind = BiquadKind::Peak {
                    gain_db: v[MID_GAIN],
                };
                self.mid.set(kind, v[MID_FREQ], v[MID_Q], sr);
            }
            self.active[1] = on;
        }
        if self.dirty[2] {
            let on = v[HIGH_GAIN] != 0.0;
            if on {
                if !self.active[2] {
                    self.shelf.reset();
                }
                let kind = BiquadKind::HighShelf {
                    gain_db: v[HIGH_GAIN],
                };
                self.shelf.set(kind, SHELF_HZ, BUTTERWORTH_Q, sr);
            }
            self.active[2] = on;
        }
        if self.dirty[3] {
            let on = v[HIGH_CUT] < HIGH_CUT_OFF;
            if on {
                if !self.active[3] {
                    self.high.reset();
                }
                self.high
                    .set(BiquadKind::Lowpass, v[HIGH_CUT], BUTTERWORTH_Q, sr);
            }
            self.active[3] = on;
        }
        self.dirty = [false; 4];
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        if self.dirty.iter().any(|&d| d) {
            self.update();
        }
        let mut y = x;
        if self.active[0] {
            y = self.low[0].process(y);
            y = self.low[1].process(y);
        }
        if self.active[1] {
            y = self.mid.process(y);
        }
        if self.active[2] {
            y = self.shelf.process(y);
        }
        if self.active[3] {
            y = self.high.process(y);
        }
        y
    }
}
