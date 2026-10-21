//! Port of `rosic_TeeBeeFilter`.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

use super::math::{ONE_OVER_SQRT2, PI, SQRT2, clip, db2amp, sin_cos};
use super::one_pole::{OnePoleFilter, OnePoleMode};

/// `TeeBeeFilter::modes` (without the `NUM_MODES` sentinel).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum FilterMode {
    Flat = 0,
    Lp6,
    Lp12,
    Lp18,
    Lp24,
    Hp6,
    Hp12,
    Hp18,
    Hp24,
    Bp12_12,
    Bp6_18,
    Bp18_6,
    Bp6_12,
    Bp12_6,
    Bp6_6,
    /// "ala mystran & kunn".
    #[default]
    Tb303,
}

/// `rosic::TeeBeeFilter`: the TB-303 filter emulation.
#[derive(Clone, Debug)]
pub struct TeeBeeFilter {
    b0: f64,
    a1: f64,
    y1: f64,
    y2: f64,
    y3: f64,
    y4: f64,
    c0: f64,
    c1: f64,
    c2: f64,
    c3: f64,
    c4: f64,
    k: f64,
    g: f64,
    drive_factor: f64,
    cutoff: f64,
    drive: f64,
    resonance_raw: f64,
    resonance_skewed: f64,
    sample_rate: f64,
    two_pi_over_sample_rate: f64,
    mode: FilterMode,
    feedback_highpass: OnePoleFilter,
}

impl Default for TeeBeeFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl TeeBeeFilter {
    /// Mirrors the C++ constructor (including its order of calls).
    pub fn new() -> Self {
        let sample_rate = 44100.0;
        let mut s = Self {
            // Uninitialised in C++ until `setMode` / the coefficient calls.
            b0: 0.0,
            a1: 0.0,
            y1: 0.0,
            y2: 0.0,
            y3: 0.0,
            y4: 0.0,
            c0: 0.0,
            c1: 0.0,
            c2: 0.0,
            c3: 0.0,
            c4: 0.0,
            k: 0.0,
            g: 1.0,
            drive_factor: 1.0,
            cutoff: 1000.0,
            drive: 0.0,
            resonance_raw: 0.0,
            resonance_skewed: 0.0,
            sample_rate,
            two_pi_over_sample_rate: 2.0 * PI / sample_rate,
            mode: FilterMode::Tb303,
            feedback_highpass: OnePoleFilter::new(),
        };

        s.feedback_highpass.set_mode(OnePoleMode::Highpass);
        s.feedback_highpass.set_cutoff(150.0);

        s.set_mode(FilterMode::Tb303);
        s.calculate_coefficients_exact();
        s.reset();
        s
    }

    pub fn set_sample_rate(&mut self, new_sample_rate: f64) {
        if new_sample_rate > 0.0 {
            self.sample_rate = new_sample_rate;
        }
        self.two_pi_over_sample_rate = 2.0 * PI / self.sample_rate;
        self.feedback_highpass.set_sample_rate(new_sample_rate);
        self.calculate_coefficients_exact();
    }

    /// `setCutoff(newCutoff, updateCoefficients)`.
    pub fn set_cutoff(&mut self, new_cutoff: f64, update_coefficients: bool) {
        if new_cutoff != self.cutoff {
            if new_cutoff < 200.0 {
                self.cutoff = 200.0;
            } else if new_cutoff > 20000.0 {
                self.cutoff = 20000.0;
            } else {
                self.cutoff = new_cutoff;
            }

            if update_coefficients {
                self.calculate_coefficients_approx4();
            }
        }
    }

    /// `setResonance(newResonance, updateCoefficients)`; 100 % is self oscillation.
    pub fn set_resonance(&mut self, new_resonance: f64, update_coefficients: bool) {
        self.resonance_raw = 0.01 * new_resonance;
        self.resonance_skewed =
            (1.0 - (-3.0 * self.resonance_raw).exp()) / (1.0 - (-3.0_f64).exp());
        if update_coefficients {
            self.calculate_coefficients_approx4();
        }
    }

    /// Input drive in decibels.
    pub fn set_drive(&mut self, new_drive: f64) {
        self.drive = new_drive;
        self.drive_factor = db2amp(self.drive);
    }

