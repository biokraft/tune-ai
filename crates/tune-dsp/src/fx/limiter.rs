//! Lookahead true-peak master limiter, written from the published
//! description of a lookahead brickwall limiter (oversampled peak detector,
//! running-minimum hold over the lookahead window, moving-average smoothing,
//! exponential release).
//!
//! The detector estimates the inter-sample peak with a 4x interpolator: three
//! points between each pair of samples, from a 12-tap Blackman-windowed sinc.
//! The audio runs [`Limiter::latency`] frames behind the detector, so the
//! gain has reached its target by the time a peak leaves the delay line.
//! Below the ceiling the gain is exactly `1.0` and the output is the input,
//! delayed, bit for bit. Nothing is random and `process` never allocates.

use crate::acid303::math::db2amp;
use crate::param::{Curve, ParamSpec};

/// Index of `master.gain` in [`MASTER_PARAMS`].
pub const MASTER_GAIN: usize = 0;
/// Index of `master.ceiling` in [`MASTER_PARAMS`].
pub const MASTER_CEILING: usize = 1;
/// Index of `master.release` in [`MASTER_PARAMS`].
pub const MASTER_RELEASE: usize = 2;

/// Master stage params, in plain units.
pub const MASTER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "master.gain",
        label: "Master gain",
        unit: "dB",
        min: -12.0,
        max: 24.0,
        curve: Curve::Linear,
        default: 0.0,
        help: "Drives the whole mix into the master stage. With the limiter, this is the loudness knob.",
    },
    ParamSpec {
        id: "master.ceiling",
        label: "Ceiling",
        unit: "dB",
        min: -6.0,
        max: -0.1,
        curve: Curve::Linear,
        default: -1.5,
        help: "The limiter never lets the true peak above this.",
    },
    ParamSpec {
        id: "master.release",
        label: "Release",
        unit: "ms",
        min: 10.0,
        max: 500.0,
        curve: Curve::Exponential,
        default: 100.0,
        help: "How fast the limiter lets go. Short is louder but pumps; long is smoother.",
    },
];

/// Lookahead window, in milliseconds.
pub const LIMITER_LOOKAHEAD_MS: f64 = 1.5;

/// Frames the peak detector runs behind its newest input (half its taps).
pub const DETECTOR_DELAY: usize = 6;

/// Taps of each interpolator phase.
const TAPS: usize = 2 * DETECTOR_DELAY;
/// Interpolated points between two samples (4x oversampling).
const PHASES: usize = 3;
/// Below this distance from 1 the gain snaps to exactly 1.
const UNITY_SNAP: f64 = 1e-9;

/// Interpolator phase `j` (1..=3) for the point `j/4` of a sample after the
/// detector's centre: Blackman-windowed sinc, normalised to sum 1.
fn phase_coeffs(j: usize) -> [f64; TAPS] {
    let half = DETECTOR_DELAY as f64;
    let mut c = [0.0; TAPS];
    for (i, ci) in c.iter_mut().enumerate() {
        let d = half - i as f64 - j as f64 / 4.0;
        let pd = std::f64::consts::PI * d;
        let sinc = if d == 0.0 { 1.0 } else { pd.sin() / pd };
        let window = 0.42
            + 0.5 * (std::f64::consts::PI * d / half).cos()
            + 0.08 * (2.0 * std::f64::consts::PI * d / half).cos();
        *ci = sinc * window;
    }
    let sum: f64 = c.iter().sum();
    for ci in &mut c {
        *ci /= sum;
    }
    c
}

/// Stereo-linked lookahead limiter with a true-peak detector.
///
/// Every ring is allocated in [`Limiter::new`]; `process` never allocates.
#[derive(Clone, Debug)]
pub struct Limiter {
    sample_rate: f64,
    /// Lookahead window W, in frames.
    window: usize,
    /// Linear ceiling.
    ceiling: f64,
    /// Release coefficient per frame.
    alpha: f64,
    coeffs: [[f64; TAPS]; PHASES],
    /// Detector input per channel, newest first: `hist[ch][i]` is `x[n - i]`.
    hist: [[f64; TAPS]; 2],
    /// Audio delay lines, `latency()` frames long.
    delay_l: Vec<f64>,
    delay_r: Vec<f64>,
    delay_pos: usize,
    /// The last `W + 1` target gains.
    targets: Vec<f64>,
    target_pos: usize,
    /// The last `W` held (minimum) gains.
    held: Vec<f64>,
    held_pos: usize,
    gain: f64,
}

