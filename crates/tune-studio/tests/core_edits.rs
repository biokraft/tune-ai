//! The studio core's edits: commands onto the document, debounced
//! write-back, snapshot undo/redo and `human` log entries.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tune_studio::{
    Command, DrumHitDto, EditEntry, EditError, EditLog, Source, StudioCore, StudioError, text_hash,
};

const FIXTURE: &str = include_str!("fixtures/commented.toml");

/// A temp dir holding a copy of the commented fixture as `song.toml`.
struct Song {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn song() -> Song {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("song.toml");
    fs::write(&path, FIXTURE).expect("write fixture");
    Song { _dir: dir, path }
}

fn open(s: &Song) -> StudioCore {
    StudioCore::open(&s.path).expect("opens")
}

fn disk(s: &Song) -> String {
    fs::read_to_string(&s.path).expect("read song")
}

fn log(s: &Song) -> Vec<EditEntry> {
    EditLog::for_song(&s.path)
        .read_since(None)
        .expect("read log")
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn cutoff(value: f64) -> Command {
    Command::SetParam {
        track: "acid".into(),
        param: "acid303.cutoff".into(),
        value,
        commit: true,
    }
}

fn drum_step(index: usize, hit: DrumHitDto) -> Command {
    Command::SetDrumStep {
        pattern: "h".into(),
        index,
        hit,
    }
}

fn entry_summary(e: &EditEntry) -> (Source, String, Value, Value) {
    (e.source, e.path.clone(), e.old.clone(), e.new.clone())
}

#[test]
fn edit_pending_until_debounce() {
    let s = song();
    let mut core = open(&s);
    let t0 = Instant::now();
    let changes = core.apply_edit(&cutoff(820.0), t0).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path.to_string(), "tracks.acid.acid303.cutoff");
    assert_eq!(core.pending().len(), 1);
    assert_eq!(core.pending()[0].command, cutoff(820.0));
    assert_eq!(core.pending()[0].changes, changes);

    assert_eq!(disk(&s), FIXTURE, "nothing is written before the flush");
    assert!(!core.flush_due(t0 + ms(100)));
    assert!(core.flush_due(t0 + ms(150)));

    let flushed = core.flush().unwrap();
    assert_eq!(flushed, changes);
    let text = disk(&s);
    assert!(text.contains("\"acid303.cutoff\" = 820"), "{text}");
    assert!(text.contains("# the squelch"));
    assert!(text.contains("gain_db = -3 # headroom"));
    assert!(core.pending().is_empty());
    assert!(!core.flush_due(t0 + ms(1000)));

    let entries = log(&s);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entry_summary(&entries[0]),
        (
            Source::Human,
            "tracks.acid.acid303.cutoff".into(),
            json!(500),
            json!(820)
        )
    );
}

#[test]
fn burst_is_one_undo_step() {
    let s = song();
    let mut core = open(&s);
    let t0 = Instant::now();
    core.apply_edit(&drum_step(0, DrumHitDto::Hit), t0).unwrap();
    core.apply_edit(&drum_step(1, DrumHitDto::Accent), t0 + ms(50))
        .unwrap();
    core.apply_edit(&drum_step(3, DrumHitDto::Hit), t0 + ms(100))
        .unwrap();
    assert!(!core.flush_due(t0 + ms(200)), "150 ms after the last edit");
    assert!(core.flush_due(t0 + ms(250)));
    core.flush().unwrap();
    let flushed = disk(&s);
    assert_ne!(flushed, FIXTURE);
    assert!(
        flushed.contains("steps = \"xXxx ..x. ..x. ..x.\""),
        "{flushed}"
    );

    let undone = core.undo().unwrap();
    assert_eq!(undone.len(), 1);
    assert_eq!(disk(&s), FIXTURE, "undo restores the bytes");
    assert!(core.can_redo());
    assert!(!core.can_undo(), "the burst was one step");

    core.redo().unwrap();
    assert_eq!(disk(&s), flushed);
    assert!(core.can_undo());
    assert!(!core.can_redo());
}

