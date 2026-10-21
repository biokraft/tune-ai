//! Port of `rosic_OnePoleFilter`.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

use super::math::{PI, TINY, db2amp};

/// `OnePoleFilter::modes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum OnePoleMode {
    Bypass = 0,
    Lowpass,
    Highpass,
    Lowshelv,
    Highshelv,
    Allpass,
}

/// `rosic::OnePoleFilter`.
#[derive(Clone, Debug)]
pub struct OnePoleFilter {
    x1: f64,
    y1: f64,
    b0: f64,
    b1: f64,
    a1: f64,
    cutoff: f64,
    shelving_gain: f64,
    mode: OnePoleMode,
    sample_rate: f64,
    sample_rate_rec: f64,
}

impl Default for OnePoleFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl OnePoleFilter {
    /// Mirrors the C++ constructor: 44100 Hz, bypass, cutoff 20000 Hz. (The
    /// C++ calls `calcCoeffs` before `mode` and `cutoff` are set; the final
    /// state after the three setters is what matters and is reproduced here.)
    pub fn new() -> Self {
        let mut s = Self {
            x1: 0.0,
            y1: 0.0,
            b0: 1.0,
            b1: 0.0,
            a1: 0.0,
            cutoff: 20000.0,
            shelving_gain: 1.0,
            mode: OnePoleMode::Bypass,
            sample_rate: 44100.0,
            sample_rate_rec: 1.0 / 44100.0,
        };
        s.set_sample_rate(44100.0);
        s.set_mode(OnePoleMode::Bypass);
        s.set_cutoff(20000.0);
        s.reset();
        s
    }

    pub fn set_sample_rate(&mut self, new_sample_rate: f64) {
        if new_sample_rate > 0.0 {
            self.sample_rate = new_sample_rate;
        }
        self.sample_rate_rec = 1.0 / self.sample_rate;
        self.calc_coeffs();
    }

    pub fn set_mode(&mut self, new_mode: OnePoleMode) {
        self.mode = new_mode;
        self.calc_coeffs();
    }

    pub fn set_cutoff(&mut self, new_cutoff: f64) {
        if new_cutoff > 0.0 && new_cutoff <= 20000.0 {
            self.cutoff = new_cutoff;
        } else {
            self.cutoff = 20000.0;
        }
        self.calc_coeffs();
    }

    pub fn set_lowpass_time_constant(&mut self, new_time_constant: f64) {
        self.set_cutoff(1.0 / (2.0 * PI * new_time_constant));
    }

    /// Linear shelving gain; non-positive values are ignored (the C++ hits
    /// `DEBUG_BREAK` and leaves the state alone).
    pub fn set_shelving_gain(&mut self, new_gain: f64) {
        if new_gain > 0.0 {
            self.shelving_gain = new_gain;
            self.calc_coeffs();
        }
    }

    pub fn set_shelving_gain_in_decibels(&mut self, new_gain: f64) {
        self.set_shelving_gain(db2amp(new_gain));
    }

    pub fn set_coefficients(&mut self, new_b0: f64, new_b1: f64, new_a1: f64) {
        self.b0 = new_b0;
        self.b1 = new_b1;
        self.a1 = new_a1;
    }

    pub fn set_internal_state(&mut self, new_x1: f64, new_y1: f64) {
        self.x1 = new_x1;
        self.y1 = new_y1;
    }

    pub fn get_cutoff(&self) -> f64 {
        self.cutoff
    }

    #[inline]
    pub fn get_sample(&mut self, input: f64) -> f64 {
        self.y1 = self.b0 * input + self.b1 * self.x1 + self.a1 * self.y1 + TINY;
        self.x1 = input;
        self.y1
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }

    fn calc_coeffs(&mut self) {
        match self.mode {
            OnePoleMode::Lowpass => {
                let x = (-2.0 * PI * self.cutoff * self.sample_rate_rec).exp();
                self.b0 = 1.0 - x;
                self.b1 = 0.0;
                self.a1 = x;
            }
            OnePoleMode::Highpass => {
                let x = (-2.0 * PI * self.cutoff * self.sample_rate_rec).exp();
                self.b0 = 0.5 * (1.0 + x);
                self.b1 = -0.5 * (1.0 + x);
                self.a1 = x;
            }
            OnePoleMode::Lowshelv => {
                let c = 0.5 * (self.shelving_gain - 1.0);
                let t = (PI * self.cutoff * self.sample_rate_rec).tan();
                let a = if self.shelving_gain >= 1.0 {
                    (t - 1.0) / (t + 1.0)
                } else {
                    (t - self.shelving_gain) / (t + self.shelving_gain)
                };
                self.b0 = 1.0 + c + c * a;
                self.b1 = c + c * a + a;
                self.a1 = -a;
            }
            OnePoleMode::Highshelv => {
                let c = 0.5 * (self.shelving_gain - 1.0);
                let t = (PI * self.cutoff * self.sample_rate_rec).tan();
                let a = if self.shelving_gain >= 1.0 {
                    (t - 1.0) / (t + 1.0)
                } else {
                    (self.shelving_gain * t - 1.0) / (self.shelving_gain * t + 1.0)
                };
                self.b0 = 1.0 + c - c * a;
                self.b1 = a + c * a - c;
                self.a1 = -a;
            }
            OnePoleMode::Allpass => {
                let t = (PI * self.cutoff * self.sample_rate_rec).tan();
                let x = (t - 1.0) / (t + 1.0);
                self.b0 = x;
                self.b1 = 1.0;
                self.a1 = -x;
            }
            OnePoleMode::Bypass => {
                self.b0 = 1.0;
                self.b1 = 0.0;
                self.a1 = 0.0;
            }
        }
    }
}
