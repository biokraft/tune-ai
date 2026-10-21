//! Live knob overrides on the [`Engine`]: they win over static params and
//! lanes, apply before `on_step`, are clamped, and clear back to the song.

mod common;

use common::{Event, MINI, compiled, probe_engine, probe_engine_fx, run};
use tune_dsp::DIST_PARAMS;
use tune_dsp::acid303::ACID303_PARAMS;
use tune_engine::ParamTarget;

/// One bar at 120 BPM and 48 kHz.
const BAR: u64 = 96_000;
/// Track 1 of `mini` is `acid`.
const ACID: usize = 1;
const CUTOFF: ParamTarget = ParamTarget::Instrument(2);

#[test]
fn override_wins_over_static_and_lane() {
    let s = compiled(MINI);
    // A control run with the same calls but no override.
    let (mut control, control_log) = probe_engine(&s, "acid");
    control.seek(2);
    run(&mut control, 300);

    let (mut engine, log) = probe_engine(&s, "acid");
    engine.seek(2); // section `b`, whose lane drives the cutoff
    run(&mut engine, 100);
    engine.set_override(ACID, CUTOFF, Some(2000.0));
    assert_eq!(engine.overrides_active(), 1);
    run(&mut engine, 100);
    engine.set_override(ACID, CUTOFF, None);
    assert_eq!(engine.overrides_active(), 0);
    run(&mut engine, 100);

    let lane = control_log.lock().unwrap().trace(2, 300);
    let got = log.lock().unwrap().trace(2, 300);
    assert_eq!(got[..100], lane[..100], "before the override");
    for (n, v) in got.iter().enumerate().take(200).skip(100) {
        assert_eq!(*v, Some(2000.0), "sample {n}");
    }
    assert_ne!(
        lane[150],
        Some(2000.0),
        "the lane alone gives another value"
    );
    assert_eq!(got[201], lane[201], "the lane is back after the clear");
    assert_eq!(got[200..], lane[200..]);
}

#[test]
fn override_applies_after_lanes_before_on_step() {
    let s = compiled(MINI);
    let (mut engine, log) = probe_engine(&s, "acid");
    engine.seek(2);
    run(&mut engine, 100);
    engine.set_override(ACID, CUTOFF, Some(2000.0));
    // Run through the next step boundary (step 33 at probe sample 6000).
    run(&mut engine, 6000);
    let log = log.lock().unwrap();
    let at: Vec<&Event> = log
        .events
        .iter()
        .filter(|e| matches!(e, Event::Step { sample, .. } | Event::Param { sample, .. } if *sample == 6000))
        .collect();
    assert_eq!(at.len(), 3, "{at:?}");
    assert!(matches!(at[0], Event::Param { index: 2, plain, .. } if *plain != 2000.0));
    assert!(matches!(at[1], Event::Param { index: 2, plain, .. } if *plain == 2000.0));
    assert!(matches!(at[2], Event::Step { .. }));
}

#[test]
fn override_survives_section_change() {
    let s = compiled(MINI);
    let (mut engine, log) = probe_engine(&s, "acid");
    run(&mut engine, 100);
    engine.set_override(ACID, CUTOFF, Some(2000.0));
    // Across the static reset at bar 2 and into the lane of section `b`.
    run(&mut engine, 2 * BAR + 100 - 100);
    let trace = log.lock().unwrap().trace(2, 2 * BAR + 100);
    assert!(trace[100..].iter().all(|&v| v == Some(2000.0)));
}

#[test]
fn clear_overrides_restores_static() {
    let s = compiled(MINI);
    let (mut engine, log) = probe_engine(&s, "acid");
    run(&mut engine, 100);
    engine.set_override(ACID, CUTOFF, Some(2000.0));
    run(&mut engine, 50);
    assert_eq!(engine.overrides_active(), 1);
    engine.clear_overrides();
    assert_eq!(engine.overrides_active(), 0);
    run(&mut engine, 50);
    let trace = log.lock().unwrap().trace(2, 200);
    assert_eq!(trace[149], Some(2000.0));
    assert_eq!(trace[150], Some(500.0), "static value on the next sample");
    assert!(trace[150..].iter().all(|&v| v == Some(500.0)));
}

#[test]
fn fx_override_applies() {
    let s = compiled(MINI);
    let (mut engine, _, fx_log) = probe_engine_fx(&s, "acid", true);
    let fx_log = fx_log.unwrap();
    run(&mut engine, 10);
    let drive = ParamTarget::Fx { slot: 0, index: 0 };
    engine.set_override(ACID, drive, Some(30.0));
    run(&mut engine, 10);
    engine.set_override(ACID, drive, None);
    run(&mut engine, 10);
    let trace = fx_log.lock().unwrap().trace(0, 30);
    assert_eq!(trace[9], Some(12.0));
    assert!(trace[10..20].iter().all(|&v| v == Some(30.0)));
    assert!(trace[20..].iter().all(|&v| v == Some(12.0)), "static again");
}

#[test]
fn override_values_are_clamped_and_checked() {
    let s = compiled(MINI);
    let spec = &ACID303_PARAMS[2];
    let (mut engine, log) = probe_engine(&s, "acid");
    engine.set_override(ACID, CUTOFF, Some(1e9));
    run(&mut engine, 1);
    engine.set_override(ACID, CUTOFF, Some(-1e9));
    run(&mut engine, 1);
    let trace = log.lock().unwrap().trace(2, 2);
    assert_eq!(trace, [Some(spec.max), Some(spec.min)]);

    // NaN clears; targets outside the song are ignored.
    engine.set_override(ACID, CUTOFF, Some(f64::NAN));
    assert_eq!(engine.overrides_active(), 0);
    engine.set_override(9, CUTOFF, Some(1.0));
    engine.set_override(ACID, ParamTarget::Instrument(99), Some(1.0));
    engine.set_override(ACID, ParamTarget::Fx { slot: 3, index: 0 }, Some(1.0));
    engine.set_override(
        ACID,
        ParamTarget::Fx {
            slot: 0,
            index: DIST_PARAMS.len(),
        },
        Some(1.0),
    );
    assert_eq!(engine.overrides_active(), 0);
    run(&mut engine, 10);

    // Setting the same param twice counts once.
    engine.set_override(ACID, CUTOFF, Some(800.0));
    engine.set_override(ACID, CUTOFF, Some(900.0));
    assert_eq!(engine.overrides_active(), 1);
}

#[test]
fn param_target_resolves() {
    let s = compiled(MINI);
    let (target, spec) = s.param_target(ACID, "acid303.cutoff").unwrap();
    assert_eq!(target, ParamTarget::Instrument(2));
    assert_eq!(spec.id, "acid303.cutoff");
    let (target, spec) = s.param_target(ACID, "dist.drive").unwrap();
    assert_eq!(target, ParamTarget::Fx { slot: 0, index: 0 });
    assert_eq!(spec.id, "dist.drive");
    assert!(s.param_target(ACID, "kick.tune").is_none());
    assert!(s.param_target(0, "kick.tune").is_some());
    assert!(s.param_target(9, "acid303.cutoff").is_none());
}