/// Review Focus 5: a held arrow key commits about every 10 ms.
#[test]
fn held_arrow_coalesces() {
    let s = song();
    let mut core = open(&s);
    let t0 = Instant::now();
    let mut flushes = 0;
    let mut now = t0;
    for i in 0..30u32 {
        now = t0 + ms(10 * u64::from(i));
        core.apply_edit(&cutoff(500.0 + f64::from(i)), now).unwrap();
        if core.flush_due(now) {
            core.flush().unwrap();
            flushes += 1;
        }
    }
    let end = now + ms(160);
    if core.flush_due(end) {
        core.flush().unwrap();
        flushes += 1;
    }
    assert_eq!(flushes, 1);
    let entries = log(&s);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entry_summary(&entries[0]),
        (
            Source::Human,
            "tracks.acid.acid303.cutoff".into(),
            json!(500),
            json!(529)
        )
    );
    assert!(core.can_undo());
    core.undo().unwrap();
    assert!(!core.can_undo(), "exactly one undo entry");
    assert_eq!(disk(&s), FIXTURE);
}

#[test]
fn undo_flushes_pending_first() {
    let s = song();
    let mut core = open(&s);
    core.apply_edit(&cutoff(820.0), Instant::now()).unwrap();
    assert!(core.can_undo(), "pending edits are undoable");
    let undone = core.undo().unwrap();
    assert_eq!(undone.len(), 1);
    assert_eq!(undone[0].new, json!(500));
    assert_eq!(disk(&s), FIXTURE);
    assert!(core.pending().is_empty());
    assert_eq!(log(&s).len(), 2, "the flush and the undo");
    assert!(core.can_redo());
    core.redo().unwrap();
    assert!(disk(&s).contains("\"acid303.cutoff\" = 820"));
}

#[test]
fn new_edit_clears_redo() {
    let s = song();
    let mut core = open(&s);
    let t0 = Instant::now();
    core.apply_edit(&cutoff(820.0), t0).unwrap();
    core.flush().unwrap();
    core.undo().unwrap();
    assert!(core.can_redo());
    core.apply_edit(&cutoff(700.0), t0 + ms(500)).unwrap();
    assert!(!core.can_redo());
    core.flush().unwrap();
    assert!(!core.can_redo());
    assert_eq!(core.redo().unwrap(), vec![]);
    assert!(disk(&s).contains("\"acid303.cutoff\" = 700"));
}

#[test]
fn undo_logged_as_human_reversed() {
    let s = song();
    let mut core = open(&s);
    core.apply_edit(&cutoff(820.0), Instant::now()).unwrap();
    core.flush().unwrap();
    core.undo().unwrap();
    core.redo().unwrap();
    let entries: Vec<_> = log(&s).iter().map(entry_summary).collect();
    let p = || "tracks.acid.acid303.cutoff".to_owned();
    assert_eq!(
        entries,
        vec![
            (Source::Human, p(), json!(500), json!(820)),
            (Source::Human, p(), json!(820), json!(500)),
            (Source::Human, p(), json!(500), json!(820)),
        ]
    );
}

#[test]
fn knob_value_rounded() {
    let s = song();
    let mut core = open(&s);
    let changes = core
        .apply_edit(&cutoff(823.456789), Instant::now())
        .unwrap();
    assert_eq!(changes[0].new, json!(823.5));
    core.flush().unwrap();
    assert!(
        disk(&s).contains("\"acid303.cutoff\" = 823.5,"),
        "{}",
        disk(&s)
    );
}

#[test]
fn mix_numbers_rounded() {
    let s = song();
    let mut core = open(&s);
    let cmd = Command::SetTrackMix {
        track: "hat".into(),
        gain_db: Some(-6.123456),
        pan: Some(-0.333333),
        mute: None,
        solo: None,
    };
    core.apply_edit(&cmd, Instant::now()).unwrap();
    core.flush().unwrap();
    let text = disk(&s);
    assert!(text.contains("gain_db = -6.123\n"), "{text}");
    assert!(text.contains("pan = -0.3333\n"), "{text}");
}

