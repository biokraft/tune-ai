//! Sample-for-sample parity of the TeeBee filter against the C++ reference.

mod common;

use common::{max_abs_diff, noise};
use open303_ref::RefTeeBee;
use open303_ref::consts::tee_bee_mode as m;
use tune_dsp::acid303::{FilterMode, TeeBeeFilter};

const FS: f64 = 192_000.0;
const N: usize = 192_000;
const TOL: f64 = 1e-12;

const MODES: [(i32, FilterMode); 16] = [
    (m::FLAT, FilterMode::Flat),
    (m::LP_6, FilterMode::Lp6),
    (m::LP_12, FilterMode::Lp12),
    (m::LP_18, FilterMode::Lp18),
    (m::LP_24, FilterMode::Lp24),
    (m::HP_6, FilterMode::Hp6),
    (m::HP_12, FilterMode::Hp12),
    (m::HP_18, FilterMode::Hp18),
    (m::HP_24, FilterMode::Hp24),
    (m::BP_12_12, FilterMode::Bp12_12),
    (m::BP_6_18, FilterMode::Bp6_18),
    (m::BP_18_6, FilterMode::Bp18_6),
    (m::BP_6_12, FilterMode::Bp6_12),
    (m::BP_12_6, FilterMode::Bp12_6),
    (m::BP_6_6, FilterMode::Bp6_6),
    (m::TB_303, FilterMode::Tb303),
];

/// Naive 110 Hz saw plus seeded noise.
fn input() -> Vec<f64> {
    let nz = noise(7, N);
    let mut phase = 0.0_f64;
    nz.iter()
        .map(|&n| {
            let s = 2.0 * phase - 1.0;
            phase += 110.0 / FS;
            if phase >= 1.0 {
                phase -= 1.0;
            }
            0.5 * s + 0.1 * n
        })
        .collect()
}

#[test]
fn mode_enum_matches_cpp_values() {
    for (i, (c, r)) in MODES.iter().enumerate() {
        assert_eq!(*c, i as i32);
        assert_eq!(*r as i32, *c);
    }
    assert_eq!(m::NUM_MODES, 16);
    assert_eq!(FilterMode::default(), FilterMode::Tb303);
}

#[test]
fn defaults_match_cpp() {
    let mut r = RefTeeBee::new();
    let mut f = TeeBeeFilter::new();
    assert_eq!(r.get_cutoff(), f.get_cutoff());
    assert_eq!(r.get_resonance(), f.get_resonance());
    assert_eq!(r.get_drive(), f.get_drive());
    assert_eq!(r.get_mode(), f.get_mode() as i32);
    assert_eq!(
        r.get_feedback_highpass_cutoff(),
        f.get_feedback_highpass_cutoff()
    );
    for x in [0.0, 0.3, -0.7, 1.0] {
        assert_eq!(r.get_sample(x), f.get_sample(x));
    }
}

fn run(constant: Option<f64>) {
    let inp = input();
    for (cm, rm) in MODES {
        for res in [0.0, 50.0, 85.0, 100.0] {
            let mut r = RefTeeBee::new();
            let mut f = TeeBeeFilter::new();
            r.set_sample_rate(FS);
            f.set_sample_rate(FS);
            r.set_mode(cm);
            f.set_mode(rm);
            r.set_resonance(res, true);
            f.set_resonance(res, true);
            r.set_drive(6.0);
            f.set_drive(6.0);
            if let Some(c) = constant {
                r.set_cutoff(c, true);
                f.set_cutoff(c, true);
            }
            let mut a = Vec::with_capacity(N);
            let mut b = Vec::with_capacity(N);
            for (i, &x) in inp.iter().enumerate() {
                if constant.is_none() {
                    let c = 100.0 * (8000.0_f64 / 100.0).powf(i as f64 / (N - 1) as f64);
                    r.set_cutoff(c, true);
                    f.set_cutoff(c, true);
                }
                a.push(r.get_sample(x));
                b.push(f.get_sample(x));
            }
            let d = max_abs_diff(&a, &b);
            println!("mode={cm} res={res} cutoff={constant:?}: {d:e}");
            assert!(
                d <= TOL,
                "mode={cm} res={res} cutoff={constant:?} diff {d:e}"
            );
        }
    }
}

#[test]
fn constant_cutoff_100_matches_cpp() {
    run(Some(100.0));
}

#[test]
fn constant_cutoff_1000_matches_cpp() {
    run(Some(1000.0));
}

#[test]
fn constant_cutoff_6000_matches_cpp() {
    run(Some(6000.0));
}

#[test]
fn exponential_cutoff_sweep_matches_cpp() {
    run(None);
}

#[test]
fn setters_shape_and_exact_coefficients_match_cpp() {
    let inp = input();
    let mut r = RefTeeBee::new();
    let mut f = TeeBeeFilter::new();
    // The exact coefficients are only stable for the plain ladder modes.
    r.set_mode(m::LP_24);
    f.set_mode(FilterMode::Lp24);
    for x in [-3.0, -1.5, -0.2, 0.0, 0.9, 1.4143, 2.0, 5.0] {
        assert_eq!(r.shape(x), f.shape(x));
    }
    // cutoff clamps, deferred coefficient updates, feedback highpass, 44.1 kHz.
    for (cut, res) in [(50.0, 30.0), (30000.0, 60.0), (777.0, 100.0)] {
        r.set_cutoff(cut, false);
        f.set_cutoff(cut, false);
        r.set_resonance(res, false);
        f.set_resonance(res, false);
        r.calculate_coefficients_exact();
        f.calculate_coefficients_exact();
        assert_eq!(r.get_cutoff(), f.get_cutoff());
        assert_eq!(r.get_resonance(), f.get_resonance());
        for &x in inp.iter().take(2000) {
            let (a, b) = (r.get_sample(x), f.get_sample(x));
            assert!((a - b).abs() <= TOL, "{a} vs {b}");
        }
    }
    r.set_feedback_highpass_cutoff(400.0);
    f.set_feedback_highpass_cutoff(400.0);
    r.set_sample_rate(44_100.0);
    f.set_sample_rate(44_100.0);
    r.set_mode(m::LP_24);
    f.set_mode(FilterMode::Lp24);
    r.reset();
    f.reset();
    for &x in inp.iter().take(5000) {
        let (a, b) = (r.get_sample(x), f.get_sample(x));
        assert!((a - b).abs() <= TOL, "{a} vs {b}");
    }
}
