//! Undoing one agent run: every target the run changed goes back, unless
//! someone else changed it later, in which case it is kept and listed.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use serde_json::{Value, json};
use tune_studio::{
    Action, EditLog, KeptReason, KeptTarget, Origin, RunTag, RunUndo, SongDoc, Source, StudioCore,
    Target, TargetItem, apply_to_file, target_changes, target_value,
};

const FIXTURE: &str = include_str!("fixtures/commented.toml");

/// A studio core on a temp copy of the commented fixture.
struct S {
    _dir: tempfile::TempDir,
    path: PathBuf,
    core: StudioCore,
}

fn open() -> S {
    open_on(FIXTURE)
}

/// A studio core on a temp copy of `text`.
fn open_on(text: &str) -> S {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.toml");
    fs::write(&path, text).unwrap();
    let core = StudioCore::open(&path).unwrap();
    S {
        _dir: dir,
        path,
        core,
    }
}

fn run(id: &str) -> Origin {
    Origin::claude(Some(RunTag::new(id, None).unwrap()))
}

fn target(s: &str) -> Target {
    s.parse().unwrap_or_else(|e| panic!("{s}: {e}"))
}

fn fixture_song() -> tune_song::Song {
    SongDoc::parse(FIXTURE).unwrap().song().clone()
}

fn undo_action(run: &str) -> Action {
    Action::UndoRun { run: run.into() }
}

impl S {
    /// The agent's actions in run `id`, written at once.
    fn agent(&mut self, id: &str, actions: &[Action]) {
        self.core.apply_now(actions, &run(id)).unwrap();
    }

    /// The producer's edit, written as its own burst.
    fn human(&mut self, action: Action) {
        let changes = self.core.apply_edit(&action, Instant::now()).unwrap();
        assert!(!changes.is_empty(), "{action:?} changed nothing");
        self.core.flush().unwrap();
    }

    /// The producer's "Undo this run".
    fn undo_run(&mut self, id: &str) -> RunUndo {
        let out = self
            .core
            .apply_now(&[undo_action(id)], &Origin::human())
            .unwrap();
        serde_json::from_value(out.results[0].clone()).unwrap()
    }

    fn value(&self, t: &str) -> Value {
        target_value(self.core.song(), &target(t))
    }

    fn text(&self) -> String {
        fs::read_to_string(&self.path).unwrap()
    }

    fn log(&self) -> String {
        fs::read_to_string(self.path.with_file_name("edits.jsonl")).unwrap_or_default()
    }
}

fn set(path: &str, value: Value) -> Action {
    Action::SetValue {
        path: path.parse().unwrap(),
        value,
    }
}

fn cutoff(value: f64) -> Action {
    set("tracks.acid.acid303.cutoff", json!(value))
}

fn place(track: &str, clip: &str, at: u32, bars: u32) -> Action {
    Action::AddPlacement {
        track: track.into(),
        clip: clip.into(),
        at,
        bars,
        offset: None,
    }
}

fn reverted(targets: &[&str]) -> Vec<TargetItem> {
    targets
        .iter()
        .map(|t| TargetItem {
            target: target(t),
            item: None,
        })
        .collect()
}

fn kept(t: &str, by: Option<Source>) -> KeptTarget {
    KeptTarget {
        target: target(t),
        item: None,
        reason: KeptReason::ChangedLater,
        by,
        message: None,
    }
}

fn placement(at: u32, clip: &str, bars: u32) -> Value {
    json!({ "at": at, "clip": clip, "bars": bars, "offset": 0 })
}

fn sorted(mut items: Vec<TargetItem>) -> Vec<String> {
    let mut names: Vec<String> = items.drain(..).map(|i| i.target.to_string()).collect();
    names.sort();
    names
}

#[test]
fn undo_run_reverts_every_action_of_the_run() {
    let mut s = open();
    s.agent("r1", &[cutoff(820.0), set("song.bpm", json!(130))]);
    s.agent(
        "r1",
        &[Action::AddEffect {
            track: "acid".into(),
            module: "eq".into(),
            id: None,
            index: None,
        }],
    );
    s.agent(
        "r1",
        &[Action::AddTrack {
            track: "keys".into(),
            instrument: "acid303".into(),
        }],
    );
    s.agent(
        "r1",
        &[Action::AddClip {
            clip: "lead".into(),
            kind: "notes".into(),
            steps: None,
            bars: Some(1),
            notes: Some(json!([{ "start": 0, "pitch": "C3", "len": 2 }])),
        }],
    );
    s.agent("r1", &[place("keys", "lead", 1, 2)]);
    let run_state = s.text();

    let u = s.undo_run("r1");
    assert_eq!(u.run, "r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(u.already, vec![]);
    assert_eq!(
        sorted(u.reverted),
        [
            "clips.lead",
            "song.bpm",
            "tracks.acid.acid303.cutoff",
            "tracks.acid.fx",
            "tracks.keys",
        ]
    );
    assert_eq!(target_changes(&fixture_song(), s.core.song()), vec![]);

    // One undo step: ⌘Z brings the run's state back.
    s.core.undo(Source::Human).unwrap();
    assert_eq!(s.text(), run_state);
}

#[test]
fn a_later_human_edit_is_kept() {
    let mut s = open();
    s.agent(
        "r1",
        &[cutoff(820.0), set("tracks.acid.gain_db", json!(-6))],
    );
    s.human(Action::SetParam {
        track: "acid".into(),
        param: "acid303.cutoff".into(),
        value: 700.0,
        commit: true,
    });
    let u = s.undo_run("r1");
    assert_eq!(
        u.kept,
        vec![kept("tracks.acid.acid303.cutoff", Some(Source::Human))]
    );
    assert_eq!(u.reverted, reverted(&["tracks.acid.gain_db"]));
    assert_eq!(s.value("tracks.acid.acid303.cutoff"), json!(700));
    assert_eq!(s.value("tracks.acid.gain_db"), json!(0));
}

#[test]
fn the_run_after_a_human_edit_reverts_to_the_human_value() {
    let mut s = open();
    s.agent("r1", &[cutoff(800.0)]);
    s.human(cutoff(600.0));
    s.agent("r1", &[cutoff(900.0)]);
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, reverted(&["tracks.acid.acid303.cutoff"]));
    assert_eq!(u.kept, vec![]);
    assert_eq!(s.value("tracks.acid.acid303.cutoff"), json!(600));
}

#[test]
fn an_item_the_run_created_and_the_human_changed_is_kept() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::AddTrack {
            track: "bass".into(),
            instrument: "acid303".into(),
        }],
    );
    s.human(set("tracks.bass.acid303.cutoff", json!(600)));
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![kept("tracks.bass", Some(Source::Human))]);
    assert_eq!(u.reverted, vec![]);
    assert_eq!(s.value("tracks.bass.acid303.cutoff"), json!(600));
}

#[test]
fn another_runs_edit_counts_as_someone_else() {
    let mut s = open();
    s.agent("r1", &[cutoff(820.0)]);
    s.agent("r2", &[cutoff(900.0)]);
    let u = s.undo_run("r1");
    assert_eq!(
        u.kept,
        vec![kept("tracks.acid.acid303.cutoff", Some(Source::Claude))]
    );
    assert_eq!(s.value("tracks.acid.acid303.cutoff"), json!(900));
}

#[test]
fn a_clip_still_used_is_kept() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::AddClip {
            clip: "lead".into(),
            kind: "acid".into(),
            steps: None,
            bars: None,
            notes: None,
        }],
    );
    s.human(place("acid", "lead", 1, 2));
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.kept.len(), 1, "{:?}", u.kept);
    let k = &u.kept[0];
    assert_eq!(
        (&k.target, &k.item, k.reason, k.by),
        (&target("clips.lead"), &None, KeptReason::StillUsed, None)
    );
    let message = k.message.as_deref().unwrap();
    // The validator says why: the placement names the clip.
    assert_eq!(message, "unknown clip `lead`");
    // The placement stays, and so does the clip.
    assert_eq!(
        s.value("tracks.acid.play"),
        json!([placement(1, "lead", 2), placement(3, "a1", 2)])
    );
    assert!(s.value("clips.lead").is_object());
}

#[test]
fn placements_revert_one_by_one() {
    let mut s = open();
    s.agent("r1", &[place("acid", "a1", 1, 1)]);
    s.agent("r1", &[place("acid", "a1", 2, 1)]);
    s.human(Action::RemovePlacement {
        track: "acid".into(),
        at: 1,
    });
    let u = s.undo_run("r1");
    assert_eq!(
        u.reverted,
        vec![TargetItem {
            target: target("tracks.acid.play"),
            item: Some(placement(2, "a1", 1)),
        }]
    );
    assert_eq!(
        u.kept,
        vec![KeptTarget {
            item: Some(placement(1, "a1", 1)),
            ..kept("tracks.acid.play", Some(Source::Human))
        }]
    );
    assert_eq!(s.value("tracks.acid.play"), json!([placement(3, "a1", 2)]));
}

#[test]
fn section_ops_revert_with_their_placements() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::AddSection {
            name: "break".into(),
            bars: 2,
            index: Some(1),
        }],
    );
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(
        s.value("sections"),
        json!([{ "name": "a", "bars": 2 }, { "name": "b", "bars": 2 }])
    );
    assert_eq!(target_changes(&fixture_song(), s.core.song()), vec![]);
    let text = s.text();
    assert!(
        text.contains("# acid enters\n[[sections]]\nname = \"b\""),
        "{text}"
    );
    assert!(
        text.contains("# the filter opens over the section"),
        "{text}"
    );
}

