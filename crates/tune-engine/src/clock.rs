//! The transport clock and the seed derivation every track shares.

use tune_dsp::acid303::math::round_to_int;

/// Maps song steps (16ths) to sample positions without drift: every step start
/// is computed from the step index, never accumulated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    pub sample_rate: u32,
    pub bpm: f64,
    /// `sample_rate * 15 / bpm`.
    pub samples_per_step: f64,
}

impl Clock {
    pub fn new(bpm: f64, sample_rate: u32) -> Self {
        Self {
            sample_rate,
            bpm,
            samples_per_step: f64::from(sample_rate) * 15.0 / bpm,
        }
    }

    /// First sample of `step`.
    pub fn step_start(&self, step: u64) -> u64 {
        (step as f64 * self.samples_per_step).round() as u64
    }

    /// The largest step `k` with `step_start(k) <= pos`.
    pub fn step_at(&self, pos: u64) -> u64 {
        let mut k = (pos as f64 / self.samples_per_step) as u64;
        while self.step_start(k) > pos {
            k -= 1;
        }
        while self.step_start(k + 1) <= pos {
            k += 1;
        }
        k
    }

    /// First sample of 0-based `bar` (16 steps per bar).
    pub fn bar_start(&self, bar: u32) -> u64 {
        self.step_start(16 * u64::from(bar))
    }

    /// Gate length: half a step, rounded like the 303 sequencer does.
    pub fn gate_samples(&self) -> i32 {
        round_to_int(0.5 * self.samples_per_step)
    }

    /// Song time in seconds at sample `pos`.
    pub fn seconds(&self, pos: u64) -> f64 {
        pos as f64 / f64::from(self.sample_rate)
    }
}

fn splitmix64(x: u64) -> u64 {
    let x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let z = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn fnv1a64(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Per-track noise seed: `splitmix64(song_seed ^ fnv1a64(track_id))`.
pub fn track_seed(song_seed: u64, track_id: &str) -> u64 {
    splitmix64(song_seed ^ fnv1a64(track_id))
}

/// Per-hit seed: `splitmix64(track_seed ^ step)`.
pub fn hit_seed(track_seed: u64, step: u64) -> u64 {
    splitmix64(track_seed ^ step)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_steps_145_bpm() {
        let c = Clock::new(145.0, 48_000);
        assert_eq!(c.step_start(1), 4966);
        assert_eq!(c.step_start(16), 79_448);
        assert_eq!(c.bar_start(64), 5_084_690);
        assert_eq!(c.step_at(4965), 0);
        assert_eq!(c.step_at(4966), 1);
    }

    #[test]
    fn exact_at_120_bpm() {
        let c = Clock::new(120.0, 48_000);
        for k in 0..10_000u64 {
            assert_eq!(c.step_start(k), 6000 * k);
        }
        assert_eq!(c.gate_samples(), 3000);
        assert_eq!(c.step_at(5999), 0);
        assert_eq!(c.step_at(6000), 1);
        assert_eq!(c.seconds(24_000), 0.5);
    }

    #[test]
    fn step_at_inverts_step_start() {
        let c = Clock::new(143.7, 44_100);
        for k in 0..2000u64 {
            let s = c.step_start(k);
            assert_eq!(c.step_at(s), k);
            if s > 0 {
                assert_eq!(c.step_at(s - 1), k - 1);
            }
        }
    }
}
