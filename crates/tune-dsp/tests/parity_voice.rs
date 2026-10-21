//! Sample-for-sample parity of the full Acid303 voice against the C++
//! `Open303` (sequencer off).
//!
//! The C++ `BlendOscillator::blend` member is never initialised (the
//! `Open303` constructor does not call `setWaveform`), so every scenario sets
//! the waveform explicitly on both sides.

mod common;

use common::max_abs_diff;
use open303_ref::consts::tee_bee_mode as m;
use open303_ref::{RefOpen303, RefParam};
use tune_dsp::acid303::{ACID303_PARAMS, Acid303, FilterMode};

const FS: f64 = 48_000.0;
const TOL: f64 = 1e-9;

/// One call made on both voices before the sample at `at` is rendered.
#[derive(Clone, Copy, Debug)]
enum Ev {
    Midi(u8, u8),
    AllOff,
    Trigger(u8, bool),
    Slide(u8, bool),
    Release,
}

struct Pair {
    r: RefOpen303,
    v: Acid303,
}

impl Pair {
    fn new(fs: f64, waveform: f64) -> Self {
        let mut p = Self {
            r: RefOpen303::new(fs),
            v: Acid303::new(fs),
        };
        p.both(RefParam::Waveform, waveform);
        p
    }

    /// Calls the same setter on both voices.
    fn both(&mut self, p: RefParam, x: f64) {
        self.r.set(p, x);
        let v = &mut self.v;
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

    fn event(&mut self, e: Ev) {
        match e {
            Ev::Midi(n, vel) => {
                self.r.note_on(n as i32, vel as i32);
                self.v.note_on(n, vel);
            }
            Ev::AllOff => {
                self.r.all_notes_off();
                self.v.all_notes_off();
            }
            Ev::Trigger(n, a) => {
                self.r.trigger_note(n as i32, a);
                self.v.trigger_note(n, a);
            }
            Ev::Slide(n, a) => {
                self.r.slide_to_note(n as i32, a);
                self.v.slide_to_note(n, a);
            }
            Ev::Release => {
                // The C++ releaseNote ignores its argument (it reads
                // currentNote), so any key works on the reference side.
                self.r.release_note(0);
                self.v.release_note();
            }
        }
    }

    /// Renders `n` samples on both sides, applying `events` (sorted by
    /// sample index) and calling `per_sample(i, pair)` before each sample.
    fn render(
        &mut self,
        n: usize,
        events: &[(usize, Ev)],
        mut per_sample: impl FnMut(usize, &mut Pair),
    ) -> (Vec<f64>, Vec<f64>) {
        let mut a = Vec::with_capacity(n);
        let mut b = Vec::with_capacity(n);
        let mut next = 0;
        for i in 0..n {
            while next < events.len() && events[next].0 == i {
                self.event(events[next].1);
                next += 1;
            }
            per_sample(i, self);
            a.push(self.r.get_sample());
            b.push(self.v.get_sample());
        }
        (a, b)
    }
}

fn check(label: &str, (cpp, rust): (Vec<f64>, Vec<f64>)) -> f64 {
    assert!(rust.iter().all(|x| x.is_finite()), "{label}: non-finite");
    assert!(
        cpp.iter().any(|x| x.abs() > 1e-3),
        "{label}: reference render is silent"
    );
    let d = max_abs_diff(&cpp, &rust);
    println!("{label}: max abs diff {d:e}");
    assert!(d <= TOL, "{label}: max abs diff {d:e} > {TOL:e}");
    d
}

/// Distinct pitches: overlapping notes (slides), accents (vel 117),
/// note-offs, a slide back to a still-held note, and re-triggers after the
/// note list has emptied.
fn midi_script() -> Vec<(usize, Ev)> {
    vec![
        (100, Ev::Midi(36, 90)),
        (6_000, Ev::Midi(48, 117)), // overlap -> slide, accented
        (12_000, Ev::Midi(43, 90)), // overlap -> slide
        (15_000, Ev::Midi(43, 0)),  // release 43 -> slide back to 48 (front)
        (18_000, Ev::Midi(48, 0)),
        (21_000, Ev::Midi(36, 0)),   // list empty -> amp release
        (30_000, Ev::Midi(41, 117)), // re-trigger from an empty list
        (33_000, Ev::Midi(41, 0)),
        (36_000, Ev::Midi(39, 90)),
        (38_400, Ev::Midi(39, 0)),
        (40_800, Ev::Midi(51, 117)),
        (42_000, Ev::Midi(46, 117)), // accented slide
        (43_200, Ev::Midi(51, 0)),
        (44_400, Ev::Midi(46, 0)),
        (50_000, Ev::Midi(34, 90)),
        (54_000, Ev::AllOff),
        (60_000, Ev::Midi(38, 100)), // velocity 100 is already an accent
        (63_000, Ev::Midi(38, 0)),
    ]
}

const SCRIPT_LEN: usize = 72_000;

#[test]
fn a_defaults_note_held_one_second() {
    let mut p = Pair::new(FS, 0.0);
    check(
        "defaults note_on(36,90) 1 s",
        p.render(48_000, &[(0, Ev::Midi(36, 90))], |_, _| {}),
    );
}

#[test]
fn a_defaults_square_and_blend() {
    for wf in [0.5, 1.0] {
        let mut p = Pair::new(FS, wf);
        check(
            &format!("defaults waveform {wf}"),
            p.render(48_000, &[(0, Ev::Midi(36, 90))], |_, _| {}),
        );
    }
}

#[test]
fn b_midi_sequence() {
    let mut p = Pair::new(FS, 0.0);
    check(
        "midi sequence",
        p.render(SCRIPT_LEN, &midi_script(), |_, _| {}),
    );
}

fn squelchy(p: &mut Pair) {
    p.both(RefParam::Resonance, 90.0);
    p.both(RefParam::EnvMod, 80.0);
    p.both(RefParam::Accent, 100.0);
    p.both(RefParam::Decay, 300.0);
}

/// Exponential sweep from 314 Hz to 2394 Hz over the whole render.
fn sweep(i: usize, n: usize) -> f64 {
    314.0 * (2394.0_f64 / 314.0).powf(i as f64 / (n - 1) as f64)
}

#[test]
fn c_midi_sequence_squelchy_with_cutoff_sweep() {
    for wf in [0.0, 1.0] {
        let mut p = Pair::new(FS, wf);
        squelchy(&mut p);
        check(
            &format!("squelchy sweep waveform {wf}"),
            p.render(SCRIPT_LEN, &midi_script(), |i, p| {
                p.both(RefParam::Cutoff, sweep(i, SCRIPT_LEN))
            }),
        );
    }
}

const MODES: [(i32, FilterMode); 16] = [
    (m::FLAT, FilterMode::Flat),
    (m::LP_6, FilterMode::Lp6),
    (m::LP_12, FilterMode::Lp12),
    (m::LP_18, FilterMode::Lp18),
    (m::LP_24, FilterMode::Lp24),
    (m::HP_6, FilterMode::Hp6),
    (m::HP_12, FilterMode::Hp12),
    (m::HP_18, FilterMode::Hp18),
    (m::HP_24, FilterMode::Hp24),
    (m::BP_12_12, FilterMode::Bp12_12),
    (m::BP_6_18, FilterMode::Bp6_18),
    (m::BP_18_6, FilterMode::Bp18_6),
    (m::BP_6_12, FilterMode::Bp6_12),
    (m::BP_12_6, FilterMode::Bp12_6),
    (m::BP_6_6, FilterMode::Bp6_6),
    (m::TB_303, FilterMode::Tb303),
];

#[test]
fn d_every_filter_mode() {
    for (cm, rm) in MODES {
        let mut p = Pair::new(FS, 0.0);
        squelchy(&mut p);
        p.r.set_filter_mode(cm);
        p.v.set_filter_mode(rm);
        check(
            &format!("filter mode {rm:?}"),
            p.render(SCRIPT_LEN, &midi_script(), |i, p| {
                p.both(RefParam::Cutoff, sweep(i, SCRIPT_LEN))
            }),
        );
    }
}

#[test]
fn e_direct_trigger_slide_release() {
    // The note API the player (Task 8) drives, bypassing the note list.
    let events = [
        (0, Ev::Trigger(36, false)),
        (4_965, Ev::Release),
        (9_931, Ev::Trigger(48, true)),
        (12_400, Ev::Slide(43, true)), // slide while gated
        (17_000, Ev::Slide(41, false)),
        (19_862, Ev::Release),
        (24_000, Ev::Trigger(41, true)),
        (26_000, Ev::Trigger(41, true)), // accent stacking
        (28_000, Ev::Trigger(41, true)),
        (30_000, Ev::Release),
        (34_000, Ev::Slide(50, false)), // slide after release
        (38_000, Ev::Trigger(29, false)),
        (44_000, Ev::Release),
    ];
    for wf in [0.0, 1.0] {
        let mut p = Pair::new(FS, wf);
        squelchy(&mut p);
        check(
            &format!("direct note API waveform {wf}"),
            p.render(SCRIPT_LEN, &events, |_, _| {}),
        );
    }
}

#[test]
fn f_every_setter_non_default() {
    let mut p = Pair::new(FS, 0.3);
    let settings = [
        (RefParam::Tuning, 432.0),
        (RefParam::Cutoff, 700.0),
        (RefParam::Resonance, 70.0),
        (RefParam::EnvMod, 55.0),
        (RefParam::Decay, 450.0),
        (RefParam::Accent, 65.0),
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
    for (param, x) in settings {
        p.both(param, x);
    }
    check(
        "every setter non-default",
        p.render(SCRIPT_LEN, &midi_script(), |_, _| {}),
    );
}

#[test]
fn g_other_sample_rates() {
    for fs in [44_100.0, 96_000.0] {
        let mut p = Pair::new(fs, 0.0);
        squelchy(&mut p);
        check(
            &format!("midi sequence at {fs} Hz"),
            p.render(SCRIPT_LEN, &midi_script(), |_, _| {}),
        );
    }
}

#[test]
fn h_sample_rate_change_mid_stream() {
    let mut p = Pair::new(FS, 0.0);
    let (mut a, mut b) = p.render(10_000, &[(0, Ev::Midi(36, 117))], |_, _| {});
    p.r.set_sample_rate(96_000.0);
    p.v.set_sample_rate(96_000.0);
    let (a2, b2) = p.render(10_000, &[(0, Ev::Midi(43, 90))], |_, _| {});
    a.extend(a2);
    b.extend(b2);
    check("sample rate change", (a, b));
}

fn close(c: f64, r: f64) -> bool {
    (c - r).abs() <= 1e-12 * c.abs().max(1.0)
}

#[test]
fn getters_match_cpp_after_construction_and_set() {
    let mut p = Pair::new(FS, 0.0);
    let get = |v: &Acid303, param: RefParam| -> f64 {
        match param {
            RefParam::Waveform => v.waveform(),
            RefParam::Tuning => v.tuning(),
            RefParam::Cutoff => v.cutoff(),
            RefParam::Resonance => v.resonance(),
            RefParam::EnvMod => v.env_mod(),
            RefParam::Decay => v.decay(),
            RefParam::Accent => v.accent(),
            RefParam::Volume => v.volume(),
            RefParam::AmpSustain => v.amp_sustain(),
            RefParam::TanhShaperDrive => v.tanh_shaper_drive(),
            RefParam::TanhShaperOffset => v.tanh_shaper_offset(),
            RefParam::PreFilterHighpass => v.pre_filter_highpass(),
            RefParam::FeedbackHighpass => v.feedback_highpass(),
            RefParam::PostFilterHighpass => v.post_filter_highpass(),
            RefParam::SquarePhaseShift => v.square_phase_shift(),
            RefParam::SlideTime => v.slide_time(),
            RefParam::NormalAttack => v.normal_attack(),
            RefParam::AccentAttack => v.accent_attack(),
            RefParam::AccentDecay => v.accent_decay(),
            RefParam::AmpDecay => v.amp_decay(),
            RefParam::AmpRelease => v.amp_release(),
        }
    };
    let all = [
        RefParam::Waveform,
        RefParam::Tuning,
        RefParam::Cutoff,
        RefParam::Resonance,
        RefParam::EnvMod,
        RefParam::Decay,
        RefParam::Accent,
        RefParam::Volume,
        RefParam::AmpSustain,
        RefParam::TanhShaperDrive,
        RefParam::TanhShaperOffset,
        RefParam::PreFilterHighpass,
        RefParam::FeedbackHighpass,
        RefParam::PostFilterHighpass,
        RefParam::SquarePhaseShift,
        RefParam::SlideTime,
        RefParam::NormalAttack,
        RefParam::AccentAttack,
        RefParam::AccentDecay,
        RefParam::AmpDecay,
        RefParam::AmpRelease,
    ];
    for param in all {
        let (c, r) = (p.r.get(param), get(&p.v, param));
        // amp sustain defaults to amp2dB(0) = -inf on both sides.
        assert!(
            c == r || close(c, r),
            "default {param:?}: C++ {c} vs Rust {r}"
        );
    }
    for param in all {
        p.both(param, 7.0);
        let (c, r) = (p.r.get(param), get(&p.v, param));
        assert!(close(c, r), "set {param:?}: C++ {c} vs Rust {r}");
    }
}

#[test]
fn param_defaults_are_the_cpp_constructor_values() {
    let mut r = RefOpen303::new(FS);
    for spec in ACID303_PARAMS {
        let cpp = match spec.id {
            // Uninitialised in C++ (`BlendOscillator::blend`); Rust uses 0.0.
            "acid303.waveform" => 0.0,
            "acid303.tuning" => r.get(RefParam::Tuning),
            "acid303.cutoff" => r.get(RefParam::Cutoff),
            "acid303.resonance" => r.get(RefParam::Resonance),
            "acid303.env_mod" => r.get(RefParam::EnvMod),
            "acid303.decay" => r.get(RefParam::Decay),
            "acid303.accent" => r.get(RefParam::Accent),
            "acid303.volume" => r.get(RefParam::Volume),
            "acid303.slide_time" => r.get(RefParam::SlideTime),
            other => panic!("unexpected param {other}"),
        };
        assert_eq!(spec.default, cpp, "{}", spec.id);
        assert!(spec.min <= spec.default && spec.default <= spec.max);
    }
    assert_eq!(ACID303_PARAMS.len(), 9);
}

#[test]
fn set_param_matches_the_direct_setters() {
    let mut p = Pair::new(FS, 0.0);
    let ids = [
        ("acid303.waveform", RefParam::Waveform, 0.4),
        ("acid303.tuning", RefParam::Tuning, 450.0),
        ("acid303.cutoff", RefParam::Cutoff, 900.0),
        ("acid303.resonance", RefParam::Resonance, 80.0),
        ("acid303.env_mod", RefParam::EnvMod, 60.0),
        ("acid303.decay", RefParam::Decay, 400.0),
        ("acid303.accent", RefParam::Accent, 80.0),
        ("acid303.volume", RefParam::Volume, -9.0),
        ("acid303.slide_time", RefParam::SlideTime, 90.0),
    ];
    for (id, param, x) in ids {
        p.r.set(param, x);
        p.v.set_param(id, x).unwrap();
    }
    check(
        "set_param sequence",
        p.render(SCRIPT_LEN, &midi_script(), |_, _| {}),
    );
}
