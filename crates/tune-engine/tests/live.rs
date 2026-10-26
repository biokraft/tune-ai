mod common;

use std::ops::Range;
use std::sync::{Arc, Mutex};

use common::{
    Event, MINI, MIXED, Probe, ProbeFx, SharedLog, compile_song, compiled, drop_lane, engine_with,
    keep_sections, probe_engine_slots, push_section_copy, run, song, variant_of,
};
use tune_dsp::db2amp;
use tune_engine::{
    AudioOut, CompiledSong, Engine, LiveError, LiveProcessor, ParamTarget, RenderOptions,
    SongUpdate, live, render,
};
use tune_modules::{EQ, HAT_OPEN};
use tune_song::{Device, MasterMode};

/// One bar at 120 BPM and 48 kHz.
const BAR: u64 = 96_000;
/// The end of `mini` (4 bars).
const END: u64 = 4 * BAR;
/// One step at 120 BPM and 48 kHz.
const STEP: u64 = 6000;

/// `songs/first-acid` as first mixed: limit mode, delay and reverb buses.
const FIRST_ACID_MIXED: &str = include_str!("../../tune-song/tests/fixtures/first-acid-mixed.toml");

/// An open hat once a bar into the reverb, through the limiter, with no
/// delay bus: after a hit only the hat's decay and the reverb tail sound.
const ROOM: &str = r#"
[song]
name = "room"
bpm = 120
seed = 3

[mix]
master = "limit"
params = { "reverb.size" = 70, "master.gain" = 6 }

[tracks.hat]
instrument = "hat_open"
params = { "send.reverb" = 0 }

[patterns.h]
track = "hat"
steps = "x..............."

[[sections]]
name = "a"
bars = 4
play = { hat = "h" }
"#;

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
        s.clips.shift_remove("h");
    })
}

/// `mini` cut down to its first section (2 bars).
fn two_bars() -> Arc<CompiledSong> {
    let b = mini_with(|s| keep_sections(s, 1));
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
    let longer = mini_with(|s| push_section_copy(s, 0));
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
        drop_lane(s, "acid", "acid303.cutoff");
        edit(s);
    })
}

