use open303_ref::consts::{biquad_mode, one_pole_mode, seq_mode, tee_bee_mode, waveform};
use open303_ref::*;

#[test]
fn leaky_integrator_steps_toward_input() {
    let mut f = RefLeakyIntegrator::new();
    f.set_sample_rate(48000.0);
    f.set_time_constant(5.0);
    let a = f.get_sample(1.0);
    let b = f.get_sample(1.0);
    assert!(0.0 < a && a < b && b < 1.0);
    assert_eq!(f.get_time_constant(), 5.0);
}

#[test]
fn one_pole_biquad_and_elliptic_filters_run() {
    let mut p = RefOnePole::new();
    p.set_sample_rate(48000.0);
    p.set_mode(one_pole_mode::LOWPASS);
    p.set_cutoff(1000.0);
    assert!(p.get_sample(1.0).is_finite());

    let mut b = RefBiquad::new();
    b.set_sample_rate(48000.0);
    b.set_mode(biquad_mode::LOWPASS12);
    b.set_frequency(1000.0);
    assert!(b.get_sample(1.0).is_finite());

    let mut e = RefEllipticQB::new();
    assert!(e.get_sample(1.0).is_finite());
}

#[test]
fn envelopes_run() {
    let mut d = RefDecayEnv::new();
    d.set_sample_rate(48000.0);
    d.set_decay_time_constant(100.0);
    d.trigger();
    let first = d.get_sample();
    assert!(first > d.get_sample());

    let mut a = RefAnalogEnv::new();
    a.set_sample_rate(48000.0);
    a.set_attack(1.0);
    a.set_decay(100.0);
    a.set_release(100.0);
    a.note_on(false, 64, 64);
    assert!(a.is_note_on());
    assert!(a.get_sample().is_finite());
}

#[test]
fn wavetable_and_blend_osc_run() {
    let mut w = RefWaveTable::new();
    let t = w.sample_table(waveform::SAW, 0);
    assert_eq!(t.len(), TABLE_LENGTH);
    assert!(t.iter().all(|x| x.is_finite()));
    assert!(t.iter().any(|x| x.abs() > 0.1));

    let mut o = RefBlendOsc::new();
    o.set_sample_rate(48000.0);
    o.set_wave_form1(waveform::SAW);
    o.set_wave_form2(waveform::SQUARE);
    o.set_blend_factor(0.5);
    o.set_frequency(220.0);
    o.calculate_increment();
    let s: Vec<f64> = (0..256).map(|_| o.get_sample()).collect();
    assert!(s.iter().any(|x| x.abs() > 0.1));
}

#[test]
fn teebee_filter_runs() {
    let mut f = RefTeeBee::new();
    f.set_sample_rate(48000.0);
    f.set_mode(tee_bee_mode::TB_303);
    f.set_cutoff(1000.0, true);
    f.set_resonance(50.0, true);
    assert!(f.get_sample(0.5).is_finite());
}

#[test]
fn sequencer_plays_a_pattern() {
    let mut synth = RefOpen303::new(48000.0);
    synth.set(RefParam::Cutoff, 1000.0);
    synth.seq_set_bpm(120.0);
    for i in 0..4 {
        synth.seq_set_step(i, 0, 0, false, false, true);
    }
    synth.seq_set_num_steps(4);
    synth.seq_set_mode(seq_mode::KEY_SYNC);
    synth.note_on(48, 100);
    let out = synth.render(48000);
    assert!(out.iter().all(|x| x.is_finite()));
    assert!(out.iter().any(|x| x.abs() > 1e-3));
}

#[test]
fn param_round_trip() {
    let mut synth = RefOpen303::new(44100.0);
    synth.set(RefParam::Tuning, 432.0);
    assert_eq!(synth.get(RefParam::Tuning), 432.0);
    synth.set(RefParam::Cutoff, 1500.0);
    assert_eq!(synth.get(RefParam::Cutoff), 1500.0);
}