#[test]
fn commands_map_to_paths() {
    let set_param = |param: &str, value: f64| Command::SetParam {
        track: "acid".into(),
        param: param.into(),
        value,
        commit: true,
    };
    let cases: Vec<(Command, Vec<&str>)> = vec![
        (
            set_param("acid303.resonance", 80.0),
            vec!["tracks.acid.acid303.resonance"],
        ),
        (
            Command::SetFxMode {
                track: "acid".into(),
                mode: "hard".into(),
            },
            vec!["tracks.acid.dist.mode"],
        ),
        (
            Command::SetAcidStep {
                pattern: "a1".into(),
                index: 0,
                note: Some(48),
                accent: true,
                slide: false,
            },
            vec!["clips.a1.steps"],
        ),
        (drum_step(0, DrumHitDto::Accent), vec!["clips.h.steps"]),
        (Command::MoveSection { from: 1, to: 0 }, vec!["sections"]),
        (Command::DuplicateSection { index: 0 }, vec!["sections"]),
        (Command::DeleteSection { index: 0 }, vec!["sections"]),
        (
            // Section `b`'s lane stretch moves with it, unchanged within `b`.
            Command::ResizeSection { index: 0, bars: 8 },
            vec!["sections[0].bars"],
        ),
        (
            Command::SetSectionPlay {
                section: 0,
                track: "acid".into(),
                pattern: Some("a1".into()),
            },
            vec!["sections[0].play.acid"],
        ),
        (
            Command::SetTrackMix {
                track: "hat".into(),
                gain_db: Some(-6.0),
                pan: None,
                mute: Some(true),
                solo: None,
            },
            vec!["tracks.hat.gain_db", "tracks.hat.mute"],
        ),
    ];
    for (cmd, expected) in cases {
        let s = song();
        let mut core = open(&s);
        let changes = core
            .apply_edit(&cmd, Instant::now())
            .unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
        let paths: Vec<String> = changes.iter().map(|c| c.path.to_string()).collect();
        assert_eq!(paths, expected, "{cmd:?}");
        match &cmd {
            Command::SetAcidStep { .. } => {
                let new = changes[0].new.as_str().unwrap();
                assert!(new.starts_with("C3! "), "{new}");
            }
            Command::SetDrumStep { .. } => {
                let new = changes[0].new.as_str().unwrap();
                assert!(new.starts_with("X.x."), "{new}");
            }
            _ => {}
        }
        // Every mapped edit writes a file that loads again.
        core.flush().unwrap();
        StudioCore::open(&s.path).expect("the written file is a valid song");
    }
}

#[test]
fn non_edits_are_refused() {
    let s = song();
    let mut core = open(&s);
    let live_drag = Command::SetParam {
        track: "acid".into(),
        param: "acid303.cutoff".into(),
        value: 900.0,
        commit: false,
    };
    for cmd in [
        live_drag,
        Command::ListSongs,
        Command::OpenSong { path: "x".into() },
        Command::Play { from_bar: None },
        Command::Stop,
        Command::SeekBar { bar: 1 },
        Command::SetLoop { bars: None },
        Command::Undo,
        Command::Redo,
        Command::ResolveOffered { reapply: true },
    ] {
        assert_eq!(
            core.apply_edit(&cmd, Instant::now()),
            Err(StudioError::NotAnEdit),
            "{cmd:?}"
        );
    }
    assert!(core.pending().is_empty());
}

#[test]
fn step_index_out_of_range() {
    let s = song();
    let mut core = open(&s);
    assert_eq!(
        core.apply_edit(&drum_step(99, DrumHitDto::Hit), Instant::now()),
        Err(StudioError::Edit(EditError::OutOfBounds {
            index: 99,
            len: 16
        }))
    );
    let acid = Command::SetAcidStep {
        pattern: "a1".into(),
        index: 8,
        note: None,
        accent: false,
        slide: false,
    };
    assert_eq!(
        core.apply_edit(&acid, Instant::now()),
        Err(StudioError::Edit(EditError::OutOfBounds {
            index: 8,
            len: 8
        }))
    );
    assert!(core.pending().is_empty());
    assert!(!core.can_undo());
}

