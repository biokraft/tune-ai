//! The arrangement as the engine plays it: per-track placements and lane
//! stretches that need not line up with the sections. Each song is a parsed
//! `mini` edited in the model and compiled without a re-parse (the v1 file
//! format cannot hold these shapes).

mod common;

use common::{MINI, compile_song, probe_engine, run, song};
use tune_dsp::DrumPattern;
use tune_dsp::acid303::ACID303_PARAMS;
use tune_engine::Cell;
use tune_song::{Clip, Placement};

/// One bar at 120 BPM and 48 kHz.
const BAR: u64 = 96_000;
/// The end of `mini` (4 bars).
const END: u64 = 4 * BAR;
/// `acid303.cutoff` in the acid's param table.
const CUTOFF: usize = 2;

fn placement(start_bar: u32, bars: u32, clip: &str, offset_bars: u32) -> Placement {
    Placement {
        start_bar,
        bars,
        clip: clip.to_owned(),
        offset_bars,
    }
}

/// The steps at which the probe saw a hit.
fn hit_steps(log: &common::SharedLog) -> Vec<u64> {
    log.lock()
        .unwrap()
        .steps()
        .into_iter()
        .filter(|(_, cell, _)| matches!(cell, Cell::Hit { .. }))
        .map(|(_, _, ctx)| ctx.step)
        .collect()
}

#[test]
fn placement_offset_starts_mid_clip() {
    let mut s = song(MINI);
    let mut steps = "x".to_owned();
    steps.push_str(&".".repeat(23));
    s.clips.insert(
        "k24".into(),
        Clip::Drum(DrumPattern::parse(&steps).unwrap()),
    );
    // Bars 1..4, one bar (16 steps) into the 24-step clip: its hit, step 0
    // of the clip, comes 8 steps into the placement, then every 24 steps.
    s.tracks.get_mut("kick").unwrap().play = vec![placement(1, 3, "k24", 1)];
    let song = compile_song(&s);
    let (mut engine, log) = probe_engine(&song, "kick");
    run(&mut engine, END);
    assert_eq!(hit_steps(&log), [16 + 8, 16 + 8 + 24]);
}

#[test]
fn lane_gap_falls_back_to_static() {
    let mut s = song(MINI);
    // Bar 3 only (1-based), not the whole of section `b`.
    let lane = &mut s.automation[0];
    lane.start_bar = 2;
    lane.bars = 1;
    lane.points = vec![(0.0, 0.1), (1.0, 0.8)];
    let song = compile_song(&s);
    let (mut engine, log) = probe_engine(&song, "acid");
    run(&mut engine, END);
    let trace = log.lock().unwrap().trace(CUTOFF, END);
    let at = |n: u64| trace[n as usize].unwrap();
    let spec = &ACID303_PARAMS[CUTOFF];
    assert_eq!(at(2 * BAR), spec.to_plain(0.1));
    assert!(at(3 * BAR - 1) > spec.to_plain(0.7), "the ramp runs");
    // Bar 4 (1-based) lies in the gap after the stretch: the static value.
    for n in [3 * BAR, 3 * BAR + 1, 3 * BAR + BAR / 2, END - 1] {
        assert_eq!(at(n), 500.0, "sample {n}");
    }
}

#[test]
fn placement_ending_mid_section_silences_the_track() {
    let mut s = song(MINI);
    // Bar 0 only inside section `a` (bars 0..2); section `b` as before.
    s.tracks.get_mut("kick").unwrap().play =
        vec![placement(0, 1, "k4", 0), placement(2, 2, "k4", 0)];
    let song = compile_song(&s);
    let (mut engine, log) = probe_engine(&song, "kick");
    run(&mut engine, END);
    let hits = hit_steps(&log);
    assert_eq!(hits.iter().filter(|&&n| n < 16).count(), 4);
    assert!(
        hits.iter().all(|&n| !(16..32).contains(&n)),
        "no hit in bar 1: {hits:?}"
    );
    assert_eq!(hits.iter().filter(|&&n| n >= 32).count(), 8);
}
