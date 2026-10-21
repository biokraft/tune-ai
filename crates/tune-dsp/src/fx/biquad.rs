//! RBJ Audio EQ Cookbook biquad (transposed direct form II).

use crate::acid303::math::db2amp;

/// Filter shape for [`Biquad::set`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BiquadKind {
    Lowpass,
    Highpass,
    Peak { gain_db: f64 },
    HighShelf { gain_db: f64 },
}

/// A second-order section with RBJ cookbook coefficients.
///
/// This is a new RBJ biquad beside `acid303::BiquadFilter`; `acid303/` is
/// frozen and the EQ needs the cookbook peak and shelf forms the Open303 port
/// lacks.
#[derive(Clone, Debug)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    s1: f64,
    s2: f64,
}

impl Default for Biquad {
    fn default() -> Self {
        Self::new()
    }
}

impl Biquad {
    /// Identity filter: output equals input.
    pub fn new() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    /// Sets the coefficients for `kind` at `hz` with quality `q`. `hz` is
    /// clamped to 0.49 times the sample rate. The state is kept.
    pub fn set(&mut self, kind: BiquadKind, hz: f64, q: f64, sample_rate: f64) {
        let hz = hz.min(0.49 * sample_rate);
        let w0 = 2.0 * std::f64::consts::PI * hz / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadKind::Lowpass => (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            BiquadKind::Highpass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            BiquadKind::Peak { gain_db } => {
                let a = db2amp(gain_db / 2.0);
                (
                    1.0 + alpha * a,
                    -2.0 * cos,
                    1.0 - alpha * a,
                    1.0 + alpha / a,
                    -2.0 * cos,
                    1.0 - alpha / a,
                )
            }
            BiquadKind::HighShelf { gain_db } => {
                let a = db2amp(gain_db / 2.0);
                let t = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + t),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - t),
                    (a + 1.0) - (a - 1.0) * cos + t,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - t,
                )
            }
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        y
    }

    /// Clears the state. Coefficients stay.
    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_identity() {
        let mut b = Biquad::new();
        for x in [0.3, -0.7, 1.0] {
            assert_eq!(b.process(x), x);
        }
    }

    #[test]
    fn lowpass_passes_dc() {
        let mut b = Biquad::new();
        b.set(
            BiquadKind::Lowpass,
            1000.0,
            std::f64::consts::FRAC_1_SQRT_2,
            48000.0,
        );
        let mut y = 0.0;
        for _ in 0..5000 {
            y = b.process(1.0);
        }
        assert!((y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn highpass_blocks_dc() {
        let mut b = Biquad::new();
        b.set(
            BiquadKind::Highpass,
            1000.0,
            std::f64::consts::FRAC_1_SQRT_2,
            48000.0,
        );
        let mut y = 1.0;
        for _ in 0..5000 {
            y = b.process(1.0);
        }
        assert!(y.abs() < 1e-9);
    }

    #[test]
    fn hz_is_clamped_below_nyquist() {
        let mut b = Biquad::new();
        b.set(
            BiquadKind::Lowpass,
            1e9,
            std::f64::consts::FRAC_1_SQRT_2,
            48000.0,
        );
        for i in 0..1000 {
            assert!(b.process((i as f64).sin()).is_finite());
        }
    }
}
