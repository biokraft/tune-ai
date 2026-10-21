//! Lookahead true-peak master limiter, written from the published
//! description of a lookahead brickwall limiter (oversampled peak detector,
//! running-minimum hold over the lookahead window, moving-average smoothing,
//! exponential release).
//!
//! The detector estimates the inter-sample peak at 4x: three points between
//! each pair of samples, each read by two interpolators, keeping the larger.
//! A 24-tap Blackman-windowed sinc follows the reconstructed waveform. The
//! 12-tap Hann-windowed sinc of a BS.1770 true-peak meter (the 48-tap, 4x
//! design of libebur128, which `tune analyze` measures with) can read up to
//! about 1 dB higher on dense content near Nyquist, such as a distorted 303
//! under hats, so the limiter answers to it too.
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
        help: "The loudest the limiter lets a peak get. Sample peaks stay at or under it; a true-peak meter reads at most about 0.3 dB over it.",
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
pub const DETECTOR_DELAY: usize = 12;

/// Taps of each interpolator phase.
const TAPS: usize = 2 * DETECTOR_DELAY;
/// Taps of each phase of the meter's interpolator (48 taps at 4x).
const METER_TAPS: usize = 12;
/// `hist` index of the meter interpolator's newest tap, which centres it on
/// the frame `DETECTOR_DELAY` back.
const METER_OFFSET: usize = DETECTOR_DELAY - METER_TAPS / 2;
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

/// Meter interpolator phase `j` (1..=3), for the same point as
/// [`phase_coeffs`]`(j)`: taps `4i + j` of a 48-tap sinc cut off at
/// the original Nyquist, under a Hann window over 49 points, rounded to
/// `f32` like the meter's. Not normalised: the meter's phases are not.
fn meter_coeffs(j: usize) -> [f64; METER_TAPS] {
    const N: f64 = 48.0;
    let mut c = [0.0; METER_TAPS];
    for (i, ci) in c.iter_mut().enumerate() {
        let q = (4 * i + j) as f64;
        let window = 0.5 * (1.0 - (2.0 * std::f64::consts::PI * q / N).cos());
        let m = (q - N / 2.0) * std::f64::consts::PI / 4.0;
        let sinc = if m == 0.0 { 1.0 } else { m.sin() / m };
        *ci = (window * sinc) as f32 as f64;
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
    meter_coeffs: [[f64; METER_TAPS]; PHASES],
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
            meter_coeffs: [meter_coeffs(1), meter_coeffs(2), meter_coeffs(3)],
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
    /// and the three interpolated points after it, each by both
    /// interpolators, over both channels.
    #[inline]
    fn detect(&self) -> f64 {
        let mut peak = 0.0f64;
        for h in &self.hist {
            peak = peak.max(h[DETECTOR_DELAY].abs());
            for (c, m) in self.coeffs.iter().zip(&self.meter_coeffs) {
                let mut y = 0.0;
                for (ci, xi) in c.iter().zip(h) {
                    y += ci * xi;
                }
                let mut z = 0.0;
                for (mi, xi) in m.iter().zip(&h[METER_OFFSET..]) {
                    z += mi * xi;
                }
                peak = peak.max(y.abs()).max(z.abs());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// What a BS.1770 meter built like libebur128 reads at input step `m`:
    /// the largest of its four 4x phases, `sum_i h[4i + k] * x[m - i]`,
    /// with the 48 taps written out from that meter's formula.
    fn meter_reading(x: &[f64], m: usize) -> f64 {
        let mut h = [0.0f64; 48];
        for (q, hq) in h.iter_mut().enumerate() {
            let w = 0.5 * (1.0 - (2.0 * std::f64::consts::PI * q as f64 / 48.0).cos());
            let a = (q as f64 - 24.0) * std::f64::consts::PI / 4.0;
            *hq = if a == 0.0 { w } else { w * a.sin() / a } as f32 as f64;
        }
        (0..4)
            .map(|k| (0..12).map(|i| h[4 * i + k] * x[m - i]).sum::<f64>().abs())
            .fold(0.0, f64::max)
    }

    /// The detector covers the meter: at every frame its estimate is at
    /// least what the meter reads over the same quarter-sample points.
    #[test]
    fn detector_covers_the_meter() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut noise = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 52) as f64 - 1.0
        };
        let n = 20_000;
        let l: Vec<f64> = (0..n).map(|_| noise()).collect();
        let r: Vec<f64> = (0..n).map(|_| noise()).collect();
        let mut lim = Limiter::new(48000.0);
        let mut short = 0;
        for i in 0..n {
            lim.process(l[i], r[i]);
            // The detector's centre is DETECTOR_DELAY frames back; the
            // meter centres its newest phases half its 12 taps back.
            if i < DETECTOR_DELAY + 12 {
                continue;
            }
            let m = i - (DETECTOR_DELAY - 6);
            let meter = meter_reading(&l, m).max(meter_reading(&r, m));
            if lim.detect() < meter * (1.0 - 1e-12) {
                short += 1;
            }
        }
        assert_eq!(short, 0, "frames where the meter reads above the detector");
    }
}
