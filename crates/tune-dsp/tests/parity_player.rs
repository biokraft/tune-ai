//! Sample-for-sample parity of `AcidPlayer` + `Acid303` against the C++
//! `Open303` running its own `AcidSequencer` (KEY_SYNC mode).
//!
//! The C++ pattern stores each step as key 0..=12 plus octave relative to the
//! note that holds the sequencer running, so every scenario holds root 36
//! (`C2`) and converts the absolute notes of our steps to key/octave. That
//! limits parity patterns to C1..=C4 and 16 steps; longer patterns are a
//! tune-ai extension covered by `player_behaviour.rs`.
//!
//! The C++ `BlendOscillator::blend` member is never initialised, so every
//! scenario sets the waveform (and every other parameter) on both sides.

mod common;

use common::max_abs_diff;
use open303_ref::consts::seq_mode;
use open303_ref::{RefOpen303, RefParam, SEQ_MAX_STEPS};
use tune_dsp::acid303::{Acid303, AcidPattern, AcidPlayer};

const FS: f64 = 48_000.0;
const TOL: f64 = 1e-9;
const ROOT: i32 = 36;
const BARS: f64 = 8.0;

/// Calls the matching Rust setter.
fn set_rust(v: &mut Acid303, p: RefParam, x: f64) {
    match p {
        RefParam::Waveform => v.set_waveform(x),
        RefParam::Tuning => v.set_tuning(x),
        RefParam::Cutoff => v.set_cutoff(x),
        RefParam::Resonance => v.set_resonance(x),
        RefParam::EnvMod => v.set_env_mod(x),
        RefParam::Decay => v.set_decay(x),
        RefParam::Accent => v.set_accent(x),
        RefParam::Volume => v.set_volume(x),
        RefParam::AmpSustain => v.set_amp_sustain(x),
        RefParam::TanhShaperDrive => v.set_tanh_shaper_drive(x),
        RefParam::TanhShaperOffset => v.set_tanh_shaper_offset(x),
        RefParam::PreFilterHighpass => v.set_pre_filter_highpass(x),
        RefParam::FeedbackHighpass => v.set_feedback_highpass(x),
        RefParam::PostFilterHighpass => v.set_post_filter_highpass(x),
        RefParam::SquarePhaseShift => v.set_square_phase_shift(x),
        RefParam::SlideTime => v.set_slide_time(x),
        RefParam::NormalAttack => v.set_normal_attack(x),
        RefParam::AccentAttack => v.set_accent_attack(x),
        RefParam::AccentDecay => v.set_accent_decay(x),
        RefParam::AmpDecay => v.set_amp_decay(x),
        RefParam::AmpRelease => v.set_amp_release(x),
    }
}

/// Every `Open303` parameter, set explicitly: a classic saw setting close to
/// the constructor values.
const CLASSIC: &[(RefParam, f64)] = &[
    (RefParam::Waveform, 0.0),
    (RefParam::Tuning, 440.0),
    (RefParam::Cutoff, 1000.0),
    (RefParam::Resonance, 0.0),
    (RefParam::EnvMod, 25.0),
    (RefParam::Decay, 1000.0),
    (RefParam::Accent, 0.0),
    (RefParam::Volume, -12.0),
    (RefParam::AmpSustain, -60.0),
    (RefParam::TanhShaperDrive, 36.9),
    (RefParam::TanhShaperOffset, 4.37),
    (RefParam::PreFilterHighpass, 44.486),
    (RefParam::FeedbackHighpass, 150.0),
    (RefParam::PostFilterHighpass, 24.167),
    (RefParam::SquarePhaseShift, 189.0),
    (RefParam::SlideTime, 60.0),
    (RefParam::NormalAttack, 3.0),
    (RefParam::AccentAttack, 3.0),
    (RefParam::AccentDecay, 200.0),
    (RefParam::AmpDecay, 1230.0),
    (RefParam::AmpRelease, 1.0),
];

/// A squelchy square-wave setting with every parameter off its default.
const SQUELCHY: &[(RefParam, f64)] = &[
    (RefParam::Waveform, 1.0),
    (RefParam::Tuning, 432.0),
    (RefParam::Cutoff, 600.0),
    (RefParam::Resonance, 92.0),
    (RefParam::EnvMod, 85.0),
    (RefParam::Decay, 300.0),
    (RefParam::Accent, 100.0),
    (RefParam::Volume, -6.0),
    (RefParam::AmpSustain, -18.0),
    (RefParam::TanhShaperDrive, 30.0),
    (RefParam::TanhShaperOffset, 3.5),
    (RefParam::PreFilterHighpass, 60.0),
    (RefParam::FeedbackHighpass, 120.0),
    (RefParam::PostFilterHighpass, 30.0),
    (RefParam::SquarePhaseShift, 90.0),
    (RefParam::SlideTime, 120.0),
    (RefParam::NormalAttack, 5.0),
    (RefParam::AccentAttack, 2.0),
    (RefParam::AccentDecay, 300.0),
    (RefParam::AmpDecay, 800.0),
    (RefParam::AmpRelease, 20.0),
];

/// Converts an absolute note to the C++ key (0..=12) and octave relative to
/// [`ROOT`].
fn key_octave(note: u8) -> (i32, i32) {
    let rel = i32::from(note) - ROOT;
    let octave = if rel < 0 {
        -1
    } else if rel <= 12 {
        0
    } else {
        1
    };
    let key = rel - 12 * octave;
    assert!(
        (0..=12).contains(&key),
        "note {note} is outside C1..=C4 and cannot be expressed in the C++ pattern"
    );
    (key, octave)
}

