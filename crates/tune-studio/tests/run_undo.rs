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
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.toml");
    fs::write(&path, FIXTURE).unwrap();
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
