//! Musical behaviour of `AcidPlayer`: pattern edge cases (Review Focus 3),
//! notes at the MIDI range ends (Review Focus 5) and live pattern changes.

use open303_ref::{RefOpen303, RefParam};
use tune_dsp::acid303::{Acid303, AcidPattern, AcidPlayer, AcidStep};

const FS: f64 = 48_000.0;
const BPM: f64 = 145.0;

/// Samples per 16th step at [`BPM`] (fractional; the player rounds).
fn step_len() -> f64 {
    FS * 60.0 / BPM / 4.0
}

/// Gate length in samples: `roundToInt(sr * 0.5 * beatsToSeconds(0.25, bpm))`.
fn gate_len() -> f64 {
    (FS * 0.5 * (0.25 * 60.0 / BPM)).round()
}

fn bars(n: f64) -> usize {
    (n * 16.0 * step_len()).round() as usize
}

fn voice() -> Acid303 {
    let mut v = Acid303::new(FS);
    v.set_waveform(0.0);
    v
}

fn render_with(mut v: Acid303, pattern: AcidPattern, n: usize) -> Vec<f64> {
    let mut p = AcidPlayer::new(pattern, BPM, FS);
    (0..n)
        .map(|_| {
            p.tick(&mut v);
            v.get_sample()
        })
        .collect()
}

fn render(pattern: &str, n: usize) -> Vec<f64> {
    render_with(voice(), AcidPattern::parse(pattern).unwrap(), n)
}

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|s| s * s).sum::<f64>() / x.len() as f64).sqrt()
}