#[test]
fn failed_mix_edit_changes_nothing() {
    let s = song();
    let mut core = open(&s);
    // The gain is fine, the pan is out of range: neither is applied.
    let cmd = Command::SetTrackMix {
        track: "hat".into(),
        gain_db: Some(-6.0),
        pan: Some(5.0),
        mute: None,
        solo: None,
    };
    assert!(matches!(
        core.apply_edit(&cmd, Instant::now()),
        Err(StudioError::Edit(EditError::Invalid(_)))
    ));
    assert!(core.pending().is_empty());
    assert_eq!(core.song().tracks["hat"].gain_db, -10.0);
}

#[test]
fn no_tmp_left() {
    let s = song();
    let mut core = open(&s);
    core.apply_edit(&cutoff(820.0), Instant::now()).unwrap();
    core.flush().unwrap();
    let mut names: Vec<String> = fs::read_dir(s.path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["edits.jsonl", "song.toml"]);
}

#[test]
fn own_writes_are_remembered() {
    let s = song();
    let mut core = open(&s);
    let t0 = Instant::now();
    let mut hashes = Vec::new();
    for i in 0..9u32 {
        core.apply_edit(&cutoff(600.0 + f64::from(i)), t0).unwrap();
        core.flush().unwrap();
        hashes.push(text_hash(&disk(&s)));
    }
    assert!(core.is_own_write(&hashes[8]));
    assert!(core.is_own_write(&hashes[1]));
    assert!(!core.is_own_write(&hashes[0]), "the ring holds 8");
    core.undo().unwrap();
    assert!(
        core.is_own_write(&text_hash(&disk(&s))),
        "undo writes count"
    );
}

#[test]
fn undo_depth_capped() {
    let s = song();
    let mut core = open(&s);
    let t0 = Instant::now();
    for i in 0..501u32 {
        core.apply_edit(&cutoff(600.0 + f64::from(i)), t0).unwrap();
        core.flush().unwrap();
    }
    for n in 0..500 {
        assert!(core.can_undo(), "undo {n}");
        assert_eq!(core.undo().unwrap().len(), 1, "undo {n}");
    }
    assert!(!core.can_undo());
    assert_eq!(core.undo().unwrap(), vec![]);
    assert!(
        disk(&s).contains("\"acid303.cutoff\" = 600,"),
        "the oldest step was dropped"
    );
}

#[test]
fn automation_lookup() {
    let s = song();
    let core = open(&s);
    // Section b (bars 2..4) automates acid's cutoff.
    assert!(!core.is_automated("acid", "acid303.cutoff", 1));
    assert!(core.is_automated("acid", "acid303.cutoff", 2));
    assert!(core.is_automated("acid", "acid303.cutoff", 3));
    assert!(
        !core.is_automated("acid", "acid303.cutoff", 4),
        "past the end"
    );
    assert!(!core.is_automated("acid", "acid303.resonance", 2));
    assert!(!core.is_automated("hat", "acid303.cutoff", 2));
}

