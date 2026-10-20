//! Sample-for-sample parity of the wavetables and the blend oscillator
//! against the C++ reference.

use open303_ref::{NUM_TABLES, RefBlendOsc, RefWaveTable, TABLE_LENGTH, consts::waveform as rw};
use tune_dsp::acid303::{BlendOscillator, WaveTable, Waveform};

mod common;
use common::max_abs_diff;

const TOL: f64 = 1e-12;
const OSC_FS: f64 = 192_000.0; // Open303's 4x oversampled rate for 48 kHz

fn all_levels(t: &WaveTable) -> Vec<Vec<f64>> {
    (0..NUM_TABLES).map(|i| t.table(i).to_vec()).collect()
}

fn check_tables(label: &str, r: &mut RefWaveTable, m: &WaveTable, wf: i32) {
    let mut worst = 0.0f64;
    for (lvl, mine) in all_levels(m).iter().enumerate() {
        let theirs = r.sample_table(wf, lvl as i32);
        assert_eq!(theirs.len(), TABLE_LENGTH);
        let d = max_abs_diff(&theirs, mine);
        worst = worst.max(d);
        assert!(d <= TOL, "{label} level {lvl}: diff {d:e}");
    }
    println!("{label}: max abs diff {worst:e}");
}

#[test]
fn saw303_tables_match_cpp() {
    let mut r = RefWaveTable::new();
    let mut m = WaveTable::new();
    m.set_waveform(Waveform::Saw303);
    check_tables("saw303", &mut r, &m, rw::SAW303);
}

#[test]
fn square303_tables_match_cpp_for_both_phase_shifts() {
    for shift in [0.0, 180.0] {
        let mut r = RefWaveTable::new();
        let mut m = WaveTable::new();
        r.set_303_square_phase_shift(shift);
        m.set_303_square_phase_shift(shift);
        m.set_waveform(Waveform::Square303);
        check_tables(
            &format!("square303 shift={shift}"),
            &mut r,
            &m,
            rw::SQUARE303,
        );
    }
}

#[test]
fn square303_shaper_settings_and_default_getters_match() {
    let mut r = RefWaveTable::new();
    let mut m = WaveTable::new();
    assert_eq!(
        r.get_tanh_shaper_drive_for_303_square(),
        m.tanh_shaper_drive_for_303_square()
    );
    assert_eq!(
        r.get_tanh_shaper_offset_for_303_square(),
        m.tanh_shaper_offset_for_303_square()
    );
    assert_eq!(r.get_303_square_phase_shift(), m.square_phase_shift_303());
    r.set_tanh_shaper_drive_for_303_square(30.0);
    m.set_tanh_shaper_drive_for_303_square(30.0);
    r.set_tanh_shaper_offset_for_303_square(3.1);
    m.set_tanh_shaper_offset_for_303_square(3.1);
    r.set_303_square_phase_shift(-90.0);
    m.set_303_square_phase_shift(-90.0);
    m.set_waveform(Waveform::Square303);
    check_tables("square303 custom", &mut r, &m, rw::SQUARE303);
}

#[test]
fn other_waveforms_and_symmetry_match_cpp() {
    for (wf, rwf) in [
        (Waveform::Sine, rw::SINE),
        (Waveform::Triangle, rw::TRIANGLE),
        (Waveform::Square, rw::SQUARE),
        (Waveform::Saw, rw::SAW),
    ] {
        for sym in [0.5, 0.2, 0.9] {
            let mut r = RefWaveTable::new();
            let mut m = WaveTable::new();
            r.set_symmetry(sym);
            m.set_symmetry(sym);
            m.set_waveform(wf);
            check_tables(&format!("{wf:?} sym={sym}"), &mut r, &m, rwf);
        }
    }
}

#[test]
fn get_value_linear_matches_cpp() {
    let mut r = RefWaveTable::new();
    let mut m = WaveTable::new();
    r.set_waveform(rw::SAW303);
    m.set_waveform(Waveform::Saw303);
    let mut worst = 0.0f64;
    for k in 0..5000 {
        let phase = k as f64 * 0.4097;
        for idx in [-1, 0, 3, 11, 20] {
            let a = r.get_value_linear(phase % 2047.0, idx);
            let b = m.get_value_linear(phase % 2047.0, idx);
            worst = worst.max((a - b).abs());
        }
    }
    println!("get_value_linear: {worst:e}");
    assert!(worst <= TOL);
}

