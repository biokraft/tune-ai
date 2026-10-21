//! Behaviour of the tempo-synced ping-pong delay.

mod common;

use common::{noise, rms, tone};
use tune_dsp::{DELAY_MAX_STEPS, DELAY_PARAMS, Delay, ParamError};

const SR: f64 = 48000.0;

fn make(sps: f64, params: &[(&str, f64)]) -> Delay {
    let mut d = Delay::new(SR, sps);
    for (id, v) in params {
        d.set_param(id, *v).unwrap();
    }
    d
}

const OFF: [(&str, f64); 2] = [("delay.low_cut", 20.0), ("delay.high_cut", 20000.0)];

fn run(d: &mut Delay, input: &[f64]) -> (Vec<f64>, Vec<f64>) {
    input.iter().map(|&x| d.process(x)).unzip()
}

fn impulse(n: usize) -> Vec<f64> {
    let mut v = vec![0.0; n];
    v[0] = 1.0;
    v
}

#[test]
fn echo_on_the_dotted_eighth() {
    let mut d = make(6000.0, &OFF);
    d.set_param("delay.width", 100.0).unwrap();
    d.set_param("delay.feedback", 50.0).unwrap();
    let (l, r) = run(&mut d, &impulse(60000));
    assert_eq!(l[18000], 1.0);
    assert_eq!(r[36000], 0.5);
    assert_eq!(l[54000], 0.25);
    for i in 0..60000 {
        if i != 18000 && i != 54000 {
            assert_eq!(l[i], 0.0, "L[{i}]");
        }
        if i != 36000 {
            assert_eq!(r[i], 0.0, "R[{i}]");
        }
    }
}

#[test]
fn width_zero_is_mono() {
    let mut d = make(6000.0, &[("delay.width", 0.0)]);
    let (l, r) = run(&mut d, &noise(3, 2 * SR as usize));
    for (a, b) in l.iter().zip(&r) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn fractional_time_interpolates() {
    let sps = 48000.0 * 15.0 / 145.0;
    let mut d = make(sps, &OFF);
    d.set_param("delay.feedback", 0.0).unwrap();
    let (l, _) = run(&mut d, &impulse(20000));
    let frac = (3.0 * sps).fract();
    assert!((l[14896] - (1.0 - frac)).abs() < 1e-12, "{}", l[14896]);
    assert!((l[14897] - frac).abs() < 1e-12, "{}", l[14897]);
    for (i, v) in l.iter().enumerate() {
        if i != 14896 && i != 14897 {
            assert_eq!(*v, 0.0, "L[{i}]");
        }
    }
}

fn burst(freq: f64, n: usize) -> Vec<f64> {
    let mut v = tone(freq, SR, 4800);
    v.resize(n, 0.0);
    v
}

fn first_echo_db(freq: f64) -> f64 {
    let mut d = make(6000.0, &[]);
    let (l, _) = run(&mut d, &burst(freq, 30000));
    let echo = &l[18000..22800];
    20.0 * (rms(echo) / rms(&tone(freq, SR, 4800))).log10()
}

#[test]
fn filters_shape_the_repeats() {
    assert!(first_echo_db(50.0) <= -12.0, "{}", first_echo_db(50.0));
    assert!(
        first_echo_db(1000.0).abs() <= 1.0,
        "{}",
        first_echo_db(1000.0)
    );
}

#[test]
fn max_feedback_is_stable() {
    let mut d = make(6000.0, &[("delay.feedback", 90.0)]);
    let sr = SR as usize;
    let mut input = noise(11, 10 * sr);
    input.resize(30 * sr, 0.0);
    let (l, r) = run(&mut d, &input);
    for v in l.iter().chain(&r) {
        assert!(v.is_finite() && v.abs() < 10.0);
    }
    let after = |c: &[f64]| rms(&c[10 * sr..11 * sr]);
    let last = |c: &[f64]| rms(&c[29 * sr..]);
    assert!(last(&l) < 0.01 * after(&l), "L {} {}", last(&l), after(&l));
    assert!(last(&r) < 0.01 * after(&r), "R {} {}", last(&r), after(&r));
}

#[test]
fn silence_stays_exactly_zero() {
    let mut d = make(6000.0, &[("delay.feedback", 90.0)]);
    for _ in 0..200_000 {
        assert_eq!(d.process(0.0), (0.0, 0.0));
    }
}

#[test]
fn buffer_holds_eight_steps_at_40_bpm() {
    let sps = 18000.0;
    let mut d = make(sps, &OFF);
    d.set_param("delay.time", DELAY_MAX_STEPS).unwrap();
    d.set_param("delay.feedback", 0.0).unwrap();
    let (l, _) = run(&mut d, &impulse(150_000));
    assert_eq!(l[144_000], 1.0);
    assert_eq!(l.iter().filter(|v| **v != 0.0).count(), 1);
}

#[test]
fn time_automation_never_reads_outside() {
    let sr = 384_000.0;
    let sps = sr * 15.0 / 40.0;
    let mut d = Delay::new(sr, sps);
    let time = DELAY_PARAMS
        .iter()
        .position(|s| s.id == "delay.time")
        .unwrap();
    d.set_param("delay.feedback", 90.0).unwrap();
    let n = 10 * sr as usize;
    let input = noise(5, n);
    for (i, &x) in input.iter().enumerate() {
        let phase = (i % 2_000_000) as f64 / 1_000_000.0;
        let tri = if phase < 1.0 { phase } else { 2.0 - phase };
        d.set_param_at(time, 1.0 + 7.0 * tri);
        let (l, r) = d.process(x);
        assert!(l.is_finite() && r.is_finite());
    }
}

#[test]
fn bad_params_are_rejected() {
    let mut d = Delay::new(SR, 6000.0);
    assert!(matches!(
        d.set_param("delay.nope", 1.0),
        Err(ParamError::UnknownId(_))
    ));
    assert!(matches!(
        d.set_param("delay.time", 9.0),
        Err(ParamError::OutOfRange { .. })
    ));
    assert_eq!(d.samples_per_step(), 6000.0);
}
