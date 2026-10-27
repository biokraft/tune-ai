//! Proves `LiveProcessor::process` and `Engine::process` neither allocate
//! nor free memory, even across blocks that apply a swap (one that keeps the
//! mix, one that rebuilds the delay for a new bpm, master-mode switches that
//! crossfade between the clipper and the limiter, and swaps that add,
//! reorder, replace or remove devices or move a placement, swaps that edit
//! a note clip while its notes are held, and structural swaps that
//! crossfade a track's old and new paths), a seek, a loop wrap, the song
//! end and knob overrides on tracks, strips and the mix.
//!
//! A counting global allocator counts every allocation, reallocation and
//! deallocation made while the thread-local "audio" flag is set. The flag is
//! set only around `process` calls, so the controller's work (building
//! updates, dropping garbage) is not counted. The counts are thread-local
//! too, so tests running in parallel do not see each other's.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use common::{MINI, MIXED, compile_song, compiled, keep_sections, song, variant_of};
use tune_dsp::{MASTER_GAIN, STRIP_SEND_DELAY};
use tune_engine::{CompiledSong, Engine, LiveProcessor, MIX_TRACK, ParamTarget, SongUpdate, live};
use tune_modules::{CLAP, EQ, KICK, ModuleDescriptor};
use tune_song::{Device, MasterMode, MixUnit, Track};

struct Counting;

thread_local! {
    static AUDIO: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
    static DEALLOCS: Cell<usize> = const { Cell::new(0) };
}

fn on_audio_thread() -> bool {
    AUDIO.try_with(Cell::get).unwrap_or(false)
}

fn count(counter: &'static std::thread::LocalKey<Cell<usize>>) {
    let _ = counter.try_with(|c| c.set(c.get() + 1));
}

/// This thread's `(allocations, deallocations)` under the flag so far.
fn counts() -> (usize, usize) {
    (ALLOCS.with(Cell::get), DEALLOCS.with(Cell::get))
}

// SAFETY: every call forwards to the system allocator unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if on_audio_thread() {
            count(&ALLOCS);
        }
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if on_audio_thread() {
            count(&ALLOCS);
        }
        // SAFETY: as for `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if on_audio_thread() {
            count(&ALLOCS);
            count(&DEALLOCS);
        }
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if on_audio_thread() {
            count(&DEALLOCS);
        }
        // SAFETY: the caller upholds `GlobalAlloc::dealloc`'s contract.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Runs `frames` frames through `process` in `block`-frame calls with the
/// audio flag set, checking each block is finite. `out` is allocated up
/// front, off the flag.
fn under_flag(out: &mut [f32], frames: u64, block: usize, mut process: impl FnMut(&mut [f32])) {
    let mut done = 0u64;
    while done < frames {
        let n = (frames - done).min(block as u64) as usize;
        AUDIO.with(|a| a.set(true));
        process(&mut out[..2 * n]);
        AUDIO.with(|a| a.set(false));
        assert!(out[..2 * n].iter().all(|x| x.is_finite()));
        done += n as u64;
    }
}

fn audio(p: &mut LiveProcessor, out: &mut [f32], frames: u64, block: usize) {
    under_flag(out, frames, block, |o| p.process(o));
}

#[track_caller]
fn assert_no_alloc() {
    let (allocs, deallocs) = counts();
    assert_eq!(allocs, 0, "allocations on the audio thread");
    assert_eq!(deallocs, 0, "deallocations on the audio thread");
}