impl Limiter {
    /// Ceiling -1.5 dB, release 100 ms. Allocates its rings: build off the
    /// audio thread.
    pub fn new(sample_rate: f64) -> Self {
        let window = ((LIMITER_LOOKAHEAD_MS * 0.001 * sample_rate).round() as usize).max(1);
        let latency = window + DETECTOR_DELAY;
        let mut lim = Self {
            sample_rate,
            window,
            ceiling: 1.0,
            alpha: 1.0,
            coeffs: [phase_coeffs(1), phase_coeffs(2), phase_coeffs(3)],
            hist: [[0.0; TAPS]; 2],
            delay_l: vec![0.0; latency],
            delay_r: vec![0.0; latency],
            delay_pos: 0,
            targets: vec![1.0; window + 1],
            target_pos: 0,
            held: vec![1.0; window],
            held_pos: 0,
            gain: 1.0,
        };
        lim.set_ceiling_db(MASTER_PARAMS[MASTER_CEILING].default);
        lim.set_release_ms(MASTER_PARAMS[MASTER_RELEASE].default);
        lim
    }

    /// Frames from a frame's input to its output: the lookahead window plus
    /// the detector delay.
    pub fn latency(&self) -> usize {
        self.window + DETECTOR_DELAY
    }

    /// Ceiling in dB, clamped to the `master.ceiling` range (NaN is the
    /// minimum).
    pub fn set_ceiling_db(&mut self, db: f64) {
        let db = clamp_to(&MASTER_PARAMS[MASTER_CEILING], db);
        self.ceiling = db2amp(db);
    }

    /// Release time in milliseconds, clamped to the `master.release` range
    /// (NaN is the minimum).
    pub fn set_release_ms(&mut self, ms: f64) {
        let ms = clamp_to(&MASTER_PARAMS[MASTER_RELEASE], ms);
        self.alpha = 1.0 - (-1.0 / (ms * 0.001 * self.sample_rate)).exp();
    }

    /// Clears every ring: gains back to `1.0`, audio to silence. Settings stay.
    pub fn reset(&mut self) {
        self.hist = [[0.0; TAPS]; 2];
        self.delay_l.fill(0.0);
        self.delay_r.fill(0.0);
        self.delay_pos = 0;
        self.targets.fill(1.0);
        self.target_pos = 0;
        self.held.fill(1.0);
        self.held_pos = 0;
        self.gain = 1.0;
    }

    /// Peak estimate for the frame `DETECTOR_DELAY` frames back: its sample
    /// and the three interpolated points after it, over both channels.
    #[inline]
    fn detect(&self) -> f64 {
        let mut peak = 0.0f64;
        for h in &self.hist {
            peak = peak.max(h[DETECTOR_DELAY].abs());
            for c in &self.coeffs {
                let mut y = 0.0;
                for (ci, xi) in c.iter().zip(h) {
                    y += ci * xi;
                }
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    /// One stereo frame in; the frame from [`Limiter::latency`] frames ago
    /// out, with the gain applied to it. Returns `(left, right, gain)`.
    #[inline]
    pub fn process(&mut self, l: f64, r: f64) -> (f64, f64, f64) {
        // Detector: shift the new frame in and estimate the centre's peak.
        for (h, x) in self.hist.iter_mut().zip([l, r]) {
            h.copy_within(0..TAPS - 1, 1);
            h[0] = x;
        }
        let peak = self.detect();
        let target = if peak > self.ceiling {
            self.ceiling / peak
        } else {
            1.0
        };

        // Hold the minimum over the window, then smooth it with a moving
        // average of the same length (summed directly, so it stays exact).
        self.targets[self.target_pos] = target;
        self.target_pos = (self.target_pos + 1) % self.targets.len();
        let held = self.targets.iter().copied().fold(f64::INFINITY, f64::min);
        self.held[self.held_pos] = held;
        self.held_pos = (self.held_pos + 1) % self.held.len();
        let avg = self.held.iter().sum::<f64>() / self.window as f64;

        // Instant attack to the smoothed target, exponential release.
        let mut g = if avg < self.gain {
            avg
        } else {
            self.gain + self.alpha * (avg - self.gain)
        };
        if 1.0 - g < UNITY_SNAP {
            g = 1.0;
        }
        self.gain = g;

        // Delay line: read the frame `latency()` back, write the new one.
        let dl = self.delay_l[self.delay_pos];
        let dr = self.delay_r[self.delay_pos];
        self.delay_l[self.delay_pos] = l;
        self.delay_r[self.delay_pos] = r;
        self.delay_pos = (self.delay_pos + 1) % self.delay_l.len();

        let c = self.ceiling;
        ((dl * g).clamp(-c, c), (dr * g).clamp(-c, c), g)
    }
}

fn clamp_to(spec: &ParamSpec, v: f64) -> f64 {
    if v.is_nan() {
        spec.min
    } else {
        v.clamp(spec.min, spec.max)
    }
}
