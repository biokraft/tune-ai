//! Session play: clips and scenes launched live, landing at bar lines.
//!
//! `mini` at 120 BPM and 48 kHz: a step is 6000 frames, a bar 96 000 and the
//! song 384 000. Tracks: kick 0, acid 1, hat 2; clips: k4 0, a1 1 (half a
//! bar), h 2. Every comparison of two mixes is bit for bit.

mod common;

use std::sync::Arc;

use common::{MINI, MIXED, SESSION, bits, compile_song, compiled, drop_lane, song, variant_of};
use tune_engine::{
    CellSource, CompiledScene, CompiledSong, Cued, Engine, RenderOptions, SessionCue, SongUpdate,
    TrackPlay,
};
use tune_song::{Clip, ClipKind, Note, NoteClip, Placement, Scene, Song};

const BAR: u64 = 96_000;
const SONG: u64 = 4 * BAR;

const KICK: usize = 0;
const ACID: usize = 1;
const HAT: usize = 2;

const K4: usize = 0;
const A1: usize = 1;

/// Plays `frames` frames of `engine` in `block`-frame calls and returns the
/// interleaved stereo mix.
fn play(engine: &mut Engine, frames: u64, block: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; 2 * frames as usize];
    for chunk in out.chunks_mut(2 * block) {
        engine.process(chunk);
    }
    out
}

/// [`play`] in 512-frame blocks.
fn render(engine: &mut Engine, frames: u64) -> Vec<f32> {
    play(engine, frames, 512)
}

/// The first `frames` frames of `song`, each `(frame, cue)` sent once that
/// many frames have played (in time order), in `block`-frame calls.
fn with_cues(
    song: &Arc<CompiledSong>,
    cues: &[(u64, SessionCue)],
    frames: u64,
    block: usize,
) -> Vec<f32> {
    let mut engine = Engine::new(song.clone());
    let mut out = Vec::with_capacity(2 * frames as usize);
    let mut done = 0;
    for &(at, cue) in cues {
        out.extend(play(&mut engine, at - done, block));
        done = at;
        engine.cue(cue);
    }
    out.extend(play(&mut engine, frames - done, block));
    out
}

/// The whole song, plain.
fn plain(song: &Song) -> Vec<f32> {
    render(&mut Engine::new(compile_song(song)), SONG)
}

fn placement(start_bar: u32, bars: u32, clip: &str, offset_bars: u32) -> Placement {
    Placement {
        start_bar,
        bars,
        clip: clip.into(),
        offset_bars,
    }
}

fn set_play(s: &mut Song, track: &str, play: Vec<Placement>) {
    s.tracks.get_mut(track).expect("a track").play = play;
}

fn scene(slots: &[(&str, &str)]) -> Scene {
    Scene {
        slots: slots
            .iter()
            .map(|&(t, c)| (t.to_owned(), c.to_owned()))
            .collect(),
    }
}

fn clip(clip: usize, start_step: u64, offset_steps: u64) -> TrackPlay {
    TrackPlay::Clip(CellSource {
        clip,
        start_step,
        offset_steps,
    })
}

#[test]
fn scenes_compile_to_clip_indices() {
    let s = compiled(SESSION);
    assert_eq!(
        s.scenes,
        vec![
            CompiledScene {
                id: "groove".into(),
                slots: vec![Some(0), Some(1), None],
            },
            CompiledScene {
                id: "break".into(),
                slots: vec![None, None, Some(2)],
            },
        ]
    );
    assert_eq!(s.clip_ids, ["k4", "a1", "h"]);
    assert_eq!(
        s.clip_kinds,
        [ClipKind::Drum, ClipKind::Acid, ClipKind::Drum]
    );
    assert_eq!(s.clip_index("h"), Some(2));
    assert_eq!(s.clip_index("nope"), None);
    assert_eq!(s.scene_index("break"), Some(1));
    assert_eq!(s.scene_index("nope"), None);
}

#[test]
fn scenes_do_not_change_a_render() {
    let with = song(SESSION);
    let mut without = with.clone();
    without.scenes.clear();
    let opts = RenderOptions {
        start_bar: 0,
        end_bar: 4,
        stems: true,
    };
    let a = tune_engine::render(compile_song(&with), &opts).unwrap();
    let b = tune_engine::render(compile_song(&without), &opts).unwrap();
    assert_eq!(bits(&a.mix), bits(&b.mix));
    for ((_, x), (_, y)) in a.stems.iter().zip(&b.stems) {
        assert_eq!(bits(x), bits(y));
    }
}

