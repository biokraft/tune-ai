//! Behaviour of the lookahead true-peak master limiter.

mod common;

use common::noise;
use std::f64::consts::PI;
use tune_analysis::{Audio, measure_loudness};
use tune_dsp::fx::{
    DETECTOR_DELAY, LIMITER_LOOKAHEAD_MS, Limiter, MASTER_CEILING, MASTER_GAIN, MASTER_PARAMS,
    MASTER_RELEASE,
};
use tune_dsp::{Curve, db2amp};

const SR: f64 = 48000.0;

/// Ceiling the true peak must stay under: the default -1.5 dB plus 0.3 dB.
const TRUE_PEAK_MAX_DBTP: f64 = -1.2;

struct Out {
    l: Vec<f64>,
    r: Vec<f64>,
    g: Vec<f64>,
}

fn run(lim: &mut Limiter, l: &[f64], r: &[f64]) -> Out {
    assert_eq!(l.len(), r.len());
    let mut out = Out {
        l: Vec::with_capacity(l.len()),
        r: Vec::with_capacity(l.len()),
        g: Vec::with_capacity(l.len()),
    };
    for (&a, &b) in l.iter().zip(r) {
        let (ol, or, g) = lim.process(a, b);
        out.l.push(ol);
        out.r.push(or);
        out.g.push(g);
    }
    out
}

fn sine(freq: f64, amp: f64, phase: f64, n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| amp * (2.0 * PI * freq * i as f64 / SR + phase).sin())
        .collect()
}

fn seconds(s: f64) -> usize {
    (s * SR).round() as usize
}

fn sample_peak(x: &[f64]) -> f64 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

fn true_peak_dbtp(out: &Out) -> f64 {
    let audio = Audio {
        sample_rate: SR as u32,
        channels: vec![
            out.l.iter().map(|&v| v as f32).collect(),
            out.r.iter().map(|&v| v as f32).collect(),
        ],
    };
    measure_loudness(&audio).unwrap().0.true_peak_dbtp
}

#[test]
fn latency_is_lookahead_plus_detector() {
    assert_eq!(LIMITER_LOOKAHEAD_MS, 1.5);
    assert_eq!(DETECTOR_DELAY, 12);
    assert_eq!(Limiter::new(48000.0).latency(), 84);
    assert_eq!(Limiter::new(44100.0).latency(), 78);
}

#[test]
fn transparent_below_ceiling() {
    let mut lim = Limiter::new(SR);
    let lat = lim.latency();
    assert_eq!(lat, 84);
    let x = sine(1000.0, db2amp(-12.0), 0.0, seconds(1.0));
    let out = run(&mut lim, &x, &x);
    assert!(out.g.iter().all(|&g| g == 1.0));
    assert!(out.l[..lat].iter().all(|&v| v == 0.0));
    for (n, v) in x[..x.len() - lat].iter().enumerate() {
        assert_eq!(out.l[n + lat].to_bits(), v.to_bits(), "frame {n}");
        assert_eq!(out.r[n + lat].to_bits(), v.to_bits(), "frame {n}");
    }
}

#[test]
fn sample_peak_never_exceeds_ceiling() {
    let c = db2amp(-1.5);
    let hot = db2amp(12.0);
    let n = seconds(2.0);

    let mut lim = Limiter::new(SR);
    let l: Vec<f64> = noise(1, n).iter().map(|v| v * hot).collect();
    let r: Vec<f64> = noise(2, n).iter().map(|v| v * hot).collect();
    let out = run(&mut lim, &l, &r);
    assert!(sample_peak(&out.l) <= c, "{}", sample_peak(&out.l));
    assert!(sample_peak(&out.r) <= c, "{}", sample_peak(&out.r));

    let mut lim = Limiter::new(SR);
    let x = sine(1000.0, hot, 0.0, n);
    let out = run(&mut lim, &x, &x);
    assert!(sample_peak(&out.l) <= c, "{}", sample_peak(&out.l));
    assert!(sample_peak(&out.r) <= c, "{}", sample_peak(&out.r));
}

#[test]
fn true_peak_stays_under_ceiling() {
    let n = seconds(5.0);

    // (a) A sine at fs/4 sampled 45 degrees off its peaks: every sample sits
    // 3 dB under the true peak.
    let x = sine(12000.0, 2.0, PI / 4.0, n);
    let out = run(&mut Limiter::new(SR), &x, &x);
    let tp = true_peak_dbtp(&out);
    eprintln!("12 kHz sine: {tp:.3} dBTP");
    assert!(tp <= TRUE_PEAK_MAX_DBTP, "12 kHz sine: {tp} dBTP");

    // (b) Hot white noise: full energy up to Nyquist, where inter-sample
    // peaks run about 4 dB over the sample peak.
    let l: Vec<f64> = noise(3, n).iter().map(|v| v * 4.0).collect();
    let r: Vec<f64> = noise(4, n).iter().map(|v| v * 4.0).collect();
    let out = run(&mut Limiter::new(SR), &l, &r);
    let tp = true_peak_dbtp(&out);
    eprintln!("noise x4: {tp:.3} dBTP");
    assert!(tp <= TRUE_PEAK_MAX_DBTP, "noise x4: {tp} dBTP");

    // (c) 50 Hz bursts (4 per second, 80 ms each, amplitude 3) over a 3 kHz
    // naive saw at amplitude 1: steep edges on top of a pumping low end.
    let burst = seconds(0.08);
    let period = seconds(0.25);
    let x: Vec<f64> = (0..n)
        .map(|i| {
            let t = i as f64 / SR;
            let b = if i % period < burst {
                3.0 * (2.0 * PI * 50.0 * t).sin()
            } else {
                0.0
            };
            let phase = (3000.0 * t).fract();
            b + (2.0 * phase - 1.0)
        })
        .collect();
    let out = run(&mut Limiter::new(SR), &x, &x);
    let tp = true_peak_dbtp(&out);
    eprintln!("bursts + saw: {tp:.3} dBTP");
    assert!(tp <= TRUE_PEAK_MAX_DBTP, "bursts + saw: {tp} dBTP");
}