    pub fn set_mode(&mut self, new_mode: FilterMode) {
        // The C++ range check (0 <= mode < NUM_MODES) always holds for the enum.
        self.mode = new_mode;
        let (c0, c1, c2, c3, c4) = match self.mode {
            FilterMode::Flat => (1.0, 0.0, 0.0, 0.0, 0.0),
            FilterMode::Lp6 => (0.0, 1.0, 0.0, 0.0, 0.0),
            FilterMode::Lp12 => (0.0, 0.0, 1.0, 0.0, 0.0),
            FilterMode::Lp18 => (0.0, 0.0, 0.0, 1.0, 0.0),
            FilterMode::Lp24 => (0.0, 0.0, 0.0, 0.0, 1.0),
            FilterMode::Hp6 => (1.0, -1.0, 0.0, 0.0, 0.0),
            FilterMode::Hp12 => (1.0, -2.0, 1.0, 0.0, 0.0),
            FilterMode::Hp18 => (1.0, -3.0, 3.0, -1.0, 0.0),
            FilterMode::Hp24 => (1.0, -4.0, 6.0, -4.0, 1.0),
            FilterMode::Bp12_12 => (0.0, 0.0, 1.0, -2.0, 1.0),
            FilterMode::Bp6_18 => (0.0, 0.0, 0.0, 1.0, -1.0),
            FilterMode::Bp18_6 => (0.0, 1.0, -3.0, 3.0, -1.0),
            FilterMode::Bp6_12 => (0.0, 0.0, 1.0, -1.0, 0.0),
            FilterMode::Bp12_6 => (0.0, 1.0, -2.0, 1.0, 0.0),
            FilterMode::Bp6_6 => (0.0, 1.0, -1.0, 0.0, 0.0),
            FilterMode::Tb303 => (1.0, 0.0, 0.0, 0.0, 0.0), // C++ `default:` branch
        };
        self.c0 = c0;
        self.c1 = c1;
        self.c2 = c2;
        self.c3 = c3;
        self.c4 = c4;
        self.calculate_coefficients_approx4();
    }

    /// Cutoff of the highpass in the feedback path.
    pub fn set_feedback_highpass_cutoff(&mut self, new_cutoff: f64) {
        self.feedback_highpass.set_cutoff(new_cutoff);
    }

    pub fn get_cutoff(&self) -> f64 {
        self.cutoff
    }

    pub fn get_resonance(&self) -> f64 {
        100.0 * self.resonance_raw
    }

    pub fn get_drive(&self) -> f64 {
        self.drive
    }

    pub fn get_mode(&self) -> FilterMode {
        self.mode
    }

    pub fn get_feedback_highpass_cutoff(&self) -> f64 {
        self.feedback_highpass.get_cutoff()
    }

    /// Calculates one output sample.
    #[inline]
    pub fn get_sample(&mut self, input: f64) -> f64 {
        let y0;

        if self.mode == FilterMode::Tb303 {
            y0 = input - self.feedback_highpass.get_sample(self.k * self.y4);
            self.y1 += 2.0 * self.b0 * (y0 - self.y1 + self.y2);
            self.y2 += self.b0 * (self.y1 - 2.0 * self.y2 + self.y3);
            self.y3 += self.b0 * (self.y2 - 2.0 * self.y3 + self.y4);
            self.y4 += self.b0 * (self.y3 - 2.0 * self.y4);
            return 2.0 * self.g * self.y4;
        }

        // apply drive and feedback to obtain the filter's input signal:
        y0 =
            0.125 * self.drive_factor * input - self.feedback_highpass.get_sample(self.k * self.y4);

        // cascade of four 1st order sections:
        self.y1 = y0 + self.a1 * (y0 - self.y1);
        self.y2 = self.y1 + self.a1 * (self.y1 - self.y2);
        self.y3 = self.y2 + self.a1 * (self.y2 - self.y3);
        self.y4 = self.y3 + self.a1 * (self.y3 - self.y4);

        8.0 * (self.c0 * y0
            + self.c1 * self.y1
            + self.c2 * self.y2
            + self.c3 * self.y3
            + self.c4 * self.y4)
    }