#[test]
fn cues_land_at_the_bar_line_they_reach() {
    let launch = SessionCue::Launch {
        track: ACID,
        clip: A1,
    };
    let mini = compiled(MINI);

    let mut e = Engine::new(mini.clone());
    render(&mut e, BAR - 1);
    e.cue(launch);
    render(&mut e, 1);
    assert_eq!(
        e.session(ACID),
        Some((TrackPlay::Arrangement, Some(Cued::Clip(A1))))
    );
    render(&mut e, 1);
    assert_eq!(e.session(ACID), Some((clip(A1, 16, 0), None)));

    // Sent with step 16 about to fire: it lands there.
    let mut e = Engine::new(mini.clone());
    render(&mut e, BAR);
    e.cue(launch);
    render(&mut e, 1);
    assert_eq!(e.session(ACID), Some((clip(A1, 16, 0), None)));

    // Sent just after: the next bar line.
    let mut e = Engine::new(mini);
    render(&mut e, BAR + 1);
    e.cue(launch);
    render(&mut e, BAR - 1);
    assert_eq!(
        e.session(ACID),
        Some((TrackPlay::Arrangement, Some(Cued::Clip(A1))))
    );
    render(&mut e, 1);
    assert_eq!(e.session(ACID), Some((clip(A1, 32, 0), None)));
}

#[test]
fn a_launch_plays_like_a_placement_and_ignores_the_tracks_lanes() {
    let s = compile_song(&variant_of(MINI, |s| common::unplay(s, "acid")));
    let launch = SessionCue::Launch {
        track: ACID,
        clip: A1,
    };
    let session = with_cues(&s, &[(100_000, launch)], SONG, 512);
    let arranged = plain(&variant_of(MINI, |s| {
        drop_lane(s, "acid", "acid303.cutoff");
    }));
    assert_eq!(bits(&session), bits(&arranged));
}

#[test]
fn a_launch_mid_lane_plays_the_tracks_static_params() {
    // Index 2 of the acid303 is the cutoff: 500 static, driven by the lane
    // over bars 2-3.
    const CUTOFF: usize = 2;
    let (mut e, log) = common::probe_engine(&compiled(MINI), "acid");
    render(&mut e, 200_000);
    e.cue(SessionCue::Launch {
        track: ACID,
        clip: A1,
    });
    render(&mut e, SONG - 200_000);
    let cutoff = log.lock().unwrap().trace(CUTOFF, SONG);
    let landing = 3 * BAR as usize;
    assert_ne!(cutoff[landing - 1], Some(500.0), "the lane drives it");
    assert!(cutoff[landing..].iter().all(|&v| v == Some(500.0)));
}

#[test]
fn back_to_arrangement_picks_the_lanes_up_again() {
    let cues = [
        (
            0,
            SessionCue::Launch {
                track: ACID,
                clip: A1,
            },
        ),
        (150_000, SessionCue::Back(Some(ACID))),
    ];
    let session = with_cues(&compiled(MINI), &cues, SONG, 512);
    let arranged = plain(&variant_of(MINI, |s| {
        set_play(
            s,
            "acid",
            vec![placement(0, 2, "a1", 0), placement(2, 2, "a1", 0)],
        );
    }));
    assert_eq!(bits(&session), bits(&arranged));
}

#[test]
fn stop_silences_until_back() {
    let no_lane = variant_of(MINI, |s| drop_lane(s, "acid", "acid303.cutoff"));
    let cues = [
        (
            0,
            SessionCue::Launch {
                track: ACID,
                clip: A1,
            },
        ),
        (100_000, SessionCue::Stop(ACID)),
        (200_000, SessionCue::Back(Some(ACID))),
    ];
    let session = with_cues(&compile_song(&no_lane), &cues, SONG, 512);
    let mut arranged = no_lane.clone();
    set_play(
        &mut arranged,
        "acid",
        vec![placement(0, 2, "a1", 0), placement(3, 1, "a1", 1)],
    );
    assert_eq!(bits(&session), bits(&plain(&arranged)));
}

