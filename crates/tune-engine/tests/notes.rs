//! Note clips as the engine plays them: every event at its own sample, the
//! clip looping inside its placement, `AllOff` wherever a note is cut, and
//! the same sound as a step clip when the notes sit on the grid.

mod common;

use std::sync::Arc;

use common::{SharedLog, bits, compiled, probe_engine, rms, run, run_stems};
use tune_engine::{
    Clock, CompiledSong, Engine, EventCtx, NOTE_CLIP_IDS, NoteEvent, NoteId, SongUpdate, hit_seed,
    track_seed,
};

/// The song seed of every probe song.
const SEED: u64 = 7;
/// One step at 120 BPM and 48 kHz.
const STEP: u64 = 6000;
/// One bar at 120 BPM and 48 kHz.
const BAR: u64 = 16 * STEP;

/// A v2 song at `bpm` with one section of `bars` bars around `body` (its
/// tracks and clips).
fn song_src(bpm: u32, bars: u32, body: &str) -> String {
    format!(
        "[song]\nversion = 2\nname = \"notes\"\nbpm = {bpm}\nseed = {SEED}\n\n{body}\n\n\
         [[sections]]\nname = \"a\"\nbars = {bars}\n"
    )
}

/// One track `t` of `instrument` playing `play`, with one notes clip `c` of
/// `clip_bars` bars holding `notes` (TOML entries).
fn one_track(instrument: &str, play: &str, clip_bars: u32, notes: &[&str]) -> String {
    format!(
        "[tracks.t]\ninstrument = \"{instrument}\"\nplay = [{play}]\n\n{}",
        notes_clip("c", clip_bars, notes)
    )
}

/// A notes clip `id` of `bars` bars holding `notes` (TOML entries).
fn notes_clip(id: &str, bars: u32, notes: &[&str]) -> String {
    let mut clip = format!("[clips.{id}]\nkind = \"notes\"\nbars = {bars}\n");
    if !notes.is_empty() {
        clip.push_str("notes = [\n");
        for n in notes {
            clip.push_str(&format!("  {n},\n"));
        }
        clip.push_str("]\n");
    }
    clip
}

fn id(index: u32) -> NoteId {
    NoteId(NOTE_CLIP_IDS | index)
}

/// Every note event the probe saw, as `(sample, event)`.
fn events(log: &SharedLog) -> Vec<(u64, NoteEvent)> {
    log.lock()
        .unwrap()
        .notes()
        .into_iter()
        .map(|(s, e, _)| (s, e))
        .collect()
}

fn on_samples(log: &SharedLog) -> Vec<u64> {
    log.lock()
        .unwrap()
        .ons()
        .into_iter()
        .map(|(s, _, _)| s)
        .collect()
}

fn off_samples(log: &SharedLog) -> Vec<u64> {
    log.lock()
        .unwrap()
        .offs()
        .into_iter()
        .map(|(s, _, _)| s)
        .collect()
}

/// The left channel of an interleaved stereo stem.
fn left(stem: &[f32]) -> Vec<f32> {
    stem.iter().step_by(2).copied().collect()
}

/// The stem of track `t` over `frames` frames, from a fresh engine.
fn stem_of(song: &Arc<CompiledSong>, t: usize, frames: u64) -> Vec<f32> {
    let mut engine = Engine::new(song.clone());
    let (_, stems) = run_stems(&mut engine, frames);
    stems[t].clone()
}

