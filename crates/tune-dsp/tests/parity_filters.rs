//! Sample-for-sample parity of the small filters against the C++ reference.

mod common;

use common::{max_abs_diff, noise};
use open303_ref::consts::{biquad_mode, one_pole_mode};
use open303_ref::{RefBiquad, RefEllipticQB, RefLeakyIntegrator, RefOnePole};
use tune_dsp::acid303::{
    BiquadFilter, BiquadMode, EllipticQuarterBandFilter, LeakyIntegrator, OnePoleFilter,
    OnePoleMode,
};

const FS: f64 = 48_000.0;
const N: usize = 48_000;
const TOL: f64 = 1e-12;
const CUTOFFS: [f64; 4] = [20.0, 150.0, 1000.0, 8000.0];

#[test]
fn leaky_integrator_matches_cpp() {
    let input = noise(1, N);
    for tau in [0.0, 0.5, 5.0, 10.0, 200.0] {
        let mut r = RefLeakyIntegrator::new();
        r.set_sample_rate(FS);
        r.set_time_constant(tau);
        let mut m = LeakyIntegrator::new();
        m.set_sample_rate(FS);
        m.set_time_constant(tau);
        assert_eq!(r.get_time_constant(), m.get_time_constant());
        let a: Vec<f64> = input.iter().map(|&x| r.get_sample(x)).collect();
        let b: Vec<f64> = input.iter().map(|&x| m.get_sample(x)).collect();
        let d = max_abs_diff(&a, &b);
        println!("leaky tau={tau}: {d:e}");
        assert!(d <= TOL, "tau={tau} diff {d:e}");
    }
}

#[test]
fn leaky_integrator_state_reset_and_defaults() {
    let mut r = RefLeakyIntegrator::new();
    let mut m = LeakyIntegrator::new();
    r.set_state(0.7);
    m.set_state(0.7);
    assert_eq!(r.get_sample(0.1), m.get_sample(0.1));
    r.reset();
    m.reset();
    assert_eq!(r.get_sample(0.3), m.get_sample(0.3));
}

#[test]
fn leaky_integrator_normalizer_matches_cpp() {
    for fs in [44_100.0, 48_000.0, 96_000.0] {
        for (t1, t2) in [
            (0.0, 0.0),
            (3.0, 0.0),
            (0.0, 4.0),
            (10.0, 10.0),
            (200.0, 3.0),
            (3.0, 200.0),
        ] {
            let a = RefLeakyIntegrator::get_normalizer(t1, t2, fs);
            let b = LeakyIntegrator::get_normalizer(t1, t2, fs);
            assert!(
                (a - b).abs() <= TOL * a.abs().max(1.0),
                "{t1} {t2} {fs}: {a} vs {b}"
            );
        }
    }
}

#[test]
fn one_pole_matches_cpp() {
    let input = noise(2, N);
    let modes = [
        (one_pole_mode::BYPASS, OnePoleMode::Bypass),
        (one_pole_mode::LOWPASS, OnePoleMode::Lowpass),
        (one_pole_mode::HIGHPASS, OnePoleMode::Highpass),
        (one_pole_mode::LOWSHELV, OnePoleMode::Lowshelv),
        (one_pole_mode::HIGHSHELV, OnePoleMode::Highshelv),
        (one_pole_mode::ALLPASS, OnePoleMode::Allpass),
    ];
    for (cm, rm) in modes {
        for cutoff in CUTOFFS {
            for gain_db in [-9.0, 6.0] {
                let mut r = RefOnePole::new();
                r.set_sample_rate(FS);
                r.set_mode(cm);
                r.set_cutoff(cutoff);
                r.set_shelving_gain_in_decibels(gain_db);
                let mut m = OnePoleFilter::new();
                m.set_sample_rate(FS);
                m.set_mode(rm);
                m.set_cutoff(cutoff);
                m.set_shelving_gain_in_decibels(gain_db);
                assert_eq!(r.get_cutoff(), m.get_cutoff());
                let a: Vec<f64> = input.iter().map(|&x| r.get_sample(x)).collect();
                let b: Vec<f64> = input.iter().map(|&x| m.get_sample(x)).collect();
                let d = max_abs_diff(&a, &b);
                println!("onepole mode={cm} cutoff={cutoff} gain={gain_db}: {d:e}");
                assert!(
                    d <= TOL,
                    "mode={cm} cutoff={cutoff} gain={gain_db} diff {d:e}"
                );
            }
        }
    }
}

