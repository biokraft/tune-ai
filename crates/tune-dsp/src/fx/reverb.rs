//! Short stereo reverb: Freeverb topology (public domain), implemented from
//! its published description.
//!
//! Eight damped feedback combs in parallel feed four series allpasses per
//! channel; the right channel's lines are 23 samples (at 44.1 kHz) longer for
//! a decorrelated image. A pre-delay and a highpass sit in front of the
//! network. Nothing is random: two instances fed the same input agree bitwise.

use super::biquad::{Biquad, BiquadKind};
use crate::param::{Curve, ParamError, ParamSpec, find_spec};

/// User-facing parameters of [`Reverb`], in plain units.
pub const REVERB_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "reverb.size",
        label: "Size",
        unit: "%",
        min: 0.0,
        max: 100.0,
        curve: Curve::Linear,
        default: 25.0,
        help: "Room size: how long the tail rings. Keep it small so the groove stays tight.",
    },
    ParamSpec {
        id: "reverb.damp",
        label: "Damp",
        unit: "%",
        min: 0.0,
        max: 100.0,
        curve: Curve::Linear,
        default: 50.0,
        help: "Darkens the tail, like a room with soft walls.",
    },
    ParamSpec {
        id: "reverb.pre_delay",
        label: "Pre-delay",
        unit: "ms",
        min: 0.0,
        max: 100.0,
        curve: Curve::Linear,
        default: 0.0,
        help: "Gap before the tail starts, so the dry hit stays clear.",
    },
    ParamSpec {
        id: "reverb.low_cut",
        label: "Low cut",
        unit: "Hz",
        min: 20.0,
        max: 2000.0,
        curve: Curve::Exponential,
        default: 400.0,
        help: "Keeps the tail out of the low end. 20 Hz is off.",
    },
    ParamSpec {
        id: "reverb.width",
        label: "Width",
        unit: "%",
        min: 0.0,
        max: 100.0,
        curve: Curve::Linear,
        default: 100.0,
        help: "0 is a mono tail; 100 spreads it wide.",
    },
];

const SIZE: usize = 0;
const DAMP: usize = 1;
const PRE_DELAY: usize = 2;
const LOW_CUT: usize = 3;
const WIDTH: usize = 4;

const LOW_CUT_OFF: f64 = 20.0;
const Q: f64 = std::f64::consts::FRAC_1_SQRT_2;
const INPUT_GAIN: f64 = 0.015;
const MAX_PRE_DELAY_MS: f64 = 100.0;

const COMB_LENGTHS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_LENGTHS: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;
const ALLPASS_FEEDBACK: f64 = 0.5;

#[derive(Clone, Debug)]
struct Comb {
    buf: Vec<f64>,
    index: usize,
    filt: f64,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            index: 0,
            filt: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, input: f64, damp: f64, feedback: f64) -> f64 {
        let out = self.buf[self.index];
        self.filt = out * (1.0 - damp) + self.filt * damp;
        self.buf[self.index] = input + self.filt * feedback;
        self.index += 1;
        if self.index == self.buf.len() {
            self.index = 0;
        }
        out
    }

    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.index = 0;
        self.filt = 0.0;
    }
}

#[derive(Clone, Debug)]
struct Allpass {
    buf: Vec<f64>,
    index: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            index: 0,
        }
    }

    #[inline]
    fn process(&mut self, input: f64) -> f64 {
        let b = self.buf[self.index];
        let out = -input + b;
        self.buf[self.index] = input + b * ALLPASS_FEEDBACK;
        self.index += 1;
        if self.index == self.buf.len() {
            self.index = 0;
        }
        out
    }

    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.index = 0;
    }
}

#[derive(Clone, Debug)]
struct Channel {
    combs: Vec<Comb>,
    allpasses: Vec<Allpass>,
}

impl Channel {
    fn new(sample_rate: f64, extra: usize) -> Self {
        let scale = |len: usize| ((len + extra) as f64 * sample_rate / 44100.0).round() as usize;
        Self {
            combs: COMB_LENGTHS.iter().map(|&l| Comb::new(scale(l))).collect(),
            allpasses: ALLPASS_LENGTHS
                .iter()
                .map(|&l| Allpass::new(scale(l)))
                .collect(),
        }
    }

    #[inline]
    fn process(&mut self, input: f64, damp: f64, feedback: f64) -> f64 {
        let mut sum = 0.0;
        for c in &mut self.combs {
            sum += c.process(input, damp, feedback);
        }
        for a in &mut self.allpasses {
            sum = a.process(sum);
        }
        sum
    }

