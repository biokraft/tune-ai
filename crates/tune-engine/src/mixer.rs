//! Mixer math: the constant-power pan law.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