#[test]
fn process_never_allocates_or_frees() {
    const BAR: u64 = 96_000;
    const STEP: u64 = 6000;

    // The counter works: an allocation under the flag is seen.
    AUDIO.with(|a| a.set(true));
    drop(std::hint::black_box(Box::new(1u64)));
    AUDIO.with(|a| a.set(false));
    assert_eq!(counts(), (1, 1));
    ALLOCS.with(|c| c.set(0));
    DEALLOCS.with(|c| c.set(0));

    // B: drops `hat`, changes the acid distortion mode (a fresh distortion),
    // adds a `clap` track (fresh runtime) and mutes the kick (kept runtime).
    let mut b = song(MINI);
    b.tracks.shift_remove("hat");
    b.clips.shift_remove("h");
    b.tracks.get_mut("kick").unwrap().mute = true;
    b.tracks.get_mut("acid").unwrap().fx[0].settings[0] = "hard";
    b.tracks.insert(
        "clap".into(),
        Track {
            instrument: Device::new("clap", &CLAP),
            ..b.tracks["kick"].clone()
        },
    );
    let b = compile_song(&b);
    let a = compiled(MINI);
    let mut two_bars = song(MINI);
    keep_sections(&mut two_bars, 1);
    let two_bars = compile_song(&two_bars);

    let (mut ctl, mut p) = live(a.clone(), 0, None);
    let mut out = vec![0.0f32; 2 * 4096];

    // A swap applied at the step boundary.
    audio(&mut p, &mut out, 1000, 512);
    ctl.swap(b.clone()).unwrap();
    audio(&mut p, &mut out, STEP, 512);
    assert_eq!(ctl.swaps_applied(), 1);
    assert_eq!(ctl.collect_garbage(), 1);

    // A pending swap replaced by a newer one, both popped in one block.
    ctl.swap(b.clone()).unwrap();
    ctl.swap(a.clone()).unwrap();
    audio(&mut p, &mut out, STEP, 333);
    assert_eq!(ctl.swaps_applied(), 2);
    assert_eq!(ctl.collect_garbage(), 2);

    // A seek and a loop wrap.
    ctl.seek(3).unwrap();
    ctl.set_loop(Some(3..4)).unwrap();
    audio(&mut p, &mut out, BAR + 100, 4096);
    assert_eq!(ctl.position(), 3 * BAR + 100);

    // The song end.
    ctl.set_loop(None).unwrap();
    audio(&mut p, &mut out, BAR, 4096);
    assert!(ctl.finished());

    // A swap that lands past the new end and finishes playback.
    ctl.seek(3).unwrap();
    audio(&mut p, &mut out, 1000, 512);
    assert!(!ctl.finished());
    ctl.swap(two_bars).unwrap();
    audio(&mut p, &mut out, STEP, 512);
    assert!(ctl.finished());
    assert_eq!(ctl.swaps_applied(), 3);
    assert_eq!(ctl.collect_garbage(), 1);

    // Overrides: set two, clear one, carry one across a swap that rebuilds
    // the fx chain, then a swap that clears them.
    let cutoff = ParamTarget::Instrument(2);
    let drive = ParamTarget::Fx { slot: 0, index: 0 };
    // `two_bars` is playing and finished; bar 1 is its last.
    ctl.seek(1).unwrap();
    ctl.set_override(1, cutoff, Some(2000.0)).unwrap();
    ctl.set_override(1, drive, Some(30.0)).unwrap();
    audio(&mut p, &mut out, 1000, 512);
    assert_eq!(ctl.overrides_active(), 2);
    ctl.set_override(1, drive, None).unwrap();
    audio(&mut p, &mut out, 1000, 512);
    assert_eq!(ctl.overrides_active(), 1);
    ctl.set_override(1, drive, Some(30.0)).unwrap();
    ctl.swap(b.clone()).unwrap();
    audio(&mut p, &mut out, STEP, 512);
    assert_eq!(ctl.swaps_applied(), 4);
    assert_eq!(ctl.overrides_active(), 1);
    assert_eq!(ctl.collect_garbage(), 1);
    ctl.swap_clearing_overrides(a.clone()).unwrap();
    audio(&mut p, &mut out, STEP, 512);
    assert_eq!(ctl.swaps_applied(), 5);
    assert_eq!(ctl.overrides_active(), 0);
    assert_eq!(ctl.collect_garbage(), 1);

    // Devices: an eq added to the acid and the kick, the acid's chain
    // reordered (both devices kept, in new slots), then the kick keeping its
    // id with a new instrument (its strip and fx kept).
    let chain = |reversed: bool, kick: &'static ModuleDescriptor| {
        compile_song(&variant_of(MINI, |s| {
            let acid = s.tracks.get_mut("acid").unwrap();
            acid.fx.push(Device::new("eq", &EQ));
            if reversed {
                acid.fx.reverse();
            }
            let k = s.tracks.get_mut("kick").unwrap();
            k.fx.push(Device::new("eq", &EQ));
            k.instrument = Device::new(kick.name, kick);
        }))
    };
    for (i, next) in [chain(false, &KICK), chain(true, &KICK), chain(true, &CLAP)]
        .into_iter()
        .enumerate()
    {
        ctl.swap(next).unwrap();
        audio(&mut p, &mut out, STEP, 512);
        assert_eq!(ctl.swaps_applied(), 6 + i as u64);
        assert_eq!(ctl.collect_garbage(), 1);
    }

    // Back to plain `mini`: the acid's and the kick's eq are removed and
    // the kick is a kick again.
    ctl.swap(a.clone()).unwrap();
    audio(&mut p, &mut out, STEP, 512);
    assert_eq!(ctl.swaps_applied(), 9);
    assert_eq!(ctl.collect_garbage(), 1);

    assert_no_alloc();
}

