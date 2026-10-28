//! Devices addressed by id in version 2 songs: chains with two eqs render in
//! chain order, a lane reaches the device it names, and a swap keeps every
//! device whose id it keeps.

mod common;

use std::sync::Arc;

use common::{SharedLog, compiled, probe_engine_slots, run};
use tune_engine::{CompiledSong, RenderOptions, SongUpdate, render};

/// One bar at 120 BPM and 48 kHz.
const BAR: u64 = 96_000;

/// A four-bar acid line through the chain `fx` (the entries of the list),
/// with `extra` appended (e.g. an `[[automation]]` table).
fn acid(fx: &str, extra: &str) -> String {
    format!(
        r#"
[song]
version = 2
name = "devices"
bpm = 120
seed = 7

[tracks.acid]
instrument = "acid303"
params = {{ "acid303.cutoff" = 900, "acid303.resonance" = 80 }}
fx = [{fx}]
play = [{{ at = 1, clip = "a1", bars = 4 }}]

[clips.a1]
kind = "acid"
steps = "C2 C2! C3~ C2 . D#2 C2~ G2! C2 . C3 C2~ A#1 C2! . F2~"

[[sections]]
name = "a"
bars = 4
{extra}"#
    )
}

const EQ: &str = r#"{ type = "eq", params = { "eq.low_cut" = 300, "eq.high_gain" = -6 } }"#;
const DIST: &str = r#"{ type = "distortion", mode = "hard", params = { "dist.drive" = 30 } }"#;
const EQ2: &str =
    r#"{ type = "eq", id = "eq2", params = { "eq2.mid_freq" = 1200, "eq2.mid_gain" = 9 } }"#;

/// The acid stem of `song`, every bar.
fn stem(song: &Arc<CompiledSong>) -> Vec<f32> {
    let rendered = render(
        song.clone(),
        &RenderOptions {
            start_bar: 0,
            end_bar: song.total_bars,
            stems: true,
        },
    )
    .expect("the song renders");
    rendered.stems.into_iter().next().expect("one stem").1
}

/// The stereo samples of bars `bars` (0-based, half-open).
fn bars(x: &[f32], bars: std::ops::Range<u64>) -> &[f32] {
    &x[2 * (bars.start * BAR) as usize..2 * (bars.end * BAR) as usize]
}

fn max_diff(a: &[f32], b: &[f32]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| (f64::from(x) - f64::from(y)).abs())
        .fold(0.0, f64::max)
}

#[test]
fn two_eqs_and_a_distortion_render_in_chain_order() {
    let chain_a = compiled(&acid(&format!("{EQ}, {DIST}, {EQ2}"), ""));
    let chain_b = compiled(&acid(&format!("{DIST}, {EQ}, {EQ2}"), ""));
    let ids: Vec<&str> = chain_a.tracks[0].fx.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, ["eq", "dist", "eq2"]);
    let a = stem(&chain_a);
    let b = stem(&chain_b);
    assert!(a.iter().any(|&x| x.abs() > 1e-3), "the acid sounds");
    assert!(max_diff(&a, &b) > 1e-3, "the chain order is heard");

    // The second eq's mid band pulled all the way down over bar 2.
    let lane =
        "\n[[automation]]\nparam = \"acid.eq2.mid_gain\"\nat = 2\nbars = 1\npoints = [[0, 0]]\n";
    let laned = stem(&compiled(&acid(&format!("{EQ}, {DIST}, {EQ2}"), lane)));
    assert_eq!(bars(&laned, 0..1), bars(&a, 0..1), "before the stretch");
    assert!(max_diff(bars(&laned, 1..2), bars(&a, 1..2)) > 1e-3, "in it");
    // Back at its static value, the eq's filter memory settles within a bar.
    assert!(
        max_diff(bars(&laned, 3..4), bars(&a, 3..4)) < 1e-6,
        "after it"
    );
}

fn samples(log: &SharedLog) -> u64 {
    log.lock().unwrap().samples
}

/// Plays a bar of `from` with a probe in fx slot `slot`, swaps to `to` and
/// plays another bar; returns how many samples the probe processed.
fn probe_through_swap(from: &str, slot: usize, to: &str) -> u64 {
    let a = compiled(from);
    let b = compiled(to);
    let (mut e, _, fx) = probe_engine_slots(&a, "acid", &[slot]);
    // Bar 1 starts on a step: the swap lands on its first sample.
    run(&mut e, BAR);
    assert!(e.queue_swap(SongUpdate::new(b)).is_none());
    run(&mut e, BAR);
    assert_eq!(e.swaps_applied(), 1);
    samples(&fx[0])
}

#[test]
fn swap_adding_a_second_eq_keeps_the_first() {
    let kept = probe_through_swap(&acid(EQ, ""), 0, &acid(&format!("{EQ2}, {EQ}"), ""));
    assert_eq!(kept, 2 * BAR, "the eq moved to slot 1 and kept running");
}

#[test]
fn swap_removing_a_device_keeps_the_rest() {
    let kept = probe_through_swap(&acid(&format!("{EQ}, {DIST}"), ""), 1, &acid(DIST, ""));
    assert_eq!(
        kept,
        2 * BAR,
        "the distortion moved to slot 0 and kept running"
    );
}