#[test]
fn remove_track_reverts_with_its_lanes() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::RemoveTrack {
            track: "acid".into(),
        }],
    );
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    let order: Vec<&String> = s.core.song().tracks.keys().collect();
    assert_eq!(order, ["kick", "acid", "hat"]);
    assert_eq!(target_changes(&fixture_song(), s.core.song()), vec![]);
}

#[test]
fn studio_closed_hand_edit_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.toml");
    fs::write(&path, FIXTURE).unwrap();
    apply_to_file(&path, &[cutoff(820.0)], &run("r1")).unwrap();
    let edited = fs::read_to_string(&path)
        .unwrap()
        .replace("\"acid303.cutoff\" = 820", "\"acid303.cutoff\" = 900");
    fs::write(&path, &edited).unwrap();
    let log = fs::read_to_string(dir.path().join("edits.jsonl")).unwrap();

    let out = apply_to_file(&path, &[undo_action("r1")], &Origin::claude(None)).unwrap();
    let result = &out.results[0];
    assert_eq!(result["kept"][0]["by"], Value::Null);
    let u: RunUndo = serde_json::from_value(result.clone()).unwrap();
    assert_eq!(u.kept, vec![kept("tracks.acid.acid303.cutoff", None)]);
    assert_eq!(u.reverted, vec![]);
    // Nothing reverted: nothing written or logged.
    assert_eq!(out.file_hash, None);
    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
    assert_eq!(
        fs::read_to_string(dir.path().join("edits.jsonl")).unwrap(),
        log
    );
}

#[test]
fn undo_run_twice_changes_nothing() {
    let mut s = open();
    s.agent("r1", &[cutoff(820.0), place("acid", "a1", 1, 2)]);
    let run_state = s.text();
    s.undo_run("r1");
    let (text, log) = (s.text(), s.log());

    let u = s.undo_run("r1");
    assert_eq!((u.reverted, u.kept), (vec![], vec![]));
    let mut already: Vec<String> = u.already.iter().map(Target::to_string).collect();
    already.sort();
    assert_eq!(already, ["tracks.acid.acid303.cutoff", "tracks.acid.play"]);
    assert_eq!((s.text(), s.log()), (text, log));
    // No undo step: ⌘Z undoes the first run undo.
    s.core.undo(Source::Human).unwrap();
    assert_eq!(s.text(), run_state);
}

#[test]
fn after_cmd_z_the_rest_reverts() {
    let mut s = open();
    s.agent("r1", &[cutoff(820.0)]);
    s.agent("r1", &[set("song.bpm", json!(130))]);
    s.core.undo(Source::Human).unwrap();
    let u = s.undo_run("r1");
    assert_eq!(u.already, vec![target("song.bpm")]);
    assert_eq!(u.reverted, reverted(&["tracks.acid.acid303.cutoff"]));
    assert_eq!(u.kept, vec![]);
    assert_eq!(s.value("tracks.acid.acid303.cutoff"), json!(500));
    assert_eq!(s.value("song.bpm"), json!(120));
}

#[test]
fn no_such_run() {
    let mut s = open();
    s.agent("r1", &[cutoff(820.0)]);
    let err = s
        .core
        .apply_now(&[undo_action("zz")], &Origin::human())
        .unwrap_err();
    assert_eq!(err.to_string(), "no edits of run `zz` in the edit log");
}

// ----- cases where the algorithm left a choice -----

/// A damaged record could be one of the run's: nothing is undone.
#[test]
fn a_damaged_log_refuses_the_undo() {
    let mut s = open();
    s.agent("r1", &[cutoff(820.0)]);
    let file = s.path.with_file_name("edits.jsonl");
    let mut log = fs::read_to_string(&file).unwrap();
    log.push_str(r#"{"ts":"2026-10-27T10:00:01.000Z","source":"claude","record":"action"}"#);
    log.push('\n');
    fs::write(&file, &log).unwrap();
    let line = log.lines().count();
    let err = s
        .core
        .apply_now(&[undo_action("r1")], &Origin::human())
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("the edit log has a damaged record at line {line}; nothing was changed")
    );
    assert_eq!(s.value("tracks.acid.acid303.cutoff"), json!(820));
}

/// The producer changed an item the run created between two of the run's
/// edits to it: its value before the run's later edit is not known as a
/// whole, so it is kept.
#[test]
fn a_created_item_changed_between_the_runs_edits_is_kept() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::AddTrack {
            track: "bass".into(),
            instrument: "acid303".into(),
        }],
    );
    s.human(set("tracks.bass.acid303.cutoff", json!(600)));
    s.agent("r1", &[set("tracks.bass.gain_db", json!(-3))]);
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![kept("tracks.bass", Some(Source::Human))]);
    assert_eq!(s.value("tracks.bass.acid303.cutoff"), json!(600));
    assert_eq!(s.value("tracks.bass.gain_db"), json!(-3));
}

/// The run's later edits to an item it created are part of it: the item
/// goes as one, and a producer's edit to a part of it keeps it.
#[test]
fn edits_to_an_effect_the_run_added_go_with_it() {
    let add_eq = Action::AddEffect {
        track: "acid".into(),
        module: "eq".into(),
        id: None,
        index: None,
    };
    let mut s = open();
    s.agent("r1", std::slice::from_ref(&add_eq));
    s.agent("r1", &[set("tracks.acid.eq.low_cut", json!(120))]);
    let u = s.undo_run("r1");
    assert_eq!(
        (u.reverted, u.kept),
        (reverted(&["tracks.acid.fx"]), vec![])
    );
    assert_eq!(target_changes(&fixture_song(), s.core.song()), vec![]);

    let mut s = open();
    s.agent("r1", &[add_eq]);
    s.agent("r1", &[set("tracks.acid.eq.low_cut", json!(120))]);
    s.human(set("tracks.acid.eq.mid_gain", json!(3)));
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![kept("tracks.acid.fx", Some(Source::Human))]);
    assert_eq!(s.value("tracks.acid.eq.mid_gain"), json!(3));
}

/// A target's value is read when its turn comes, after the run's newer
/// edits are undone: the cutoff the run set before removing the track
/// comes back with the track, then goes back to its own old value.
#[test]
fn a_value_set_before_the_run_removed_its_track_reverts() {
    let mut s = open();
    s.agent("r1", &[cutoff(820.0)]);
    s.agent(
        "r1",
        &[Action::RemoveTrack {
            track: "acid".into(),
        }],
    );
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(target_changes(&fixture_song(), s.core.song()), vec![]);
}

/// Changed points of a stretch go back into its table, comments kept.
#[test]
fn lane_points_revert_in_place() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::SetLanePoints {
            param: "acid.acid303.cutoff".into(),
            at: 3,
            points: vec![[0.0, 0.3], [2.0, 0.9]],
        }],
    );
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(s.text(), FIXTURE);
}

/// A run edit then a run undo by the producer: the undo is one human
/// write, logged as such and in no run.
#[test]
fn the_undo_is_logged_as_the_producers_write() {
    let mut s = open();
    s.agent("r1", &[cutoff(820.0)]);
    s.undo_run("r1");
    let records = EditLog::for_song(&s.path).read_records().unwrap();
    let last = records.last().unwrap();
    assert_eq!(
        (last.source, last.actions.clone(), last.run.clone()),
        (Source::Human, vec!["undo_run".to_owned()], None)
    );
}

#[test]
fn the_result_has_a_pinned_shape() {
    let mut s = open();
    s.agent(
        "r1",
        &[cutoff(820.0), set("tracks.acid.gain_db", json!(-6))],
    );
    s.agent("r1", &[place("acid", "a1", 1, 2)]);
    s.human(cutoff(700.0));
    let out = s
        .core
        .apply_now(&[undo_action("r1")], &Origin::human())
        .unwrap();
    assert_eq!(
        out.results[0],
        json!({
            "run": "r1",
            "title": null,
            "reverted": [
                { "target": "tracks.acid.play", "item": placement(1, "a1", 2) },
                { "target": "tracks.acid.gain_db", "item": null },
            ],
            "kept": [{
                "target": "tracks.acid.acid303.cutoff",
                "item": null,
                "reason": "changed_later",
                "by": "human",
                "message": null,
            }],
            "already": [],
        })
    );
}

/// The producer's edit still waiting for its write when the run undo
/// comes is written first, so it counts as a later edit.
#[test]
fn a_pending_producer_edit_is_kept() {
    let mut s = open();
    s.agent("r1", &[cutoff(820.0)]);
    s.core.apply_edit(&cutoff(700.0), Instant::now()).unwrap();
    let u = s.undo_run("r1");
    assert_eq!(
        u.kept,
        vec![kept("tracks.acid.acid303.cutoff", Some(Source::Human))]
    );
    assert_eq!(s.value("tracks.acid.acid303.cutoff"), json!(700));
}