/// Renders `pattern` for [`BARS`] bars through both implementations and
/// returns (C++, Rust).
fn render_both(pattern: &str, bpm: f64, settings: &[(RefParam, f64)]) -> (Vec<f64>, Vec<f64>) {
    let pat = AcidPattern::parse(pattern).unwrap();
    assert!(pat.steps().len() <= SEQ_MAX_STEPS);

    let mut r = RefOpen303::new(FS);
    // The C++ constructor leaves the normalizers n1/n2 uninitialised and only
    // triggerNote / slideToNote assign them (always 1.0). In sequencer mode
    // the DSP chain runs from the start, so a pattern that opens with a rest
    // reads them before any note: undefined behaviour that in practice gives
    // 0.0 for fresh heap memory and 1.0 for reused memory. Pin them to the
    // 1.0 the C++ always writes (and the Rust port starts with).
    r.set_normalizers(1.0, 1.0);
    let mut v = Acid303::new(FS);
    for &(p, x) in settings {
        r.set(p, x);
        set_rust(&mut v, p, x);
    }

    r.seq_set_mode(seq_mode::KEY_SYNC);
    r.seq_set_bpm(bpm);
    for (i, st) in pat.steps().iter().enumerate() {
        let (key, octave) = st.note.map_or((0, 0), key_octave);
        r.seq_set_step(i, key, octave, st.accent, st.slide, st.note.is_some());
    }
    r.seq_set_num_steps(pat.steps().len());
    r.note_on(ROOT, 100); // starts the sequencer, holds the root

    let mut player = AcidPlayer::new(pat, bpm, FS);
    let n = (BARS * 4.0 * 60.0 / bpm * FS).round() as usize;
    let mut cpp = Vec::with_capacity(n);
    let mut rust = Vec::with_capacity(n);
    for _ in 0..n {
        cpp.push(r.get_sample());
        player.tick(&mut v);
        rust.push(v.get_sample());
    }
    (cpp, rust)
}

fn check(label: &str, pattern: &str, bpm: f64, settings: &[(RefParam, f64)]) {
    let (cpp, rust) = render_both(pattern, bpm, settings);
    assert!(rust.iter().all(|x| x.is_finite()), "{label}: non-finite");
    assert!(
        cpp.iter().any(|x| x.abs() > 1e-3),
        "{label}: reference render is silent"
    );
    let d = max_abs_diff(&cpp, &rust);
    println!("{label} @ {bpm} BPM: max abs diff {d:e}");
    assert!(
        d <= TOL,
        "{label} @ {bpm} BPM: max abs diff {d:e} > {TOL:e}"
    );
}

const BPMS: [f64; 2] = [145.0, 133.7];

/// Accents, rests, slides, a same-pitch slide (C3!~ C3), a slide into a rest
/// (F2~ .) and no slide on the last step.
const P_CLASSIC: &str = "C2 C2! . D#2~ F2 C3!~ C3 . G1 F2~ . F#2 . C2~ D#2! A#1";

/// Slide chains, the C1 and C4 range ends, a same-pitch slide (C2~ C2 at
/// steps 13-14) and a slide on the last step that wraps into the gated
/// step 0.
const P_WRAP_SLIDE: &str = "C2! D2~ E2~ F2 . G2!~ A2 B2 C4~ C2 . . C1!~ C2~ C2 G1~";

/// Starts on a rest; the last step slides into that rest (a normal gate).
const P_REST_FIRST: &str = ". C2~ C2~ C2 G2! . C1~ C4! . D2 D2~ E2! . A#3~ G3 C2~";

/// A 7-step loop (odd length, wraps mid-bar) with a wrapping slide.
const P_SEVEN: &str = "C2! . D#2~ D#2 G1! C3~ A#2~";

#[test]
fn classic_pattern() {
    for bpm in BPMS {
        check("classic / classic", P_CLASSIC, bpm, CLASSIC);
        check("classic / squelchy", P_CLASSIC, bpm, SQUELCHY);
    }
}

#[test]
fn slide_wrapping_into_step_0() {
    for bpm in BPMS {
        check("wrap slide / classic", P_WRAP_SLIDE, bpm, CLASSIC);
        check("wrap slide / squelchy", P_WRAP_SLIDE, bpm, SQUELCHY);
    }
}

#[test]
fn pattern_starting_with_a_rest() {
    for bpm in BPMS {
        check("rest first / classic", P_REST_FIRST, bpm, CLASSIC);
        check("rest first / squelchy", P_REST_FIRST, bpm, SQUELCHY);
    }
}

#[test]
fn seven_step_loop() {
    for bpm in BPMS {
        check("seven steps / squelchy", P_SEVEN, bpm, SQUELCHY);
    }
}

#[test]
fn all_rests_are_silent_on_both_sides() {
    for bpm in BPMS {
        // Not bitwise zero on either side: the C++ sequencer runs the DSP
        // chain through rests, and the decaying amp envelope leaves residue
        // around 1e-35 (measured C++ peak 5.85e-36 at 145 and 133.7 BPM).
        let (cpp, rust) = render_both(". . . . . . . . . . . . . . . .", bpm, CLASSIC);
        let d = max_abs_diff(&cpp, &rust);
        let peak = rust.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
        println!("all rests @ {bpm} BPM: max abs diff {d:e}, Rust peak {peak:e}");
        assert!(d <= TOL, "all rests @ {bpm} BPM: max abs diff {d:e}");
        assert!(peak < 1e-30, "all rests @ {bpm} BPM: peak {peak:e}");
    }
}