#[test]
fn a_scene_switches_every_track() {
    let s = compile_song(&variant_of(MINI, |s| {
        s.scenes
            .insert("drop".into(), scene(&[("acid", "a1"), ("hat", "h")]));
    }));
    let session = with_cues(&s, &[(10_000, SessionCue::Scene(0))], SONG, 512);
    let arranged = plain(&variant_of(MINI, |s| {
        drop_lane(s, "acid", "acid303.cutoff");
        set_play(s, "kick", vec![placement(0, 1, "k4", 0)]);
        set_play(s, "acid", vec![placement(1, 3, "a1", 0)]);
        set_play(s, "hat", vec![placement(0, 4, "h", 0)]);
    }));
    assert_eq!(bits(&session), bits(&arranged));
}

#[test]
fn ducking_follows_the_duck_sources_session_clip() {
    let s = compile_song(&variant_of(MIXED, |s| common::unplay(s, "kick")));
    let launch = SessionCue::Launch {
        track: KICK,
        clip: K4,
    };
    let session = with_cues(&s, &[(10_000, launch)], SONG, 512);
    let arranged = plain(&variant_of(MIXED, |s| {
        set_play(s, "kick", vec![placement(1, 3, "k4", 0)]);
    }));
    assert_eq!(bits(&session), bits(&arranged));
}

#[test]
fn stopping_a_notes_clip_ends_its_notes() {
    let padded = variant_of(MINI, |s| {
        common::unplay(s, "acid");
        let c2 = Note {
            start: 0,
            len: 64 * 24,
            pitch: 36,
            velocity: 100,
        };
        s.clips.insert(
            "pad".into(),
            Clip::Notes(NoteClip {
                bars: 4,
                notes: vec![c2],
            }),
        );
    });
    let s = compile_song(&padded);
    let pad = s.clip_index("pad").expect("the pad");
    let cues = [
        (
            0,
            SessionCue::Launch {
                track: ACID,
                clip: pad,
            },
        ),
        (10_000, SessionCue::Stop(ACID)),
    ];
    let session = with_cues(&s, &cues, SONG, 512);
    let arranged = variant_of(&padded.to_toml(), |s| {
        drop_lane(s, "acid", "acid303.cutoff");
        set_play(s, "acid", vec![placement(0, 1, "pad", 0)]);
    });
    assert_eq!(bits(&session), bits(&plain(&arranged)));
}

#[test]
fn the_same_launches_give_the_same_samples() {
    let mini = compiled(MINI);
    let launch = SessionCue::Launch {
        track: ACID,
        clip: A1,
    };
    let early = with_cues(
        &mini,
        &[(10, launch), (20, SessionCue::Stop(HAT))],
        SONG,
        512,
    );
    let late = with_cues(
        &mini,
        &[(90_000, launch), (95_000, SessionCue::Stop(HAT))],
        SONG,
        333,
    );
    assert_eq!(bits(&early), bits(&late));
}

#[test]
fn session_clips_keep_their_phase_across_a_wrap_and_a_seek() {
    let s = compile_song(&variant_of(MINI, |s| {
        let c24 = tune_dsp::DrumPattern::parse("x..x..x..x..x..x..x..x..").unwrap();
        s.clips.insert("c24".into(), Clip::Drum(c24));
    }));
    let c24 = s.clip_index("c24").expect("c24");
    assert_eq!(c24, 3);
    let launch = SessionCue::Launch {
        track: HAT,
        clip: c24,
    };

    // Around the loop: step 32 wraps to 0, at clip step 32 % 24 = 8.
    let mut e = Engine::new(s.clone());
    e.set_loop(Some(0..2));
    e.cue(launch);
    render(&mut e, 2 * BAR + 1);
    assert_eq!(e.session(HAT).map(|s| s.0), Some(clip(c24, 0, 8)));
    render(&mut e, 2 * BAR);

    // A seek from step 20 to bar 3 plays on from clip step 20.
    let mut e = Engine::new(s);
    e.cue(launch);
    render(&mut e, 120_000);
    e.seek(3);
    assert_eq!(e.session(HAT).map(|s| s.0), Some(clip(c24, 48, 20)));
    render(&mut e, BAR + 1);
    assert!(e.finished());
}