fn peak(x: &[f64]) -> f64 {
    x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

/// Deepest level drop, in dB, of a one-period RMS window sliding from `from`
/// to `to` (window start), relative to the one-period window ending at `from`.
fn worst_dip_db(x: &[f64], from: usize, to: usize, period: usize) -> f64 {
    let reference = rms(&x[from - period..from]);
    let worst = (from..=to)
        .step_by(16)
        .map(|w| rms(&x[w..w + period]))
        .fold(f64::INFINITY, f64::min);
    20.0 * (worst / reference).log10()
}

/// One period of C2 (65.41 Hz) in samples.
fn c2_period() -> usize {
    (FS / 65.406).round() as usize
}

#[test]
fn all_rests_are_silent() {
    // Silent, though not bitwise zero: like the C++ sequencer, the player
    // keeps the voice's DSP chain running through rests, and the decaying amp
    // envelope leaves residue near 1e-35 (peak 3.1e-35 here). The C++
    // `Open303` sequencer does the same: parity_player.rs measures a C++ peak
    // of 5.85e-36 for 16 rests with its classic setting, bit-exact with Rust.
    for pattern in [".", ". . . .", &vec!["."; 64].join(" ")] {
        let out = render(pattern, bars(4.0));
        let p = peak(&out);
        println!("`{pattern}`: peak {p:e}");
        assert!(out.iter().all(|x| x.is_finite()), "`{pattern}`: non-finite");
        assert!(p < 1e-30, "`{pattern}` is not silent: peak {p:e}");
    }
}

#[test]
fn same_pitch_slide_sustains() {
    // C2~ C2: the note is tied into step 1 and only released half-way through
    // it. Scan from where step 0's gate would have closed to past the step 1
    // boundary.
    let period = c2_period();
    let from = gate_len() as usize - 50;
    let to = step_len() as usize + period;
    let tie = render("C2~ C2 . .", bars(1.0));
    let retrig = render("C2 C2 . .", bars(1.0));
    let tie_dip = worst_dip_db(&tie, from, to, period);
    let retrig_dip = worst_dip_db(&retrig, from, to, period);
    println!("same-pitch slide dip {tie_dip:.2} dB, without slide {retrig_dip:.2} dB");
    assert!(tie_dip > -3.0, "tie dips by {tie_dip:.2} dB");
    assert!(
        retrig_dip < -20.0,
        "control: retrigger only dips {retrig_dip:.2} dB"
    );
}

#[test]
fn same_pitch_slide_is_one_long_note() {
    // A tie is exactly one trigger held for 1.5 steps: no retrigger. Compare
    // the first loop only (the 4-step loop retriggers at its restart).
    let tie = render("C2~ C2 . .", 4 * step_len().round() as usize);
    let mut v = voice();
    // AcidSequencer::getNote fires a step every countDown + 1 samples, so
    // step 1 starts at round(step length) + 1 (no drift correction yet).
    let release_at = step_len().round() as usize + 1 + gate_len() as usize;
    let mut held = Vec::with_capacity(tie.len());
    v.trigger_note(36, false);
    for i in 0..tie.len() {
        if i == release_at {
            v.release_note();
        }
        held.push(v.get_sample());
    }
    let first = tie.iter().zip(&held).position(|(a, b)| a != b);
    assert_eq!(
        first, None,
        "tie and held note differ from sample {first:?}"
    );
}

#[test]
fn slide_on_last_step_wraps_into_step_0() {
    // D2~ on the last step slides into the C2 of the next loop: no release at
    // the loop point.
    let period = c2_period();
    let s = step_len();
    let from = (3.0 * s).round() as usize + gate_len() as usize - 50;
    let to = (4.0 * s).round() as usize + period;
    let wrap = render("C2 . . D2~", bars(1.0));
    let no_wrap = render("C2 . . D2", bars(1.0));
    let wrap_dip = worst_dip_db(&wrap, from, to, period);
    let no_wrap_dip = worst_dip_db(&no_wrap, from, to, period);
    println!("wrap slide dip {wrap_dip:.2} dB, without slide {no_wrap_dip:.2} dB");
    assert!(wrap_dip > -3.0, "loop point dips by {wrap_dip:.2} dB");
    assert!(
        no_wrap_dip < -20.0,
        "control: release only dips {no_wrap_dip:.2} dB"
    );
}

#[test]
fn slide_into_a_rest_is_a_normal_gate() {
    for (slid, plain) in [
        ("C2~ . E2 .", "C2 . E2 ."),
        ("C2 . G2!~ .", "C2 . G2! ."),
        (". C2 D2~ .", ". C2 D2 ."),
    ] {
        assert_eq!(render(slid, bars(2.0)), render(plain, bars(2.0)), "{slid}");
    }
}

#[test]
fn one_step_patterns() {
    for pattern in ["C2", "C2!", "C2~", "C2!~"] {
        let out = render(pattern, bars(4.0));
        assert!(out.iter().all(|x| x.is_finite()), "{pattern}: non-finite");
        assert!(peak(&out) > 1e-3, "{pattern}: silent");
        assert!(peak(&out) < 4.0, "{pattern}: peak {}", peak(&out));
    }
}

#[test]
fn sixty_four_step_pattern() {
    let names = ["C", "D#", "F", "G", "A#"];
    let tokens: Vec<String> = (0..64)
        .map(|i| {
            if i % 7 == 3 {
                return ".".to_string();
            }
            let mut t = format!("{}{}", names[i % 5], 1 + i % 3);
            if i % 4 == 0 {
                t.push('!');
            }
            if i % 3 == 1 {
                t.push('~');
            }
            t
        })
        .collect();
    let pattern = AcidPattern::parse(&tokens.join(" ")).unwrap();
    assert_eq!(pattern.steps().len(), 64);
    let mut v = voice();
    v.set_resonance(85.0);
    v.set_env_mod(70.0);
    v.set_accent(100.0);
    let out = render_with(v, pattern, bars(4.0));
    assert!(out.iter().all(|x| x.is_finite()), "non-finite");
    println!("64-step pattern peak {}", peak(&out));
    assert!(peak(&out) > 1e-3, "silent");
    // every bar of the 64-step loop sounds:
    for b in 0..4 {
        let bar = &out[bars(b as f64)..bars(b as f64 + 1.0)];
        assert!(rms(bar) > 1e-3, "bar {b} is silent");
    }
}

/// Extreme knob settings, to push the range-end notes as hard as possible.
fn extreme_voice() -> Acid303 {
    let mut v = voice();
    v.set_resonance(100.0);
    v.set_env_mod(100.0);
    v.set_accent(100.0);
    v.set_cutoff(2394.0);
    v
}

#[test]
fn notes_at_the_midi_range_ends() {
    // Finite, but not bounded by 4.0: at these settings a held G9 (12.5 kHz)
    // peaks at 14.2 and accented G9 steps reach ~33. That is the C++ voice's
    // behaviour (the port is bit-exact; the next test checks the G9 tie
    // against the C++ directly).
    for pattern in [
        "C-1!",
        "G9~",
        "C-1! G9~ G9~ C-1!~ G9 . C-1 G9!",
        "G9!~ C-1~",
    ] {
        let out = render_with(
            extreme_voice(),
            AcidPattern::parse(pattern).unwrap(),
            bars(2.0),
        );
        println!("{pattern}: peak {}", peak(&out));
        assert!(out.iter().all(|x| x.is_finite()), "{pattern}: non-finite");
    }
}

#[test]
fn g9_tie_matches_the_cpp_voice() {
    // A 1-step `G9~` slides into itself forever: one trigger of note 127,
    // never released. The C++ voice given the same single trigger must agree.
    let n = bars(2.0);
    let rust = render_with(extreme_voice(), AcidPattern::parse("G9~").unwrap(), n);
    let mut r = RefOpen303::new(FS);
    for (p, x) in [
        (RefParam::Waveform, 0.0),
        (RefParam::Resonance, 100.0),
        (RefParam::EnvMod, 100.0),
        (RefParam::Accent, 100.0),
        (RefParam::Cutoff, 2394.0),
    ] {
        r.set(p, x);
    }
    r.trigger_note(127, false);
    let cpp = r.render(n);
    let d = cpp
        .iter()
        .zip(&rust)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    println!("G9~: C++ peak {}, max abs diff {d:e}", peak(&cpp));
    assert!(d <= 1e-9, "G9~ differs from the C++ voice by {d:e}");
}

#[test]
fn notes_above_127_are_rejected_at_construction() {
    let step = |note| AcidStep {
        note: Some(note),
        accent: true,
        slide: false,
    };
    assert!(AcidPattern::new(vec![step(255), step(200)]).is_err());
    assert!(AcidPattern::new(vec![step(127), step(127)]).is_ok());
}

#[test]
fn set_pattern_takes_effect_at_the_next_step_boundary() {
    let old = AcidPattern::parse("C2 D#2 F2 G2").unwrap();
    let new = AcidPattern::parse("A#1! A#1! A#1! A#1!").unwrap();
    let s = step_len();
    let change_at = (1.5 * s) as usize; // mid step 1
    let boundary = (2.0 * s).round() as usize; // start of step 2 (+-1 sample)
    let n = bars(1.0);

    let reference = render_with(voice(), old.clone(), n);
    let mut v = voice();
    let mut p = AcidPlayer::new(old, BPM, FS);
    let mut changed = Vec::with_capacity(n);
    for i in 0..n {
        if i == change_at {
            p.set_pattern(new.clone());
        }
        p.tick(&mut v);
        changed.push(v.get_sample());
    }
    assert_eq!(&changed[..boundary - 1], &reference[..boundary - 1]);
    assert_ne!(
        &changed[boundary + 1..boundary + 200],
        &reference[boundary + 1..boundary + 200]
    );
}

#[test]
fn set_pattern_does_not_hang_a_pending_slide() {
    // Step 0 slides into step 1, but the new pattern has a rest there: the
    // slide has nowhere to go, so the note must still be released.
    let mut v = voice();
    let mut p = AcidPlayer::new(AcidPattern::parse("C2~ C2").unwrap(), BPM, FS);
    let n = bars(1.0);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        if i == 1_000 {
            p.set_pattern(AcidPattern::parse("C2 . . .").unwrap());
        }
        p.tick(&mut v);
        out.push(v.get_sample());
    }
    // step 3 of the first loop (a rest) must be quiet:
    let s = step_len();
    let step3 = &out[(3.0 * s) as usize..(4.0 * s) as usize - 10];
    println!("rms during the rest after the cut slide: {:e}", rms(step3));
    assert!(rms(step3) < 1e-3, "note hangs: rms {:e}", rms(step3));
}

#[test]
fn set_pattern_to_a_shorter_loop() {
    let mut v = voice();
    let long: String = vec!["C2"; 16].join(" ");
    let mut p = AcidPlayer::new(AcidPattern::parse(&long).unwrap(), BPM, FS);
    let n = bars(2.0);
    for i in 0..n {
        if i == (10.5 * step_len()) as usize {
            p.set_pattern(AcidPattern::parse("G2 . D#2").unwrap());
        }
        p.tick(&mut v);
        assert!(v.get_sample().is_finite());
    }
}
