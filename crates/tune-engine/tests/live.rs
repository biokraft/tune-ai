mod common;

use std::ops::Range;
use std::sync::Arc;

use common::{MINI, compile_song, compiled, song};
use tune_dsp::DistortionMode;
use tune_engine::{
    CompiledSong, LiveError, LiveProcessor, ParamTarget, RenderOptions, live, render,
};
use tune_song::FxSpec;

/// One bar at 120 BPM and 48 kHz.
const BAR: u64 = 96_000;
/// The end of `mini` (4 bars).
const END: u64 = 4 * BAR;
/// One step at 120 BPM and 48 kHz.
const STEP: u64 = 6000;

fn render_bars(song: &Arc<CompiledSong>, start_bar: u32, end_bar: u32) -> Vec<f32> {
    render(
        song.clone(),
        &RenderOptions {
            start_bar,
            end_bar,
            stems: false,
        },
    )
    .unwrap()
    .mix
}

/// Runs the processor for `frames` frames in blocks of `block` frames and
/// returns the interleaved stereo output.
fn play(p: &mut LiveProcessor, frames: u64, block: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; 2 * frames as usize];
    for chunk in out.chunks_mut(2 * block) {
        p.process(chunk);
    }
    out
}

/// The interleaved samples of `frames` in `x`.
fn frames(x: &[f32], frames: Range<u64>) -> &[f32] {
    &x[2 * frames.start as usize..2 * frames.end as usize]
}

/// Bitwise equality, reporting the first differing sample.
#[track_caller]
fn assert_bits(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len(), "length");
    if let Some(i) = actual
        .iter()
        .zip(expected)
        .position(|(a, e)| a.to_bits() != e.to_bits())
    {
        panic!(
            "sample {i} (frame {}) differs: {} != {}",
            i / 2,
            actual[i],
            expected[i]
        );
    }
}

fn mini_with(edit: impl FnOnce(&mut tune_song::Song)) -> Arc<CompiledSong> {
    let mut s = song(MINI);
    edit(&mut s);
    compile_song(&s)
}

fn kick_muted() -> Arc<CompiledSong> {
    mini_with(|s| s.tracks.get_mut("kick").unwrap().mute = true)
}

fn without_hat() -> Arc<CompiledSong> {
    mini_with(|s| {
        s.tracks.shift_remove("hat");
        s.patterns.shift_remove("h");
        for section in &mut s.sections {
            section.play.shift_remove("hat");
        }
    })
}

/// `mini` cut down to its first section (2 bars).
fn two_bars() -> Arc<CompiledSong> {
    let b = mini_with(|s| s.sections.truncate(1));
    assert_eq!(b.total_bars, 2);
    b
}

#[test]
fn live_matches_offline_any_block_size() {
    let a = compiled(MINI);
    let expected = render_bars(&a, 0, 4);
    let (ctl, mut p) = live(a, 0, None);
    assert_eq!(p.sample_rate(), 48_000);
    let sizes = [1usize, 7, 64, 333, 512, 4096];
    let mut out = vec![0.0f32; 2 * END as usize];
    let mut done = 0usize;
    for &n in sizes.iter().cycle() {
        if done == END as usize {
            break;
        }
        let n = n.min(END as usize - done);
        p.process(&mut out[2 * done..2 * (done + n)]);
        done += n;
        assert_eq!(ctl.position(), done as u64);
    }
    assert_bits(&out, &expected);
    assert!(!ctl.finished());

    let tail = play(&mut p, 1000, 512);
    assert!(tail.iter().all(|&x| x == 0.0));
    assert!(ctl.finished());
    assert_eq!(ctl.bar(), 4);
    assert_eq!(ctl.position(), END);
}