#[test]
fn instances_do_not_share_state() {
    // The C++ keeps a static scratch buffer; the port must not. Build two
    // tables interleaved and compare with a table built alone.
    let mut a = WaveTable::new();
    let mut b = WaveTable::new();
    a.set_waveform(Waveform::Saw303);
    b.set_waveform(Waveform::Sine);
    a.set_symmetry(0.5);
    let mut alone = WaveTable::new();
    alone.set_waveform(Waveform::Saw303);
    for i in 0..NUM_TABLES {
        assert_eq!(a.table(i), alone.table(i));
    }
}

fn make_pair() -> (RefBlendOsc, BlendOscillator) {
    let mut r = RefBlendOsc::new();
    let mut m = BlendOscillator::new();
    // exactly what Open303's constructor and setSampleRate do
    r.set_wave_form1(rw::SAW303);
    m.set_wave_form_1(Waveform::Saw303);
    r.set_wave_form2(rw::SQUARE303);
    m.set_wave_form_2(Waveform::Square303);
    r.set_pulse_width(50.0);
    m.set_pulse_width(50.0);
    r.set_sample_rate(OSC_FS);
    m.set_sample_rate(OSC_FS);
    (r, m)
}

fn render(r: &mut RefBlendOsc, m: &mut BlendOscillator, n: usize) -> (Vec<f64>, Vec<f64>) {
    let a = (0..n).map(|_| r.get_sample()).collect();
    let b = (0..n).map(|_| m.get_sample()).collect();
    (a, b)
}

#[test]
fn blend_oscillator_matches_cpp() {
    for blend in [0.0, 0.5, 1.0] {
        for freq in [41.2, 440.0, 3520.0] {
            let (mut r, mut m) = make_pair();
            r.set_blend_factor(blend);
            m.set_blend_factor(blend);
            r.set_frequency(freq);
            m.set_frequency(freq);
            r.calculate_increment();
            m.calculate_increment();
            r.reset_phase();
            m.reset_phase();
            assert_eq!(r.get_increment(), m.increment());
            assert_eq!(r.get_blend_factor(), m.blend_factor());
            let (a, b) = render(&mut r, &mut m, OSC_FS as usize);
            let d = max_abs_diff(&a, &b);
            println!("osc blend={blend} f={freq}: {d:e}");
            assert!(d <= TOL, "blend={blend} f={freq}: diff {d:e}");
        }
    }
}

#[test]
fn blend_oscillator_frequency_change_mid_render() {
    let (mut r, mut m) = make_pair();
    r.set_blend_factor(0.5);
    m.set_blend_factor(0.5);
    let mut worst = 0.0f64;
    let steps = [
        (110.0, 20_000),
        (2093.0, 20_000),
        (55.0, 20_000),
        (7.0, 20_000),
    ];
    for (f, n) in steps {
        r.set_frequency(f);
        m.set_frequency(f);
        r.calculate_increment();
        m.calculate_increment();
        let (a, b) = render(&mut r, &mut m, n);
        worst = worst.max(max_abs_diff(&a, &b));
    }
    println!("osc freq change: {worst:e}");
    assert!(worst <= TOL);
}

#[test]
fn blend_oscillator_ignores_bad_frequency_and_honours_phase() {
    let (mut r, mut m) = make_pair();
    for f in [0.0, -5.0, 20_000.0, 30_000.0] {
        r.set_frequency(f);
        m.set_frequency(f);
    }
    r.calculate_increment();
    m.calculate_increment();
    assert_eq!(r.get_increment(), m.increment());
    r.set_start_phase(90.0);
    m.set_start_phase(90.0);
    r.set_start_phase(400.0);
    m.set_start_phase(400.0);
    r.reset_phase();
    m.reset_phase();
    r.set_phase(100.5);
    m.set_phase(100.5);
    r.set_blend_factor(0.3);
    m.set_blend_factor(0.3);
    let (a, b) = render(&mut r, &mut m, 5000);
    assert!(max_abs_diff(&a, &b) <= TOL);
    r.set_increment(3000.0);
    m.set_increment(3000.0);
    let (a, b) = render(&mut r, &mut m, 5000);
    assert!(max_abs_diff(&a, &b) <= TOL);
}