fn set_cutoff(s: &mut tune_song::Song, plain: f64) {
    s.tracks
        .get_mut("acid")
        .unwrap()
        .instrument
        .params
        .insert("cutoff".into(), plain);
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
fn fx_overrides_follow_their_device() {
    let hard = mini_with(|s| s.tracks.get_mut("acid").unwrap().fx[0].settings[0] = "hard");
    let (mut ctl, mut p) = live(compiled(MINI), 0, None);
    ctl.set_override(ACID, CUTOFF, Some(2000.0)).unwrap();
    ctl.set_override(ACID, DRIVE, Some(30.0)).unwrap();
    play(&mut p, 1000, 512);
    assert_eq!(ctl.overrides_active(), 2);
    // A kept distortion keeps its overrides.
    ctl.swap(kick_muted()).unwrap();
    play(&mut p, STEP, 512);
    assert_eq!(ctl.overrides_active(), 2);
    // A rebuilt distortion starts without them; the instrument keeps its own.
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

/// An output fixed at one rate that plays one block on start and captures it.
struct CaptureOut {
    rate: u32,
    frames: usize,
    captured: Arc<Mutex<Vec<f32>>>,
}

impl AudioOut for CaptureOut {
    fn sample_rate(&mut self, _wanted: u32) -> Result<u32, String> {
        Ok(self.rate)
    }

    fn start(&mut self, mut processor: LiveProcessor) -> Result<Box<dyn Send>, String> {
        if processor.sample_rate() != self.rate {
            return Err(format!("expected {} Hz", self.rate));
        }
        let mut out = vec![0.0f32; 2 * self.frames];
        processor.process(&mut out);
        *self.captured.lock().unwrap() = out;
        Ok(Box::new(processor))
    }
}

#[test]
fn audio_out_drives_the_processor() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut device: Box<dyn AudioOut> = Box::new(CaptureOut {
        rate: 48_000,
        frames: 512,
        captured: captured.clone(),
    });
    let rate = device.sample_rate(44_100).unwrap();
    assert_eq!(rate, 48_000, "the device's rate, not the wanted one");
    let a = Arc::new(CompiledSong::new(&song(MINI), rate));
    let (ctl, p) = live(a.clone(), 0, None);
    let handle = device.start(p).unwrap();
    assert_eq!(ctl.position(), 512);
    assert_bits(
        &captured.lock().unwrap(),
        frames(&render_bars(&a, 0, 1), 0..512),
    );
    drop(handle);

    let other = Arc::new(CompiledSong::new(&song(MINI), 44_100));
    let (_ctl, p) = live(other, 0, None);
    assert!(device.start(p).is_err());
}

/// `mixed.toml` changed by `edit`.
fn mixed_with(edit: impl FnOnce(&mut tune_song::Song)) -> Arc<CompiledSong> {
    compile_song(&variant_of(MIXED, edit))
}

fn set_mix_param(s: &mut tune_song::Song, id: &str, plain: f64) {
    s.mix.params.insert(id.into(), plain);
}

/// Queues `song` on `engine` and plays one step of the playing song, so the
/// swap lands; drops the retired update. Returns what was played.
fn swap_and_play(engine: &mut Engine, song: &Arc<CompiledSong>) -> Vec<f32> {
    let applied = engine.swaps_applied();
    assert!(engine.queue_swap(SongUpdate::new(song.clone())).is_none());
    let step = engine.song().clock.samples_per_step.ceil() as u64;
    let out = run(engine, step);
    assert_eq!(engine.swaps_applied(), applied + 1);
    drop(engine.take_retired_swap().expect("the swap landed"));
    out
}

#[test]
fn mix_live_matches_offline() {
    let s = compiled(MIXED);
    let expected = render_bars(&s, 0, s.total_bars);
    let len = expected.len() as u64 / 2;
    for block in [1usize, 61, 512, 4096] {
        let (ctl, mut p) = live(s.clone(), 0, None);
        let out = play(&mut p, len, block);
        assert_eq!(ctl.position(), len, "block {block}");
        assert_bits(&out, &expected);
    }
}

#[test]
fn swap_keeps_mix_when_alike() {
    let feedback = mixed_with(|s| set_mix_param(s, "delay.feedback", 45.0));
    let clip = mixed_with(|s| {
        set_mix_param(s, "delay.feedback", 45.0);
        s.mix.master = MasterMode::Clip;
    });
    let faster = mixed_with(|s| {
        set_mix_param(s, "delay.feedback", 45.0);
        s.mix.master = MasterMode::Clip;
        s.bpm = 121.0;
    });
    let mut e = Engine::new(compiled(MIXED));
    // Inside section `b`, with the delay ringing.
    let mut out = run(&mut e, 2 * BAR + 1000);
    out.extend(swap_and_play(&mut e, &feedback));
    assert_eq!(e.mix_swaps_kept(), 1, "built alike: kept");
    out.extend(swap_and_play(&mut e, &clip));
    assert_eq!(e.mix_swaps_kept(), 1, "another master mode: replaced");
    out.extend(swap_and_play(&mut e, &faster));
    assert_eq!(e.mix_swaps_kept(), 1, "another bpm: replaced");
    assert_eq!(e.swaps_applied(), 3);
    let rest = run(&mut e, BAR);
    assert!(!e.finished());
    assert!(out.iter().chain(&rest).all(|x| x.is_finite()));
    assert!(rest.iter().any(|&x| x != 0.0), "playback continues");
}

#[test]
fn kept_mix_keeps_bus_tails() {
    // A swap to the same song keeps every runtime, buses and limiter
    // included, so playback goes on as if no swap had happened.
    let a = compiled(MIXED);
    let expected = render_bars(&a, 0, a.total_bars);
    let mut e = Engine::new(a.clone());
    let mut out = run(&mut e, 2 * BAR + 1000);
    out.extend(swap_and_play(&mut e, &compiled(MIXED)));
    assert_eq!(e.mix_swaps_kept(), 1);
    let len = expected.len() as u64 / 2;
    out.extend(run(&mut e, len - out.len() as u64 / 2));
    assert_bits(&out, &expected);
}

#[test]
fn swap_builds_missing_units() {
    // From the clip mode to the limiter: the swap brings one.
    let clip = mixed_with(|s| s.mix.master = MasterMode::Clip);
    let mut e = Engine::new(clip);
    run(&mut e, 2 * BAR + 1000);
    swap_and_play(&mut e, &compiled(MIXED));
    assert_eq!(e.mix_swaps_kept(), 0);
    e.reset_meter();
    run(&mut e, BAR / 2);
    assert!(
        e.meter().max_reduction_db > 0.0,
        "the limiter runs after the swap"
    );

    // From no delay bus to one (all else alike): the delay's params are
    // then heard.
    let no_delay = mixed_with(|s| {
        s.tracks
            .get_mut("acid")
            .unwrap()
            .strip
            .shift_remove("send.delay");
        drop_lane(s, "acid", "send.delay");
    });
    let echo = |feedback: f64| {
        heard_after_swap(
            &no_delay,
            &mixed_with(|s| set_mix_param(s, "delay.feedback", feedback)),
        )
    };
    assert_ne!(echo(10.0), echo(80.0), "the swap built the delay bus");

    // From no reverb bus to one (all else alike): likewise.
    let no_reverb = mixed_with(|s| {
        for t in ["acid", "hat"] {
            s.tracks
                .get_mut(t)
                .unwrap()
                .strip
                .shift_remove("send.reverb");
        }
    });
    let room = |damp: f64| {
        heard_after_swap(
            &no_reverb,
            &mixed_with(|s| set_mix_param(s, "reverb.damp", damp)),
        )
    };
    assert_ne!(room(0.0), room(100.0), "the swap built the reverb bus");
}

/// The longest run of frames in which both channels are exactly 0.0.
fn longest_silence(x: &[f32]) -> usize {
    let (mut run, mut longest) = (0, 0);
    for f in x.chunks_exact(2) {
        if f[0] == 0.0 && f[1] == 0.0 {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

#[test]
fn reload_in_the_drop_never_goes_silent() {
    let limit = compiled(FIRST_ACID_MIXED);
    let faster = compile_song(&variant_of(FIRST_ACID_MIXED, |s| s.bpm = 146.0));
    let clip = compile_song(&variant_of(FIRST_ACID_MIXED, |s| {
        s.mix.master = MasterMode::Clip;
    }));
    for (name, from, to) in [
        ("bpm 145 -> 146", &limit, &faster),
        ("clip -> limit", &clip, &limit),
        ("limit -> clip", &limit, &clip),
    ] {
        let mut e = Engine::new(from.clone());
        let bar = (16.0 * from.clock.samples_per_step) as u64;
        // One bar into the drop (bar 81, 1-based), then the reload.
        e.seek(80);
        let before = run(&mut e, bar);
        let loud = before.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(loud > 0.5, "{name}: the drop is loud ({loud})");
        assert!(e.queue_swap(SongUpdate::new(to.clone())).is_none());
        let after = run(&mut e, bar);
        assert_eq!(e.swaps_applied(), 1, "{name}");
        let silent = longest_silence(&after);
        assert!(
            silent < 8,
            "{name}: {silent} silent frames after the reload"
        );
        if to.mix.master == MasterMode::Limit {
            // The swap lands within a step; a step and 20 ms on, the
            // limiter alone holds the output (the clipper would allow
            // -1 dBFS).
            let settled = (from.clock.samples_per_step + 960.0) as usize;
            let peak = after[2 * settled..]
                .iter()
                .fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(peak <= db2amp(-1.5) as f32, "{name}: peak {peak}");
        }
    }
}

#[test]
fn bpm_reload_keeps_the_reverb_tail() {
    // The same song played on without a reload is the reference: the hat,
    // the reverb and the limiter all carry on, so until the next hit the
    // output must not change by a bit.
    let a = compiled(ROOM);
    let faster = compile_song(&variant_of(ROOM, |s| s.bpm = 121.0));
    let mut reference = Engine::new(a.clone());
    let mut e = Engine::new(a);
    let lead = 4 * STEP + 10;
    assert_bits(&run(&mut e, lead), &run(&mut reference, lead));
    assert!(e.queue_swap(SongUpdate::new(faster)).is_none());
    // The swap lands at step 5; the next hit is 11 steps later.
    let tail = 6 * STEP;
    let heard = run(&mut e, tail);
    assert_eq!(e.swaps_applied(), 1);
    assert!(heard.iter().any(|&x| x.abs() > 1e-3), "the tail rings");
    assert_bits(&heard, &run(&mut reference, tail));
}

/// Plays `from` into section `b`, swaps to `to` (which must replace the mix
/// runtime) and returns the bar after the swap.
fn heard_after_swap(from: &Arc<CompiledSong>, to: &Arc<CompiledSong>) -> Vec<f32> {
    let mut e = Engine::new(from.clone());
    run(&mut e, 2 * BAR + 1000);
    swap_and_play(&mut e, to);
    assert_eq!(e.mix_swaps_kept(), 0, "the mix is built anew");
    run(&mut e, BAR)
}

// ----- device identity across swaps -----

/// The `set_param_at` values a probe logged at `sample`, by param index.
fn params_at(log: &SharedLog, sample: u64) -> Vec<(usize, f64)> {
    log.lock()
        .unwrap()
        .params()
        .into_iter()
        .filter(|&(s, _, _)| s == sample)
        .map(|(_, index, plain)| (index, plain))
        .collect()
}

fn samples(log: &SharedLog) -> u64 {
    log.lock().unwrap().samples
}

/// `mixed` with the acid's chain `[dist, eq]` turned around.
fn reordered_mixed() -> Arc<CompiledSong> {
    mixed_with(|s| s.tracks.get_mut("acid").unwrap().fx.reverse())
}

#[test]
fn swap_reordering_fx_keeps_both_devices() {
    let a = compiled(MIXED);
    let b = reordered_mixed();
    assert_eq!(b.tracks[ACID].fx[1].id, "dist");
    let (mut e, _, fx) = probe_engine_slots(&a, "acid", &[0, 1]);
    let (dist, eq) = (&fx[0], &fx[1]);
    // Bar 1 starts on a step: the swap lands on its first sample.
    run(&mut e, BAR);
    assert!(e.queue_swap(SongUpdate::new(b.clone())).is_none());
    run(&mut e, BAR);
    assert_eq!(e.swaps_applied(), 1);
    // Both probes ran through the swap, each in its new slot, and got their
    // own device's static params when it landed.
    assert_eq!(samples(dist), 2 * BAR, "the distortion moved to slot 1");
    assert_eq!(samples(eq), 2 * BAR, "the eq moved to slot 0");
    let statics = |params: &[f64]| params.iter().copied().enumerate().collect::<Vec<_>>();
    assert_eq!(params_at(dist, BAR), statics(&b.tracks[ACID].fx[1].params));
    assert_eq!(params_at(eq, BAR), statics(&b.tracks[ACID].fx[0].params));
}

#[test]
fn swap_changing_the_mode_rebuilds_only_that_device() {
    let a = compiled(MIXED);
    let hard = mixed_with(|s| s.tracks.get_mut("acid").unwrap().fx[0].settings[0] = "hard");
    let (mut e, _, fx) = probe_engine_slots(&a, "acid", &[0, 1]);
    let (dist, eq) = (&fx[0], &fx[1]);
    run(&mut e, BAR);
    assert!(e.queue_swap(SongUpdate::new(hard)).is_none());
    run(&mut e, BAR);
    assert_eq!(e.swaps_applied(), 1);
    assert_eq!(samples(eq), 2 * BAR, "the eq is kept");
    assert!(!params_at(eq, BAR).is_empty(), "statics re-applied");
    assert_eq!(samples(dist), BAR, "the distortion is rebuilt");
    assert!(params_at(dist, BAR).is_empty());
}

#[test]
fn swap_changing_the_instrument_keeps_fx_and_strip() {
    // The ride ducks under the kick through an eq; its eq probe outputs a
    // constant, so what the ride adds to the mix depends on the probe, the
    // ducker and the gain alone, not on the instrument.
    let ride_eq = |s: &mut tune_song::Song| {
        let ride = s.tracks.get_mut("ride").unwrap();
        ride.fx.push(Device::new("eq", &EQ));
        ride.strip.insert("duck.depth".into(), 12.0);
        ride.strip.insert("duck.release".into(), 400.0);
    };
    let a = mixed_with(ride_eq);
    let b = mixed_with(|s| {
        ride_eq(s);
        s.tracks.get_mut("ride").unwrap().instrument = Device::new("hat_open", &HAT_OPEN);
    });
    let probed = |song: &Arc<CompiledSong>| {
        let mut logs = None;
        let engine = engine_with(song, |t, rt| {
            if t.id == "ride" {
                let (probe, log) = Probe::new(t.instrument.module.params);
                let (fx, fx_log) = ProbeFx::constant(t.fx[0].module.params, 0.25);
                rt.instrument = Box::new(probe);
                rt.fx[0] = Box::new(fx);
                logs = Some((log, fx_log));
            }
        });
        let (log, fx_log) = logs.expect("a ride track");
        (engine, log, fx_log)
    };
    let (mut reference, _, _) = probed(&a);
    let (mut e, instrument, fx) = probed(&a);
    // The kick hits on step 16 (bar 1); the swap lands a step later, while
    // the ride is still ducked.
    let lead = BAR + 1000;
    assert_bits(&run(&mut e, lead), &run(&mut reference, lead));
    assert!(e.queue_swap(SongUpdate::new(b)).is_none());
    let heard = run(&mut e, BAR);
    assert_eq!(e.swaps_applied(), 1);
    assert_eq!(e.mix_swaps_kept(), 1);
    // The eq and the strip (its ducker mid-release) carry on as if no swap
    // had happened.
    assert_bits(&heard, &run(&mut reference, BAR));
    assert_eq!(samples(&fx), lead + BAR, "the eq is kept");
    // The instrument was rebuilt: the probe saw nothing from the swap on.
    let swap_at = BAR + STEP;
    let log = instrument.lock().unwrap();
    assert!(log.events.iter().all(|e| match e {
        Event::Step { sample, .. } | Event::Param { sample, .. } | Event::Reset { sample } =>
            *sample < swap_at,
    }));
    assert!(!log.events.is_empty());
}
