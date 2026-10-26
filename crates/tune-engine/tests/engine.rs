mod common;

use common::{
    Event, MINI, compile_song, compiled, probe_engine, probe_engine_fx, rms, run, run_stems, song,
};
use tune_dsp::DIST_PARAMS;
use tune_dsp::acid303::ACID303_PARAMS;
use tune_engine::{CLIP_KNEE, Cell, Engine, ParamTarget};

/// One bar at 120 BPM and 48 kHz.
const BAR: u64 = 96_000;

#[test]
fn steps_fire_on_drift_free_clock() {
    let s = compiled(
        r#"
[song]
name = "clock"
bpm = 145
[tracks.p]
instrument = "kick"
[patterns.x]
track = "p"
steps = "x"
[[sections]]
name = "a"
bars = 64
play = { p = "x" }
"#,
    );
    assert_eq!(s.end_sample(), 5_084_690);
    let (mut engine, log) = probe_engine(&s, "p");
    run(&mut engine, s.end_sample());
    let steps = log.lock().unwrap().steps();
    assert_eq!(steps.len(), 1024);
    for (k, (sample, cell, ctx)) in steps.iter().enumerate() {
        let k = k as u64;
        assert_eq!(*sample, s.clock.step_start(k), "step {k}");
        assert_eq!(ctx.step, k);
        assert_eq!(*cell, Cell::Hit { accent: false });
        assert_eq!(ctx.gate_samples, s.clock.gate_samples());
    }
}

/// Note of the acid cell at `step` in the probe's log.
fn note_at(steps: &[(u64, Cell, tune_engine::StepCtx)], step: u64) -> Option<u8> {
    match &steps[step as usize].1 {
        Cell::Note(s) => s.note,
        _ => None,
    }
}

#[test]
fn odd_length_pattern_restarts_each_section() {
    // 13 distinct notes: MIDI 36 + pattern index.
    let s = compiled(
        r#"
[song]
name = "odd"
bpm = 120
[tracks.acid]
instrument = "acid303"
[patterns.p13]
track = "acid"
steps = "C2 C#2 D2 D#2 E2 F2 F#2 G2 G#2 A2 A#2 B2 C3"
[[sections]]
name = "a"
bars = 1
play = { acid = "p13" }
[[sections]]
name = "b"
bars = 1
play = { acid = "p13" }
"#,
    );
    let (mut engine, log) = probe_engine(&s, "acid");
    run(&mut engine, s.end_sample());
    let steps = log.lock().unwrap().steps();
    assert_eq!(steps.len(), 32);
    for k in [0, 13, 16] {
        assert_eq!(note_at(&steps, k), Some(36), "step {k}");
    }
    assert_eq!(note_at(&steps, 12), Some(48));
    assert_eq!(note_at(&steps, 15), Some(38));
    assert_eq!(note_at(&steps, 17), Some(37));
    assert_eq!(note_at(&steps, 29), Some(36));
}

fn slide_song(second: &str) -> String {
    format!(
        r#"
[song]
name = "slide"
bpm = 120
[tracks.acid]
instrument = "acid303"
[patterns.last_slide]
track = "acid"
steps = "C2 . . . . . . . . . . . . . . C2~"
[patterns.note_first]
track = "acid"
steps = "C2 . . ."
[patterns.rest_first]
track = "acid"
steps = ". C2 . ."
[[sections]]
name = "a"
bars = 1
play = {{ acid = "last_slide" }}
[[sections]]
name = "b"
bars = 1
{second}
"#
    )
}

fn next_has_gate_at_15(second: &str) -> bool {
    let s = compiled(&slide_song(second));
    let (mut engine, log) = probe_engine(&s, "acid");
    run(&mut engine, s.end_sample());
    let steps = log.lock().unwrap().steps();
    let (_, cell, ctx) = &steps[15];
    assert!(matches!(cell, Cell::Note(n) if n.slide));
    ctx.next_has_gate
}

