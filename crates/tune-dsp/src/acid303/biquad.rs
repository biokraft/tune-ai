//! Port of `rosic_BiquadFilter`.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

use super::math::{PI, TINY, asinh, db2amp, sin_cos};

/// `BiquadFilter::modes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum BiquadMode {
    Bypass = 0,
    Lowpass6,
    Lowpass12,
    Highpass6,
    Highpass12,
    Bandpass,
    Bandreject,
    Peak,
    LowShelf,
}

/// `rosic::BiquadFilter`.
#[derive(Clone, Debug)]
pub struct BiquadFilter {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
    frequency: f64,
    gain: f64,
    bandwidth: f64,
    sample_rate: f64,
    mode: BiquadMode,
}

impl Default for BiquadFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl BiquadFilter {
    pub fn new() -> Self {
        let mut s = Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            frequency: 1000.0,
            gain: 0.0,
            bandwidth: 2.0 * asinh(1.0 / 2.0_f64.sqrt()) / 2.0_f64.ln(),
            sample_rate: 44100.0,
            mode: BiquadMode::Bypass,
        };
        s.calc_coeffs();
        s.reset();
        s
    }

    pub fn set_sample_rate(&mut self, new_sample_rate: f64) {
        if new_sample_rate > 0.0 {
            self.sample_rate = new_sample_rate;
        }
        self.calc_coeffs();
    }

    pub fn set_mode(&mut self, new_mode: BiquadMode) {
        self.mode = new_mode;
        self.calc_coeffs();
    }

    pub fn set_frequency(&mut self, new_frequency: f64) {
        self.frequency = new_frequency;
        self.calc_coeffs();
    }

    pub fn set_gain(&mut self, new_gain: f64) {
        self.gain = new_gain;
        self.calc_coeffs();
    }

    pub fn set_bandwidth(&mut self, new_bandwidth: f64) {
        self.bandwidth = new_bandwidth;
        self.calc_coeffs();
    }

    pub fn get_mode(&self) -> BiquadMode {
        self.mode
    }

    pub fn get_frequency(&self) -> f64 {
        self.frequency
    }

    pub fn get_gain(&self) -> f64 {
        self.gain
    }

    pub fn get_bandwidth(&self) -> f64 {
        self.bandwidth
    }

    #[inline]
    pub fn get_sample(&mut self, input: f64) -> f64 {
        let y = self.b0 * input
            + self.b1 * self.x1
            + self.b2 * self.x2
            + self.a1 * self.y1
            + self.a2 * self.y2
            + TINY;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    fn calc_coeffs(&mut self) {
        let w = 2.0 * PI * self.frequency / self.sample_rate;
        match self.mode {
            BiquadMode::Lowpass6 => {
                let x = (-w).exp();
                self.a1 = x;
                self.a2 = 0.0;
                self.b0 = 1.0 - x;
                self.b1 = 0.0;
                self.b2 = 0.0;
            }
            BiquadMode::Lowpass12 => {
                let (s, c) = sin_cos(w);
                let q = db2amp(self.gain);
                let alpha = s / (2.0 * q);
                let scale = 1.0 / (1.0 + alpha);
                self.a1 = 2.0 * c * scale;
                self.a2 = (alpha - 1.0) * scale;
                self.b1 = (1.0 - c) * scale;
                self.b0 = 0.5 * self.b1;
                self.b2 = self.b0;
            }
            BiquadMode::Highpass6 => {
                let x = (-w).exp();
                self.a1 = x;
                self.a2 = 0.0;
                self.b0 = 0.5 * (1.0 + x);
                self.b1 = -self.b0;
                self.b2 = 0.0;
            }
            BiquadMode::Highpass12 => {
                let (s, c) = sin_cos(w);
                let q = db2amp(self.gain);
                let alpha = s / (2.0 * q);
                let scale = 1.0 / (1.0 + alpha);
                self.a1 = 2.0 * c * scale;
                self.a2 = (alpha - 1.0) * scale;
                self.b1 = -(1.0 + c) * scale;
                self.b0 = -0.5 * self.b1;
                self.b2 = self.b0;
            }
            BiquadMode::Bandpass => {
                let (s, c) = sin_cos(w);
                let alpha = s * (0.5 * LN2_STD * self.bandwidth * w / s).sinh();
                let scale = 1.0 / (1.0 + alpha);
                self.a1 = 2.0 * c * scale;
                self.a2 = (alpha - 1.0) * scale;
                self.b1 = 0.0;
                self.b0 = 0.5 * s * scale;
                self.b2 = -self.b0;
            }
            BiquadMode::Bandreject => {
                let (s, c) = sin_cos(w);
                let alpha = s * (0.5 * LN2_STD * self.bandwidth * w / s).sinh();
                let scale = 1.0 / (1.0 + alpha);
                self.a1 = 2.0 * c * scale;
                self.a2 = (alpha - 1.0) * scale;
                self.b0 = 1.0 * scale;
                self.b1 = -2.0 * c * scale;
                self.b2 = 1.0 * scale;
            }
            BiquadMode::Peak => {
                let (s, c) = sin_cos(w);
                let alpha = s * (0.5 * LN2_STD * self.bandwidth * w / s).sinh();
                let a = db2amp(self.gain);
                let scale = 1.0 / (1.0 + alpha / a);
                self.a1 = 2.0 * c * scale;
                self.a2 = ((alpha / a) - 1.0) * scale;
                self.b0 = (1.0 + alpha * a) * scale;
                self.b1 = -2.0 * c * scale;
                self.b2 = (1.0 - alpha * a) * scale;
            }
            BiquadMode::LowShelf => {
                let (s, c) = sin_cos(w);
                let a = db2amp(0.5 * self.gain);
                let q = 1.0 / (2.0 * (0.5 * LN2_STD * self.bandwidth).sinh());
                let beta = a.sqrt() / q;
                let scale = 1.0 / ((a + 1.0) + (a - 1.0) * c + beta * s);
                self.a1 = 2.0 * ((a - 1.0) + (a + 1.0) * c) * scale;
                self.a2 = -((a + 1.0) + (a - 1.0) * c - beta * s) * scale;
                self.b0 = a * ((a + 1.0) - (a - 1.0) * c + beta * s) * scale;
                self.b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * c) * scale;
                self.b2 = a * ((a + 1.0) - (a - 1.0) * c - beta * s) * scale;
            }
            BiquadMode::Bypass => {
                self.b0 = 1.0;
                self.b1 = 0.0;
                self.b2 = 0.0;
                self.a1 = 0.0;
                self.a2 = 0.0;
            }
        }
    }
}

/// C++ calls `log(2.0)` here, i.e. libm's natural log of two.
const LN2_STD: f64 = std::f64::consts::LN_2;