#[test]
fn view_describes_the_song() {
    let s = song();
    let core = open(&s);
    let view = core.view();
    assert_eq!(view.name, "mini");
    assert_eq!(view.bpm, 120.0);
    assert_eq!(view.total_bars, 4);

    let acid = view.tracks.iter().find(|t| t.id == "acid").unwrap();
    assert_eq!(acid.instrument, "acid303");
    let cutoff = acid
        .params
        .iter()
        .find(|p| p.id == "acid303.cutoff")
        .unwrap();
    assert_eq!(cutoff.label, "Cutoff");
    assert_eq!(cutoff.value, 500.0);
    assert!(cutoff.explicit);
    assert_eq!(cutoff.curve, "exponential");
    assert!(!cutoff.help.is_empty());
    assert_eq!(acid.fx.len(), 1);
    assert_eq!(acid.fx[0].kind, "distortion");
    assert_eq!(acid.fx[0].mode, "soft");
    assert_eq!(acid.fx[0].modes, ["soft", "hard", "overdrive"]);
    let drive = &acid.fx[0].params[0];
    assert_eq!((drive.id.as_str(), drive.value), ("dist.drive", 12.0));

    let hat = view.tracks.iter().find(|t| t.id == "hat").unwrap();
    assert_eq!((hat.gain_db, hat.pan, hat.mute), (-10.0, 0.3, false));
    assert!(
        hat.params
            .iter()
            .all(|p| !p.explicit && p.value == p.default)
    );

    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["patterns"][1]["kind"], "acid");
    assert_eq!(json["patterns"][1]["id"], "a1");
    assert_eq!(
        json["patterns"][1]["steps"][1],
        json!({"note": 36, "accent": true, "slide": false})
    );
    assert_eq!(json["patterns"][2]["kind"], "drum");
    assert_eq!(json["patterns"][2]["hits"][2], "hit");
    assert_eq!(json["sections"][1]["start_bar"], 2);
    assert_eq!(
        json["sections"][1]["play"],
        json!({"kick": "k4", "acid": "a1", "hat": "h"})
    );
    assert_eq!(
        json["sections"][1]["lanes"][0],
        json!({"track": "acid", "param": "acid303.cutoff", "points": [[0.0, 0.1], [2.0, 0.8]]})
    );
}

#[test]
fn command_serde_form() {
    let cmd = cutoff(820.0);
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({"cmd": "set_param", "args": {"track": "acid", "param": "acid303.cutoff", "value": 820.0, "commit": true}})
    );
    assert_eq!(
        serde_json::to_value(Command::ListSongs).unwrap(),
        json!({"cmd": "list_songs"})
    );
    let parsed: Command = serde_json::from_value(
        json!({"cmd": "set_drum_step", "args": {"pattern": "h", "index": 3, "hit": "accent"}}),
    )
    .unwrap();
    assert_eq!(parsed, drum_step(3, DrumHitDto::Accent));
    let mix: Command = serde_json::from_value(
        json!({"cmd": "set_track_mix", "args": {"track": "hat", "mute": true}}),
    )
    .unwrap();
    assert_eq!(
        mix,
        Command::SetTrackMix {
            track: "hat".into(),
            gain_db: None,
            pan: None,
            mute: Some(true),
            solo: None
        }
    );
    let undo: Command = serde_json::from_value(json!({"cmd": "undo"})).unwrap();
    assert_eq!(undo, Command::Undo);
}

// ----- the mixer -----

const MIXED: &str = include_str!("../../tune-song/tests/fixtures/mixed.toml");

/// A temp dir holding `text` as `song.toml`.
fn song_with(text: &str) -> Song {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("song.toml");
    fs::write(&path, text).expect("write song");
    Song { _dir: dir, path }
}

#[test]
fn set_param_on_mix_commits() {
    let s = song();
    let mut core = open(&s);
    let t0 = Instant::now();
    let reverb = Command::SetParam {
        track: "mix".into(),
        param: "reverb.size".into(),
        value: 42.0,
        commit: true,
    };
    let changes = core.apply_edit(&reverb, t0).unwrap();
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0].path.to_string(), "mix.reverb.size");
    assert_eq!((&changes[0].old, &changes[0].new), (&json!(25), &json!(42)));
    assert_eq!(core.song().mix.params["reverb.size"], 42.0);

    core.flush().unwrap();
    let text = disk(&s);
    assert!(
        text.contains("[mix]\nparams = { \"reverb.size\" = 42 }\n"),
        "{text}"
    );
    let logged: Vec<_> = log(&s).iter().map(entry_summary).collect();
    assert_eq!(
        logged,
        [(
            Source::Human,
            "mix.reverb.size".to_owned(),
            json!(25),
            json!(42)
        )]
    );

    // A strip knob on a track goes to the track's params.
    let send = Command::SetParam {
        track: "hat".into(),
        param: "send.reverb".into(),
        value: -20.0,
        commit: true,
    };
    let changes = core.apply_edit(&send, t0).unwrap();
    assert_eq!(changes[0].path.to_string(), "tracks.hat.send.reverb");
    assert_eq!(core.song().tracks["hat"].strip["send.reverb"], -20.0);

    // An unknown mix id is refused.
    let bad = Command::SetParam {
        track: "mix".into(),
        param: "reverb.sise".into(),
        value: 1.0,
        commit: true,
    };
    assert!(matches!(
        core.apply_edit(&bad, t0),
        Err(StudioError::Edit(EditError::UnknownParam { .. }))
    ));
}