#[test]
fn swap_at_next_step_keeps_playhead() {
    let a = compiled(MINI);
    let b = kick_muted();
    let (ra, rb) = (render_bars(&a, 0, 4), render_bars(&b, 0, 4));
    let (mut ctl, mut p) = live(a, 0, None);
    let mut out = play(&mut p, 1000, 1000);
    ctl.swap(b).unwrap();
    out.extend(play(&mut p, STEP - 1000, 512));
    assert_eq!(ctl.position(), STEP);
    assert_eq!(
        ctl.swaps_applied(),
        0,
        "the swap waits for the step boundary"
    );
    let mut done = STEP;
    while done < END {
        let n = 512.min(END - done);
        out.extend(play(&mut p, n, 512));
        done += n;
        assert_eq!(ctl.position(), done);
    }
    assert_eq!(ctl.swaps_applied(), 1);
    assert_bits(frames(&out, 0..STEP), frames(&ra, 0..STEP));
    assert_bits(frames(&out, STEP..END), frames(&rb, STEP..END));
    assert_ne!(frames(&out, STEP..END), frames(&ra, STEP..END));
}

#[test]
fn swap_new_bpm_keeps_step() {
    let a = compiled(MINI);
    let b = mini_with(|s| s.bpm = 150.0);
    let (mut ctl, mut p) = live(a, 0, None);
    play(&mut p, 1000, 1000);
    ctl.swap(b).unwrap();
    play(&mut p, 5100, 64);
    assert_eq!(ctl.swaps_applied(), 1);
    assert_eq!(ctl.position(), 4800 + 100);
    assert_eq!(ctl.bar(), 0);
}

#[test]
fn swap_removing_track() {
    let a = compiled(MINI);
    let b = without_hat();
    let (ra, rb) = (render_bars(&a, 0, 4), render_bars(&b, 0, 4));
    let (mut ctl, mut p) = live(a, 0, None);
    let mut out = play(&mut p, 1000, 1000);
    ctl.swap(b).unwrap();
    out.extend(play(&mut p, END - 1000, 333));
    assert_eq!(ctl.swaps_applied(), 1);
    assert_bits(frames(&out, 0..STEP), frames(&ra, 0..STEP));
    assert_bits(frames(&out, STEP..END), frames(&rb, STEP..END));
}

#[test]
fn swap_shorter_song() {
    let a = compiled(MINI);
    let b = two_bars();
    let r3 = render_bars(&a, 3, 4);
    // Bar 3 starts at 288 000; the swap lands on step 49 (294 000).
    let to_boundary = STEP - 1000;

    // Without a loop: the playhead lies past the new end, so playback ends.
    let (mut ctl, mut p) = live(a.clone(), 3, None);
    let mut out = play(&mut p, 1000, 250);
    ctl.swap(b.clone()).unwrap();
    out.extend(play(&mut p, to_boundary, 333));
    assert!(!ctl.finished());
    assert_bits(&out, frames(&r3, 0..STEP));
    let at = play(&mut p, 1, 1);
    assert!(ctl.finished(), "finished at the boundary");
    assert_eq!(ctl.swaps_applied(), 1);
    assert_eq!(at, [0.0, 0.0]);
    let after = play(&mut p, 4096, 512);
    assert!(after.iter().all(|&x| x == 0.0));
    assert!(ctl.finished());
    assert_eq!(ctl.bar(), 2);
    assert_eq!(ctl.position(), b.end_sample());

    // With loop 0..4: clamped to 0..2, and the playhead jumps to its start.
    let (mut ctl, mut p) = live(a.clone(), 3, Some(0..4));
    play(&mut p, 1000, 1000);
    ctl.swap(b.clone()).unwrap();
    play(&mut p, to_boundary, 512);
    let at = play(&mut p, 1, 1);
    assert_eq!(ctl.swaps_applied(), 1);
    assert_eq!(ctl.position(), 1, "jumped to the loop start");
    assert_eq!(ctl.bar(), 0);
    assert!(!ctl.finished());
    assert!(at.iter().all(|x| x.is_finite()));
    // Two bars later the clamped loop wraps instead of finishing.
    play(&mut p, 2 * BAR, 4096);
    assert_eq!(ctl.position(), 1);
    assert!(!ctl.finished());

    // A loop that starts past the new end is dropped: playback ends.
    let (mut ctl, mut p) = live(a, 3, Some(3..4));
    play(&mut p, 1000, 1000);
    ctl.swap(b).unwrap();
    play(&mut p, to_boundary + 1, 512);
    assert!(ctl.finished());
    let after = play(&mut p, 2 * BAR, 4096);
    assert!(after.iter().all(|&x| x == 0.0));
    assert!(ctl.finished(), "no loop survives to restart playback");
}