#[test]
fn a_session_clip_slides_across_a_whole_song_loop() {
    // An acid clip that slides on its last step, looped over the whole
    // song: at step 63 it slides into the loop start's step 0, as the same
    // clip placed on the arrangement does.
    let jam = variant_of(MINI, |s| {
        drop_lane(s, "acid", "acid303.cutoff");
        common::unplay(s, "acid");
        let s16 = tune_dsp::acid303::AcidPattern::parse("C2 . . . . . . . . . . . . . . C3~")
            .expect("a pattern");
        s.clips.insert("s16".into(), Clip::Acid(s16));
    });
    let s = compile_song(&jam);
    let s16 = s.clip_index("s16").expect("s16");
    let mut session = Engine::new(s);
    session.set_loop(Some(0..4));
    session.cue(SessionCue::Launch {
        track: ACID,
        clip: s16,
    });
    let session = render(&mut session, SONG + BAR);
    let placed = variant_of(&jam.to_toml(), |s| {
        set_play(s, "acid", vec![placement(0, 4, "s16", 0)]);
    });
    let mut arranged = Engine::new(compile_song(&placed));
    arranged.set_loop(Some(0..4));
    let arranged = render(&mut arranged, SONG + BAR);
    let first_difference = session
        .iter()
        .zip(&arranged)
        .position(|(a, b)| a.to_bits() != b.to_bits())
        .map(|i| i / 2);
    assert_eq!(first_difference, None, "the first frame that differs");
    assert_eq!(session.len(), arranged.len());
}

#[test]
fn bad_cues_are_ignored() {
    let mut e = Engine::new(compiled(MINI));
    for cue in [
        // A drum clip on the acid303.
        SessionCue::Launch {
            track: ACID,
            clip: K4,
        },
        SessionCue::Launch { track: 9, clip: 0 },
        SessionCue::Launch {
            track: ACID,
            clip: 9,
        },
        SessionCue::Scene(5),
        SessionCue::Stop(9),
        SessionCue::Back(Some(9)),
    ] {
        e.cue(cue);
    }
    for t in 0..3 {
        assert_eq!(e.session(t), Some((TrackPlay::Arrangement, None)), "{t}");
    }
    assert_eq!(e.session(9), None);
    render(&mut e, BAR + 1);
    for t in 0..3 {
        assert_eq!(e.session(t), Some((TrackPlay::Arrangement, None)), "{t}");
    }
}

#[test]
fn a_finished_engine_lands_cues_at_the_seek_bar() {
    let mut e = Engine::new(compiled(MINI));
    render(&mut e, SONG + 1000);
    assert!(e.finished());
    e.cue(SessionCue::Launch {
        track: ACID,
        clip: A1,
    });
    e.seek(1);
    render(&mut e, 1);
    assert_eq!(e.session(ACID).map(|s| s.0), Some(clip(A1, 16, 0)));
}

// Session state across hot reloads.

const H: usize = 2;
const STEP: u64 = 6000;

/// Queues a swap to `to` on `e` and plays up to the next step boundary,
/// where it lands.
fn swap_at_next_step(e: &mut Engine, to: Arc<CompiledSong>) {
    let swaps = e.swaps_applied();
    assert!(e.queue_swap(SongUpdate::new(to)).is_none());
    let into_step = e.position() % STEP;
    render(
        e,
        if into_step == 0 {
            1
        } else {
            STEP - into_step + 1
        },
    );
    assert_eq!(e.swaps_applied(), swaps + 1, "the swap landed");
}

/// `mini` with one launch of `a1` on the acid playing from step 0 and
/// another waiting for bar 1.
fn a1_launched_and_cued(s: &Arc<CompiledSong>) -> Engine {
    let launch = SessionCue::Launch {
        track: ACID,
        clip: A1,
    };
    let mut e = Engine::new(s.clone());
    e.cue(launch);
    render(&mut e, 1);
    e.cue(launch);
    assert_eq!(
        e.session(ACID),
        Some((clip(A1, 0, 0), Some(Cued::Clip(A1))))
    );
    e
}

/// The first frame where two mixes differ, bit for bit.
fn first_difference(a: &[f32], b: &[f32]) -> Option<usize> {
    a.iter()
        .zip(b)
        .position(|(x, y)| x.to_bits() != y.to_bits())
        .map(|i| i / 2)
}

/// `src` plus the notes clip `pad`: one C2 over its 4 bars.
fn with_pad(src: &str, edit: impl FnOnce(&mut Song)) -> Song {
    variant_of(src, |s| {
        let c2 = Note {
            start: 0,
            len: 64 * 24,
            pitch: 36,
            velocity: 100,
        };
        s.clips.insert(
            "pad".into(),
            Clip::Notes(NoteClip {
                bars: 4,
                notes: vec![c2],
            }),
        );
        edit(s);
    })
}