/// Every edit action, alone in a run, is undone back to the fixture.
#[test]
fn every_edit_action_reverts() {
    let id = |s: &str| s.to_owned();
    let actions = [
        Action::SetDeviceSetting {
            track: id("acid"),
            device: id("dist"),
            setting: id("mode"),
            value: id("hard"),
        },
        Action::SetAcidStep {
            clip: id("a1"),
            index: 4,
            note: Some(40),
            accent: true,
            slide: false,
        },
        Action::SetDrumStep {
            clip: id("k4"),
            index: 1,
            hit: tune_studio::DrumHitDto::Accent,
        },
        Action::MoveSection { from: 0, to: 1 },
        Action::ResizeSection { index: 1, bars: 4 },
        Action::DuplicateSection { index: 1 },
        Action::DeleteSection { index: 0 },
        Action::SetSectionPlay {
            section: 0,
            track: id("acid"),
            clip: Some(id("a1")),
        },
        Action::SetTrackMix {
            track: id("hat"),
            gain_db: Some(-4.0),
            pan: Some(-0.5),
            mute: Some(true),
            solo: None,
        },
        Action::RenameTrack {
            track: id("acid"),
            to: id("lead"),
        },
        Action::RemoveEffect {
            track: id("acid"),
            device: id("dist"),
        },
        Action::RenameDevice {
            track: id("acid"),
            device: id("dist"),
            to: id("drive"),
        },
        Action::RenameDevice {
            track: id("acid"),
            device: id("acid303"),
            to: id("bass303"),
        },
        Action::SetInstrument {
            track: id("acid"),
            module: id("kick"),
        },
        Action::RemoveClip { clip: id("k4") },
        Action::RenameClip {
            clip: id("a1"),
            to: id("bass"),
        },
        Action::DuplicateClip {
            clip: id("h"),
            to: id("h2"),
        },
        Action::MovePlacement {
            track: id("acid"),
            at: 3,
            to: 1,
        },
        Action::ResizePlacement {
            track: id("acid"),
            at: 3,
            bars: 1,
        },
        Action::RemovePlacement {
            track: id("kick"),
            at: 3,
        },
        Action::AddSection {
            name: id("outro"),
            bars: 4,
            index: None,
        },
        Action::RenameSection {
            index: 1,
            name: id("drop"),
        },
        Action::AddLane {
            param: id("mix.reverb.size"),
            at: 1,
            bars: 2,
            points: vec![[0.0, 0.2], [2.0, 0.6]],
        },
        Action::RemoveLane {
            param: id("acid.acid303.cutoff"),
            at: 3,
        },
    ];
    for action in actions {
        let mut s = open();
        s.agent("r1", std::slice::from_ref(&action));
        assert_ne!(s.text(), FIXTURE, "{action:?} changed nothing");
        let u = s.undo_run("r1");
        assert_eq!(u.kept, vec![], "{action:?}");
        assert_eq!(
            target_changes(&fixture_song(), s.core.song()),
            vec![],
            "{action:?}"
        );
    }
}

/// The run moved a placement and the producer moved it again: the run's
/// original does not come back beside the producer's copy. Both of the
/// run's elements are kept and listed.
#[test]
fn a_placement_moved_again_is_kept_with_its_original() {
    let mut s = open();
    // Before the run: acid's placement is one bar long.
    s.human(Action::ResizePlacement {
        track: "acid".into(),
        at: 3,
        bars: 1,
    });
    s.agent(
        "r1",
        &[Action::MovePlacement {
            track: "acid".into(),
            at: 3,
            to: 1,
        }],
    );
    s.human(Action::MovePlacement {
        track: "acid".into(),
        at: 1,
        to: 2,
    });
    let left = s.value("tracks.acid.play");
    let u = s.undo_run("r1");
    let kept_at = |at| KeptTarget {
        item: Some(placement(at, "a1", 1)),
        ..kept("tracks.acid.play", Some(Source::Human))
    };
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.kept, vec![kept_at(1), kept_at(3)]);
    assert_eq!(s.value("tracks.acid.play"), left);
    assert_eq!(left, json!([placement(2, "a1", 1)]));
}

/// The same for a lane stretch the run moved and the producer moved again.
#[test]
fn a_stretch_moved_again_is_kept_with_its_original() {
    let lane = "acid.acid303.cutoff";
    let remove = |at| Action::RemoveLane {
        param: lane.into(),
        at,
    };
    let add = |at| Action::AddLane {
        param: lane.into(),
        at,
        bars: 1,
        points: vec![[0.0, 0.1], [1.0, 0.8]],
    };
    let mut s = open();
    // Before the run: the stretch is one bar long.
    s.human(remove(3));
    s.human(add(3));
    s.agent("r1", &[remove(3)]);
    s.agent("r1", &[add(1)]);
    s.human(remove(1));
    s.human(add(2));
    let left = s.value("automation.acid.acid303.cutoff");
    let u = s.undo_run("r1");
    let stretch = |at| json!({ "at": at, "bars": 1, "points": [[0, 0.1], [1, 0.8]] });
    let kept_at = |at| KeptTarget {
        item: Some(stretch(at)),
        ..kept("automation.acid.acid303.cutoff", Some(Source::Human))
    };
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.kept, vec![kept_at(1), kept_at(3)]);
    assert_eq!(s.value("automation.acid.acid303.cutoff"), left);
    assert_eq!(left, json!([stretch(2)]));
}

// ----- review round 1: list items and containers someone else touched -----

fn kept_item(t: &str, item: Value, by: Option<Source>) -> KeptTarget {
    KeptTarget {
        item: Some(item),
        ..kept(t, by)
    }
}

fn lane_stretch(at: u32, bars: u32, points: Value) -> Value {
    json!({ "at": at, "bars": bars, "points": points })
}

/// p1: the producer's ⌘Z then ⌘⇧Z re-did the run's placement: it is the
/// producer's now and stays.
#[test]
fn a_placement_the_producer_redid_is_kept() {
    let mut s = open();
    s.agent("r1", &[place("acid", "a1", 1, 1)]);
    s.core.undo(Source::Human).unwrap();
    s.core.redo(Source::Human).unwrap();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        u.kept,
        vec![kept_item(
            "tracks.acid.play",
            placement(1, "a1", 1),
            Some(Source::Human)
        )]
    );
    assert_eq!(
        s.value("tracks.acid.play"),
        json!([placement(1, "a1", 1), placement(3, "a1", 2)])
    );
}

/// p1b: the same for a value.
#[test]
fn a_value_the_producer_redid_is_kept() {
    let mut s = open();
    s.agent("r1", &[cutoff(820.0)]);
    s.core.undo(Source::Human).unwrap();
    s.core.redo(Source::Human).unwrap();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        u.kept,
        vec![kept("tracks.acid.acid303.cutoff", Some(Source::Human))]
    );
    assert_eq!(s.value("tracks.acid.acid303.cutoff"), json!(820));
}

/// p2: the producer's ⌘Z then ⌘⇧Z re-did the run's removal: the placement
/// does not come back.
#[test]
fn a_removal_the_producer_redid_is_kept() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::RemovePlacement {
            track: "kick".into(),
            at: 3,
        }],
    );
    s.core.undo(Source::Human).unwrap();
    s.core.redo(Source::Human).unwrap();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        u.kept,
        vec![kept_item(
            "tracks.kick.play",
            placement(3, "k4", 2),
            Some(Source::Human)
        )]
    );
    assert_eq!(s.value("tracks.kick.play"), json!([placement(1, "k4", 2)]));
}

/// p3: the producer removed the run's placement and placed an identical
/// one: that one is the producer's and stays.
#[test]
fn an_identical_placement_the_producer_made_is_kept() {
    let mut s = open();
    s.agent("r1", &[place("acid", "a1", 1, 1)]);
    s.human(Action::RemovePlacement {
        track: "acid".into(),
        at: 1,
    });
    s.human(place("acid", "a1", 1, 1));
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        u.kept,
        vec![kept_item(
            "tracks.acid.play",
            placement(1, "a1", 1),
            Some(Source::Human)
        )]
    );
    assert_eq!(
        s.value("tracks.acid.play"),
        json!([placement(1, "a1", 1), placement(3, "a1", 2)])
    );
}

/// p3b: the producer put the removed placement back and removed it again:
/// it stays removed.
#[test]
fn a_placement_the_producer_removed_again_stays_removed() {
    let mut s = open();
    let remove = Action::RemovePlacement {
        track: "kick".into(),
        at: 3,
    };
    s.agent("r1", std::slice::from_ref(&remove));
    s.human(place("kick", "k4", 3, 2));
    s.human(remove);
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        u.kept,
        vec![kept_item(
            "tracks.kick.play",
            placement(3, "k4", 2),
            Some(Source::Human)
        )]
    );
    assert_eq!(s.value("tracks.kick.play"), json!([placement(1, "k4", 2)]));
}

/// p4: the producer's section edit moved every bar: a removed placement
/// would come back at a bar that now means something else.
#[test]
fn a_removed_placement_is_kept_after_the_sections_changed() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::RemovePlacement {
            track: "kick".into(),
            at: 3,
        }],
    );
    s.human(Action::AddSection {
        name: "intro".into(),
        bars: 4,
        index: Some(0),
    });
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        u.kept,
        vec![kept_item(
            "tracks.kick.play",
            placement(3, "k4", 2),
            Some(Source::Human)
        )]
    );
    assert_eq!(s.text(), text);
}

/// p6: the producer made a new track under the removed one's name: the
/// old track's lane stays away from it.
#[test]
fn a_track_the_producer_made_again_gets_no_old_lanes() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::RemoveTrack {
            track: "acid".into(),
        }],
    );
    s.human(Action::AddTrack {
        track: "acid".into(),
        instrument: "acid303".into(),
    });
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let lane = lane_stretch(3, 2, json!([[0, 0.1], [2, 0.8]]));
    assert_eq!(
        u.kept,
        vec![
            kept_item("automation.acid.acid303.cutoff", lane, Some(Source::Human)),
            kept("tracks.acid", Some(Source::Human)),
        ]
    );
    assert_eq!(s.text(), text);
}

/// p7: the producer made a new clip under the removed one's name: the old
/// clip's placements do not come back to play it.
#[test]
fn a_clip_the_producer_made_again_gets_no_old_placements() {
    let mut s = open();
    s.agent("r1", &[Action::RemoveClip { clip: "a1".into() }]);
    s.human(Action::AddClip {
        clip: "a1".into(),
        kind: "acid".into(),
        steps: Some("C3 . . .".into()),
        bars: None,
        notes: None,
    });
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        u.kept,
        vec![
            kept_item(
                "tracks.acid.play",
                placement(3, "a1", 2),
                Some(Source::Human)
            ),
            kept("clips.a1", Some(Source::Human)),
        ]
    );
    assert_eq!(s.text(), text);
}

