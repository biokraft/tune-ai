//! Seeded white noise (xorshift64*), deterministic across platforms.

const ZERO_SEED_REPLACEMENT: u64 = 0x9E37_79B9_7F4A_7C15;
const MULTIPLIER: u64 = 0x2545_F491_4F6C_DD1D;

/// White noise generator. The same seed always gives the same sequence.
#[derive(Clone, Debug)]
pub struct Noise {
    state: u64,
}

impl Noise {
    /// Creates a generator from `seed`. A seed of 0 is replaced by a fixed constant.
    pub fn new(seed: u64) -> Self {
        let mut n = Self { state: 1 };
        n.reseed(seed);
        n
    }

    /// Restarts the sequence from `seed`.
    pub fn reseed(&mut self, seed: u64) {
        self.state = if seed == 0 {
            ZERO_SEED_REPLACEMENT
        } else {
            seed
        };
    }

    /// Next value in `[-1, 1)`.
    pub fn sample(&mut self) -> f64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        let out = x.wrapping_mul(MULTIPLIER);
        (out >> 11) as f64 / 9_007_199_254_740_992.0 * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence_different_seed_differs() {
        let mut a = Noise::new(7);
        let mut b = Noise::new(7);
        let mut c = Noise::new(8);
        let sa: Vec<f64> = (0..64).map(|_| a.sample()).collect();
        let sb: Vec<f64> = (0..64).map(|_| b.sample()).collect();
        let sc: Vec<f64> = (0..64).map(|_| c.sample()).collect();
        assert_eq!(sa, sb);
        assert_ne!(sa, sc);
    }

    #[test]
    fn reseed_restarts_and_zero_seed_works() {
        let mut a = Noise::new(0);
        let first = a.sample();
        assert_ne!(first, 0.0);
        a.sample();
        a.reseed(0);
        assert_eq!(a.sample(), first);
    }

    #[test]
    fn statistics() {
        let mut n = Noise::new(12345);
        let (mut sum, mut sq) = (0.0, 0.0);
        let count = 1_000_000;
        for _ in 0..count {
            let v = n.sample();
            assert!((-1.0..1.0).contains(&v));
            sum += v;
            sq += v * v;
        }
        let mean = sum / count as f64;
        let rms = (sq / count as f64).sqrt();
        assert!(mean.abs() < 0.005, "mean {mean}");
        assert!((0.57..0.58).contains(&rms), "rms {rms}");
    }
}