    fn reset(&mut self) {
        self.combs.iter_mut().for_each(Comb::reset);
        self.allpasses.iter_mut().for_each(Allpass::reset);
    }
}

/// Mono send in, stereo return out.
///
/// Every line is allocated in [`Reverb::new`]; `process` never allocates.
#[derive(Clone, Debug)]
pub struct Reverb {
    sample_rate: f64,
    values: [f64; 5],
    pre: Vec<f64>,
    pre_write: usize,
    pre_frames: usize,
    high_pass: Biquad,
    hp_on: bool,
    left: Channel,
    right: Channel,
    feedback: f64,
    damp: f64,
    wet1: f64,
    wet2: f64,
}

impl Reverb {
    /// Allocates all lines, with a pre-delay buffer sized for 100 ms: build
    /// off the audio thread.
    pub fn new(sample_rate: f64) -> Self {
        let mut values = [0.0; 5];
        for (v, spec) in values.iter_mut().zip(REVERB_PARAMS) {
            *v = spec.default;
        }
        let capacity = (MAX_PRE_DELAY_MS * sample_rate / 1000.0).round().max(0.0) as usize + 1;
        let mut r = Self {
            sample_rate,
            values,
            pre: vec![0.0; capacity],
            pre_write: 0,
            pre_frames: 0,
            high_pass: Biquad::new(),
            hp_on: false,
            left: Channel::new(sample_rate, 0),
            right: Channel::new(sample_rate, STEREO_SPREAD),
            feedback: 0.0,
            damp: 0.0,
            wet1: 0.0,
            wet2: 0.0,
        };
        r.update_size();
        r.update_damp();
        r.update_pre();
        r.update_hp();
        r.update_width();
        r
    }

    /// Sets a parameter in plain units (see [`REVERB_PARAMS`]).
    pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError> {
        let (index, spec) = find_spec(REVERB_PARAMS, id)?;
        spec.check(plain)?;
        self.set_param_at(index, plain);
        Ok(())
    }

    /// Sets the parameter at `index` of [`REVERB_PARAMS`] in plain units,
    /// clamped to its range (NaN is treated as the minimum).
    ///
    /// # Panics
    /// If `index` is out of range of [`REVERB_PARAMS`].
    pub fn set_param_at(&mut self, index: usize, plain: f64) {
        let spec = &REVERB_PARAMS[index];
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
            SIZE => self.update_size(),
            DAMP => self.update_damp(),
            PRE_DELAY => self.update_pre(),
            LOW_CUT => self.update_hp(),
            WIDTH => self.update_width(),
            _ => {}
        }
    }

    fn update_size(&mut self) {
        self.feedback = 0.7 + 0.28 * self.values[SIZE] / 100.0;
    }

    fn update_damp(&mut self) {
        self.damp = 0.4 * self.values[DAMP] / 100.0;
    }

    fn update_pre(&mut self) {
        let frames = (self.values[PRE_DELAY] * self.sample_rate / 1000.0).round() as usize;
        self.pre_frames = frames.min(self.pre.len() - 1);
    }

    fn update_hp(&mut self) {
        let hz = self.values[LOW_CUT];
        let on = hz > LOW_CUT_OFF;
        if on {
            if !self.hp_on {
                self.high_pass.reset();
            }
            self.high_pass
                .set(BiquadKind::Highpass, hz, Q, self.sample_rate);
        }
        self.hp_on = on;
    }

    fn update_width(&mut self) {
        let w = self.values[WIDTH];
        self.wet1 = w / 200.0 + 0.5;
        self.wet2 = (1.0 - w / 100.0) / 2.0;
    }

    /// Clears every line and filter state. Parameters stay.
    pub fn reset(&mut self) {
        self.pre.fill(0.0);
        self.pre_write = 0;
        self.high_pass.reset();
        self.left.reset();
        self.right.reset();
    }

    /// One frame: mono send in, stereo return out.
    #[inline]
    pub fn process(&mut self, x: f64) -> (f64, f64) {
        let len = self.pre.len();
        self.pre[self.pre_write] = x;
        let mut y = self.pre[(self.pre_write + len - self.pre_frames) % len];
        self.pre_write = (self.pre_write + 1) % len;
        if self.hp_on {
            y = self.high_pass.process(y);
        }
        let input = y * INPUT_GAIN;
        let ol = self.left.process(input, self.damp, self.feedback);
        let or = self.right.process(input, self.damp, self.feedback);
        (
            ol * self.wet1 + or * self.wet2,
            or * self.wet1 + ol * self.wet2,
        )
    }
}