/// p8: the studio was reopened between the run's edits: the log still
/// holds the whole run.
#[test]
fn a_run_spanning_a_reopen_reverts() {
    let mut s = open();
    s.agent("r1", &[cutoff(820.0)]);
    s.core = StudioCore::open(&s.path).unwrap();
    s.agent("r1", &[set("song.bpm", json!(130))]);
    let u = s.undo_run("r1");
    assert_eq!(
        u.reverted,
        reverted(&["song.bpm", "tracks.acid.acid303.cutoff"])
    );
    assert_eq!(u.kept, vec![]);
    assert_eq!(target_changes(&fixture_song(), s.core.song()), vec![]);
}

/// p9: the producer added an effect under the removed one's id: the old
/// effect's lane stays away from it.
#[test]
fn an_effect_the_producer_made_again_gets_no_old_lanes() {
    let mut s = open();
    let points = vec![[0.0, 0.2], [2.0, 0.6]];
    s.human(Action::AddLane {
        param: "acid.dist.drive".into(),
        at: 1,
        bars: 2,
        points,
    });
    s.agent(
        "r1",
        &[Action::RemoveEffect {
            track: "acid".into(),
            device: "dist".into(),
        }],
    );
    s.human(Action::AddEffect {
        track: "acid".into(),
        module: "distortion".into(),
        id: None,
        index: None,
    });
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let lane = lane_stretch(1, 2, json!([[0, 0.2], [2, 0.6]]));
    assert_eq!(
        u.kept,
        vec![kept_item(
            "automation.acid.dist.drive",
            lane,
            Some(Source::Human)
        )]
    );
    // The chain holds `dist` again, as before the run (its params are
    // targets of their own): nothing to do there.
    assert_eq!(u.already, vec![target("tracks.acid.fx")]);
    assert_eq!(s.text(), text);
}

// ----- review round 1: a section edit reverts whole or not at all -----

/// p5: the producer changed one placement the run's section edit moved:
/// reverting the rest would shift everything but that placement and leave
/// the section in. The whole section edit is kept and listed.
#[test]
fn a_section_edit_with_one_part_changed_later_is_kept_whole() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::AddSection {
            name: "break".into(),
            bars: 2,
            index: Some(1),
        }],
    );
    s.human(Action::ResizePlacement {
        track: "kick".into(),
        at: 5,
        bars: 1,
    });
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(s.text(), text);
    assert!(
        u.kept.iter().all(|k| k.reason == KeptReason::ChangedLater),
        "{:?}",
        u.kept
    );
    let mut targets: Vec<String> = u.kept.iter().map(|k| k.target.to_string()).collect();
    targets.dedup();
    targets.sort();
    assert_eq!(
        targets,
        [
            "automation.acid.acid303.cutoff",
            "sections",
            "tracks.acid.play",
            "tracks.hat.play",
            "tracks.kick.play",
        ]
    );
    // Each moved placement and stretch is listed: what the run added, and
    // what it took away.
    assert_eq!(u.kept.len(), 1 + 2 * 4, "{:?}", u.kept);
    // The producer's change to kick's placement keeps all of it: each part
    // names the producer, the sections too.
    assert!(
        u.kept.iter().all(|k| k.by == Some(Source::Human)),
        "{:?}",
        u.kept
    );
    let sections = u.kept.iter().find(|k| k.target == Target::Sections);
    assert_eq!(sections.and_then(|k| k.by), Some(Source::Human));
}

/// A section edit refused while its clip is gone goes through once the
/// clip is back, all of it in one write.
#[test]
fn a_section_edit_reverts_once_its_clip_is_back() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::AddSection {
            name: "break".into(),
            bars: 2,
            index: Some(1),
        }],
    );
    s.agent("r1", &[Action::RemoveClip { clip: "a1".into() }]);
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(target_changes(&fixture_song(), s.core.song()), vec![]);
}

// ----- review round 2: a section edit is kept whole -----

/// Every kept entry names the producer, and nothing went back.
fn all_kept_by_human(u: &RunUndo) {
    assert_eq!(u.reverted, vec![]);
    assert!(!u.kept.is_empty());
    assert!(
        u.kept
            .iter()
            .all(|k| k.reason == KeptReason::ChangedLater && k.by == Some(Source::Human)),
        "{:?}",
        u.kept
    );
}

/// q5: the section edit moved a placement of a track the run created, and
/// the producer changed that track: the track is kept, so the section edit
/// that moved its placement is kept with it.
#[test]
fn a_section_edit_is_kept_with_a_track_it_moved_that_was_kept() {
    let mut s = open();
    s.human(Action::AddSection {
        name: "c".into(),
        bars: 4,
        index: None,
    });
    s.agent(
        "r1",
        &[Action::AddTrack {
            track: "bass".into(),
            instrument: "acid303".into(),
        }],
    );
    s.agent("r1", &[place("bass", "a1", 5, 1)]);
    s.agent(
        "r1",
        &[Action::AddSection {
            name: "break".into(),
            bars: 2,
            index: Some(1),
        }],
    );
    s.human(set("tracks.bass.gain_db", json!(-3)));
    let text = s.text();
    let u = s.undo_run("r1");
    all_kept_by_human(&u);
    assert!(u.kept.iter().any(|k| k.target == target("tracks.bass")));
    assert!(u.kept.iter().any(|k| k.target == Target::Sections));
    assert_eq!(s.text(), text);
}

fn add_break(s: &mut S) {
    s.agent(
        "r1",
        &[Action::AddSection {
            name: "break".into(),
            bars: 2,
            index: Some(1),
        }],
    );
}

/// q4: the producer put a lane stretch in the run's new section: taking
/// the section out would leave it under the placements that move back.
#[test]
fn a_section_edit_is_kept_after_a_later_lane_edit() {
    let mut s = open();
    add_break(&mut s);
    s.human(Action::AddLane {
        param: "mix.reverb.size".into(),
        at: 3,
        bars: 2,
        points: vec![[0.0, 0.2], [2.0, 0.6]],
    });
    let text = s.text();
    let u = s.undo_run("r1");
    all_kept_by_human(&u);
    assert!(u.kept.iter().any(|k| k.target == Target::Sections));
    assert_eq!(s.text(), text);
}

/// q4b: the same for a placement on a track the section edit did not move.
#[test]
fn a_section_edit_is_kept_after_a_later_placement_elsewhere() {
    let mut s = open();
    s.human(Action::AddTrack {
        track: "x".into(),
        instrument: "acid303".into(),
    });
    add_break(&mut s);
    s.human(place("x", "a1", 3, 2));
    let text = s.text();
    let u = s.undo_run("r1");
    all_kept_by_human(&u);
    assert!(u.kept.iter().any(|k| k.target == Target::Sections));
    assert_eq!(s.text(), text);
}

/// q3: the producer removed the track and brought it back with ⌘Z: the
/// placement the run added is part of the producer's restored track now.
#[test]
fn an_addition_on_a_track_the_producer_rebuilt_is_kept() {
    let mut s = open();
    s.agent("r1", &[place("acid", "a1", 1, 1)]);
    s.human(Action::RemoveTrack {
        track: "acid".into(),
    });
    s.core.undo(Source::Human).unwrap();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        u.kept,
        vec![kept_item(
            "tracks.acid.play",
            placement(1, "a1", 1),
            Some(Source::Human)
        )]
    );
    assert_eq!(
        s.value("tracks.acid.play"),
        json!([placement(1, "a1", 1), placement(3, "a1", 2)])
    );
}

// ----- review round 3 -----

/// r1: the producer moved the placement between two of the run's moves of
/// it. The list's state before the run's later move is the producer's, not
/// known element by element, so the whole list is kept: the producer's
/// move is never undone.
#[test]
fn a_list_the_producer_edited_between_the_runs_edits_is_kept() {
    let mut s = open();
    let shift = |at, to| Action::MovePlacement {
        track: "acid".into(),
        at,
        to,
    };
    s.human(Action::ResizePlacement {
        track: "acid".into(),
        at: 3,
        bars: 1,
    });
    s.agent("r1", &[shift(3, 1)]);
    s.human(shift(1, 2));
    s.agent("r1", &[shift(2, 4)]);
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let kept_at = |at| {
        kept_item(
            "tracks.acid.play",
            placement(at, "a1", 1),
            Some(Source::Human),
        )
    };
    assert_eq!(u.kept, vec![kept_at(4), kept_at(3)]);
    assert_eq!(s.value("tracks.acid.play"), json!([placement(4, "a1", 1)]));
}

/// r2: a track with a placement in the run's new section came in from
/// outside the studio: the section edit is kept whole, so that placement
/// stays in the section.
#[test]
fn a_section_edit_is_kept_after_a_track_came_in_from_outside() {
    let mut s = open();
    add_break(&mut s);
    let text = s.text().replacen(
        "[clips.k4]",
        "[tracks.x]\ninstrument = \"acid303\"\nplay = [{ at = 3, clip = \"a1\", bars = 2 }]\n\n[clips.k4]",
        1,
    );
    fs::write(&s.path, &text).unwrap();
    s.core
        .on_disk_text(text.clone())
        .expect("an external change");
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert!(u.kept.iter().any(|k| k.target == Target::Sections));
    assert!(
        u.kept
            .iter()
            .all(|k| k.reason == KeptReason::ChangedLater && k.by == Some(Source::External)),
        "{:?}",
        u.kept
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.value("tracks.x.play"), json!([placement(3, "a1", 2)]));
}