/// Track 1 of `mixed` is `acid`.
const ACID: usize = 1;

const MASTER_GAIN_KNOB: ParamTarget = ParamTarget::Mix {
    unit: MixUnit::Master,
    index: MASTER_GAIN,
};

#[test]
fn mixed_song_runs_without_alloc() {
    const STEP: u64 = 6000;
    let kept = compile_song(&variant_of(MIXED, |s| {
        s.mix.params.insert("delay.feedback".into(), 45.0);
    }));
    let again = compiled(MIXED);
    let (mut ctl, mut p) = live(compiled(MIXED), 0, None);
    let mut out = vec![0.0f32; 2 * 4096];

    // Section `b`, where the acid sends into the delay.
    ctl.seek(2).unwrap();
    audio(&mut p, &mut out, 1000, 512);
    // A mix override and a strip override.
    ctl.set_override(MIX_TRACK, MASTER_GAIN_KNOB, Some(3.0))
        .unwrap();
    let send = ParamTarget::Strip(STRIP_SEND_DELAY);
    ctl.set_override(ACID, send, Some(0.0)).unwrap();
    audio(&mut p, &mut out, STEP, 333);
    assert_eq!(ctl.overrides_active(), 2);

    // A swap that keeps the mix runtime, with its override.
    ctl.swap(kept).unwrap();
    audio(&mut p, &mut out, STEP, 512);
    assert_eq!(ctl.swaps_applied(), 1);
    assert_eq!(ctl.overrides_active(), 2);
    assert_eq!(ctl.collect_garbage(), 1);

    ctl.set_override(MIX_TRACK, MASTER_GAIN_KNOB, None).unwrap();
    audio(&mut p, &mut out, 1000, 61);
    assert_eq!(ctl.overrides_active(), 1);
    ctl.swap_clearing_overrides(again).unwrap();
    audio(&mut p, &mut out, STEP, 4096);
    assert_eq!(ctl.swaps_applied(), 2);
    assert_eq!(ctl.overrides_active(), 0);
    assert_eq!(ctl.collect_garbage(), 1);

    assert_no_alloc();
}

#[test]
fn bpm_swap_rebuilds_mix_without_alloc() {
    // The extreme of the delay buffer: 8 steps at 40 BPM and 384 kHz.
    const RATE: u32 = 384_000;
    let at_rate = |s: &tune_song::Song| Arc::new(CompiledSong::new(s, RATE));
    let fast = at_rate(&song(MIXED));
    let slow = at_rate(&variant_of(MIXED, |s| {
        s.bpm = 40.0;
        s.mix.params.insert("delay.time".into(), 8.0);
    }));
    let step = fast.clock.samples_per_step.ceil() as u64;
    let slow_step = slow.clock.samples_per_step.ceil() as u64;
    let mut e = Engine::new(fast);
    let mut out = vec![0.0f32; 2 * 4096];

    // Two steps of the acid into the delay, then a bpm swap while it rings.
    e.seek(2);
    under_flag(&mut out, 2 * step + 1000, 4096, |o| e.process(o));
    assert!(e.queue_swap(SongUpdate::new(slow)).is_none());
    under_flag(&mut out, step, 4096, |o| e.process(o));
    assert_eq!(e.swaps_applied(), 1);
    assert_eq!(e.mix_swaps_kept(), 0, "a new bpm rebuilds the mix");
    drop(e.take_retired_swap().expect("the swap landed"));
    // Nine slow steps: the delay reads back its whole 8 steps.
    under_flag(&mut out, 9 * slow_step, 4096, |o| e.process(o));
    assert!(!e.finished());

    assert_no_alloc();
}