#[test]
fn a_launched_clip_follows_its_id() {
    let mut e = a1_launched_and_cued(&compiled(MINI));
    let edited = compile_song(&variant_of(MINI, |s| {
        let z = tune_dsp::DrumPattern::parse("x...............").expect("a pattern");
        s.clips.shift_insert(0, "z".into(), Clip::Drum(z));
        let a1 = tune_dsp::acid303::AcidPattern::parse("C3 . . . . . . .").expect("a pattern");
        s.clips["a1"] = Clip::Acid(a1);
    }));
    assert_eq!(edited.clip_index("a1"), Some(2));
    swap_at_next_step(&mut e, edited);
    assert_eq!(e.session(ACID), Some((clip(2, 0, 0), Some(Cued::Clip(2)))));
}

#[test]
fn a_clip_that_goes_stops_the_track() {
    let mut e = a1_launched_and_cued(&compiled(MINI));
    let gone = compile_song(&variant_of(MINI, |s| {
        common::unplay(s, "acid");
        s.clips.shift_remove("a1");
    }));
    swap_at_next_step(&mut e, gone);
    assert_eq!(
        e.session(ACID),
        Some((TrackPlay::Stopped, Some(Cued::Stop)))
    );
}

#[test]
fn an_instrument_swap_stops_a_clip_it_cannot_play() {
    let padded = with_pad(MINI, |_| {});
    let kicked = compile_song(&variant_of(&padded.to_toml(), |s| {
        let edited = tune_song::set_instrument(s, tune_song::Registry::builtin(), "acid", "kick")
            .expect("a kick");
        *s = edited.arranged.song;
    }));
    let s = compile_song(&padded);
    let pad = s.clip_index("pad").expect("the pad");
    assert_eq!(kicked.clip_index("pad"), Some(pad));

    let mut e = Engine::new(s.clone());
    e.cue(SessionCue::Launch {
        track: ACID,
        clip: A1,
    });
    render(&mut e, 1);
    swap_at_next_step(&mut e, kicked.clone());
    assert_eq!(e.session(ACID), Some((TrackPlay::Stopped, None)));

    // Any instrument plays a notes clip.
    let mut e = Engine::new(s);
    e.cue(SessionCue::Launch {
        track: ACID,
        clip: pad,
    });
    render(&mut e, 1);
    swap_at_next_step(&mut e, kicked);
    assert_eq!(e.session(ACID), Some((clip(pad, 0, 0), None)));
}

#[test]
fn a_renamed_track_follows_the_arrangement() {
    let mut e = a1_launched_and_cued(&compiled(MINI));
    let renamed = compile_song(&variant_of(MINI, |s| {
        *s = tune_song::rename_track(s, "acid", "bass")
            .expect("a rename")
            .arranged
            .song;
    }));
    swap_at_next_step(&mut e, renamed);
    assert_eq!(e.song().tracks[ACID].id, "bass");
    assert_eq!(e.session(ACID), Some((TrackPlay::Arrangement, None)));
}

#[test]
fn a_value_swap_keeps_a_session_notes_clip_sounding() {
    // `process_stems` refuses a swap, so the muting stands in for an acid
    // stem.
    let padded = with_pad(MINI, |s| {
        common::unplay(s, "acid");
        s.tracks.get_mut("kick").expect("a kick").mute = true;
        s.tracks.get_mut("hat").expect("a hat").mute = true;
    });
    let s = compile_song(&padded);
    let pad = s.clip_index("pad").expect("the pad");
    let louder_hat = compile_song(&variant_of(&padded.to_toml(), |s| {
        s.tracks.get_mut("hat").expect("a hat").gain_db = -4.0;
    }));
    let run = |swap: bool| {
        let mut e = Engine::new(s.clone());
        e.cue(SessionCue::Launch {
            track: ACID,
            clip: pad,
        });
        let mut out = render(&mut e, 3 * BAR / 2);
        if swap {
            assert!(e.queue_swap(SongUpdate::new(louder_hat.clone())).is_none());
        }
        out.extend(render(&mut e, SONG - 3 * BAR / 2));
        if swap {
            assert_eq!(e.swaps_applied(), 1);
            assert!(!e.swap_fading());
        }
        out
    };
    let (swapped, plain) = (run(true), run(false));
    assert_eq!(swapped.len(), plain.len());
    assert_eq!(
        first_difference(&swapped, &plain),
        None,
        "the first frame that differs"
    );
}