// ----- final review -----

/// A song file with the studio closed: the run writes through
/// `apply_to_file`, the producer edits the text by hand (no log entry).
struct F {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn closed() -> F {
    closed_on(FIXTURE)
}

fn closed_on(text: &str) -> F {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.toml");
    fs::write(&path, text).unwrap();
    F { _dir: dir, path }
}

impl F {
    fn agent(&self, id: &str, actions: &[Action]) {
        apply_to_file(&self.path, actions, &run(id)).unwrap();
    }

    /// A hand edit in an editor: `from` replaced by `to`, nothing logged.
    fn hand(&self, from: &str, to: &str) {
        let text = self.text();
        assert!(text.contains(from), "{from:?} not in {text}");
        fs::write(&self.path, text.replacen(from, to, 1)).unwrap();
    }

    fn undo_run(&self, id: &str) -> RunUndo {
        let out = apply_to_file(&self.path, &[undo_action(id)], &Origin::claude(None)).unwrap();
        serde_json::from_value(out.results[0].clone()).unwrap()
    }

    fn text(&self) -> String {
        fs::read_to_string(&self.path).unwrap()
    }

    fn value(&self, t: &str) -> Value {
        target_value(SongDoc::parse(&self.text()).unwrap().song(), &target(t))
    }
}

/// C1: with the studio closed, the producer added a placement by hand
/// between two of the run's writes to the same list. The run's second
/// write carried it in its old and new values, so it is not the run's:
/// the whole list is kept, nothing written.
#[test]
fn a_placement_added_by_hand_between_the_runs_writes_is_kept() {
    let f = closed();
    f.agent("r1", &[place("acid", "a1", 1, 1)]);
    f.hand(
        "  { at = 1, clip = \"a1\", bars = 1 },\n",
        "  { at = 1, clip = \"a1\", bars = 1 },\n  { at = 2, clip = \"a1\", bars = 1 }, # mine\n",
    );
    f.agent(
        "r1",
        &[Action::ResizePlacement {
            track: "acid".into(),
            at: 3,
            bars: 1,
        }],
    );
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let kept_at = |p: Value| kept_item("tracks.acid.play", p, None);
    assert_eq!(
        u.kept,
        vec![
            kept_at(placement(1, "a1", 1)),
            kept_at(placement(2, "a1", 1)),
            kept_at(placement(3, "a1", 1)),
            kept_at(placement(3, "a1", 2)),
        ]
    );
    assert_eq!(f.text(), text);
    assert_eq!(
        f.value("tracks.acid.play"),
        json!([
            placement(1, "a1", 1),
            placement(2, "a1", 1),
            placement(3, "a1", 1)
        ])
    );
}

/// C1: the same for a lane stretch the producer added by hand.
#[test]
fn a_stretch_added_by_hand_between_the_runs_writes_is_kept() {
    let f = closed();
    let param = "acid.acid303.cutoff";
    f.agent(
        "r1",
        &[Action::AddLane {
            param: param.into(),
            at: 1,
            bars: 1,
            points: vec![[0.0, 0.2], [1.0, 0.6]],
        }],
    );
    f.hand(
        "# acid enters\n",
        "# mine\n[[automation]]\nparam = \"acid.acid303.cutoff\"\nat = 2\nbars = 1\npoints = [[0, 0.5], [1, 0.5]]\n\n# acid enters\n",
    );
    f.agent(
        "r1",
        &[Action::SetLanePoints {
            param: param.into(),
            at: 1,
            points: vec![[0.0, 0.3], [1.0, 0.9]],
        }],
    );
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let kept_at = |p: Value| kept_item("automation.acid.acid303.cutoff", p, None);
    assert_eq!(
        u.kept,
        vec![
            kept_at(lane_stretch(1, 1, json!([[0, 0.3], [1, 0.9]]))),
            kept_at(lane_stretch(2, 1, json!([[0, 0.5], [1, 0.5]]))),
        ]
    );
    assert_eq!(f.text(), text);
    assert_eq!(
        f.value("automation.acid.acid303.cutoff"),
        json!([
            lane_stretch(1, 1, json!([[0, 0.3], [1, 0.9]])),
            lane_stretch(2, 1, json!([[0, 0.5], [1, 0.5]])),
            lane_stretch(3, 2, json!([[0, 0.1], [2, 0.8]])),
        ])
    );
}

/// C1: a leaf the producer changed by hand between two of the run's
/// writes is kept, never written back over their value or the run's
/// original one.
#[test]
fn a_value_changed_by_hand_between_the_runs_writes_is_kept() {
    let f = closed();
    f.agent("r1", &[cutoff(800.0)]);
    f.hand("\"acid303.cutoff\" = 800", "\"acid303.cutoff\" = 650");
    f.agent("r1", &[cutoff(900.0)]);
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.kept, vec![kept("tracks.acid.acid303.cutoff", None)]);
    assert_eq!(f.text(), text);
    assert_eq!(f.value("tracks.acid.acid303.cutoff"), json!(900));
}

/// Every kept entry's targets, sorted, and that each is kept by `by`.
fn kept_targets(u: &RunUndo, by: Option<Source>) -> Vec<String> {
    for k in &u.kept {
        assert_eq!((k.reason, k.by), (KeptReason::ChangedLater, by), "{k:?}");
    }
    let mut names: Vec<String> = u.kept.iter().map(|k| k.target.to_string()).collect();
    names.sort();
    names.dedup();
    names
}

/// I1: a renamed track the producer edited later is kept, and so is the
/// rest of the rename: the old name does not come back beside it.
#[test]
fn a_renamed_track_edited_later_keeps_the_whole_rename() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::RenameTrack {
            track: "acid".into(),
            to: "lead".into(),
        }],
    );
    s.human(set("tracks.lead.gain_db", json!(-3)));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let names = kept_targets(&u, Some(Source::Human));
    assert!(names.contains(&"tracks.acid".to_owned()), "{names:?}");
    assert!(names.contains(&"tracks.lead".to_owned()), "{names:?}");
    assert_eq!(s.text(), text);
    let tracks: Vec<&String> = s.core.song().tracks.keys().collect();
    assert_eq!(tracks, ["kick", "lead", "hat"]);
    assert_eq!(s.value("tracks.lead.gain_db"), json!(-3));
    assert_eq!(
        s.value("automation.lead.acid303.cutoff"),
        json!([lane_stretch(3, 2, json!([[0, 0.1], [2, 0.8]]))])
    );
}

/// I1: a renamed clip the producer edited later: the old clip does not
/// come back, and the placement still plays the edited one.
#[test]
fn a_renamed_clip_edited_later_keeps_the_whole_rename() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::RenameClip {
            clip: "a1".into(),
            to: "bass1".into(),
        }],
    );
    s.human(set("clips.bass1.steps", json!("C2 . C3 .")));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let names = kept_targets(&u, Some(Source::Human));
    assert!(names.contains(&"clips.a1".to_owned()), "{names:?}");
    assert!(names.contains(&"clips.bass1".to_owned()), "{names:?}");
    assert_eq!(s.text(), text);
    assert_eq!(s.value("clips.a1"), Value::Null);
    assert_eq!(
        s.value("tracks.acid.play"),
        json!([placement(3, "bass1", 2)])
    );
}

/// I1: a renamed device the producer edited later: its lane stays with
/// it, not taken out as the run's while the old one is refused.
#[test]
fn a_renamed_device_edited_later_keeps_the_whole_rename() {
    let mut s = open();
    s.agent(
        "r1",
        &[Action::RenameDevice {
            track: "acid".into(),
            device: "acid303".into(),
            to: "bass303".into(),
        }],
    );
    s.human(set("tracks.acid.bass303.cutoff", json!(700)));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let names = kept_targets(&u, Some(Source::Human));
    assert!(
        names.contains(&"tracks.acid.instrument".to_owned()),
        "{names:?}"
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.value("tracks.acid.bass303.cutoff"), json!(700));
    assert_eq!(
        s.value("automation.acid.bass303.cutoff"),
        json!([lane_stretch(3, 2, json!([[0, 0.1], [2, 0.8]]))])
    );
}

/// I1: a rename and a section edit in one write are one group: the
/// producer's later edit to the renamed track keeps both.
#[test]
fn a_rename_and_a_section_edit_in_one_write_go_back_together() {
    let mut s = open();
    s.agent(
        "r1",
        &[
            Action::RenameTrack {
                track: "acid".into(),
                to: "lead".into(),
            },
            Action::AddSection {
                name: "break".into(),
                bars: 2,
                index: Some(1),
            },
        ],
    );
    s.human(set("tracks.lead.gain_db", json!(-3)));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let names = kept_targets(&u, Some(Source::Human));
    for t in ["sections", "tracks.acid", "tracks.lead"] {
        assert!(names.contains(&t.to_owned()), "{t}: {names:?}");
    }
    assert_eq!(s.text(), text);
    let tracks: Vec<&String> = s.core.song().tracks.keys().collect();
    assert_eq!(tracks, ["kick", "lead", "hat"]);
}

// ----- review round 2: a rename whose new name was taken away later -----

fn rename_track(track: &str, to: &str) -> Action {
    Action::RenameTrack {
        track: track.into(),
        to: to.into(),
    }
}