#[test]
fn master_switches_swap_without_alloc() {
    const STEP: u64 = 6000;
    let clip = |bpm: f64| {
        compile_song(&variant_of(MIXED, |s| {
            s.mix.master = MasterMode::Clip;
            s.bpm = bpm;
        }))
    };
    let faster = compile_song(&variant_of(MIXED, |s| s.bpm = 121.0));
    let (mut ctl, mut p) = live(clip(120.0), 0, None);
    let mut out = vec![0.0f32; 2 * 4096];

    // Section `b`: the acid rings in both buses.
    ctl.seek(2).unwrap();
    audio(&mut p, &mut out, 1000, 512);
    // Clip to limit: the fresh limiter fills, then the crossfade (well
    // inside a step).
    ctl.swap(compiled(MIXED)).unwrap();
    audio(&mut p, &mut out, STEP, 512);
    assert_eq!(ctl.swaps_applied(), 1);
    assert_eq!(ctl.collect_garbage(), 1);
    // A new bpm: the delay is rebuilt, the reverb and the limiter stay.
    ctl.swap(faster).unwrap();
    audio(&mut p, &mut out, STEP, 333);
    assert_eq!(ctl.swaps_applied(), 2);
    assert_eq!(ctl.collect_garbage(), 1);
    // Limit to clip: the crossfade back; the limiter stays, idle.
    ctl.swap(clip(120.0)).unwrap();
    audio(&mut p, &mut out, STEP, 61);
    assert_eq!(ctl.swaps_applied(), 3);
    assert_eq!(ctl.collect_garbage(), 1);
    // Clip to clip: the idle limiter goes back as garbage.
    ctl.swap(clip(122.0)).unwrap();
    audio(&mut p, &mut out, STEP, 4096);
    assert_eq!(ctl.swaps_applied(), 4);
    assert_eq!(ctl.collect_garbage(), 1);
    audio(&mut p, &mut out, STEP, 512);

    assert_no_alloc();
}

/// A v2 song: the acid through the chain `fx`, the kick's first placement at
/// 1-based bar `kick_at`.
fn v2_song(fx: &str, kick_at: u32) -> Arc<CompiledSong> {
    compiled(&format!(
        r#"
[song]
version = 2
name = "v2"
bpm = 120
seed = 7

[tracks.kick]
instrument = "kick"
play = [
  {{ at = {kick_at}, clip = "k4", bars = 1 }},
  {{ at = 3, clip = "k4", bars = 2 }},
]

[tracks.acid]
instrument = "acid303"
fx = [{fx}]
play = [{{ at = 1, clip = "a1", bars = 4, offset = 1 }}]

[clips.k4]
kind = "drum"
steps = "x...x...x...X..."

[clips.a1]
kind = "acid"
steps = "C2 C2! C3~ C2 . D#2 C2~ G2!"

[[sections]]
name = "a"
bars = 4

[[automation]]
param = "acid.eq2.mid_gain"
at = 2
bars = 2
points = [[0, 0.2], [2, 0.8]]
"#
    ))
}

#[test]
fn device_and_placement_swaps_never_allocate() {
    const STEP: u64 = 6000;
    const EQ: &str = r#"{ type = "eq", params = { "eq.low_cut" = 85 } }"#;
    const EQ2: &str = r#"{ type = "eq", id = "eq2", params = { "eq2.mid_gain" = 3 } }"#;
    const DIST: &str = r#"{ type = "distortion", params = { "dist.drive" = 20 } }"#;
    let songs = [
        // A device added at the end of the chain.
        v2_song(&format!("{EQ}, {EQ2}, {DIST}"), 1),
        // One removed.
        v2_song(&format!("{EQ2}, {DIST}"), 1),
        // Two eqs, then the same two the other way round.
        v2_song(&format!("{EQ}, {EQ2}"), 1),
        v2_song(&format!("{EQ2}, {EQ}"), 1),
        // The kick's first placement one bar later.
        v2_song(&format!("{EQ2}, {EQ}"), 2),
    ];
    let (mut ctl, mut p) = live(v2_song(&format!("{EQ}, {EQ2}"), 1), 0, None);
    let mut out = vec![0.0f32; 2 * 4096];
    audio(&mut p, &mut out, 1000, 512);
    for (i, next) in songs.into_iter().enumerate() {
        ctl.swap(next).unwrap();
        audio(&mut p, &mut out, STEP, 512);
        assert_eq!(ctl.swaps_applied(), 1 + i as u64);
        assert_eq!(ctl.collect_garbage(), 1);
    }
    assert_no_alloc();
}