#[test]
fn mix_lane_is_automated() {
    let s = song_with(MIXED);
    let core = open(&s);
    // Section b (bars 2..4) has the lane `mix.reverb.size`.
    assert!(!core.is_automated("mix", "reverb.size", 1));
    assert!(core.is_automated("mix", "reverb.size", 2));
    assert!(core.is_automated("mix", "reverb.size", 3));
    assert!(!core.is_automated("mix", "delay.feedback", 2));
    assert!(core.is_automated("acid", "send.delay", 2));
    assert!(!core.is_automated("hat", "send.delay", 2));
}

#[test]
fn view_describes_the_mix() {
    let s = song_with(MIXED);
    let view = open(&s).view();

    let acid = view.tracks.iter().find(|t| t.id == "acid").unwrap();
    let strip: Vec<_> = acid
        .strip
        .iter()
        .map(|p| (p.id.as_str(), p.value, p.explicit))
        .collect();
    assert_eq!(
        strip,
        [
            ("send.delay", -12.0, true),
            ("send.reverb", -18.0, true),
            ("duck.depth", 6.0, true),
            ("duck.release", 80.0, false),
        ]
    );
    assert!(
        acid.params.iter().all(|p| p.id.starts_with("acid303.")),
        "strip ids stay out of the instrument params"
    );
    let eq = &acid.fx[1];
    assert_eq!(eq.kind, "eq");
    assert_eq!(eq.mode, "");
    assert!(eq.modes.is_empty());
    assert_eq!(
        (eq.params[0].id.as_str(), eq.params[0].value),
        ("eq.low_cut", 85.0)
    );

    let ride = view.tracks.iter().find(|t| t.id == "ride").unwrap();
    assert!(
        ride.strip
            .iter()
            .all(|p| !p.explicit && p.value == p.default)
    );

    let mix = &view.mix;
    assert_eq!(mix.master, "limit");
    assert_eq!(mix.masters, ["clip", "limit"]);
    assert_eq!(mix.duck_source.as_deref(), Some("kick"));
    let ids = |ps: &[tune_studio::ParamView]| -> Vec<String> {
        ps.iter().map(|p| p.id.clone()).collect()
    };
    assert_eq!(
        ids(&mix.delay),
        [
            "delay.time",
            "delay.feedback",
            "delay.low_cut",
            "delay.high_cut",
            "delay.width"
        ]
    );
    assert_eq!(ids(&mix.reverb)[0], "reverb.size");
    assert_eq!(
        ids(&mix.master_params),
        ["master.gain", "master.ceiling", "master.release"]
    );
    assert_eq!((mix.delay[1].value, mix.delay[1].explicit), (40.0, true));
    assert_eq!((mix.delay[0].value, mix.delay[0].explicit), (3.0, false));
    assert_eq!(mix.reverb[0].value, 30.0);
    assert_eq!(mix.master_params[0].value, 6.0);
    assert!(mix.delay_active && mix.reverb_active);

    // The mini song: the default mix, no buses, no duck source but the kick.
    let s = song();
    let view = open(&s).view();
    assert_eq!(view.mix.master, "clip");
    assert_eq!(view.mix.duck_source.as_deref(), Some("kick"));
    assert!(!view.mix.delay_active && !view.mix.reverb_active);
    assert!(view.mix.delay.iter().all(|p| !p.explicit));
    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["mix"]["masters"], json!(["clip", "limit"]));
    assert_eq!(json["tracks"][0]["strip"][0]["id"], "send.delay");
}