/// C1 (p48): the producer renamed the run's new track name again. The new
/// name is gone as before the run, but the old one must not come back
/// beside the producer's track.
#[test]
fn a_rename_the_producer_renamed_again_is_kept_whole() {
    let mut s = open();
    s.agent("r1", &[rename_track("acid", "lead")]);
    s.human(rename_track("lead", "lead2"));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.already, vec![]);
    let names = kept_targets(&u, Some(Source::Human));
    for t in ["tracks.acid", "tracks.lead"] {
        assert!(names.contains(&t.to_owned()), "{t}: {names:?}");
    }
    assert_eq!(s.text(), text);
    let tracks: Vec<&String> = s.core.song().tracks.keys().collect();
    assert_eq!(tracks, ["kick", "lead2", "hat"]);

    // The same with the studio closed, the producer's rename logged.
    let f = closed();
    f.agent("r1", &[rename_track("acid", "lead")]);
    apply_to_file(&f.path, &[rename_track("lead", "lead2")], &Origin::human()).unwrap();
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let names = kept_targets(&u, Some(Source::Human));
    assert!(names.contains(&"tracks.acid".to_owned()), "{names:?}");
    assert_eq!(f.text(), text);
}

/// C1 (p49): the producer removed the run's renamed track. It does not
/// come back under its old name.
#[test]
fn a_rename_the_producer_removed_is_kept_whole() {
    let mut s = open();
    s.agent("r1", &[rename_track("acid", "lead")]);
    s.human(Action::RemoveTrack {
        track: "lead".into(),
    });
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.already, vec![]);
    let names = kept_targets(&u, Some(Source::Human));
    for t in ["tracks.acid", "tracks.lead"] {
        assert!(names.contains(&t.to_owned()), "{t}: {names:?}");
    }
    assert_eq!(s.text(), text);
    let tracks: Vec<&String> = s.core.song().tracks.keys().collect();
    assert_eq!(tracks, ["kick", "hat"]);
}

/// C1, a clip: an unplaced clip the run renamed and the producer renamed
/// again does not come back under its old name. (A placed clip is kept by
/// its placements already.)
#[test]
fn a_clip_rename_the_producer_renamed_again_is_kept_whole() {
    let rename = |clip: &str, to: &str| Action::RenameClip {
        clip: clip.into(),
        to: to.into(),
    };
    let mut s = open();
    s.human(Action::DuplicateClip {
        clip: "h".into(),
        to: "h2".into(),
    });
    s.agent("r1", &[rename("h2", "h3")]);
    s.human(rename("h3", "h4"));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let names = kept_targets(&u, Some(Source::Human));
    for t in ["clips.h2", "clips.h3"] {
        assert!(names.contains(&t.to_owned()), "{t}: {names:?}");
    }
    assert_eq!(s.text(), text);
    assert_eq!(s.value("clips.h2"), Value::Null);
}

// ----- review round 3: a removed track made again by hand -----

/// M1 (p20): with the studio closed, the run removed a track and the
/// producer added a track under its id by hand. The track is kept (the
/// log does not say who made it), and so is the removed track's lane: it
/// does not come back onto the producer's hand-made track.
#[test]
fn a_removed_tracks_lane_stays_off_a_track_made_by_hand() {
    let f = closed();
    f.agent(
        "r1",
        &[Action::RemoveTrack {
            track: "acid".into(),
        }],
    );
    f.hand(
        "[clips.k4]",
        "[tracks.acid]\ninstrument = \"acid303\"\n\n[clips.k4]",
    );
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let lane = lane_stretch(3, 2, json!([[0, 0.1], [2, 0.8]]));
    assert_eq!(
        u.kept,
        vec![
            kept_item("automation.acid.acid303.cutoff", lane, None),
            kept("tracks.acid", None),
        ]
    );
    assert_eq!(f.text(), text);
    assert_eq!(f.value("automation.acid.acid303.cutoff"), json!([]));
}

// ----- Plan 10: scenes -----

const SESSION: &str = include_str!("../../tune-song/tests/fixtures/session.toml");

/// A studio core on a temp copy of the session fixture (scenes `groove`
/// and `break`).
fn session() -> S {
    open_on(SESSION)
}

fn session_song() -> tune_song::Song {
    SongDoc::parse(SESSION).unwrap().song().clone()
}

fn add_scene(scene: &str, index: Option<usize>) -> Action {
    Action::AddScene {
        scene: scene.into(),
        index,
    }
}

fn move_scene(scene: &str, index: usize) -> Action {
    Action::MoveScene {
        scene: scene.into(),
        index,
    }
}

fn set_slot(scene: &str, track: &str, clip: Option<&str>) -> Action {
    Action::SetSlot {
        scene: scene.into(),
        track: track.into(),
        clip: clip.map(Into::into),
    }
}

impl S {
    /// The scene ids in row order.
    fn scenes(&self) -> Vec<String> {
        self.core.song().scenes.keys().cloned().collect()
    }
}

#[test]
fn scene_edits_revert() {
    let mut s = session();
    s.agent("r1", &[add_scene("jam", None)]);
    s.agent("r1", &[set_slot("jam", "acid", Some("a1"))]);
    s.agent("r1", &[move_scene("jam", 0)]);
    s.agent("r1", &[set_slot("groove", "hat", Some("h"))]);
    let run_state = s.text();

    let u = s.undo_run("r1");
    assert_eq!((u.kept, u.already), (vec![], vec![]));
    assert_eq!(
        sorted(u.reverted),
        ["scenes", "scenes.groove.slots.hat", "scenes.jam"]
    );
    assert_eq!(target_changes(&session_song(), s.core.song()), vec![]);
    assert_eq!(s.scenes(), ["groove", "break"]);
    assert_eq!(s.text(), SESSION);

    // One undo step: ⌘Z brings the run's state back.
    s.core.undo(Source::Human).unwrap();
    assert_eq!(s.text(), run_state);

    // A slot the run filled and emptied again in a scene it made folds
    // into the scene: no key left behind.
    let mut s = session();
    s.agent("r1", &[add_scene("jam", None)]);
    s.agent("r1", &[set_slot("jam", "acid", Some("a1"))]);
    s.agent("r1", &[set_slot("jam", "acid", None)]);
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(u.reverted, reverted(&["scenes.jam"]));
    assert_eq!(s.text(), SESSION);
}

#[test]
fn a_slot_changed_later_is_kept() {
    let mut s = session();
    s.agent(
        "r1",
        &[
            set_slot("groove", "kick", None),
            set("song.bpm", json!(130)),
        ],
    );
    s.human(set("scenes.groove.slots.kick", json!("h")));
    let u = s.undo_run("r1");
    assert_eq!(
        u.kept,
        vec![kept("scenes.groove.slots.kick", Some(Source::Human))]
    );
    assert_eq!(u.reverted, reverted(&["song.bpm"]));
    assert_eq!(s.value("scenes.groove.slots.kick"), json!("h"));
    assert_eq!(s.value("song.bpm"), json!(120));

    // Set back to its value before the run: already there.
    let mut s = session();
    s.agent(
        "r1",
        &[
            set_slot("groove", "kick", None),
            set("song.bpm", json!(130)),
        ],
    );
    s.human(set("scenes.groove.slots.kick", json!("k4")));
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(u.already, vec![target("scenes.groove.slots.kick")]);
    assert_eq!(u.reverted, reverted(&["song.bpm"]));
    assert_eq!(target_changes(&session_song(), s.core.song()), vec![]);
}

#[test]
fn a_scene_the_run_made_and_the_producer_filled_is_kept() {
    let mut s = session();
    s.agent("r1", &[add_scene("jam", None)]);
    s.human(set_slot("jam", "hat", Some("h")));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.kept, vec![kept("scenes.jam", Some(Source::Human))]);
    assert_eq!(s.text(), text);
    assert_eq!(s.scenes(), ["groove", "break", "jam"]);
    assert_eq!(s.value("scenes.jam.slots.hat"), json!("h"));
}

#[test]
fn a_clip_the_run_made_and_the_producer_slotted_is_kept() {
    let mut s = session();
    s.agent(
        "r1",
        &[Action::AddClip {
            clip: "lead".into(),
            kind: "notes".into(),
            steps: None,
            bars: Some(1),
            notes: None,
        }],
    );
    s.human(set_slot("groove", "acid", Some("lead")));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        u.kept,
        vec![KeptTarget {
            target: target("clips.lead"),
            item: None,
            reason: KeptReason::StillUsed,
            by: None,
            // The validator says why: the slot names the clip.
            message: Some("unknown clip `lead`".into()),
        }]
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.value("scenes.groove.slots.acid"), json!("lead"));
}

#[test]
fn rename_scene_goes_back_whole() {
    let rename = Action::RenameScene {
        scene: "groove".into(),
        to: "drop".into(),
    };
    let mut s = session();
    s.agent("r1", std::slice::from_ref(&rename));
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(sorted(u.reverted), ["scenes.drop", "scenes.groove"]);
    assert_eq!(target_changes(&session_song(), s.core.song()), vec![]);
    // Back in place with its slots and the comment above it.
    assert_eq!(s.text(), SESSION);

    let mut s = session();
    s.agent("r1", &[rename]);
    s.human(set_slot("drop", "hat", Some("h")));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        kept_targets(&u, Some(Source::Human)),
        ["scenes.drop", "scenes.groove"]
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.scenes(), ["drop", "break"]);
}

#[test]
fn remove_track_reverts_with_its_slots() {
    let mut s = session();
    s.agent(
        "r1",
        &[Action::RemoveTrack {
            track: "acid".into(),
        }],
    );
    assert_eq!(s.value("scenes.groove.slots.acid"), Value::Null);
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(s.value("scenes.groove.slots.acid"), json!("a1"));
    assert_eq!(target_changes(&session_song(), s.core.song()), vec![]);
}