/// A v2 song whose `lead` holds a note from every step of 0 to 6 (each to
/// step 16) in a notes clip of `bars` bars, plus one at `other`, at
/// `gain_db`; with `extra`, the lead plays the clip again from bar 3.
fn notes_song(bars: u32, other: u32, extra: bool, gain_db: i32) -> Arc<CompiledSong> {
    let mut notes: Vec<String> = (0..7)
        .map(|s| format!(r#"{{ start = {s}, pitch = "C2", len = {} }}"#, 16 - s))
        .collect();
    notes.push(format!(r#"{{ start = {other}, pitch = "G2", len = 2 }}"#));
    let extra = if extra {
        r#", { at = 3, clip = "n", bars = 2 }"#
    } else {
        ""
    };
    compiled(&format!(
        r#"
[song]
version = 2
name = "notes"
bpm = 120
seed = 7

[tracks.lead]
instrument = "acid303"
gain_db = {gain_db}
play = [{{ at = 1, clip = "n", bars = 2 }}{extra}]

[clips.n]
kind = "notes"
bars = {bars}
notes = [{}]

[[sections]]
name = "a"
bars = 4
"#,
        notes.join(", ")
    ))
}

#[test]
fn note_clip_swaps_never_allocate() {
    const STEP: u64 = 6000;
    let songs = [
        // A note moved (the held one ends).
        notes_song(2, 11, false, 0),
        // The clip's length changed.
        notes_song(1, 11, false, 0),
        // A notes-clip placement added.
        notes_song(1, 11, true, 0),
        // Only a gain (the held note goes on).
        notes_song(1, 11, true, -3),
    ];
    let (mut ctl, mut p) = live(notes_song(2, 10, false, 0), 0, None);
    let mut out = vec![0.0f32; 2 * 4096];
    audio(&mut p, &mut out, 1000, 512);
    for (i, next) in songs.into_iter().enumerate() {
        ctl.swap(next).unwrap();
        audio(&mut p, &mut out, STEP, 512);
        assert_eq!(ctl.swaps_applied(), 1 + i as u64);
        assert_eq!(ctl.collect_garbage(), 1);
    }
    assert_no_alloc();
}

#[test]
fn structural_swaps_fade_without_alloc() {
    const STEP: u64 = 6000;
    let eq2 = Device {
        id: "eq2".into(),
        ..Device::new("eq", &EQ)
    };
    let with = |edit: &dyn Fn(&mut tune_song::Song)| compile_song(&variant_of(MIXED, edit));
    let songs = [
        // An eq added after the acid's two devices (they copy their state).
        with(&|s| {
            s.tracks.get_mut("acid").unwrap().fx.push(eq2.clone());
        }),
        // Its two eqs the other way round.
        with(&|s| {
            let fx = &mut s.tracks.get_mut("acid").unwrap().fx;
            fx.push(eq2.clone());
            fx.swap(1, 2);
        }),
        // The distortion's mode changed.
        with(&|s| {
            let fx = &mut s.tracks.get_mut("acid").unwrap().fx;
            fx.push(eq2.clone());
            fx.swap(1, 2);
            fx[0].settings[0] = "hard";
        }),
        // A track added: it fades in.
        with(&|s| {
            let fx = &mut s.tracks.get_mut("acid").unwrap().fx;
            fx.push(eq2.clone());
            fx.swap(1, 2);
            fx[0].settings[0] = "hard";
            s.tracks.insert(
                "clap".into(),
                Track {
                    instrument: Device::new("clap", &CLAP),
                    ..s.tracks["kick"].clone()
                },
            );
        }),
        // The hat (into the reverb) removed: it fades out.
        with(&|s| {
            let fx = &mut s.tracks.get_mut("acid").unwrap().fx;
            fx.push(eq2.clone());
            fx.swap(1, 2);
            fx[0].settings[0] = "hard";
            s.tracks.shift_remove("hat");
            s.clips.shift_remove("h");
        }),
    ];
    let (mut ctl, mut p) = live(compiled(MIXED), 0, None);
    let mut out = vec![0.0f32; 2 * 4096];
    // Section `b`: the acid sounds, into both buses.
    ctl.seek(2).unwrap();
    audio(&mut p, &mut out, 1000, 512);
    for (i, next) in songs.into_iter().enumerate() {
        ctl.swap(next).unwrap();
        // The swap lands 5000 frames in; its fade ends 480 frames later.
        audio(&mut p, &mut out, STEP, if i % 2 == 0 { 512 } else { 4096 });
        assert_eq!(ctl.swaps_applied(), 1 + i as u64);
        assert_eq!(ctl.collect_garbage(), 1, "swap {i}");
    }
    assert_no_alloc();
}