#[test]
fn seek_and_loop_commands() {
    let a = compiled(MINI);
    let (mut ctl, mut p) = live(a.clone(), 0, None);
    play(&mut p, 1000, 1000);
    ctl.seek(2).unwrap();
    let block = play(&mut p, 4096, 4096);
    assert_eq!(ctl.position(), a.clock.bar_start(2) + 4096);
    assert_eq!(ctl.bar(), 2);
    assert_bits(&block, frames(&render_bars(&a, 2, 4), 0..4096));

    ctl.seek(0).unwrap();
    ctl.set_loop(Some(0..1)).unwrap();
    play(&mut p, BAR + 10, 512);
    assert_eq!(ctl.position(), 10, "wrapped at the end of bar 0");
    assert_eq!(ctl.bar(), 0);
    assert!(!ctl.finished());

    ctl.set_loop(None).unwrap();
    play(&mut p, END, 4096);
    assert!(ctl.finished());
}

#[test]
fn garbage_returns() {
    let (mut ctl, mut p) = live(compiled(MINI), 0, None);
    play(&mut p, 1000, 1000);
    ctl.swap(kick_muted()).unwrap();
    play(&mut p, 1000, 1000);
    assert_eq!(
        ctl.collect_garbage(),
        0,
        "nothing comes back before the swap"
    );
    play(&mut p, STEP, 1000);
    assert_eq!(ctl.swaps_applied(), 1);
    assert_eq!(ctl.collect_garbage(), 1);
    assert_eq!(ctl.collect_garbage(), 0);
}

#[test]
fn queue_full() {
    let (mut ctl, _p) = live(compiled(MINI), 0, None);
    let b = kick_muted();
    for i in 0..16 {
        assert_eq!(ctl.swap(b.clone()), Ok(()), "swap {i}");
    }
    assert_eq!(ctl.swap(b), Err(LiveError::QueueFull));
    assert_eq!(ctl.seek(1), Err(LiveError::QueueFull));
    assert_eq!(ctl.set_loop(None), Err(LiveError::QueueFull));
}

#[test]
fn swap_rejects_another_sample_rate() {
    let (mut ctl, _p) = live(compiled(MINI), 0, None);
    let other = Arc::new(CompiledSong::new(&song(MINI), 44_100));
    assert_eq!(
        ctl.swap(other),
        Err(LiveError::SampleRateMismatch {
            song: 44_100,
            engine: 48_000
        })
    );
}

#[test]
fn latest_swap_wins() {
    let a = compiled(MINI);
    let b = kick_muted();
    let rb = render_bars(&b, 0, 4);
    let (mut ctl, mut p) = live(a, 0, None);
    let mut out = play(&mut p, 1000, 1000);
    ctl.swap(without_hat()).unwrap();
    ctl.swap(b).unwrap();
    out.extend(play(&mut p, END - 1000, 4096));
    assert_eq!(
        ctl.swaps_applied(),
        1,
        "the newer swap replaces the pending one"
    );
    assert_eq!(ctl.collect_garbage(), 2);
    assert_bits(frames(&out, STEP..END), frames(&rb, STEP..END));
}

#[test]
fn swap_waits_for_garbage_room() {
    let (mut ctl, mut p) = live(compiled(MINI), 0, None);
    let b = kick_muted();
    for _ in 0..16 {
        ctl.swap(b.clone()).unwrap();
    }
    play(&mut p, STEP + 1, 512);
    assert_eq!(ctl.swaps_applied(), 1);
    // The garbage queue is now full: 15 replaced updates and 1 applied one.
    ctl.swap(b.clone()).unwrap();
    play(&mut p, 2 * STEP, 512);
    assert_eq!(ctl.swaps_applied(), 1, "no room to return the old song yet");
    assert_eq!(ctl.collect_garbage(), 16);
    play(&mut p, 2 * STEP, 512);
    assert_eq!(ctl.swaps_applied(), 2);
    assert_eq!(ctl.collect_garbage(), 1);
}

