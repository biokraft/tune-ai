//! Port of `rosic_DecayEnvelope`: a pure exponential decay envelope.

/// `rosic::DecayEnvelope`.
#[derive(Clone, Debug)]
pub struct DecayEnvelope {
    /// Coefficient for multiplicative accumulation.
    c: f64,
    /// Previous output.
    y: f64,
    /// Initial value for the previous output (`y / c`).
    y_init: f64,
    /// Time constant in milliseconds.
    tau: f64,
    sample_rate: f64,
    normalize_sum: bool,
}

impl Default for DecayEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

impl DecayEnvelope {
    pub fn new() -> Self {
        let mut s = Self {
            c: 1.0,
            y: 1.0,
            y_init: 1.0,
            tau: 200.0,
            sample_rate: 44100.0,
            normalize_sum: false,
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

    /// Sets the decay time constant in milliseconds (at least 0.001 ms).
    pub fn set_decay_time_constant(&mut self, new_time_constant: f64) {
        if new_time_constant > 0.001 {
            self.tau = new_time_constant;
            self.calculate_coefficient();
        }
    }

    /// Normalizes to the sum of the impulse response instead of the peak.
    pub fn set_normalize_sum(&mut self, should_normalize_sum: bool) {
        self.normalize_sum = should_normalize_sum;
        self.calculate_coefficient();
    }

    pub fn get_decay_time_constant(&self) -> f64 {
        self.tau
    }

    /// True if the output is below `threshold`.
    pub fn end_is_reached(&self, threshold: f64) -> bool {
        self.y < threshold
    }

    #[inline]
    pub fn get_sample(&mut self) -> f64 {
        self.y *= self.c;
        self.y
    }

    /// Triggers the envelope: the next sample retrieved is 1 (or the
    /// normalized peak).
    pub fn trigger(&mut self) {
        self.y = self.y_init;
    }

    fn calculate_coefficient(&mut self) {
        self.c = (-1.0 / (0.001 * self.tau * self.sample_rate)).exp();
        if self.normalize_sum {
            self.y_init = (1.0 - self.c) / self.c;
        } else {
            self.y_init = 1.0 / self.c;
        }
    }
}
