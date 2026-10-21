//! Mixer math: constant-power pan, the master soft clipper and its meter.

/// Where the master clipper starts to bend: `db2amp(-6.0)`.
pub const CLIP_KNEE: f64 = 0.501_187_233_627_272_2;

/// The level the master clipper never exceeds: `db2amp(-1.0)`.
pub const CLIP_CEILING: f64 = 0.891_250_938_133_745_6;

/// Master soft clipper: the identity up to [`CLIP_KNEE`], then a `tanh`
/// curve that approaches [`CLIP_CEILING`].
///
/// `|x| <= K: x`, else `sign(x) * (K + (C - K) * tanh((|x| - K) / (C - K)))`.
pub fn soft_clip(x: f64) -> f64 {
    let a = x.abs();
    if a <= CLIP_KNEE {
        return x;
    }
    let span = CLIP_CEILING - CLIP_KNEE;
    let y = (CLIP_KNEE + span * ((a - CLIP_KNEE) / span).tanh()).min(CLIP_CEILING);
    y.copysign(x)
}

/// Constant-power pan gains `(left, right)` for `pan` in `-1..=1`.
///
/// `θ = (pan + 1)·π/4`, giving `(cos θ, sin θ)`. The right gain is computed
/// as `cos((1 - pan)·π/4)`, which equals `sin θ` but makes the law exactly
/// mirror-symmetric: `pan_gains(-p)` is `pan_gains(p)` swapped, bit for bit,
/// and the centre gives two equal gains.
pub fn pan_gains(pan: f64) -> (f64, f64) {
    let quarter = std::f64::consts::FRAC_PI_4;
    (((pan + 1.0) * quarter).cos(), ((1.0 - pan) * quarter).cos())
}

/// Master level statistics since the last reset.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meter {
    /// Peak of the clipped output (what is written), either channel.
    pub peak: f64,
    /// Peak of the master bus before the clipper, either channel.
    pub pre_clip_peak: f64,
    /// Frames in which either channel of the master bus exceeded [`CLIP_KNEE`].
    pub knee_samples: u64,
}

impl Meter {
    /// Records one master frame and returns it clipped.
    pub fn clip_frame(&mut self, l: f64, r: f64) -> (f64, f64) {
        let pre = l.abs().max(r.abs());
        self.pre_clip_peak = self.pre_clip_peak.max(pre);
        if pre > CLIP_KNEE {
            self.knee_samples += 1;
        }
        let (cl, cr) = (soft_clip(l), soft_clip(r));
        self.peak = self.peak.max(cl.abs().max(cr.abs()));
        (cl, cr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tune_dsp::acid303::math::db2amp;

    #[test]
    fn constants_match_db2amp() {
        assert_eq!(CLIP_KNEE, db2amp(-6.0));
        assert_eq!(CLIP_CEILING, db2amp(-1.0));
    }

    #[test]
    fn soft_clip_shape() {
        assert_eq!(soft_clip(0.25), 0.25);
        assert_eq!(soft_clip(-0.25), -0.25);
        assert!(soft_clip(1e6) <= CLIP_CEILING);
        assert!(soft_clip(-1e6) >= -CLIP_CEILING);
        assert!(soft_clip(f64::INFINITY) <= CLIP_CEILING);
        let x = CLIP_KNEE + 1e-9;
        assert!((soft_clip(x) - x).abs() < 1e-12);
        assert_eq!(soft_clip(CLIP_KNEE), CLIP_KNEE);
    }

    #[test]
    fn soft_clip_monotonic() {
        let n = 10_000;
        let mut prev = f64::NEG_INFINITY;
        for i in 0..=n {
            let x = -10.0 + 20.0 * f64::from(i) / f64::from(n);
            let y = soft_clip(x);
            assert!(y >= prev, "not monotonic at {x}: {y} < {prev}");
            assert!(y.abs() <= CLIP_CEILING);
            prev = y;
        }
    }

    #[test]
    fn pan_constant_power() {
        // 0.7071067811865476 on both sides.
        let centre = std::f64::consts::FRAC_1_SQRT_2;
        assert_eq!(pan_gains(0.0), (centre, centre));
        assert!(pan_gains(1.0).0 < 1e-15);
        assert!(pan_gains(-1.0).1 < 1e-15);
        for i in -10..=10 {
            let pan = f64::from(i) * 0.1;
            let (l, r) = pan_gains(pan);
            assert!((l * l + r * r - 1.0).abs() < 1e-12, "pan {pan}");
            let (ml, mr) = pan_gains(-pan);
            assert_eq!((ml.to_bits(), mr.to_bits()), (r.to_bits(), l.to_bits()));
        }
        let (l, r) = pan_gains(0.3);
        assert!(r > l);
    }

    #[test]
    fn meter_tracks_peaks_and_knee() {
        let mut m = Meter::default();
        assert_eq!(m.clip_frame(0.1, -0.2), (0.1, -0.2));
        m.clip_frame(2.0, 0.0);
        m.clip_frame(0.0, -0.6);
        assert_eq!(m.pre_clip_peak, 2.0);
        assert_eq!(m.knee_samples, 2);
        assert!(m.peak > CLIP_KNEE && m.peak <= CLIP_CEILING);
    }
}