#[test]
fn swap_after_the_end_stays_finished() {
    let a = compiled(MINI);
    let longer = mini_with(|s| {
        let extra = s.sections[0].clone();
        s.sections.push(extra);
    });
    assert_eq!(longer.total_bars, 6);
    let (mut ctl, mut p) = live(a, 3, None);
    play(&mut p, BAR + 10, 4096);
    assert!(ctl.finished());
    ctl.swap(longer.clone()).unwrap();
    let out = play(&mut p, 4096, 512);
    assert_eq!(ctl.swaps_applied(), 1);
    assert!(ctl.finished(), "a swap does not restart a finished song");
    assert!(out.iter().all(|&x| x == 0.0));
    assert_eq!(ctl.position(), longer.end_sample());
    assert_eq!(ctl.bar(), 6);
    assert_eq!(ctl.collect_garbage(), 1);

    ctl.seek(4).unwrap();
    let out = play(&mut p, BAR, 4096);
    assert!(!ctl.finished());
    assert!(out.iter().any(|&x| x != 0.0));
}

/// Track 1 of `mini` is `acid`.
const ACID: usize = 1;
const CUTOFF: ParamTarget = ParamTarget::Instrument(2);
const DRIVE: ParamTarget = ParamTarget::Fx { slot: 0, index: 0 };

/// `mini` without section `b`'s cutoff lane (so an override and a static
/// value sound the same), then `edit`.
fn no_lane(edit: impl FnOnce(&mut tune_song::Song)) -> Arc<CompiledSong> {
    mini_with(|s| {
        s.sections[1].automation.clear();
        edit(s);
    })
}

fn set_cutoff(s: &mut tune_song::Song, plain: f64) {
    s.tracks
        .get_mut("acid")
        .unwrap()
        .params
        .insert("acid303.cutoff".into(), plain);
}

fn mute_hat(s: &mut tune_song::Song) {
    s.tracks.get_mut("hat").unwrap().mute = true;
}

#[test]
fn live_override_equals_static_value() {
    let a = no_lane(|_| {});
    let expected = render_bars(&no_lane(|s| set_cutoff(s, 2394.0)), 0, 4);
    let (mut ctl, mut p) = live(a.clone(), 0, None);
    ctl.set_override(ACID, CUTOFF, Some(2394.0)).unwrap();
    let out = play(&mut p, END, 512);
    assert_eq!(ctl.overrides_active(), 1);
    assert_bits(&out, &expected);
    assert_ne!(out, render_bars(&a, 0, 4), "the override is heard");
}

#[test]
fn override_survives_swap() {
    let a = no_lane(|_| {});
    let b = no_lane(mute_hat);
    let ra = render_bars(&no_lane(|s| set_cutoff(s, 2394.0)), 0, 4);
    let rb = render_bars(
        &no_lane(|s| {
            mute_hat(s);
            set_cutoff(s, 2394.0);
        }),
        0,
        4,
    );
    let (mut ctl, mut p) = live(a, 0, None);
    ctl.set_override(ACID, CUTOFF, Some(2394.0)).unwrap();
    // Inside section `b`, where the acid plays.
    let swap_at = 2 * BAR + 1000;
    let boundary = 2 * BAR + STEP;
    let mut out = play(&mut p, swap_at, 512);
    ctl.swap(b.clone()).unwrap();
    out.extend(play(&mut p, END - swap_at, 512));
    assert_eq!(ctl.swaps_applied(), 1);
    assert_eq!(ctl.overrides_active(), 1);
    assert_bits(frames(&out, 0..boundary), frames(&ra, 0..boundary));
    assert_bits(frames(&out, boundary..END), frames(&rb, boundary..END));
    assert_ne!(
        frames(&out, boundary..END),
        frames(&render_bars(&b, 0, 4), boundary..END),
        "the override is still heard after the swap"
    );
}

#[test]
fn stale_override_dropped() {
    let (mut ctl, mut p) = live(compiled(MINI), 0, None);
    assert_eq!(ctl.playing_generation(), 0);
    ctl.swap(kick_muted()).unwrap();
    // Meant for the swapped-in song, but it reaches the processor first.
    ctl.set_override(ACID, CUTOFF, Some(2000.0)).unwrap();
    play(&mut p, STEP + 1, 512);
    assert_eq!(ctl.swaps_applied(), 1);
    assert_eq!(ctl.playing_generation(), 1);
    assert_eq!(ctl.overrides_active(), 0);
    // Once the swap has landed, the next one applies.
    ctl.set_override(ACID, CUTOFF, Some(2000.0)).unwrap();
    play(&mut p, 1, 1);
    assert_eq!(ctl.overrides_active(), 1);
}

