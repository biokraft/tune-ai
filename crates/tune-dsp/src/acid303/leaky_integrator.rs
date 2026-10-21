//! Port of `rosic_LeakyIntegrator`: a leaky-integrator lowpass with a time
//! constant given in milliseconds.

/// `rosic::LeakyIntegrator`.
#[derive(Clone, Debug)]
pub struct LeakyIntegrator {
    coeff: f64,
    y1: f64,
    sample_rate: f64,
    tau: f64,
}

impl Default for LeakyIntegrator {
    fn default() -> Self {
        Self::new()
    }
}

impl LeakyIntegrator {
    pub fn new() -> Self {
        let mut s = Self {
            coeff: 0.0,
            y1: 0.0,
            sample_rate: 44100.0,
            tau: 10.0,
        };
        s.calculate_coefficient();
        s
    }

    pub fn set_sample_rate(&mut self, new_sample_rate: f64) {
        if new_sample_rate > 0.0 {
            self.sample_rate = new_sample_rate;
            self.calculate_coefficient();
        }
    }

    /// Sets the time constant (tau) in milliseconds.
    pub fn set_time_constant(&mut self, new_time_constant: f64) {
        if new_time_constant >= 0.0 && new_time_constant != self.tau {
            self.tau = new_time_constant;
            self.calculate_coefficient();
        }
    }

    pub fn set_state(&mut self, new_state: f64) {
        self.y1 = new_state;
    }

    pub fn get_time_constant(&self) -> f64 {
        self.tau
    }

    /// Normalizer for the series connection of two digital RC filters with
    /// time constants `tau1` and `tau2` (milliseconds) at sample rate `fs`.
    pub fn get_normalizer(tau1: f64, tau2: f64, fs: f64) -> f64 {
        let td = 0.001 * tau1;
        let ta = 0.001 * tau2;

        // catch some special cases:
        if ta == 0.0 && td == 0.0 {
            return 1.0;
        } else if ta == 0.0 {
            return 1.0 / (1.0 - (-1.0 / (fs * td)).exp());
        } else if td == 0.0 {
            return 1.0 / (1.0 - (-1.0 / (fs * ta)).exp());
        }

        // compute the filter coefficients:
        let mut x = (-1.0 / (fs * td)).exp();
        let bd = 1.0 - x;
        let ad = -x;
        x = (-1.0 / (fs * ta)).exp();
        let ba = 1.0 - x;
        let aa = -x;

        // compute the location and height of the peak:
        let xp = if ta == td {
            let tp = ta;
            let np = fs * tp;
            (np + 1.0) * ba * ba * aa.powf(np)
        } else {
            let tp = (ta / td).ln() / ((1.0 / td) - (1.0 / ta));
            let np = fs * tp;
            let s = 1.0 / (aa - ad);
            let b01 = s * aa * ba * bd;
            let b02 = s * ad * ba * bd;
            let a01 = s * (ad - aa) * aa;
            let a02 = s * (ad - aa) * ad;
            b01 * a01.powf(np) - b02 * a02.powf(np)
        };

        // return the normalizer as reciprocal of the peak height:
        1.0 / xp
    }

    #[inline]
    pub fn get_sample(&mut self, input: f64) -> f64 {
        self.y1 = input + self.coeff * (self.y1 - input);
        self.y1
    }

    pub fn reset(&mut self) {
        self.y1 = 0.0;
    }

    fn calculate_coefficient(&mut self) {
        if self.tau > 0.0 {
            self.coeff = (-1.0 / (self.sample_rate * 0.001 * self.tau)).exp();
        } else {
            self.coeff = 0.0;
        }
    }
}
