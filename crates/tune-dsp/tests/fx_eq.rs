//! Behaviour of the track EQ.

mod common;

use common::{noise, rms, tone};
use tune_dsp::{EQ_PARAMS, Equalizer, ParamError};

const SR: f64 = 48000.0;

/// Steady-state gain in dB of `eq` at `freq` Hz.
fn gain_db(eq: &mut Equalizer, freq: f64) -> f64 {
    eq.reset();
    let n = SR as usize;
    let input = tone(freq, SR, n);
    let out: Vec<f64> = input.iter().map(|&x| eq.process(x)).collect();
    let skip = n / 2;
    20.0 * (rms(&out[skip..]) / rms(&input[skip..])).log10()
}

fn make(params: &[(&str, f64)]) -> Equalizer {
    let mut eq = Equalizer::new(SR);
    for (id, v) in params {
        eq.set_param(id, *v).unwrap();
    }
    eq
}

#[test]
fn defaults_pass_bitwise() {
    let input = noise(7, SR as usize);
    let mut eq = Equalizer::new(SR);
    for &x in &input {
        assert_eq!(eq.process(x).to_bits(), x.to_bits());
    }
}

#[test]
fn low_cut_is_24_db_per_octave() {
    let mut eq = make(&[("eq.low_cut", 100.0)]);
    let g25 = gain_db(&mut eq, 25.0);
    let g100 = gain_db(&mut eq, 100.0);
    let g400 = gain_db(&mut eq, 400.0);
    assert!(g25 <= -40.0, "25 Hz {g25}");
    assert!((g100 + 3.0).abs() <= 0.5, "100 Hz {g100}");
    assert!(g400.abs() <= 0.3, "400 Hz {g400}");
}

#[test]
fn mid_band() {
    let mut eq = make(&[
        ("eq.mid_freq", 1000.0),
        ("eq.mid_gain", 6.0),
        ("eq.mid_q", 1.0),
    ]);
    let g1k = gain_db(&mut eq, 1000.0);
    let g100 = gain_db(&mut eq, 100.0);
    let g10k = gain_db(&mut eq, 10000.0);
    assert!((g1k - 6.0).abs() <= 0.1, "1 kHz {g1k}");
    assert!(g100.abs() <= 0.3, "100 Hz {g100}");
    assert!(g10k.abs() <= 0.3, "10 kHz {g10k}");
}

#[test]
fn high_shelf() {
    let mut eq = make(&[("eq.high_gain", 6.0)]);
    let g16k = gain_db(&mut eq, 16000.0);
    let g200 = gain_db(&mut eq, 200.0);
    assert!((g16k - 6.0).abs() <= 0.5, "16 kHz {g16k}");
    assert!(g200.abs() <= 0.1, "200 Hz {g200}");
}

#[test]
fn high_cut_is_12_db_per_octave() {
    let mut eq = make(&[("eq.high_cut", 2000.0)]);
    let g2k = gain_db(&mut eq, 2000.0);
    let g8k = gain_db(&mut eq, 8000.0);
    assert!((g2k + 3.0).abs() <= 0.5, "2 kHz {g2k}");
    assert!(g8k <= -20.0, "8 kHz {g8k}");
}

#[test]
fn swept_params_stay_finite() {
    let n = 2 * SR as usize;
    // Half-scale noise: a steady +18 dB shelf (7.9x) on full-scale noise
    // legitimately peaks at 8.4, which is response, not instability.
    let input: Vec<f64> = noise(11, n).iter().map(|x| 0.5 * x).collect();
    for (index, spec) in EQ_PARAMS.iter().enumerate() {
        let mut eq = Equalizer::new(SR);
        for (i, &x) in input.iter().enumerate() {
            let phase = i as f64 / n as f64;
            let tri = 1.0 - (2.0 * phase - 1.0).abs();
            eq.set_param_at(index, spec.to_plain(tri));
            let y = eq.process(x);
            assert!(
                y.is_finite() && y.abs() < 8.0,
                "{} sample {i}: {y}",
                spec.id
            );
        }
    }
}

#[test]
fn rejects_out_of_range() {
    let mut eq = Equalizer::new(SR);
    assert!(matches!(
        eq.set_param("eq.mid_gain", 30.0),
        Err(ParamError::OutOfRange { .. })
    ));
    assert!(matches!(
        eq.set_param("eq.nope", 0.0),
        Err(ParamError::UnknownId(_))
    ));
}