#[test]
fn slide_across_section_boundary() {
    assert!(next_has_gate_at_15(r#"play = { acid = "note_first" }"#));
    assert!(!next_has_gate_at_15(r#"play = { acid = "rest_first" }"#));
    assert!(!next_has_gate_at_15("play = {}"));
    assert!(!next_has_gate_at_15(""));
}

#[test]
fn slide_on_last_step_of_song_sees_no_gate() {
    let s = compiled(&slide_song(r#"play = { acid = "last_slide" }"#));
    let (mut engine, log) = probe_engine(&s, "acid");
    run(&mut engine, s.end_sample() + 10);
    let steps = log.lock().unwrap().steps();
    assert_eq!(steps.len(), 32);
    assert!(!steps[31].2.next_has_gate);
}

#[test]
fn empty_section_and_unplayed_track() {
    let s = compiled(
        r#"
[song]
name = "empty"
bpm = 120
seed = 3
[tracks.kick]
instrument = "kick"
[tracks.acid]
instrument = "acid303"
[tracks.hat]
instrument = "hat_closed"
[patterns.k]
track = "kick"
steps = "x...x...x...x..."
[patterns.h]
track = "hat"
steps = "..x."
[[sections]]
name = "a"
bars = 1
play = { kick = "k", hat = "h" }
[[sections]]
name = "b"
bars = 2
"#,
    );
    let mut engine = Engine::new(s.clone());
    let (mix, stems) = run_stems(&mut engine, s.end_sample());
    assert_eq!(engine.position(), s.end_sample());
    assert!(rms(&mix[..2 * BAR as usize]) > 1e-2);
    let last_bar = &mix[2 * 2 * BAR as usize..];
    assert!(rms(last_bar) < 1e-4, "tail rms {}", rms(last_bar));
    assert!(mix.iter().all(|v| v.is_finite()));
    // The unplayed acid track stays silent.
    assert!(rms(&stems[1]) < 1e-4);
}

#[test]
fn automation_pure_function_of_position() {
    let s = compiled(
        r#"
[song]
name = "auto"
bpm = 120
[tracks.acid]
instrument = "acid303"
params = { "acid303.cutoff" = 500 }
[patterns.a]
track = "acid"
steps = "C2 . C3 ."
[[sections]]
name = "ramp"
bars = 2
play = { acid = "a" }
[[sections.automation]]
param = "acid.acid303.cutoff"
points = [[0, 0.0], [1, 1.0]]
[[sections]]
name = "plain"
bars = 1
play = { acid = "a" }
[[sections]]
name = "single"
bars = 1
play = { acid = "a" }
[[sections.automation]]
param = "acid.acid303.cutoff"
points = [[0.5, 0.3]]
[[sections]]
name = "jump"
bars = 2
play = { acid = "a" }
[[sections.automation]]
param = "acid.acid303.cutoff"
points = [[1, 0.2], [1, 0.9]]
"#,
    );
    const CUTOFF: usize = 2;
    let spec = &ACID303_PARAMS[CUTOFF];
    assert_eq!(spec.id, "acid303.cutoff");
    let end = s.end_sample();
    assert_eq!(end, 6 * BAR);

    let (mut engine, log) = probe_engine(&s, "acid");
    run(&mut engine, end);
    let log = log.lock().unwrap();
    let trace = log.trace(CUTOFF, end);
    let at = |n: u64| trace[n as usize].unwrap();

    // Ramp: a pure function of the bar offset; holds after the last point.
    assert_eq!(at(0), spec.to_plain(0.0));
    assert!((at(BAR / 2) - spec.to_plain(0.5)).abs() < 1e-9);
    for n in [BAR, BAR + 1, BAR + 12_345, 2 * BAR - 1] {
        assert_eq!(at(n), 2394.0, "sample {n}");
    }
    // Exactly one lane write per sample in the ramp (plus the static reset
    // at its first sample).
    let ramp_writes = log
        .params()
        .iter()
        .filter(|&&(n, i, _)| i == CUTOFF && n < 2 * BAR)
        .count();
    assert_eq!(ramp_writes as u64, 2 * BAR + 1);

    // A section without a lane falls back to the static value at once.
    let plain: Vec<_> = log
        .params()
        .into_iter()
        .filter(|&(n, i, _)| i == CUTOFF && (2 * BAR..3 * BAR).contains(&n))
        .collect();
    assert_eq!(plain, vec![(2 * BAR, CUTOFF, 500.0)]);
    assert_eq!(at(3 * BAR - 1), 500.0);

    // A single point holds its value across the whole section.
    for n in [3 * BAR, 3 * BAR + 1, 3 * BAR + BAR / 2, 4 * BAR - 1] {
        assert_eq!(at(n), spec.to_plain(0.3), "sample {n}");
    }

    // Two points at the same bar: a jump, taking the later point at the bar.
    assert_eq!(at(4 * BAR), spec.to_plain(0.2));
    assert_eq!(at(5 * BAR - 1), spec.to_plain(0.2));
    assert_eq!(at(5 * BAR), spec.to_plain(0.9));
    assert_eq!(at(6 * BAR - 1), spec.to_plain(0.9));

    // Seeking into the middle of the lane gives the same values bitwise.
    for bar in [1u32, 3, 4] {
        let start = s.clock.bar_start(bar);
        let (mut engine, seek_log) = probe_engine(&s, "acid");
        engine.seek(bar);
        run(&mut engine, end - start);
        let seek_trace = seek_log.lock().unwrap().trace(CUTOFF, end - start);
        for (i, v) in seek_trace.iter().enumerate() {
            let n = start as usize + i;
            assert_eq!(
                v.map(f64::to_bits),
                trace[n].map(f64::to_bits),
                "seek({bar}) sample {n}"
            );
        }
    }
}

#[test]
fn lanes_apply_before_on_step() {
    let s = compiled(
        r#"
[song]
name = "order"
bpm = 120
[tracks.acid]
instrument = "acid303"
[patterns.a]
track = "acid"
steps = "C2"
[[sections]]
name = "a"
bars = 1
play = { acid = "a" }
[[sections.automation]]
param = "acid.acid303.cutoff"
points = [[0, 0.25], [1, 0.75]]
[[sections]]
name = "b"
bars = 1
play = { acid = "a" }
"#,
    );
    let spec = &ACID303_PARAMS[2];
    let n_params = ACID303_PARAMS.len();
    let (mut engine, log) = probe_engine(&s, "acid");
    run(&mut engine, s.end_sample());
    let log = log.lock().unwrap();
    let at = |sample: u64| -> Vec<Event> {
        log.events
            .iter()
            .filter(|e| match e {
                Event::Step { sample: n, .. } | Event::Param { sample: n, .. } => *n == sample,
                Event::Reset { .. } => false,
            })
            .cloned()
            .collect()
    };

    // First sample of the song: static params, then the lane, then the step.
    let first = at(0);
    assert_eq!(first.len(), n_params + 2);
    for (i, e) in first[..n_params].iter().enumerate() {
        assert!(matches!(e, Event::Param { index, .. } if *index == i));
    }
    assert_eq!(
        first[n_params],
        Event::Param {
            sample: 0,
            index: 2,
            plain: spec.to_plain(0.25)
        }
    );
    assert!(matches!(first[n_params + 1], Event::Step { .. }));

    // A later step boundary in the section: the lane value, then the step.
    let step1 = at(s.clock.step_start(1));
    assert_eq!(step1.len(), 2);
    assert!(matches!(step1[0], Event::Param { index: 2, .. }));
    assert!(matches!(step1[1], Event::Step { .. }));

    // The next section (no lane): static params, then the step.
    let b = at(BAR);
    assert_eq!(b.len(), n_params + 1);
    assert_eq!(
        b[2],
        Event::Param {
            sample: BAR,
            index: 2,
            plain: ACID303_PARAMS[2].default
        }
    );
    assert!(matches!(b[n_params], Event::Step { .. }));
}

#[test]
fn fx_lane_and_muted_track() {
    let s = compiled(
        r#"
[song]
name = "fx"
bpm = 120
[tracks.acid]
instrument = "acid303"
mute = true
fx = [{ type = "distortion", mode = "soft", params = { "dist.drive" = 6 } }]
[patterns.a]
track = "acid"
steps = "C2"
[[sections]]
name = "a"
bars = 1
play = { acid = "a" }
[[sections.automation]]
param = "acid.dist.drive"
points = [[0, 0.25]]
[[sections.automation]]
param = "acid.acid303.cutoff"
points = [[0, 0.5]]
"#,
    );
    let lanes = &s.sections[0].lanes;
    assert_eq!(lanes[0].track, 0);
    assert_eq!(lanes[0].target, ParamTarget::Fx { slot: 0, index: 0 });
    assert_eq!(lanes[0].spec.id, "dist.drive");
    assert_eq!(lanes[1].target, ParamTarget::Instrument(2));
    assert!(s.tracks[0].mute);
    assert_eq!(s.tracks[0].fx[0].params[0], 6.0);

    let (mut engine, log, fx_log) = probe_engine_fx(&s, "acid", true);
    let mix = run(&mut engine, s.end_sample());
    assert!(mix.iter().all(|&v| v == 0.0));
    let trace = log.lock().unwrap().trace(2, s.end_sample());
    assert_eq!(trace[1000], Some(ACID303_PARAMS[2].to_plain(0.5)));
    let fx_trace = fx_log.unwrap().lock().unwrap().trace(0, s.end_sample());
    assert_eq!(fx_trace[1000], Some(DIST_PARAMS[0].to_plain(0.25)));
}

#[test]
fn mute_and_solo() {
    let base = song(MINI);
    let frames = compile_song(&base).end_sample();
    assert_eq!(frames, 4 * BAR);

    // Solo the hat: the mix is the hat stem, bit for bit.
    let mut solo = base.clone();
    solo.tracks.get_mut("hat").unwrap().solo = true;
    let s = compile_song(&solo);
    let hat = s.track_index("hat").unwrap();
    let (mix, stems) = run_stems(&mut Engine::new(s.clone()), frames);
    let hat_peak = stems[hat].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(hat_peak > 0.0 && f64::from(hat_peak) < CLIP_KNEE);
    assert!(
        mix.iter()
            .zip(&stems[hat])
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );

    // Mute the kick: the mix equals mini without the kick track, but the
    // kick is still processed and its stem continues.
    let mut muted = base.clone();
    muted.tracks.get_mut("kick").unwrap().mute = true;
    let s = compile_song(&muted);
    let kick = s.track_index("kick").unwrap();
    let (mix, stems) = run_stems(&mut Engine::new(s), frames);
    assert!(rms(&stems[kick]) > 1e-2);

    let mut removed = base.clone();
    removed.tracks.shift_remove("kick");
    removed.patterns.shift_remove("k4");
    for section in &mut removed.sections {
        section.play.shift_remove("kick");
    }
    let s = compile_song(&removed);
    assert_eq!(s.track_index("kick"), None);
    let reference = run(&mut Engine::new(s), frames);
    assert!(rms(&reference) > 1e-2);
    assert!(
        mix.iter()
            .zip(&reference)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
}

#[test]
fn process_and_process_stems_agree() {
    let s = compiled(MINI);
    let a = run(&mut Engine::new(s.clone()), s.end_sample());
    let (b, _) = run_stems(&mut Engine::new(s.clone()), s.end_sample());
    assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
    let mut e = Engine::new(s.clone());
    run(&mut e, s.end_sample());
    let m = e.meter();
    assert!(m.peak > 0.0 && m.peak <= tune_engine::CLIP_CEILING);
    assert!(m.pre_clip_peak >= m.peak);
    e.reset_meter();
    assert_eq!(e.meter().peak, 0.0);
    assert_eq!(e.meter().knee_samples, 0);
}

#[test]
fn loop_wraps_without_reset() {
    let s = compiled(MINI);
    assert_eq!(s.clock.bar_start(1), BAR);
    let (mut engine, log) = probe_engine(&s, "kick");
    engine.set_loop(Some(0..1));
    run(&mut engine, 144_000);
    assert_eq!(engine.position(), 48_000);
    assert_eq!(engine.bar(), 0);
    let log = log.lock().unwrap();
    assert!(!log.events.iter().any(|e| matches!(e, Event::Reset { .. })));
    // Step 0 fires again at the wrap.
    let steps = log.steps();
    assert_eq!(steps[16].0, BAR);
    assert_eq!(steps[16].2.step, 0);
}

#[test]
fn set_loop_clamps_and_drops_empty() {
    let s = compiled(MINI);
    let mut engine = Engine::new(s.clone());
    engine.set_loop(Some(3..9));
    engine.seek(3);
    run(&mut engine, BAR + 10);
    assert_eq!(engine.position(), s.clock.bar_start(3) + 10);
    assert!(!engine.finished());

    let mut engine = Engine::new(s.clone());
    #[allow(clippy::reversed_empty_ranges)]
    engine.set_loop(Some(2..2));
    engine.seek(3);
    run(&mut engine, BAR + 10);
    assert!(engine.finished());
}

#[test]
fn finished_outputs_silence() {
    let s = compiled(MINI);
    let end = s.end_sample();
    let mut engine = Engine::new(s.clone());
    let out = run(&mut engine, end + 1000);
    assert!(engine.finished());
    assert_eq!(engine.position(), end);
    assert_eq!(engine.bar(), s.total_bars);
    assert!(out[2 * end as usize..].iter().all(|&v| v == 0.0));
    let more = run(&mut engine, 500);
    assert!(more.iter().all(|&v| v == 0.0));

    // Seeking past the end clamps, and seeking back resumes playback.
    engine.seek(99);
    assert_eq!(engine.position(), end);
    let silent = run(&mut engine, 10);
    assert!(engine.finished() && silent.iter().all(|&v| v == 0.0));
    engine.seek(0);
    assert!(!engine.finished());
    assert_eq!(engine.position(), 0);
}

#[test]
fn seek_resets_and_matches_fresh_engine() {
    let s = compiled(MINI);
    let start = s.clock.bar_start(2);
    let frames = s.end_sample() - start;

    let mut fresh = Engine::new(s.clone());
    fresh.seek(2);
    let a = run(&mut fresh, frames);

    let mut used = Engine::new(s.clone());
    run(&mut used, 12_345);
    used.seek(2);
    assert_eq!(used.position(), start);
    assert_eq!(used.bar(), 2);
    let b = run(&mut used, frames);
    assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
}

#[test]
fn slide_on_last_loop_step_wraps_to_loop_start() {
    // Looping bar 0, whose step 15 slides into step 0 (a note): the second
    // pass must sound exactly like play-through into an identical bar.
    let looped = compiled(&slide_song(""));
    let through = compiled(
        r#"
[song]
name = "through"
bpm = 120
[tracks.acid]
instrument = "acid303"
[patterns.last_slide]
track = "acid"
steps = "C2 . . . . . . . . . . . . . . C2~"
[[sections]]
name = "a"
bars = 3
play = { acid = "last_slide" }
"#,
    );
    let mut a = Engine::new(looped.clone());
    a.set_loop(Some(0..1));
    let out_a = run(&mut a, 2 * BAR);
    let mut b = Engine::new(through.clone());
    let out_b = run(&mut b, 3 * BAR);
    let second = 2 * BAR as usize..4 * BAR as usize;
    assert!(out_a[second.clone()] == out_b[second.clone()]);
    let tail = 4 * BAR as usize - 2 * 6000..4 * BAR as usize;
    assert!(rms(&out_a[tail]) > 1e-3, "slide tail must keep sounding");

    // Loop start is a rest: no gate across the wrap.
    let rest = compiled(
        r#"
[song]
name = "rest"
bpm = 120
[tracks.acid]
instrument = "acid303"
[patterns.p]
track = "acid"
steps = ". . . . . . . . . . . . . . . C2~"
[[sections]]
name = "a"
bars = 1
play = { acid = "p" }
"#,
    );
    let (mut e, log) = probe_engine(&rest, "acid");
    e.set_loop(Some(0..1));
    run(&mut e, 2 * BAR);
    let steps = log.lock().unwrap().steps();
    assert_eq!(steps[31].2.step, 15);
    assert!(!steps[31].2.next_has_gate);
    assert!(!steps[15].2.next_has_gate);
}

/// The distortion's `mode` setting reaches the voice: the same hot acid
/// through `soft` and through `hard` sounds different, so a setting dropped
/// between the song and the factory fails here.
#[test]
fn distortion_mode_reaches_the_voice() {
    let mode = |m: &'static str| {
        let mut s = song(MINI);
        let acid = s.tracks.get_mut("acid").unwrap();
        acid.fx[0].settings[0] = m;
        acid.fx[0].params.insert("drive".into(), 36.0);
        compile_song(&s)
    };
    let (soft, hard) = (mode("soft"), mode("hard"));
    assert_eq!(hard.tracks[1].fx[0].settings, ["hard"]);
    // Section `b` (bars 2 and 3) plays the acid.
    let acid_bars = |s| {
        let mut e = Engine::new(s);
        e.seek(2);
        run_stems(&mut e, BAR).1.swap_remove(1)
    };
    let (a, b) = (acid_bars(soft), acid_bars(hard));
    assert!(rms(&a) > 0.01, "the acid plays");
    assert!(a != b, "soft and hard clip the same hot input differently");
}