#[test]
fn rename_track_takes_its_slots_back() {
    let mut s = session();
    s.agent(
        "r1",
        &[Action::RenameTrack {
            track: "acid".into(),
            to: "bass".into(),
        }],
    );
    assert_eq!(s.value("scenes.groove.slots.bass"), json!("a1"));
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(s.value("scenes.groove.slots.acid"), json!("a1"));
    assert_eq!(s.value("scenes.groove.slots.bass"), Value::Null);
    assert_eq!(target_changes(&session_song(), s.core.song()), vec![]);
}

#[test]
fn the_order_is_kept_when_scenes_changed_since() {
    let mut s = session();
    s.agent("r1", &[move_scene("break", 0)]);
    s.human(add_scene("jam", None));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.kept, vec![kept("scenes", Some(Source::Human))]);
    assert_eq!(s.text(), text);
    assert_eq!(s.scenes(), ["break", "groove", "jam"]);

    // A slot edit is no change to the order: the move goes back.
    let mut s = session();
    s.agent("r1", &[move_scene("break", 0)]);
    s.human(set_slot("groove", "hat", Some("h")));
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(u.reverted, reverted(&["scenes"]));
    assert_eq!(s.scenes(), ["groove", "break"]);
    assert_eq!(s.value("scenes.groove.slots.hat"), json!("h"));
}

// ----- Plan 10: scenes, adversarial -----

/// Protects the producer's removal of a scene: a slot the run set (here,
/// emptied) in it does not bring the scene back.
#[test]
fn a_slot_in_a_scene_the_producer_removed_stays_gone() {
    let mut s = session();
    s.agent("r1", &[set_slot("groove", "kick", None)]);
    s.human(Action::RemoveScene {
        scene: "groove".into(),
    });
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        u.kept,
        vec![kept("scenes.groove.slots.kick", Some(Source::Human))]
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.scenes(), ["break"]);

    // With the studio closed and the scene's table deleted by hand, nothing
    // in the log says so; the slot's scene is not there to take it.
    let f = closed_on(SESSION);
    f.agent("r1", &[set_slot("groove", "kick", None)]);
    f.hand(
        "# jam rows\n[scenes.groove]\nslots = { acid = \"a1\" }\n\n",
        "",
    );
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        u.kept
            .iter()
            .map(|k| (k.target.to_string(), k.reason))
            .collect::<Vec<_>>(),
        [("scenes.groove.slots.kick".to_owned(), KeptReason::StillUsed)]
    );
    assert_eq!(f.text(), text);
    assert_eq!(f.value("scenes.groove"), Value::Null);
}

/// Protects the producer's move of a scene the run made: undoing the run
/// would delete the row they placed, so it is kept, slot and all.
#[test]
fn a_scene_the_run_made_and_the_producer_moved_is_kept() {
    let mut s = session();
    s.agent("r1", &[add_scene("jam", None)]);
    s.agent("r1", &[set_slot("jam", "acid", Some("a1"))]);
    s.human(move_scene("jam", 0));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.kept, vec![kept("scenes.jam", Some(Source::Human))]);
    assert_eq!(s.text(), text);
    assert_eq!(s.scenes(), ["jam", "groove", "break"]);
    assert_eq!(s.value("scenes.jam.slots.acid"), json!("a1"));
}

/// Protects a scene the producer made under the id of one the run
/// removed: the run's old scene does not replace it.
#[test]
fn a_scene_the_producer_made_again_is_kept() {
    let mut s = session();
    s.agent(
        "r1",
        &[Action::RemoveScene {
            scene: "break".into(),
        }],
    );
    s.human(add_scene("break", Some(0)));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.kept, vec![kept("scenes.break", Some(Source::Human))]);
    assert_eq!(s.text(), text);
    assert_eq!(s.scenes(), ["break", "groove"]);
    assert_eq!(s.value("scenes.break"), json!({ "index": 0, "slots": {} }));
}

/// Protects a slot the producer set by hand with the studio closed,
/// between two of the run's writes to it: the run's second write carried
/// their value, so neither it nor the run's first value is written back.
#[test]
fn a_slot_changed_by_hand_between_the_runs_writes_is_kept() {
    let f = closed_on(SESSION);
    f.agent("r1", &[set_slot("groove", "kick", Some("h"))]);
    f.hand(
        "slots = { kick = \"h\", acid = \"a1\" }",
        "slots = { kick = \"k2\", acid = \"a1\" }\n\n[clips.k2]\nkind = \"drum\"\nsteps = \"x.x.x.x.x.x.x.x.\"",
    );
    f.agent("r1", &[set_slot("groove", "kick", None)]);
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.kept, vec![kept("scenes.groove.slots.kick", None)]);
    assert_eq!(f.text(), text);
    assert_eq!(f.value("scenes.groove.slots.kick"), Value::Null);

    // A slot the producer added by hand to a scene the run made, before the
    // run filled another of its slots: the scene is not the run's alone.
    let f = closed_on(SESSION);
    f.agent("r1", &[add_scene("jam", None)]);
    f.hand("[scenes.jam]\n", "[scenes.jam]\nslots = { hat = \"h\" }\n");
    f.agent("r1", &[set_slot("jam", "acid", Some("a1"))]);
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.kept, vec![kept("scenes.jam", None)]);
    assert_eq!(f.text(), text);
}

/// Protects a track the producer made under the id of one the run
/// removed: the old track's slots do not come back to it.
#[test]
fn a_track_the_producer_made_again_gets_no_old_slots() {
    let mut s = session();
    s.agent(
        "r1",
        &[Action::RemoveTrack {
            track: "acid".into(),
        }],
    );
    s.human(Action::AddTrack {
        track: "acid".into(),
        instrument: "acid303".into(),
    });
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let names = kept_targets(&u, Some(Source::Human));
    assert!(
        names.contains(&"scenes.groove.slots.acid".to_owned()),
        "{names:?}"
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.value("scenes.groove.slots.acid"), Value::Null);
}

/// The producer's scene above it moved the run's scene down a row. Its
/// row counts (ruling I2): it is not where the run left it, so it is kept,
/// by the producer, rather than guessed about (an accepted over-keep).
#[test]
fn a_scene_the_run_made_is_kept_when_a_scene_was_added_above() {
    let mut s = session();
    s.agent("r1", &[add_scene("jam", None)]);
    s.human(add_scene("intro", Some(0)));
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![kept("scenes.jam", Some(Source::Human))]);
    assert_eq!(u.reverted, vec![]);
    assert_eq!(s.scenes(), ["intro", "groove", "break", "jam"]);
}

// ----- Plan 10: slots and scenes whose clip or track was rebuilt -----

fn add_clip_a1_again() -> Action {
    Action::AddClip {
        clip: "a1".into(),
        kind: "acid".into(),
        steps: Some("C3 . . .".into()),
        bars: None,
        notes: None,
    }
}

/// Protects a clip the producer made under the id of one the run removed:
/// the run's old slot does not come back to play it.
#[test]
fn a_clip_the_producer_made_again_gets_no_old_slot() {
    let mut s = session();
    s.agent("r1", &[Action::RemoveClip { clip: "a1".into() }]);
    s.human(add_clip_a1_again());
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let names = kept_targets(&u, Some(Source::Human));
    assert!(
        names.contains(&"scenes.groove.slots.acid".to_owned()),
        "{names:?}"
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.value("scenes.groove.slots.acid"), Value::Null);
}

/// Protects the producer's rebuild of a clip a slot the run emptied held:
/// the slot does not come back to name their new clip.
#[test]
fn a_slot_whose_clip_the_producer_rebuilt_is_kept() {
    let mut s = session();
    s.agent("r1", &[set_slot("groove", "acid", None)]);
    s.human(Action::RemoveClip { clip: "a1".into() });
    s.human(add_clip_a1_again());
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        u.kept,
        vec![kept("scenes.groove.slots.acid", Some(Source::Human))]
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.value("scenes.groove.slots.acid"), Value::Null);
}

/// Protects a track the producer made again under the id of one a scene
/// the run removed has a slot for: the scene does not come back with a
/// slot on their new track.
#[test]
fn a_scene_whose_track_the_producer_rebuilt_is_kept() {
    let mut s = session();
    s.agent(
        "r1",
        &[Action::RemoveScene {
            scene: "groove".into(),
        }],
    );
    s.human(Action::RemoveTrack {
        track: "acid".into(),
    });
    s.human(Action::AddTrack {
        track: "acid".into(),
        instrument: "acid303".into(),
    });
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.kept, vec![kept("scenes.groove", Some(Source::Human))]);
    assert_eq!(s.text(), text);
    assert_eq!(s.scenes(), ["break"]);
    assert_eq!(s.value("scenes.groove"), Value::Null);
}

// ----- Plan 10: the scene order around the run's own scenes -----

/// The session fixture with a third, empty scene `drop` after `break`.
fn three_scenes() -> String {
    SESSION.replacen("[[sections]]", "[scenes.drop]\n\n[[sections]]", 1)
}

fn remove_scene(scene: &str) -> Action {
    Action::RemoveScene {
        scene: scene.into(),
    }
}

/// The run's own adds and removes around its moves are the run's: the
/// order goes back with them, nothing kept, nobody blamed.
#[test]
fn the_order_goes_back_around_the_runs_own_scenes() {
    let cases: [&[Action]; 4] = [
        &[
            add_scene("jam", None),
            move_scene("jam", 0),
            set_slot("jam", "acid", Some("a1")),
        ],
        &[
            move_scene("break", 0),
            add_scene("jam", None),
            move_scene("jam", 0),
        ],
        &[
            add_scene("jam", None),
            move_scene("jam", 0),
            remove_scene("jam"),
        ],
        &[move_scene("break", 0), remove_scene("groove")],
    ];
    for actions in cases {
        let mut s = session();
        for a in actions {
            s.agent("r1", std::slice::from_ref(a));
        }
        let u = s.undo_run("r1");
        assert_eq!(u.kept, vec![], "{actions:?}");
        assert_eq!(s.scenes(), ["groove", "break"], "{actions:?}");
        assert_eq!(
            target_changes(&session_song(), s.core.song()),
            vec![],
            "{actions:?}"
        );
    }

    // A scene the run removed after moving it comes back before the order
    // is put back, so it lands in its place.
    let three = three_scenes();
    let mut s = open_on(&three);
    for a in [
        move_scene("drop", 0),
        remove_scene("drop"),
        move_scene("break", 0),
    ] {
        s.agent("r1", &[a]);
    }
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(s.scenes(), ["groove", "break", "drop"]);
}