#[test]
fn releases_back_to_unity() {
    // From a gain of about 0.42 the 100 ms release needs ln(0.58 / 1e-9),
    // about 20.2 time constants (2.02 s), to reach the 1e-9 snap, so allow
    // 2.1 s.
    let loud = seconds(0.1);
    let quiet = seconds(2.1);
    let mut x = sine(1000.0, db2amp(6.0), 0.0, loud);
    x.extend(sine(1000.0, db2amp(-20.0), 0.0, quiet + seconds(0.5)));
    let mut lim = Limiter::new(SR);
    let out = run(&mut lim, &x, &x);
    assert!(out.g[..loud].iter().any(|&g| g < 0.5));
    let back = out.g[loud..]
        .iter()
        .position(|&g| g == 1.0)
        .expect("gain never returns to 1.0");
    eprintln!(
        "gain back at 1.0 {:.3} s after the loud part",
        back as f64 / SR
    );
    assert!(back <= quiet, "back after {back} frames");
    assert!(out.g[loud + back..].iter().all(|&g| g == 1.0));
}

#[test]
fn extreme_gain_stays_finite() {
    let c = db2amp(-1.5);
    let gain = db2amp(24.0);
    let n = seconds(2.0);
    let l: Vec<f64> = noise(5, n).iter().map(|v| (v + 0.5) * gain).collect();
    let r: Vec<f64> = noise(6, n).iter().map(|v| (v + 0.5) * gain).collect();
    let out = run(&mut Limiter::new(SR), &l, &r);
    assert!(
        out.l
            .iter()
            .chain(&out.r)
            .chain(&out.g)
            .all(|v| v.is_finite())
    );
    assert!(sample_peak(&out.l) <= c);
    assert!(sample_peak(&out.r) <= c);
}

#[test]
fn deterministic() {
    let n = seconds(1.0);
    let l: Vec<f64> = noise(7, n).iter().map(|v| v * 3.0).collect();
    let r: Vec<f64> = noise(8, n).iter().map(|v| v * 3.0).collect();
    let bits = |o: &Out| -> Vec<u64> {
        o.l.iter()
            .chain(&o.r)
            .chain(&o.g)
            .map(|v| v.to_bits())
            .collect()
    };
    let mut a = Limiter::new(SR);
    let first = run(&mut a, &l, &r);
    let second = run(&mut Limiter::new(SR), &l, &r);
    assert_eq!(bits(&first), bits(&second));
    // After a reset the instance plays like a fresh one.
    a.reset();
    assert_eq!(bits(&run(&mut a, &l, &r)), bits(&first));
}

#[test]
fn setters_change_ceiling_and_release() {
    let n = seconds(1.0);
    let x = sine(1000.0, 2.0, 0.0, n);
    let mut lim = Limiter::new(SR);
    lim.set_ceiling_db(-6.0);
    let out = run(&mut lim, &x, &x);
    assert!(sample_peak(&out.l) <= db2amp(-6.0));
    assert!(sample_peak(&out.l) > db2amp(-6.5));

    let release_frames = |ms: f64| {
        let mut lim = Limiter::new(SR);
        lim.set_release_ms(ms);
        let mut y = sine(1000.0, 2.0, 0.0, seconds(0.1));
        y.extend(vec![0.0; seconds(3.0)]);
        let out = run(&mut lim, &y, &y);
        out.g.iter().rposition(|&g| g != 1.0).unwrap()
    };
    assert!(release_frames(10.0) < release_frames(100.0));
}

#[test]
fn master_param_specs_are_pinned() {
    let want = [
        (
            "master.gain",
            "Master gain",
            "dB",
            -12.0,
            24.0,
            Curve::Linear,
            0.0,
        ),
        (
            "master.ceiling",
            "Ceiling",
            "dB",
            -6.0,
            -0.1,
            Curve::Linear,
            -1.5,
        ),
        (
            "master.release",
            "Release",
            "ms",
            10.0,
            500.0,
            Curve::Exponential,
            100.0,
        ),
    ];
    assert_eq!(MASTER_PARAMS.len(), want.len());
    for (p, w) in MASTER_PARAMS.iter().zip(want) {
        assert_eq!(p.id, w.0);
        assert_eq!(p.label, w.1);
        assert_eq!(p.unit, w.2);
        assert_eq!(p.min, w.3);
        assert_eq!(p.max, w.4);
        assert!(p.curve == w.5, "{}", p.id);
        assert_eq!(p.default, w.6);
        assert!(!p.help.is_empty());
    }
    assert_eq!(MASTER_PARAMS[MASTER_GAIN].id, "master.gain");
    assert_eq!(MASTER_PARAMS[MASTER_CEILING].id, "master.ceiling");
    assert_eq!(MASTER_PARAMS[MASTER_RELEASE].id, "master.release");
}
