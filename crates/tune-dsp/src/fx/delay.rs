//! Tempo-synced stereo ping-pong delay with a filter in the feedback path.

use super::biquad::{Biquad, BiquadKind};
use crate::param::{Curve, ParamError, ParamSpec, find_spec};

/// Longest echo spacing, in 16th notes. The buffer is sized for it.
pub const DELAY_MAX_STEPS: f64 = 8.0;

/// User-facing parameters of [`Delay`], in plain units.
pub const DELAY_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "delay.time",
        label: "Time",
        unit: "16ths",
        min: 1.0,
        max: DELAY_MAX_STEPS,
        curve: Curve::Linear,
        default: 3.0,
        help: "Echo spacing in 16th notes. 3 is the dotted eighth that makes acid lines roll; 2 is an eighth, 4 a beat.",
    },
    ParamSpec {
        id: "delay.feedback",
        label: "Feedback",
        unit: "%",
        min: 0.0,
        max: 90.0,
        curve: Curve::Linear,
        default: 35.0,
        help: "How many repeats. Above 70 % the echoes pile up.",
    },
    ParamSpec {
        id: "delay.low_cut",
        label: "Low cut",
        unit: "Hz",
        min: 20.0,
        max: 2000.0,
        curve: Curve::Exponential,
        default: 250.0,
        help: "Keeps the repeats out of the kick's low end. 20 Hz is off.",
    },
    ParamSpec {
        id: "delay.high_cut",
        label: "High cut",
        unit: "Hz",
        min: 1000.0,
        max: 20000.0,
        curve: Curve::Exponential,
        default: 6000.0,
        help: "Darkens each repeat, so echoes sit behind the dry line. 20 kHz is off.",
    },
    ParamSpec {
        id: "delay.width",
        label: "Width",
        unit: "%",
        min: 0.0,
        max: 100.0,
        curve: Curve::Linear,
        default: 100.0,
        help: "0 echoes in the middle; 100 bounces left and right (ping-pong).",
    },
];

const TIME: usize = 0;
const FEEDBACK: usize = 1;
const LOW_CUT: usize = 2;
const HIGH_CUT: usize = 3;
const WIDTH: usize = 4;

const LOW_CUT_OFF: f64 = 20.0;
const HIGH_CUT_OFF: f64 = 20000.0;
const Q: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// Mono send in, stereo return out. Two delay lines with linear-interpolated
/// reads, cross-fed by `width`, each repeat passing a highpass and a lowpass.
///
/// Both lines are allocated in [`Delay::new`]; `process` never allocates.
#[derive(Clone, Debug)]
pub struct Delay {
    sample_rate: f64,
    samples_per_step: f64,
    values: [f64; 5],
    line_l: Vec<f64>,
    line_r: Vec<f64>,
    write: usize,
    high_pass: [Biquad; 2],
    low_pass: [Biquad; 2],
    hp_on: bool,
    lp_on: bool,
}

impl Delay {
    /// Allocates two lines of `ceil(DELAY_MAX_STEPS * samples_per_step) + 2`
    /// frames: build off the audio thread.
    pub fn new(sample_rate: f64, samples_per_step: f64) -> Self {
        let capacity = ((DELAY_MAX_STEPS * samples_per_step).ceil().max(0.0) as usize) + 2;
        let mut values = [0.0; 5];
        for (v, spec) in values.iter_mut().zip(DELAY_PARAMS) {
            *v = spec.default;
        }
        let mut d = Self {
            sample_rate,
            samples_per_step,
            values,
            line_l: vec![0.0; capacity],
            line_r: vec![0.0; capacity],
            write: 0,
            high_pass: [Biquad::new(), Biquad::new()],
            low_pass: [Biquad::new(), Biquad::new()],
            hp_on: false,
            lp_on: false,
        };
        d.update_hp();
        d.update_lp();
        d
    }

    /// The step length this delay was built for.
    pub fn samples_per_step(&self) -> f64 {
        self.samples_per_step
    }

    /// Sets a parameter in plain units (see [`DELAY_PARAMS`]).
    pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError> {
        let (index, spec) = find_spec(DELAY_PARAMS, id)?;
        spec.check(plain)?;
        self.set_param_at(index, plain);
        Ok(())
    }

    /// Sets the parameter at `index` of [`DELAY_PARAMS`] in plain units,
    /// clamped to its range (NaN is treated as the minimum).
    ///
    /// # Panics
    /// If `index` is out of range of [`DELAY_PARAMS`].
    pub fn set_param_at(&mut self, index: usize, plain: f64) {
        let spec = &DELAY_PARAMS[index];
        let plain = if plain.is_nan() {
            spec.min
        } else {
            plain.clamp(spec.min, spec.max)
        };
        if self.values[index] == plain {
            return;
        }
        self.values[index] = plain;
        match index {
            LOW_CUT => self.update_hp(),
            HIGH_CUT => self.update_lp(),
            _ => {}
        }
    }

    fn update_hp(&mut self) {
        let hz = self.values[LOW_CUT];
        let on = hz > LOW_CUT_OFF;
        if on {
            if !self.hp_on {
                self.high_pass.iter_mut().for_each(Biquad::reset);
            }
            for b in &mut self.high_pass {
                b.set(BiquadKind::Highpass, hz, Q, self.sample_rate);
            }
        }
        self.hp_on = on;
    }

    fn update_lp(&mut self) {
        let hz = self.values[HIGH_CUT];
        let on = hz < HIGH_CUT_OFF;
        if on {
            if !self.lp_on {
                self.low_pass.iter_mut().for_each(Biquad::reset);
            }
            for b in &mut self.low_pass {
                b.set(BiquadKind::Lowpass, hz, Q, self.sample_rate);
            }
        }
        self.lp_on = on;
    }

    /// Clears the lines and filter states. Parameters stay.
    pub fn reset(&mut self) {
        self.line_l.fill(0.0);
        self.line_r.fill(0.0);
        self.write = 0;
        self.high_pass.iter_mut().for_each(Biquad::reset);
        self.low_pass.iter_mut().for_each(Biquad::reset);
    }

    #[inline]
    fn read(line: &[f64], write: usize, whole: usize, frac: f64) -> f64 {
        let len = line.len();
        let a = line[(write + len - whole) % len];
        let b = line[(write + len - whole - 1) % len];
        (1.0 - frac) * a + frac * b
    }

    #[inline]
    fn filter(&mut self, ch: usize, x: f64) -> f64 {
        let mut y = x;
        if self.hp_on {
            y = self.high_pass[ch].process(y);
        }
        if self.lp_on {
            y = self.low_pass[ch].process(y);
        }
        y
    }

    /// One frame: mono send in, stereo return out.
    #[inline]
    pub fn process(&mut self, x: f64) -> (f64, f64) {
        let len = self.line_l.len();
        let t = (self.values[TIME] * self.samples_per_step).clamp(1.0, (len - 2) as f64);
        let whole = t.floor() as usize;
        let frac = t - whole as f64;
        let dl = Self::read(&self.line_l, self.write, whole, frac);
        let dr = Self::read(&self.line_r, self.write, whole, frac);
        let w = self.values[WIDTH] / 100.0;
        let a = 1.0 - w;
        let fb = self.values[FEEDBACK] / 100.0;
        let in_l = x + fb * (a * dl + w * dr);
        let in_r = a * x + fb * (a * dr + w * dl);
        self.line_l[self.write] = self.filter(0, in_l);
        self.line_r[self.write] = self.filter(1, in_r);
        self.write = (self.write + 1) % len;
        (dl, dr)
    }
}
