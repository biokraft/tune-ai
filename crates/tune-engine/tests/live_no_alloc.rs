//! Proves `LiveProcessor::process` neither allocates nor frees memory, even
//! across blocks that apply a swap, a seek, a loop wrap, the song end and
//! knob overrides.
//!
//! A counting global allocator counts every allocation, reallocation and
//! deallocation made while the thread-local "audio" flag is set. The flag is
//! set only around `process` calls, so the controller's work (building
//! updates, dropping garbage) is not counted.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{MINI, compile_song, compiled, song};
use tune_dsp::DistortionMode;
use tune_engine::{LiveProcessor, ParamTarget, live};
use tune_song::{FxSpec, InstrumentKind, Track};

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static DEALLOCS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static AUDIO: Cell<bool> = const { Cell::new(false) };
}

fn on_audio_thread() -> bool {
    AUDIO.try_with(Cell::get).unwrap_or(false)
}

// SAFETY: every call forwards to the system allocator unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if on_audio_thread() {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if on_audio_thread() {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: as for `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if on_audio_thread() {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            DEALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if on_audio_thread() {
            DEALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the caller upholds `GlobalAlloc::dealloc`'s contract.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Runs `frames` frames through `p` in `block`-frame calls with the audio
/// flag set. `out` is allocated up front, off the flag.
fn audio(p: &mut LiveProcessor, out: &mut [f32], frames: u64, block: usize) {
    let mut done = 0u64;
    while done < frames {
        let n = (frames - done).min(block as u64) as usize;
        AUDIO.with(|a| a.set(true));
        p.process(&mut out[..2 * n]);
        AUDIO.with(|a| a.set(false));
        done += n as u64;
    }
}

#[test]
fn process_never_allocates_or_frees() {
    const BAR: u64 = 96_000;
    const STEP: u64 = 6000;

    // The counter works: an allocation under the flag is seen.
    AUDIO.with(|a| a.set(true));
    drop(std::hint::black_box(Box::new(1u64)));
    AUDIO.with(|a| a.set(false));
    assert_eq!(ALLOCS.swap(0, Ordering::Relaxed), 1);
    assert_eq!(DEALLOCS.swap(0, Ordering::Relaxed), 1);

    // B: drops `hat`, changes the acid distortion mode (fresh fx chain), adds
    // a `clap` track (fresh runtime) and mutes the kick (kept runtime).
    let mut b = song(MINI);
    b.tracks.shift_remove("hat");
    b.patterns.shift_remove("h");
    for s in &mut b.sections {
        s.play.shift_remove("hat");
    }
    b.tracks.get_mut("kick").unwrap().mute = true;
    let acid = b.tracks.get_mut("acid").unwrap();
    let FxSpec::Distortion { mode, .. } = &mut acid.fx[0];
    *mode = DistortionMode::Hard;
    b.tracks.insert(
        "clap".into(),
        Track {
            instrument: InstrumentKind::Clap,
            ..b.tracks["kick"].clone()
        },
    );
    let b = compile_song(&b);
    let a = compiled(MINI);
    let mut two_bars = song(MINI);
    two_bars.sections.truncate(1);
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

    assert_eq!(
        ALLOCS.load(Ordering::Relaxed),
        0,
        "allocations on the audio thread"
    );
    assert_eq!(
        DEALLOCS.load(Ordering::Relaxed),
        0,
        "deallocations on the audio thread"
    );
}