/// Protects a reorder by hand with the studio closed between two of the
/// run's writes: the run's later write carried it, so the order is kept
/// with `by` null, and so are the scenes the run added or removed in it.
#[test]
fn a_scene_reorder_by_hand_between_the_runs_writes_is_kept() {
    // The producer moved `drop` down a row; the run then removed it.
    let f = closed_on(&three_scenes());
    f.agent("r1", &[move_scene("drop", 0)]);
    f.hand("[scenes.drop]\n\n# jam rows\n", "# jam rows\n");
    f.hand("# hats alone\n", "[scenes.drop]\n\n# hats alone\n");
    f.agent("r1", &[remove_scene("drop")]);
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(kept_targets(&u, None), ["scenes", "scenes.drop"]);
    assert_eq!(f.text(), text);

    // The producer swapped the rows back; the run then added a scene.
    let f = closed_on(SESSION);
    f.agent("r1", &[move_scene("break", 0)]);
    let block = "# hats alone\n[scenes.break]\nslots = { hat = \"h\" }\n\n";
    f.hand(block, "");
    f.hand("[[sections]]", &format!("{block}[[sections]]"));
    f.agent("r1", &[add_scene("jam", None)]);
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(kept_targets(&u, None), ["scenes", "scenes.jam"]);
    assert_eq!(f.text(), text);
}

/// Protects the producer's move of a scene through the studio between two
/// of the run's writes: the order and the run's scene in it are kept, by
/// the producer.
#[test]
fn a_scene_move_through_the_studio_between_the_runs_writes_is_kept() {
    let mut s = session();
    s.agent("r1", &[move_scene("break", 0)]);
    s.human(move_scene("groove", 0));
    s.agent("r1", &[add_scene("jam", None)]);
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        kept_targets(&u, Some(Source::Human)),
        ["scenes", "scenes.jam"]
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.scenes(), ["groove", "break", "jam"]);

    // The same with a move of the run's after its add: the add is the
    // run's next change to the order after the producer's move, and it
    // gives no value of the order to go back to, so the order is kept.
    let mut s = session();
    s.agent("r1", &[move_scene("break", 0)]);
    s.human(move_scene("groove", 0));
    s.agent("r1", &[add_scene("jam", None)]);
    s.agent("r1", &[move_scene("jam", 0)]);
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        kept_targets(&u, Some(Source::Human)),
        ["scenes", "scenes.jam"]
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.scenes(), ["jam", "groove", "break"]);
}

// ----- review round 2: scenes -----

fn rename_scene(scene: &str, to: &str) -> Action {
    Action::RenameScene {
        scene: scene.into(),
        to: to.into(),
    }
}

/// C1 (p04): the producer renamed the run's renamed scene again. The old
/// name does not come back beside theirs.
#[test]
fn a_scene_rename_the_producer_renamed_again_is_kept_whole() {
    let mut s = session();
    s.agent("r1", &[rename_scene("groove", "drop")]);
    s.human(rename_scene("drop", "drop2"));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!((&u.reverted, &u.already), (&vec![], &vec![]));
    assert_eq!(
        kept_targets(&u, Some(Source::Human)),
        ["scenes.drop", "scenes.groove"]
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.scenes(), ["drop2", "break"]);
}

/// C1 (p05): the producer removed the run's renamed scene. It does not
/// come back under its old name, with the studio open or closed.
#[test]
fn a_scene_rename_the_producer_removed_is_kept_whole() {
    let mut s = session();
    s.agent("r1", &[rename_scene("groove", "drop")]);
    s.human(remove_scene("drop"));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!((&u.reverted, &u.already), (&vec![], &vec![]));
    assert_eq!(
        kept_targets(&u, Some(Source::Human)),
        ["scenes.drop", "scenes.groove"]
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.scenes(), ["break"]);

    // Studio closed, the scene's table deleted by hand: nothing logged.
    let f = closed_on(SESSION);
    f.agent("r1", &[rename_scene("groove", "drop")]);
    f.hand(
        "# jam rows\n[scenes.drop]\nslots = { kick = \"k4\", acid = \"a1\" }\n\n",
        "",
    );
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!((&u.reverted, &u.already), (&vec![], &vec![]));
    assert_eq!(kept_targets(&u, None), ["scenes.drop", "scenes.groove"]);
    assert_eq!(f.text(), text);
}

/// I1: a scene rename goes back by renaming in place, wherever the scene
/// sits now and whatever the run did to its slots, so its table keeps the
/// comment above it.
#[test]
fn a_scene_rename_goes_back_in_place() {
    // The run moved the renamed scene down a row.
    let mut s = session();
    s.agent("r1", &[rename_scene("groove", "drop")]);
    s.agent("r1", &[move_scene("drop", 1)]);
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(s.scenes(), ["groove", "break"]);
    assert_eq!(s.text(), SESSION);

    // The run filled a slot of the renamed scene.
    let mut s = session();
    s.agent("r1", &[rename_scene("groove", "drop")]);
    s.agent("r1", &[set_slot("drop", "hat", Some("h"))]);
    let u = s.undo_run("r1");
    assert_eq!(u.kept, vec![]);
    assert_eq!(s.text(), SESSION);
}

/// I2 (p31): with the studio closed, the producer moved the run's new
/// scene by hand after the run. Nothing in the log says so, but the scene
/// is not where the run left it: it is kept, not deleted with their move.
#[test]
fn a_scene_moved_by_hand_after_the_run_is_kept() {
    let f = closed_on(SESSION);
    f.agent("r1", &[add_scene("jam", None)]);
    f.hand("[scenes.jam]\n", "");
    f.hand("# jam rows\n", "[scenes.jam]\n\n# jam rows\n");
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(u.kept, vec![kept("scenes.jam", None)]);
    assert_eq!(f.text(), text);

    // p51: the run renamed a scene, the producer moved it below `break`
    // by hand. The rename is kept whole, the producer's row order stays.
    let f = closed_on(SESSION);
    f.agent("r1", &[rename_scene("groove", "drop")]);
    let groove = "# jam rows\n[scenes.drop]\nslots = { kick = \"k4\", acid = \"a1\" }\n\n";
    f.hand(groove, "");
    f.hand("[[sections]]", &format!("{groove}[[sections]]"));
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(kept_targets(&u, None), ["scenes.drop", "scenes.groove"]);
    assert_eq!(f.text(), text);
}

/// I2 (p57): the producer added a scene above the run's renamed one. Its
/// row moved down: kept rather than guessed, by the producer (an
/// accepted over-keep), and their top row stays on top.
#[test]
fn a_scene_shifted_by_a_scene_added_above_is_kept() {
    let mut s = session();
    s.agent("r1", &[rename_scene("groove", "drop")]);
    s.human(add_scene("intro", Some(0)));
    let text = s.text();
    let u = s.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    assert_eq!(
        kept_targets(&u, Some(Source::Human)),
        ["scenes.drop", "scenes.groove"]
    );
    assert_eq!(s.text(), text);
    assert_eq!(s.scenes(), ["intro", "drop", "break"]);
}

// ----- review round 3: scenes -----

/// M1 (p19): with the studio closed, the run removed a track and the
/// producer made a track under its id by hand, with a new scene slotting
/// it. The run's old slot does not come back onto their track.
#[test]
fn a_removed_tracks_slot_stays_off_a_track_made_by_hand() {
    let f = closed_on(SESSION);
    f.agent(
        "r1",
        &[Action::RemoveTrack {
            track: "acid".into(),
        }],
    );
    f.hand(
        "[clips.k4]",
        "[tracks.acid]\ninstrument = \"acid303\"\n\n[clips.k4]",
    );
    f.hand(
        "[[sections]]",
        "[scenes.jam]\nslots = { acid = \"a1\" }\n\n[[sections]]",
    );
    let text = f.text();
    let u = f.undo_run("r1");
    assert_eq!(u.reverted, vec![]);
    let names = kept_targets(&u, None);
    for t in ["scenes.groove.slots.acid", "tracks.acid"] {
        assert!(names.contains(&t.to_owned()), "{t}: {names:?}");
    }
    assert_eq!(f.text(), text);
    assert_eq!(f.value("scenes.groove.slots.acid"), Value::Null);
    assert_eq!(f.value("scenes.jam.slots.acid"), json!("a1"));
}

/// M3: an order put that changes nothing is not listed as reverted. The
/// run added a scene, moved it to the top and removed it again: the order
/// is back already.
#[test]
fn an_order_already_back_is_not_reverted() {
    let mut s = session();
    s.agent("r1", &[add_scene("jam", None)]);
    s.agent("r1", &[move_scene("jam", 0)]);
    s.agent("r1", &[remove_scene("jam")]);
    let u = s.undo_run("r1");
    assert_eq!((&u.reverted, &u.kept), (&vec![], &vec![]));
    let mut already: Vec<String> = u.already.iter().map(Target::to_string).collect();
    already.sort();
    assert_eq!(already, ["scenes", "scenes.jam"]);
    assert_eq!(s.scenes(), ["groove", "break"]);
}
