//! Kick-locked ducking: a volume envelope fired on the kick grid.

use crate::acid303::math::db2amp;

/// Attack time of the dip, in milliseconds.
pub const DUCK_ATTACK_MS: f64 = 2.0;

/// A per-track gain dip: linear attack to full depth, raised-cosine release.
///
/// Idle (before the first trigger, at depth 0, or after the release) the gain
/// is exactly `1.0`. No allocation, no randomness.
#[derive(Clone, Debug)]
pub struct Ducker {
    sample_rate: f64,
    depth: f64,
    release_ms: f64,
    attack_frames: f64,
    release_frames: f64,
    /// Envelope value at the last trigger.
    e0: f64,
    /// Frames since the last trigger.
    t: f64,
    active: bool,
    depth_gain: f64,
}

impl Ducker {
    /// Idle, depth 0, release 80 ms.
    pub fn new(sample_rate: f64) -> Self {
        let mut d = Self {
            sample_rate,
            depth: 0.0,
            release_ms: 80.0,
            attack_frames: DUCK_ATTACK_MS * 0.001 * sample_rate,
            release_frames: 0.0,
            e0: 0.0,
            t: 0.0,
            active: false,
            depth_gain: 1.0,
        };
        d.set_release_ms(80.0);
        d
    }

    /// Dip depth in dB; 0 is off.
    pub fn set_depth_db(&mut self, db: f64) {
        self.depth = db.max(0.0);
        self.depth_gain = db2amp(-self.depth);
    }

    /// Release time in milliseconds.
    pub fn set_release_ms(&mut self, ms: f64) {
        self.release_ms = ms;
        self.release_frames = (ms * 0.001 * self.sample_rate).max(0.0);
    }

    /// True when the depth is above 0.
    pub fn is_active(&self) -> bool {
        self.depth > 0.0
    }

    fn envelope(&self) -> f64 {
        if !self.active {
            return 0.0;
        }
        if self.t < self.attack_frames {
            self.e0 + (1.0 - self.e0) * self.t / self.attack_frames
        } else if self.t < self.attack_frames + self.release_frames {
            0.5 * (1.0
                + (std::f64::consts::PI * (self.t - self.attack_frames) / self.release_frames)
                    .cos())
        } else {
            0.0
        }
    }

    /// Restarts the envelope from its current value.
    pub fn trigger(&mut self) {
        self.e0 = self.envelope();
        self.t = 0.0;
        self.active = true;
    }

    /// This frame's gain, then advances one frame.
    pub fn next_gain(&mut self) -> f64 {
        if !self.active {
            return 1.0;
        }
        let e = self.envelope();
        self.t += 1.0;
        if self.t >= self.attack_frames + self.release_frames {
            self.active = false;
            self.e0 = 0.0;
        }
        1.0 - (1.0 - self.depth_gain) * e
    }

    /// Back to idle.
    pub fn reset(&mut self) {
        self.e0 = 0.0;
        self.t = 0.0;
        self.active = false;
    }
}