#[test]
fn override_after_replaced_swap_applies() {
    let (mut ctl, mut p) = live(compiled(MINI), 0, None);
    play(&mut p, 1000, 1000);
    ctl.swap(without_hat()).unwrap();
    ctl.swap(kick_muted()).unwrap();
    play(&mut p, STEP, 512);
    assert_eq!(ctl.swaps_applied(), 1, "the first swap was replaced");
    assert_eq!(ctl.playing_generation(), 2);
    ctl.set_override(ACID, CUTOFF, Some(2000.0)).unwrap();
    play(&mut p, 1, 1);
    assert_eq!(ctl.overrides_active(), 1);
}

#[test]
fn clearing_swap() {
    let (mut ctl, mut p) = live(compiled(MINI), 0, None);
    ctl.set_override(ACID, CUTOFF, Some(2000.0)).unwrap();
    play(&mut p, 1000, 512);
    assert_eq!(ctl.overrides_active(), 1);
    ctl.swap_clearing_overrides(kick_muted()).unwrap();
    play(&mut p, 1000, 512);
    assert_eq!(ctl.overrides_active(), 1, "kept until the swap lands");
    play(&mut p, STEP, 512);
    assert_eq!(ctl.swaps_applied(), 1);
    assert_eq!(ctl.overrides_active(), 0);
}

#[test]
fn clearing_survives_a_replacing_swap() {
    let (mut ctl, mut p) = live(compiled(MINI), 0, None);
    ctl.set_override(ACID, CUTOFF, Some(2000.0)).unwrap();
    play(&mut p, 1000, 512);
    ctl.swap_clearing_overrides(kick_muted()).unwrap();
    ctl.swap(without_hat()).unwrap();
    play(&mut p, STEP, 512);
    assert_eq!(ctl.swaps_applied(), 1);
    assert_eq!(
        ctl.overrides_active(),
        0,
        "the replaced swap's clear still lands"
    );
}

#[test]
fn fx_overrides_follow_the_fx_chain() {
    let hard = mini_with(|s| {
        let FxSpec::Distortion { mode, .. } = &mut s.tracks.get_mut("acid").unwrap().fx[0];
        *mode = DistortionMode::Hard;
    });
    let (mut ctl, mut p) = live(compiled(MINI), 0, None);
    ctl.set_override(ACID, CUTOFF, Some(2000.0)).unwrap();
    ctl.set_override(ACID, DRIVE, Some(30.0)).unwrap();
    play(&mut p, 1000, 512);
    assert_eq!(ctl.overrides_active(), 2);
    // A kept fx chain keeps its overrides.
    ctl.swap(kick_muted()).unwrap();
    play(&mut p, STEP, 512);
    assert_eq!(ctl.overrides_active(), 2);
    // A rebuilt fx chain starts without them; the instrument keeps its own.
    ctl.swap(hard).unwrap();
    play(&mut p, STEP, 512);
    assert_eq!(ctl.swaps_applied(), 2);
    assert_eq!(ctl.overrides_active(), 1);
}

#[test]
fn rejected_swap_keeps_generation() {
    let (mut ctl, mut p) = live(compiled(MINI), 0, None);
    for _ in 0..16 {
        ctl.seek(0).unwrap();
    }
    assert_eq!(ctl.swap(kick_muted()), Err(LiveError::QueueFull));
    assert_eq!(
        ctl.set_override(ACID, CUTOFF, Some(2000.0)),
        Err(LiveError::QueueFull)
    );
    play(&mut p, 10, 10);
    ctl.set_override(ACID, CUTOFF, Some(2000.0)).unwrap();
    play(&mut p, 10, 10);
    assert_eq!(ctl.playing_generation(), 0);
    assert_eq!(
        ctl.overrides_active(),
        1,
        "no swap was sent, so none is awaited"
    );
}
