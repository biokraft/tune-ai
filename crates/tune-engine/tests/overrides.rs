//! Live knob overrides on the [`Engine`]: they win over static params and
//! lanes, apply before `on_step`, are clamped, and clear back to the song.

mod common;

use std::sync::Arc;

use common::{
    Event, MINI, MIXED, bits, compile_song, compiled, drop_lane, probe_engine, probe_engine_fx,
    run, variant_of,
};
use tune_dsp::acid303::ACID303_PARAMS;
use tune_dsp::{DIST_PARAMS, MASTER_GAIN, STRIP_DUCK_DEPTH, STRIP_SEND_DELAY};
use tune_engine::{CompiledSong, Engine, MIX_TRACK, ParamTarget};
use tune_song::{MasterMode, MixUnit, Song};

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

/// The clip variant of `mixed.toml` without its static master gain (so
/// the master gain sits at 0 dB), then `edit`.
fn clip_mixed(edit: impl FnOnce(&mut Song)) -> Arc<CompiledSong> {
    compile_song(&variant_of(MIXED, |s| {
        s.mix.master = MasterMode::Clip;
        s.mix.params.shift_remove("master.gain");
        edit(s);
    }))
}

/// Removes the acid's `send.delay` lane, so its static send holds.
fn drop_send_lane(s: &mut Song) {
    drop_lane(s, "acid", "send.delay");
}

const MASTER_GAIN_KNOB: ParamTarget = ParamTarget::Mix {
    unit: MixUnit::Master,
    index: MASTER_GAIN,
};

#[test]
fn mix_override_and_clear() {
    let s = clip_mixed(|_| {});
    let bar = s.clock.bar_start(1);
    let mut control = Engine::new(s.clone());
    let mut e = Engine::new(s.clone());
    assert_eq!(bits(&run(&mut e, bar)), bits(&run(&mut control, bar)));

    // The master stage holds no state in clip mode, so the override is a
    // plain +12 dB on the pre-clip bus.
    e.set_override(MIX_TRACK, MASTER_GAIN_KNOB, Some(12.0));
    assert_eq!(e.overrides_active(), 1);
    e.reset_meter();
    control.reset_meter();
    run(&mut e, bar);
    run(&mut control, bar);
    let ratio = e.meter().pre_clip_peak / control.meter().pre_clip_peak;
    assert!(ratio >= 3.9, "pre-clip peak ratio {ratio}");

    // Clear mid-section `b` (bars 2 and 3), away from any static reset.
    run(&mut e, bar / 2);
    run(&mut control, bar / 2);
    e.set_override(MIX_TRACK, MASTER_GAIN_KNOB, None);
    assert_eq!(e.overrides_active(), 0);
    assert_eq!(
        bits(&run(&mut e, bar / 2)),
        bits(&run(&mut control, bar / 2)),
        "static again after the clear"
    );

    // `clear_overrides` clears a mix override too.
    e.set_override(MIX_TRACK, MASTER_GAIN_KNOB, Some(12.0));
    run(&mut e, 100);
    run(&mut control, 100);
    e.clear_overrides();
    assert_eq!(e.overrides_active(), 0);
    let rest = s.end_sample() - 3 * bar - 100;
    assert_eq!(bits(&run(&mut e, rest)), bits(&run(&mut control, rest)));
}

#[test]
fn strip_override_on_send() {
    let s = clip_mixed(drop_send_lane);
    let open = clip_mixed(|s| {
        drop_send_lane(s);
        let acid = s.tracks.get_mut("acid").unwrap();
        acid.strip.insert("send.delay".into(), 0.0);
    });
    let (acid, send, _) = s.resolve_param("acid", "send.delay").unwrap();
    assert_eq!(send, ParamTarget::Strip(STRIP_SEND_DELAY));
    let bar = s.clock.bar_start(1);
    // Through the first bar of section `b`, where the acid plays.
    let frames = 3 * bar;

    let mut e = Engine::new(s.clone());
    e.set_override(acid, send, Some(0.0));
    assert_eq!(e.overrides_active(), 1);
    let heard = run(&mut e, frames);
    assert_eq!(
        bits(&heard),
        bits(&run(&mut Engine::new(open), frames)),
        "the override sounds like the static value"
    );
    assert_ne!(
        bits(&heard),
        bits(&run(&mut Engine::new(s.clone()), frames)),
        "the override changes the mix"
    );

    e.set_override(acid, send, None);
    assert_eq!(e.overrides_active(), 0);
    // A seek clears every bus, so what follows depends on the params alone.
    let mut control = Engine::new(s.clone());
    e.seek(3);
    control.seek(3);
    assert_eq!(
        bits(&run(&mut e, bar)),
        bits(&run(&mut control, bar)),
        "static again"
    );

    // `clear_overrides` clears a strip override too.
    e.set_override(acid, send, Some(0.0));
    e.clear_overrides();
    assert_eq!(e.overrides_active(), 0);
}

#[test]
fn strip_clear_restores_static_mid_section() {
    // Without sends the acid's strip feeds only the dry path, which holds
    // no state after the ducker (whose envelope runs at any depth), so once
    // an override clears, every frame matches an engine that never had one.
    let s = clip_mixed(|s| {
        let acid = s.tracks.get_mut("acid").unwrap();
        acid.strip.shift_remove("send.delay");
        acid.strip.shift_remove("send.reverb");
        drop_lane(s, "acid", "send.delay");
    });
    let (acid, depth, _) = s.resolve_param("acid", "duck.depth").unwrap();
    assert_eq!(depth, ParamTarget::Strip(STRIP_DUCK_DEPTH));
    let bar = s.clock.bar_start(1);
    let mut control = Engine::new(s.clone());
    let mut e = Engine::new(s.clone());
    // Into section `b` (bars 2 and 3), where the acid plays under the kick.
    let lead = 2 * bar + 1000;
    run(&mut e, lead);
    run(&mut control, lead);

    e.set_override(acid, depth, Some(24.0));
    assert_ne!(
        bits(&run(&mut e, bar / 2)),
        bits(&run(&mut control, bar / 2)),
        "the override deepens the duck"
    );
    e.set_override(acid, depth, None);
    assert_eq!(e.overrides_active(), 0);
    assert_eq!(
        bits(&run(&mut e, bar / 4)),
        bits(&run(&mut control, bar / 4)),
        "static again after the clear"
    );

    // `clear_overrides` restores a strip param mid-section too.
    e.set_override(acid, depth, Some(24.0));
    run(&mut e, 100);
    run(&mut control, 100);
    e.clear_overrides();
    assert_eq!(e.overrides_active(), 0);
    let rest = s.end_sample() - (lead + bar / 2 + bar / 4 + 100);
    assert_eq!(bits(&run(&mut e, rest)), bits(&run(&mut control, rest)));
}
