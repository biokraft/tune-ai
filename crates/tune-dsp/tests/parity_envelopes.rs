//! Sample-for-sample parity of the envelopes against the C++ reference.

use open303_ref::{RefAnalogEnv, RefDecayEnv};
use tune_dsp::acid303::{AnalogEnvelope, DecayEnvelope};

mod common;
use common::max_abs_diff;

const FS: f64 = 48_000.0;
const N: usize = 96_000; // 2 s
const TOL: f64 = 1e-12;

#[test]
fn decay_envelope_matches_cpp() {
    for tau in [200.0, 1000.0, 2000.0] {
        for normalize in [false, true] {
            let mut r = RefDecayEnv::new();
            r.set_sample_rate(FS);
            r.set_decay_time_constant(tau);
            r.set_normalize_sum(normalize);
            let mut m = DecayEnvelope::new();
            m.set_sample_rate(FS);
            m.set_decay_time_constant(tau);
            m.set_normalize_sum(normalize);
            assert_eq!(r.get_decay_time_constant(), m.get_decay_time_constant());
            let (mut a, mut b) = (Vec::new(), Vec::new());
            for i in 0..N {
                if [0, 7000, 7001, 30_000].contains(&i) {
                    r.trigger();
                    m.trigger();
                }
                a.push(r.get_sample());
                b.push(m.get_sample());
                assert_eq!(r.end_is_reached(0.01), m.end_is_reached(0.01));
            }
            let d = max_abs_diff(&a, &b);
            println!("decay tau={tau} norm={normalize}: {d:e}");
            assert!(d <= TOL, "tau={tau} norm={normalize} diff {d:e}");
        }
    }
}

#[test]
fn decay_envelope_defaults_and_rejects_bad_values() {
    let mut r = RefDecayEnv::new();
    let mut m = DecayEnvelope::new();
    r.set_decay_time_constant(0.0005);
    m.set_decay_time_constant(0.0005);
    r.set_sample_rate(-1.0);
    m.set_sample_rate(-1.0);
    assert_eq!(r.get_decay_time_constant(), m.get_decay_time_constant());
    r.trigger();
    m.trigger();
    assert_eq!(r.get_sample(), m.get_sample());
}

fn run_analog(release: f64) -> f64 {
    let mut r = RefAnalogEnv::new();
    let mut m = AnalogEnvelope::new();
    r.set_sample_rate(FS);
    m.set_sample_rate(FS);
    r.set_attack(0.0);
    m.set_attack(0.0);
    r.set_decay(1230.0);
    m.set_decay(1230.0);
    r.set_sustain_level(0.0);
    m.set_sustain_level(0.0);
    r.set_release(release);
    m.set_release(release);
    r.set_tau_scale(1.0);
    m.set_tau_scale(1.0);
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for i in 0..N {
        match i {
            0 | 20_000 => {
                r.note_on(i != 0, 60, 100);
                m.note_on(i != 0, 60, 100);
            }
            12_000 => {
                r.note_off();
                m.note_off();
            }
            _ => {}
        }
        a.push(r.get_sample());
        b.push(m.get_sample());
        assert_eq!(r.is_note_on(), m.is_note_on());
        assert_eq!(r.end_is_reached(), m.end_is_reached());
    }
    max_abs_diff(&a, &b)
}

#[test]
fn analog_envelope_matches_cpp_open303_settings() {
    for release in [0.5, 50.0] {
        let d = run_analog(release);
        println!("analog release={release}: {d:e}");
        assert!(d <= TOL, "release={release} diff {d:e}");
    }
}

#[test]
fn analog_envelope_all_parameters_match_cpp() {
    let mut r = RefAnalogEnv::new();
    let mut m = AnalogEnvelope::new();
    macro_rules! both {
        ($f:ident, $v:expr) => {
            r.$f($v);
            m.$f($v);
        };
    }
    both!(set_sample_rate, 44_100.0);
    both!(set_start_level, 0.1);
    both!(set_peak_level, 0.9);
    both!(set_peak_scale, 1.3);
    both!(set_attack, 20.0);
    both!(set_hold, 30.0);
    both!(set_decay, 150.0);
    both!(set_sustain_in_decibels, -12.0);
    both!(set_release, 80.0);
    both!(set_end_level, 0.02);
    both!(set_time_scale, 1.5);
    both!(set_tau_scale, 0.7);
    assert_eq!(r.get_attack(), m.get_attack());
    assert_eq!(r.get_decay(), m.get_decay());
    assert_eq!(r.get_sustain(), m.get_sustain());
    assert_eq!(r.get_release(), m.get_release());
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for i in 0..60_000 {
        if i == 0 || i == 30_000 {
            r.note_on(false, 64, 64);
            m.note_on(false, 64, 64);
        }
        if i == 20_000 || i == 45_000 {
            r.note_off();
            m.note_off();
        }
        a.push(r.get_sample());
        b.push(m.get_sample());
    }
    let d = max_abs_diff(&a, &b);
    println!("analog all-params: {d:e}");
    assert!(d <= TOL, "diff {d:e}");
}