#[test]
fn note_events_land_on_their_samples() {
    let song = compiled(&song_src(
        145,
        2,
        &one_track(
            "acid303",
            r#"{ at = 1, clip = "c", bars = 2 }"#,
            1,
            &[r#"{ start = 0.5, pitch = "C3", len = 1 }"#],
        ),
    ));
    let clock = Clock::new(145.0, 48_000);
    let (mut engine, log) = probe_engine(&song, "t");
    run(&mut engine, clock.bar_start(2));
    assert_eq!(on_samples(&log), [2483, 81_931]);
    // The note ends at step 1.5 of each bar: ticks 36 and 16 * 24 + 36.
    assert_eq!(off_samples(&log), [7448, clock.tick_start(16 * 24 + 36)]);
    assert_eq!(clock.tick_start(16 * 24 + 36), 86_897);
    let (sample, event, ctx) = log.lock().unwrap().ons()[0];
    assert_eq!(sample, 2483);
    assert_eq!(
        event,
        NoteEvent::On {
            id: id(0),
            pitch: 48,
            velocity: 100,
            accent: false,
            slide: false,
        }
    );
    assert_eq!(
        ctx,
        EventCtx {
            pos: 2483,
            step: 0,
            seed: hit_seed(track_seed(SEED, "t"), 0) ^ 12,
        }
    );
    // The second bar's On falls in step 16, at the same tick of its step.
    let (_, _, ctx) = log.lock().unwrap().ons()[1];
    assert_eq!(
        ctx,
        EventCtx {
            pos: 81_931,
            step: 16,
            seed: hit_seed(track_seed(SEED, "t"), 16) ^ 12,
        }
    );
    assert!(
        !events(&log)
            .iter()
            .any(|(_, e)| matches!(e, NoteEvent::AllOff)),
        "nothing is cut"
    );
}

#[test]
fn notes_loop_with_their_clip() {
    let song = compiled(&song_src(
        120,
        3,
        &one_track(
            "acid303",
            r#"{ at = 1, clip = "c", bars = 3 }"#,
            1,
            &[
                r#"{ start = 0, pitch = "C2", len = 1 }"#,
                r#"{ start = 3.5, pitch = "D#2", len = 2 }"#,
                r#"{ start = 10.25, pitch = "G2", len = 5.75 }"#,
            ],
        ),
    ));
    let (mut engine, log) = probe_engine(&song, "t");
    run(&mut engine, 3 * BAR);
    let ons: Vec<(u64, NoteEvent)> = log
        .lock()
        .unwrap()
        .ons()
        .into_iter()
        .map(|(s, e, _)| (s, e))
        .collect();
    let first = [0, 3 * STEP + STEP / 2, 10 * STEP + STEP / 4];
    assert_eq!(ons.len(), 9);
    for bar in 0..3 {
        for (i, &at) in first.iter().enumerate() {
            let (sample, event) = ons[3 * bar + i];
            assert_eq!(sample, bar as u64 * BAR + at, "bar {bar}, note {i}");
            assert!(matches!(event, NoteEvent::On { id: n, .. } if n == id(i as u32)));
        }
    }
}

#[test]
fn an_offset_skips_notes_that_started_before() {
    let song = compiled(&song_src(
        120,
        1,
        &one_track(
            "acid303",
            r#"{ at = 1, clip = "c", bars = 1, offset = 1 }"#,
            2,
            &[
                r#"{ start = 8, pitch = "C2", len = 16 }"#,
                r#"{ start = 20, pitch = "G2", len = 2 }"#,
            ],
        ),
    ));
    let (mut engine, log) = probe_engine(&song, "t");
    run(&mut engine, BAR);
    let ons = log.lock().unwrap().ons();
    assert!(
        !ons.iter()
            .any(|(_, e, _)| matches!(e, NoteEvent::On { id: n, .. } if *n == id(0))),
        "the note that started before the placement does not sound"
    );
    // The note starting inside the placement does: clip step 20 is the
    // placement's step 4.
    assert_eq!(ons.len(), 1);
    assert_eq!(ons[0].0, 4 * STEP);
    assert!(matches!(ons[0].1, NoteEvent::On { id: n, .. } if n == id(1)));
}

/// A 2-bar clip with one note of 24 steps, played for one bar of a 2-bar
/// song.
fn cut_note_song() -> Arc<CompiledSong> {
    compiled(&song_src(
        120,
        2,
        &one_track(
            "acid303",
            r#"{ at = 1, clip = "c", bars = 1 }"#,
            2,
            &[r#"{ start = 0, pitch = "C2", len = 24 }"#],
        ),
    ))
}

#[test]
fn a_placement_ending_mid_note_sends_all_off() {
    let song = cut_note_song();
    let (mut engine, log) = probe_engine(&song, "t");
    run(&mut engine, 2 * BAR);
    let on = NoteEvent::On {
        id: id(0),
        pitch: 36,
        velocity: 100,
        accent: false,
        slide: false,
    };
    assert_eq!(
        events(&log),
        [(0, on), (song.clock.step_start(16), NoteEvent::AllOff)]
    );
}

#[test]
fn loop_wrap_sends_all_off() {
    let song = cut_note_song();
    let (mut engine, log) = probe_engine(&song, "t");
    engine.set_loop(Some(0..1));
    run(&mut engine, 3 * BAR);
    let kinds: Vec<(u64, &str)> = events(&log)
        .into_iter()
        .map(|(s, e)| {
            let kind = match e {
                NoteEvent::On { id: n, .. } => {
                    assert_eq!(n, id(0));
                    "on"
                }
                NoteEvent::Off { .. } => "off",
                NoteEvent::AllOff => "all off",
            };
            (s, kind)
        })
        .collect();
    assert_eq!(
        kinds,
        [
            (0, "on"),
            (BAR, "all off"),
            (BAR, "on"),
            (2 * BAR, "all off"),
            (2 * BAR, "on"),
        ]
    );
}

#[test]
fn seek_mid_note_is_silent_after() {
    let song = compiled(&song_src(
        120,
        2,
        &one_track(
            "acid303",
            r#"{ at = 1, clip = "c", bars = 2 }"#,
            2,
            &[r#"{ start = 8, pitch = "C2", len = 16 }"#],
        ),
    ));
    let mut engine = Engine::new(song);
    // Into the long note (steps 8 to 24), then to bar 1, where no note starts.
    let before = run(&mut engine, 12 * STEP);
    assert!(
        rms(&before[2 * 10 * STEP as usize..]) > 1e-3,
        "the note sounds before the seek"
    );
    engine.seek(1);
    let after = run(&mut engine, 10_000);
    assert!(rms(&after[2 * 2000..2 * 10_000]) < 1e-4);
}

#[test]
fn drum_note_clip_on_the_grid_renders_like_its_step_clip() {
    let play = r#"{ at = 1, clip = "c", bars = 2 }"#;
    let steps = compiled(&song_src(
        120,
        2,
        &format!(
            "[tracks.t]\ninstrument = \"kick\"\nplay = [{play}]\n\n\
             [clips.c]\nkind = \"drum\"\nsteps = \"x...x...x...X...\"\n"
        ),
    ));
    let notes = compiled(&song_src(
        120,
        2,
        &one_track(
            "kick",
            play,
            1,
            &[
                r#"{ start = 0, pitch = "C4", len = 1 }"#,
                r#"{ start = 4, pitch = "C4", len = 1 }"#,
                r#"{ start = 8, pitch = "C4", len = 1 }"#,
                r#"{ start = 12, pitch = "C4", len = 1, vel = 127 }"#,
            ],
        ),
    ));
    let a = stem_of(&steps, 0, 2 * BAR);
    let b = stem_of(&notes, 0, 2 * BAR);
    assert!(rms(&a) > 1e-3, "the kick sounds");
    assert_eq!(bits(&a), bits(&b));
}

#[test]
fn acid_note_clip_renders_like_its_step_clip() {
    let play = r#"{ at = 1, clip = "c", bars = 2 }"#;
    let steps = compiled(&song_src(
        120,
        2,
        &format!(
            "[tracks.t]\ninstrument = \"acid303\"\nplay = [{play}]\n\n\
             [clips.c]\nkind = \"acid\"\n\
             steps = \"C2 . D#2! . C3~ C2 . G2~ G2 . . . . . . .\"\n"
        ),
    ));
    let notes = compiled(&song_src(
        120,
        2,
        &one_track(
            "acid303",
            play,
            1,
            &[
                r#"{ start = 0, pitch = "C2", len = 0.5 }"#,
                r#"{ start = 2, pitch = "D#2", len = 0.5, vel = 127 }"#,
                r#"{ start = 4, pitch = "C3", len = 1.25 }"#,
                r#"{ start = 5, pitch = "C2", len = 0.5 }"#,
                r#"{ start = 7, pitch = "G2", len = 1.5 }"#,
                r#"{ start = 8, pitch = "G2", len = 0.5 }"#,
            ],
        ),
    ));
    let a = stem_of(&steps, 0, 2 * BAR);
    let b = stem_of(&notes, 0, 2 * BAR);
    assert!(rms(&a) > 1e-3, "the acid sounds");
    assert_eq!(bits(&a), bits(&b));
}

#[test]
fn velocity_110_accents() {
    let song = compiled(&song_src(
        120,
        1,
        &one_track(
            "acid303",
            r#"{ at = 1, clip = "c", bars = 1 }"#,
            1,
            &[
                r#"{ start = 0, pitch = "C3", len = 1, vel = 110 }"#,
                r#"{ start = 4, pitch = "C3", len = 1, vel = 109 }"#,
            ],
        ),
    ));
    let (mut engine, log) = probe_engine(&song, "t");
    run(&mut engine, BAR);
    let accents: Vec<(u8, bool)> = log
        .lock()
        .unwrap()
        .ons()
        .into_iter()
        .map(|(_, e, _)| match e {
            NoteEvent::On {
                velocity, accent, ..
            } => (velocity, accent),
            other => panic!("not an On: {other:?}"),
        })
        .collect();
    assert_eq!(accents, [(110, true), (109, false)]);
}

#[test]
fn chords_hit_once_per_note() {
    let chord = |instrument: &str| {
        compiled(&song_src(
            120,
            1,
            &one_track(
                instrument,
                r#"{ at = 1, clip = "c", bars = 1 }"#,
                1,
                &[
                    r#"{ start = 0, pitch = "C3", len = 2 }"#,
                    r#"{ start = 0, pitch = "G3", len = 2 }"#,
                ],
            ),
        ))
    };
    for instrument in ["kick", "acid303"] {
        let song = chord(instrument);
        let (mut engine, log) = probe_engine(&song, "t");
        run(&mut engine, BAR);
        let ons: Vec<(u64, NoteId)> = log
            .lock()
            .unwrap()
            .ons()
            .into_iter()
            .map(|(s, e, _)| match e {
                NoteEvent::On { id, .. } => (s, id),
                other => panic!("not an On: {other:?}"),
            })
            .collect();
        assert_eq!(ons, [(0, id(0)), (0, id(1))], "{instrument}");
    }
    let song = chord("acid303");
    let a = stem_of(&song, 0, BAR);
    let b = stem_of(&song, 0, BAR);
    assert!(rms(&a) > 1e-3, "the chord sounds");
    assert_eq!(bits(&a), bits(&b));
}

#[test]
fn note_clip_hits_drive_the_ducker() {
    let duck_song = |kick_notes: &[&str]| {
        compiled(&song_src(
            120,
            1,
            &format!(
                "[mix]\nduck_source = \"kick\"\n\n\
                 [tracks.kick]\ninstrument = \"kick\"\n\
                 play = [{{ at = 1, clip = \"hits\", bars = 1 }}]\n\n\
                 [tracks.acid]\ninstrument = \"acid303\"\n\
                 params = {{ \"duck.depth\" = 12 }}\n\
                 play = [{{ at = 1, clip = \"held\", bars = 1 }}]\n\n{}\n{}",
                notes_clip("hits", 1, kick_notes),
                notes_clip("held", 1, &[r#"{ start = 0, pitch = "C2", len = 16 }"#]),
            ),
        ))
    };
    let with = duck_song(&[r#"{ start = 0.5, pitch = "C4", len = 1 }"#]);
    let without = duck_song(&[]);
    let a = left(&stem_of(&with, 1, 3010));
    let b = left(&stem_of(&without, 1, 3010));
    assert!(rms(&b) > 1e-3, "the acid sounds");
    assert_eq!(bits(&a[..3000]), bits(&b[..3000]));
    assert_ne!(bits(&a[3000..3010]), bits(&b[3000..3010]));
}

/// One track `t` (acid303, `gain_db`) playing `play`, with the clips
/// `clips` (TOML), in a v2 song of `bars` bars at 120 BPM.
fn swap_song(bars: u32, gain_db: i32, play: &str, clips: &str) -> Arc<CompiledSong> {
    compiled(&song_src(
        120,
        bars,
        &format!(
            "[tracks.t]\ninstrument = \"acid303\"\ngain_db = {gain_db}\n\
             play = [{play}]\n\n{clips}"
        ),
    ))
}

/// The 16-step note of the swap tests, note 0 of a 2-bar clip.
const LONG: &str = r#"{ start = 0, pitch = "C2", len = 16 }"#;

/// Note 0 of the swap tests' clip as it starts.
fn long_on() -> NoteEvent {
    NoteEvent::On {
        id: id(0),
        pitch: 36,
        velocity: 100,
        accent: false,
        slide: false,
    }
}

/// Runs `from` on a probe engine (track `t` probed) up to `queue_at`, queues
/// a swap to `to` there (it lands at the next step boundary), runs on to
/// `until` and returns every note event the probe saw.
fn swap_events(
    from: &Arc<CompiledSong>,
    to: &Arc<CompiledSong>,
    queue_at: u64,
    until: u64,
) -> Vec<(u64, NoteEvent)> {
    let (mut engine, log) = probe_engine(from, "t");
    run(&mut engine, queue_at);
    assert!(engine.queue_swap(SongUpdate::new(to.clone())).is_none());
    run(&mut engine, until - queue_at);
    assert_eq!(engine.swaps_applied(), 1);
    events(&log)
}

#[test]
fn a_value_swap_keeps_a_held_note() {
    let play = r#"{ at = 1, clip = "c", bars = 2 }"#;
    let clip = notes_clip("c", 2, &[LONG]);
    let from = swap_song(2, 0, play, &clip);
    let louder = swap_song(2, -3, play, &clip);
    // The same clip behind another one in the file: its index moves, its
    // content does not.
    let moved = swap_song(2, 0, play, &format!("{}\n{clip}", notes_clip("a", 1, &[])));
    for to in [&louder, &moved] {
        assert_eq!(
            swap_events(&from, to, 4 * STEP + 100, 2 * BAR),
            [
                (0, long_on()),
                (
                    16 * STEP,
                    NoteEvent::Off {
                        id: id(0),
                        pitch: 36
                    }
                ),
            ],
            "the note runs to its own end"
        );
    }
}

#[test]
fn a_clip_edit_ends_held_notes() {
    let play = r#"{ at = 1, clip = "c", bars = 2 }"#;
    let other = |start: u32| format!(r#"{{ start = {start}, pitch = "G2", len = 2 }}"#);
    let from = swap_song(2, 0, play, &notes_clip("c", 2, &[LONG, &other(20)]));
    let to = swap_song(2, 0, play, &notes_clip("c", 2, &[LONG, &other(22)]));
    let g2 = NoteEvent::On {
        id: id(1),
        pitch: 43,
        velocity: 100,
        accent: false,
        slide: false,
    };
    assert_eq!(
        swap_events(&from, &to, 4 * STEP + 100, 2 * BAR),
        [
            (0, long_on()),
            (to.clock.step_start(5), NoteEvent::AllOff),
            // The new clip's events from step 5 on.
            (
                16 * STEP,
                NoteEvent::Off {
                    id: id(0),
                    pitch: 36
                }
            ),
            (22 * STEP, g2),
            (
                24 * STEP,
                NoteEvent::Off {
                    id: id(1),
                    pitch: 43
                }
            ),
        ]
    );
}

#[test]
fn moving_the_placement_ends_held_notes() {
    let clip = notes_clip("c", 2, &[LONG]);
    // One bar later: no placement plays at the swap step.
    let from = swap_song(3, 0, r#"{ at = 1, clip = "c", bars = 2 }"#, &clip);
    let to = swap_song(3, 0, r#"{ at = 2, clip = "c", bars = 2 }"#, &clip);
    assert_eq!(
        swap_events(&from, &to, 4 * STEP + 100, 3 * BAR),
        [
            (0, long_on()),
            (5 * STEP, NoteEvent::AllOff),
            (BAR, long_on()),
            (
                BAR + 16 * STEP,
                NoteEvent::Off {
                    id: id(0),
                    pitch: 36
                }
            ),
        ]
    );
    // One bar earlier: a placement plays at the swap step, from another
    // start.
    let from = swap_song(4, 0, r#"{ at = 2, clip = "c", bars = 2 }"#, &clip);
    let to = swap_song(4, 0, r#"{ at = 1, clip = "c", bars = 3 }"#, &clip);
    assert_eq!(
        swap_events(&from, &to, BAR + 4 * STEP + 100, 4 * BAR),
        [
            (BAR, long_on()),
            (BAR + 5 * STEP, NoteEvent::AllOff),
            (2 * BAR, long_on()),
            (3 * BAR, NoteEvent::AllOff),
        ]
    );
}