#[test]
fn a_swap_that_moves_the_playhead_keeps_a_session_clips_phase() {
    let launch = SessionCue::Launch {
        track: HAT,
        clip: H,
    };
    let two_bars = compile_song(&variant_of(MINI, |s| common::keep_sections(s, 1)));
    assert_eq!(two_bars.clip_index("h"), Some(H));
    // `h` launched at bar 3 (step 48); the swap lands at step 50, past the
    // new end.
    let launched = |looping: bool| {
        let mut e = Engine::new(compiled(MINI));
        if looping {
            e.set_loop(Some(0..4));
        }
        render(&mut e, 3 * BAR - 1000);
        e.cue(launch);
        render(&mut e, 1000 + 2 * STEP - 1);
        assert_eq!(e.session(HAT).map(|s| s.0), Some(clip(H, 48, 0)));
        e
    };

    // To the loop start: step 0 plays clip step 2, as step 50 would have.
    let mut e = launched(true);
    swap_at_next_step(&mut e, two_bars.clone());
    assert_eq!(e.session(HAT).map(|s| s.0), Some(clip(H, 0, 2)));
    render(&mut e, 2 * BAR);
    assert_eq!(e.session(HAT).map(|s| s.0), Some(clip(H, 0, 2)));

    // To the end: finished, and a seek plays on from the same clip step.
    let mut e = launched(false);
    swap_at_next_step(&mut e, two_bars);
    assert!(e.finished());
    assert_eq!(e.session(HAT).map(|s| s.0), Some(clip(H, 32, 2)));
    e.seek(1);
    assert_eq!(e.session(HAT).map(|s| s.0), Some(clip(H, 16, 2)));
    render(&mut e, BAR);
}

#[test]
fn a_swap_to_fewer_clips_remaps_before_a_wrap_and_a_seek() {
    let fewer = compile_song(&variant_of(MINI, |s| {
        common::unplay(s, "acid");
        s.clips.shift_remove("a1");
    }));
    assert_eq!(fewer.clip_index("h"), Some(1));
    let mut e = Engine::new(compiled(MINI));
    e.set_loop(Some(0..1));
    e.cue(SessionCue::Launch {
        track: HAT,
        clip: H,
    });
    render(&mut e, 1);
    swap_at_next_step(&mut e, fewer);
    // Through the wrap at step 16.
    render(&mut e, BAR - STEP);
    assert_eq!(e.position(), 1, "the loop wrapped");
    assert_eq!(e.session(HAT).map(|s| s.0), Some(clip(1, 0, 0)));
    // A seek from step 4 to bar 3.
    render(&mut e, 4 * STEP - 1);
    e.seek(3);
    assert_eq!(e.session(HAT).map(|s| s.0), Some(clip(1, 48, 4)));
    e.set_loop(None);
    render(&mut e, BAR + 1);
    assert!(e.finished());
}

// Session cues from the live controller.

/// Pulls `frames` frames from `p` in 512-frame blocks.
fn pull(p: &mut tune_engine::LiveProcessor, frames: u64) -> Vec<f32> {
    let mut out = vec![0.0f32; 2 * frames as usize];
    for chunk in out.chunks_mut(2 * 512) {
        p.process(chunk);
    }
    out
}

#[test]
fn live_cues_reach_the_engine() {
    let s = compile_song(&variant_of(MINI, |s| common::unplay(s, "acid")));
    let launch = SessionCue::Launch {
        track: ACID,
        clip: A1,
    };
    let (mut ctl, mut p) = tune_engine::live(s.clone(), 0, None);
    let mut session = pull(&mut p, 100_000);
    ctl.cue(launch).unwrap();
    session.extend(pull(&mut p, SONG - 100_000));
    let arranged = plain(&variant_of(MINI, |s| {
        drop_lane(s, "acid", "acid303.cutoff");
    }));
    assert_eq!(session.len(), arranged.len());
    assert_eq!(
        first_difference(&session, &arranged),
        None,
        "the first frame that differs"
    );

    // Sent after a swap and before it lands: meant for the swapped-in song,
    // so dropped.
    let (mut ctl, mut p) = tune_engine::live(s.clone(), 0, None);
    pull(&mut p, 1000);
    ctl.swap(s).unwrap();
    assert_eq!(ctl.cue(launch), Ok(()));
    for _ in 0..3 {
        pull(&mut p, BAR / 2);
        assert_eq!(p.session(ACID), Some((TrackPlay::Arrangement, None)));
    }
    assert_eq!(ctl.swaps_applied(), 1);
    // Once the swap plays, the next one lands.
    ctl.cue(launch).unwrap();
    pull(&mut p, BAR);
    assert_eq!(p.session(ACID), Some((clip(A1, 32, 0), None)));
}