    /// Re-calculates the coefficients via the exact formulas.
    pub fn calculate_coefficients_exact(&mut self) {
        let wc = self.two_pi_over_sample_rate * self.cutoff;
        let (s, c) = sin_cos(wc);
        let t = (0.25 * (wc - PI)).tan();
        let r = self.resonance_skewed;

        // a1 tuned such that the resonance frequency is just right:
        let a1_full_res = t / (s - c * t);

        // a1 as if there were no resonance:
        let x = (-wc).exp();
        let a1_no_res = -x;

        self.a1 = r * a1_full_res + (1.0 - r) * a1_no_res;
        self.b0 = 1.0 + self.a1;

        let gsq = self.b0 * self.b0 / (1.0 + self.a1 * self.a1 + 2.0 * self.a1 * c);
        self.k = r / (gsq * gsq);

        if self.mode == FilterMode::Tb303 {
            self.k *= 17.0 / 4.0;
        }
    }

    /// Re-calculates the coefficients with a polynomial approximation.
    pub fn calculate_coefficients_approx4(&mut self) {
        let wc = self.two_pi_over_sample_rate * self.cutoff;
        let wc2 = wc * wc;
        let r = self.resonance_skewed;
        let mut tmp;

        const PA12: f64 = -1.341281325101042e-02;
        const PA11: f64 = 8.168739417977708e-02;
        const PA10: f64 = -2.365036766021623e-01;
        const PA09: f64 = 4.439739664918068e-01;
        const PA08: f64 = -6.297350825423579e-01;
        const PA07: f64 = 7.529691648678890e-01;
        const PA06: f64 = -8.249882473764324e-01;
        const PA05: f64 = 8.736418933533319e-01;
        const PA04: f64 = -9.164580250284832e-01;
        const PA03: f64 = 9.583192455599817e-01;
        const PA02: f64 = -9.999994950291231e-01;
        const PA01: f64 = 9.999999927726119e-01;
        const PA00: f64 = -9.999999999857464e-01;
        tmp = wc2 * PA12 + PA11 * wc + PA10;
        tmp = wc2 * tmp + PA09 * wc + PA08;
        tmp = wc2 * tmp + PA07 * wc + PA06;
        tmp = wc2 * tmp + PA05 * wc + PA04;
        tmp = wc2 * tmp + PA03 * wc + PA02;
        self.a1 = wc2 * tmp + PA01 * wc + PA00;
        self.b0 = 1.0 + self.a1;

        const PR8: f64 = -4.554677015609929e-05;
        const PR7: f64 = -2.022131730719448e-05;
        const PR6: f64 = 2.784706718370008e-03;
        const PR5: f64 = 2.079921151733780e-03;
        const PR4: f64 = -8.333236384240325e-02;
        const PR3: f64 = -1.666668203490468e-01;
        const PR2: f64 = 1.000000012124230e+00;
        const PR1: f64 = 3.999999999650040e+00;
        const PR0: f64 = 4.000000000000113e+00;
        tmp = wc2 * PR8 + PR7 * wc + PR6;
        tmp = wc2 * tmp + PR5 * wc + PR4;
        tmp = wc2 * tmp + PR3 * wc + PR2;
        tmp = wc2 * tmp + PR1 * wc + PR0; // the scale factor
        self.k = r * tmp;
        self.g = 1.0;

        if self.mode == FilterMode::Tb303 {
            let fx = wc * ONE_OVER_SQRT2 / (2.0 * PI);
            self.b0 =
                (0.00045522346 + 6.1922189 * fx) / (1.0 + 12.358354 * fx + 4.4156345 * (fx * fx));
            self.k = fx
                * (fx * (fx * (fx * (fx * (fx + 7198.6997) - 5837.7917) - 476.47308) + 614.95611)
                    + 213.87126)
                + 16.998792;
            self.g = self.k * 0.058823529411764705882352941176471; // 17 reciprocal
            self.g = (self.g - 1.0) * r + 1.0; // r is 0 to 1.0
            self.g *= 1.0 + r;
            self.k *= r; // k is ready now
        }
    }

    /// The waveshaping nonlinearity between the stages.
    pub fn shape(&self, x: f64) -> f64 {
        let r6 = 1.0 / 6.0;
        let x = clip(x, -SQRT2, SQRT2);
        x - r6 * x * x * x
    }

    pub fn reset(&mut self) {
        self.feedback_highpass.reset();
        self.y1 = 0.0;
        self.y2 = 0.0;
        self.y3 = 0.0;
        self.y4 = 0.0;
    }
}
