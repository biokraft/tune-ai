//! Shared helpers for the parity tests.
#![allow(dead_code)]

/// Tiny deterministic xorshift64 generator for seeded test noise.
pub struct XorShift64(u64);

impl XorShift64 {
    pub fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in [-1, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }
}

/// `n` samples of seeded noise in [-1, 1).
pub fn noise(seed: u64, n: usize) -> Vec<f64> {
    let mut rng = XorShift64::new(seed);
    (0..n).map(|_| rng.next_f64()).collect()
}

/// Maximum absolute difference between two equally long signals.
pub fn max_abs_diff(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max)
}

/// `n` samples of a unit sine at `freq` Hz.
pub fn tone(freq: f64, sample_rate: f64, n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / sample_rate).sin())
        .collect()
}

/// RMS of a signal.
pub fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
}