#[test]
fn one_pole_manual_coefficients_state_and_tau() {
    let input = noise(3, 2000);
    let mut r = RefOnePole::new();
    let mut m = OnePoleFilter::new();
    r.set_coefficients(0.3, 0.2, 0.4);
    m.set_coefficients(0.3, 0.2, 0.4);
    r.set_internal_state(0.5, -0.25);
    m.set_internal_state(0.5, -0.25);
    let a: Vec<f64> = input.iter().map(|&x| r.get_sample(x)).collect();
    let b: Vec<f64> = input.iter().map(|&x| m.get_sample(x)).collect();
    assert!(max_abs_diff(&a, &b) <= TOL);

    r.set_mode(one_pole_mode::LOWPASS);
    m.set_mode(OnePoleMode::Lowpass);
    r.set_lowpass_time_constant(0.001);
    m.set_lowpass_time_constant(0.001);
    assert_eq!(r.get_cutoff(), m.get_cutoff());
    r.reset();
    m.reset();
    let a: Vec<f64> = input.iter().map(|&x| r.get_sample(x)).collect();
    let b: Vec<f64> = input.iter().map(|&x| m.get_sample(x)).collect();
    assert!(max_abs_diff(&a, &b) <= TOL);
}

#[test]
fn biquad_matches_cpp() {
    let input = noise(4, N);
    let modes = [
        (biquad_mode::BYPASS, BiquadMode::Bypass),
        (biquad_mode::LOWPASS6, BiquadMode::Lowpass6),
        (biquad_mode::LOWPASS12, BiquadMode::Lowpass12),
        (biquad_mode::HIGHPASS6, BiquadMode::Highpass6),
        (biquad_mode::HIGHPASS12, BiquadMode::Highpass12),
        (biquad_mode::BANDPASS, BiquadMode::Bandpass),
        (biquad_mode::BANDREJECT, BiquadMode::Bandreject),
        (biquad_mode::PEAK, BiquadMode::Peak),
        (biquad_mode::LOW_SHELF, BiquadMode::LowShelf),
    ];
    for (cm, rm) in modes {
        for freq in CUTOFFS {
            for (gain, bw) in [(0.0, 1.0), (6.0, 2.0)] {
                let mut r = RefBiquad::new();
                r.set_sample_rate(FS);
                r.set_mode(cm);
                r.set_frequency(freq);
                r.set_gain(gain);
                r.set_bandwidth(bw);
                let mut m = BiquadFilter::new();
                m.set_sample_rate(FS);
                m.set_mode(rm);
                m.set_frequency(freq);
                m.set_gain(gain);
                m.set_bandwidth(bw);
                assert_eq!(r.get_mode(), m.get_mode() as i32);
                assert_eq!(r.get_frequency(), m.get_frequency());
                assert_eq!(r.get_gain(), m.get_gain());
                assert_eq!(r.get_bandwidth(), m.get_bandwidth());
                let a: Vec<f64> = input.iter().map(|&x| r.get_sample(x)).collect();
                let b: Vec<f64> = input.iter().map(|&x| m.get_sample(x)).collect();
                let d = max_abs_diff(&a, &b);
                println!("biquad mode={cm} freq={freq} gain={gain} bw={bw}: {d:e}");
                assert!(d <= TOL, "mode={cm} freq={freq} diff {d:e}");
            }
        }
    }
}

#[test]
fn biquad_defaults_match_cpp() {
    let mut r = RefBiquad::new();
    let m = BiquadFilter::new();
    assert_eq!(r.get_bandwidth(), m.get_bandwidth());
    assert_eq!(r.get_frequency(), m.get_frequency());
}

#[test]
fn elliptic_qb_matches_cpp() {
    let input = noise(5, N);
    let mut r = RefEllipticQB::new();
    let mut m = EllipticQuarterBandFilter::new();
    let a: Vec<f64> = input.iter().map(|&x| r.get_sample(x)).collect();
    let b: Vec<f64> = input.iter().map(|&x| m.get_sample(x)).collect();
    let d = max_abs_diff(&a, &b);
    println!("elliptic_qb: {d:e}");
    assert!(d <= TOL, "diff {d:e}");
    r.reset();
    m.reset();
    assert_eq!(r.get_sample(0.5), m.get_sample(0.5));
}
