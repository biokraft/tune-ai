//! Behaviour of the short Freeverb-style reverb.

mod common;

use common::{noise, rms, tone};
use tune_dsp::{Biquad, BiquadKind, ParamError, REVERB_PARAMS, Reverb};

const SR: f64 = 48000.0;

fn make(params: &[(&str, f64)]) -> Reverb {
    let mut r = Reverb::new(SR);
    for (id, v) in params {
        r.set_param(id, *v).unwrap();
    }
    r
}

fn run(r: &mut Reverb, input: &[f64]) -> (Vec<f64>, Vec<f64>) {
    input.iter().map(|&x| r.process(x)).unzip()
}

fn impulse(n: usize) -> Vec<f64> {
    let mut v = vec![0.0; n];
    v[0] = 1.0;
    v
}

fn energy(x: &[f64]) -> f64 {
    x.iter().map(|v| v * v).sum()
}

fn at(sec: f64) -> usize {
    (sec * SR) as usize
}

fn db(ratio: f64) -> f64 {
    10.0 * ratio.log10()
}

#[test]
fn onset_is_pre_delay_plus_shortest_comb() {
    let mut r = make(&[("reverb.low_cut", 20.0), ("reverb.pre_delay", 0.0)]);
    let (l, _) = run(&mut r, &impulse(10000));
    assert_eq!(l.iter().position(|&v| v != 0.0), Some(1215));

    let mut r = make(&[("reverb.low_cut", 20.0), ("reverb.pre_delay", 50.0)]);
    let (l, _) = run(&mut r, &impulse(10000));
    assert_eq!(l.iter().position(|&v| v != 0.0), Some(2400 + 1215));
}

#[test]
fn impulse_tail_decays() {
    let mut r = make(&[]);
    let (l, _) = run(&mut r, &impulse(at(2.0)));
    let early = energy(&l[..at(0.1)]);
    let late = energy(&l[at(1.5)..at(2.0)]);
    assert!(early > 0.0);
    assert!(db(late / early) <= -60.0, "{}", db(late / early));
}

fn decay_time(size: f64) -> usize {
    let mut r = make(&[("reverb.size", size)]);
    let (l, _) = run(&mut r, &impulse(at(6.0)));
    let win = at(0.01);
    let env: Vec<f64> = l.chunks(win).map(rms).collect();
    let peak = env.iter().cloned().fold(0.0, f64::max);
    let thr = peak * 10f64.powf(-40.0 / 20.0);
    let top = env.iter().position(|&v| v == peak).unwrap();
    // last window still above the threshold
    let last = env.iter().rposition(|&v| v > thr).unwrap();
    assert!(last >= top);
    (last - top) * win
}

#[test]
fn size_lengthens_the_tail() {
    let small = decay_time(20.0);
    let large = decay_time(80.0);
    assert!(
        large as f64 >= 1.5 * small as f64,
        "small {small}, large {large}"
    );
}

fn high_energy(damp: f64) -> f64 {
    let mut r = make(&[("reverb.damp", damp)]);
    let (l, _) = run(&mut r, &impulse(at(1.0)));
    let mut hp = Biquad::new();
    hp.set(
        BiquadKind::Highpass,
        6000.0,
        std::f64::consts::FRAC_1_SQRT_2,
        SR,
    );
    let hi: Vec<f64> = l[at(0.2)..].iter().map(|&x| hp.process(x)).collect();
    energy(&hi)
}

#[test]
fn damp_darkens() {
    let bright = high_energy(0.0);
    let dark = high_energy(90.0);
    assert!(db(bright / dark) >= 6.0, "{}", db(bright / dark));
}

#[test]
fn width_zero_is_mono() {
    let mut r = make(&[("reverb.width", 0.0)]);
    let (l, rr) = run(&mut r, &noise(3, 2 * SR as usize));
    for (a, b) in l.iter().zip(&rr) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn wide_is_decorrelated() {
    let mut r = make(&[("reverb.width", 100.0)]);
    let (l, rr) = run(&mut r, &noise(5, 4 * SR as usize));
    let dot: f64 = l.iter().zip(&rr).map(|(a, b)| a * b).sum();
    let corr = dot / (energy(&l) * energy(&rr)).sqrt();
    assert!(corr.abs() < 0.5, "{corr}");
}

#[test]
fn low_cut_keeps_sub_out() {
    let n = 2 * SR as usize;
    let mut r = make(&[]);
    let (l50, _) = run(&mut r, &tone(50.0, SR, n));
    let mut r = make(&[]);
    let (l2k, _) = run(&mut r, &tone(2000.0, SR, n));
    let a = rms(&l50[n / 2..]);
    let b = rms(&l2k[n / 2..]);
    assert!(db(a * a / (b * b)) <= -20.0, "{}", db(a * a / (b * b)));
}

#[test]
fn deterministic_and_silent() {
    let mut r = make(&[]);
    let (l, rr) = run(&mut r, &vec![0.0; 20000]);
    assert!(l.iter().chain(&rr).all(|&v| v == 0.0));

    let x = noise(9, 30000);
    let (a, b) = run(&mut make(&[("reverb.pre_delay", 20.0)]), &x);
    let (c, d) = run(&mut make(&[("reverb.pre_delay", 20.0)]), &x);
    assert!(a.iter().zip(&c).all(|(p, q)| p.to_bits() == q.to_bits()));
    assert!(b.iter().zip(&d).all(|(p, q)| p.to_bits() == q.to_bits()));
}

#[test]
fn params_validate() {
    assert_eq!(REVERB_PARAMS.len(), 5);
    let mut r = Reverb::new(SR);
    assert!(matches!(
        r.set_param("reverb.nope", 1.0),
        Err(ParamError::UnknownId(_))
    ));
    assert!(r.set_param("reverb.size", 101.0).is_err());
    r.set_param_at(0, f64::NAN);
    let (l, _) = r.process(1.0);
    assert!(l.is_finite());
}
