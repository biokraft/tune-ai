//! Undo targets: the song as targets, the changes between two songs, `put`
//! writing a target back, and the one action record each write logs.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tune_dsp::acid303::ACID303_PARAMS;
use tune_studio::{
    Action, ActionRecord, EditEntry, EditError, EditLog, Origin, RunTag, SongDoc, SongPath, Source,
    StudioCore, Target, TargetChange, apply_to_file, target_changes, target_value,
};

const FIXTURE: &str = include_str!("fixtures/commented.toml");

fn fixture() -> SongDoc {
    SongDoc::parse(FIXTURE).expect("the fixture parses")
}

fn target(s: &str) -> Target {
    s.parse().unwrap_or_else(|e| panic!("{s}: {e}"))
}

fn names(changes: &[TargetChange]) -> Vec<String> {
    changes.iter().map(|c| c.target.to_string()).collect()
}

#[test]
fn value_changes_are_value_targets() {
    let old = fixture();
    let mut new = old.clone();
    let path: SongPath = "tracks.acid.acid303.cutoff".parse().unwrap();
    new.set(&path, &json!(820)).unwrap();
    assert_eq!(
        target_changes(old.song(), new.song()),
        vec![TargetChange {
            target: target("tracks.acid.acid303.cutoff"),
            old: json!(500),
            new: json!(820),
        }]
    );
}

#[test]
fn a_new_track_is_one_target() {
    let old = fixture();
    let mut new = old.clone();
    new.add_track("bass", "acid303").unwrap();
    let changes = target_changes(old.song(), new.song());
    assert_eq!(names(&changes), ["tracks.bass"]);
    let c = &changes[0];
    assert_eq!(c.old, Value::Null);
    let default = ACID303_PARAMS
        .iter()
        .find(|p| p.id == "acid303.cutoff")
        .unwrap()
        .default;
    assert_eq!(c.new["index"], 3);
    assert_eq!(c.new["instrument"]["type"], "acid303");
    assert_eq!(
        c.new["instrument"]["params"]["cutoff"].as_f64(),
        Some(default)
    );
    assert_eq!(c.new["gain_db"], 0);
    assert_eq!(c.new["play"], json!([]));
    assert_eq!(target_value(new.song(), &target("tracks.bass")), c.new);
}

#[test]
fn an_added_effect_is_an_fx_target() {
    let old = fixture();
    let mut new = old.clone();
    new.add_effect("acid", "eq", None, None).unwrap();
    let changes = target_changes(old.song(), new.song());
    assert_eq!(names(&changes), ["tracks.acid.fx"]);
    let c = &changes[0];
    let ids = |v: &Value| -> Vec<(Value, Value)> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|d| (d["id"].clone(), d["type"].clone()))
            .collect()
    };
    assert_eq!(ids(&c.old), [(json!("dist"), json!("distortion"))]);
    assert_eq!(
        ids(&c.new),
        [
            (json!("dist"), json!("distortion")),
            (json!("eq"), json!("eq"))
        ]
    );
    // Full devices: settings and every param, defaults filled in.
    assert_eq!(c.old[0]["settings"], json!({ "mode": "soft" }));
    assert_eq!(c.old[0]["params"]["drive"], 12);
    assert!(c.new[1]["params"]["low_cut"].is_number(), "{}", c.new);
}

#[test]
fn a_section_op_touches_sections_plays_and_lanes() {
    let old = fixture();
    let mut new = old.clone();
    new.duplicate_section(1).unwrap();
    let changes = target_changes(old.song(), new.song());
    assert_eq!(
        names(&changes),
        [
            "sections",
            "tracks.kick.play",
            "tracks.acid.play",
            "tracks.hat.play",
            "automation.acid.acid303.cutoff",
        ]
    );
    assert_eq!(
        changes[0].new,
        json!([
            { "name": "a", "bars": 2 },
            { "name": "b", "bars": 2 },
            { "name": "b", "bars": 2 },
        ])
    );
    assert_eq!(
        changes[2].new,
        json!([
            { "at": 3, "clip": "a1", "bars": 2, "offset": 0 },
            { "at": 5, "clip": "a1", "bars": 2, "offset": 0 },
        ])
    );
    assert_eq!(
        changes[4].new,
        json!([
            { "at": 3, "bars": 2, "points": [[0, 0.1], [2, 0.8]] },
            { "at": 5, "bars": 2, "points": [[0, 0.1], [2, 0.8]] },
        ])
    );
}

/// The stretches of one param are a set: their order in the file is no
/// change.
#[test]
fn a_stretch_reorder_is_no_change() {
    let extra = "\n[[automation]]\nparam = \"acid.acid303.cutoff\"\nat = 1\nbars = 2\n\
                 points = [[0, 0.5], [2, 0.5]]\n";
    let last = FIXTURE.find("# the filter opens").unwrap();
    let after = SongDoc::parse(&format!("{FIXTURE}{extra}")).unwrap();
    let before = SongDoc::parse(&format!(
        "{}{}\n{}",
        &FIXTURE[..last],
        extra.trim_start(),
        &FIXTURE[last..]
    ))
    .unwrap();
    assert_ne!(before.song().automation, after.song().automation);
    assert_eq!(target_changes(before.song(), after.song()), vec![]);
}

#[test]
fn targets_round_trip_as_strings() {
    let cases = [
        ("song.bpm", Target::Value(SongPath::Bpm)),
        (
            "tracks.acid.acid303.cutoff",
            Target::Value("tracks.acid.acid303.cutoff".parse().unwrap()),
        ),
        ("tracks.acid", Target::Track("acid".into())),
        ("tracks.acid.instrument", Target::Instrument("acid".into())),
        ("tracks.acid.fx", Target::Fx("acid".into())),
        ("clips.a1", Target::Clip("a1".into())),
        ("tracks.acid.play", Target::Play("acid".into())),
        (
            "automation.acid.acid303.cutoff",
            Target::Lanes {
                track: "acid".into(),
                param: "acid303.cutoff".into(),
            },
        ),
        (
            "automation.mix.reverb.size",
            Target::Lanes {
                track: "mix".into(),
                param: "reverb.size".into(),
            },
        ),
        ("sections", Target::Sections),
    ];
    for (text, want) in cases {
        let got: Target = text.parse().unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(got, want, "{text}");
        assert_eq!(got.to_string(), text);
        assert_eq!(serde_json::to_value(&got).unwrap(), json!(text));
        assert_eq!(serde_json::from_value::<Target>(json!(text)).unwrap(), want);
    }
    for bad in [
        "",
        "tracks.",
        "clips.",
        "automation.acid",
        "automation.acid.",
        "automation..cutoff",
        "sections[0].bars",
        "song.version",
    ] {
        assert!(bad.parse::<Target>().is_err(), "{bad:?} should not parse");
    }
}

/// An edit for the `put` cases: what it does to a document.
type Edit = fn(&mut SongDoc) -> Result<(), EditError>;

fn nothing(_: &mut SongDoc) -> Result<(), EditError> {
    Ok(())
}

/// Puts every change's `old` back, newest first; a refused put is tried
/// once more after the others.
fn restore(doc: &mut SongDoc, changes: &[TargetChange], case: &str) {
    let mut refused = Vec::new();
    for c in changes.iter().rev() {
        if doc.put(&c.target, &c.old).is_err() {
            refused.push(c);
        }
    }
    for c in refused {
        doc.put(&c.target, &c.old)
            .unwrap_or_else(|e| panic!("{case}: put {} refused twice: {e}", c.target));
    }
}

#[test]
fn put_restores_each_kind() {
    let cases: Vec<(&str, Edit, Edit)> = vec![
        ("add_track", nothing, |d| d.add_track("bass", "acid303")),
        ("remove_track", nothing, |d| {
            d.remove_track("acid").map(drop)
        }),
        ("remove_kick", nothing, |d| d.remove_track("kick").map(drop)),
        ("rename_track", nothing, |d| d.rename_track("acid", "lead")),
        ("add_effect", nothing, |d| {
            d.add_effect("acid", "eq", None, Some(0)).map(drop)
        }),
        ("remove_effect", nothing, |d| {
            d.remove_effect("acid", "dist").map(drop)
        }),
        (
            "move_effect",
            |d| d.add_effect("acid", "eq", None, None).map(drop),
            |d| d.move_effect("acid", "eq", 0),
        ),
        ("rename_fx", nothing, |d| {
            d.rename_device("acid", "dist", "drive")
        }),
        ("rename_instrument", nothing, |d| {
            d.rename_device("acid", "acid303", "bass303")
        }),
        ("set_instrument", nothing, |d| {
            d.set_instrument("acid", "kick").map(drop)
        }),
        ("add_clip", nothing, |d| {
            let notes = json!([{ "start": 0, "pitch": "C3", "len": 2 }]);
            d.add_clip("lead", "notes", None, Some(1), Some(&notes))
        }),
        ("remove_clip", nothing, |d| d.remove_clip("a1").map(drop)),
        ("rename_clip", nothing, |d| d.rename_clip("a1", "b1")),
        ("duplicate_clip", nothing, |d| d.duplicate_clip("h", "h2")),
        ("add_placement", nothing, |d| {
            d.add_placement("acid", "a1", 0, 2, 0)
        }),
        ("move_placement", nothing, |d| {
            d.move_placement("acid", 2, 0)
        }),
        ("resize_placement", nothing, |d| {
            d.resize_placement("acid", 2, 1)
        }),
        ("remove_placement", nothing, |d| {
            d.remove_placement("kick", 2).map(drop)
        }),
        ("add_section", nothing, |d| d.insert_section(1, "break", 2)),
        ("rename_section", nothing, |d| {
            d.set(&SongPath::SectionName(1), &json!("drop"))
        }),
        ("duplicate_section", nothing, |d| d.duplicate_section(1)),
        ("delete_section", nothing, |d| d.delete_section(0)),
        ("move_section", nothing, |d| d.move_section(1, 0)),
        ("add_lane", nothing, |d| {
            d.add_lane("mix.reverb.size", 0, 2, vec![(0.0, 0.2), (2.0, 0.6)])
        }),
        ("remove_lane", nothing, |d| {
            d.remove_lane("acid.acid303.cutoff", 2).map(drop)
        }),
        ("set_lane_points", nothing, |d| {
            d.set_lane_points("acid.acid303.cutoff", 2, vec![(0.0, 0.3), (2.0, 0.9)])
        }),
        // Two stretches of one param come back in one put.
        (
            "remove_two_stretches",
            |d| d.add_lane("acid.acid303.cutoff", 0, 2, vec![(0.0, 0.5), (2.0, 0.5)]),
            |d| d.set_instrument("acid", "kick").map(drop),
        ),
    ];
    restores(FIXTURE, cases);
}

/// Each case's edit on `text` (after its setup), then every change's
/// `old` put back as [`restore`] does, gives the song before the edit, with
/// its tracks in their order.
fn restores(text: &str, cases: Vec<(&str, Edit, Edit)>) {
    for (case, setup, edit) in cases {
        let mut base = SongDoc::parse(text).expect("parses");
        setup(&mut base).unwrap_or_else(|e| panic!("{case}: setup: {e}"));
        let mut doc = base.clone();
        edit(&mut doc).unwrap_or_else(|e| panic!("{case}: {e}"));
        let changes = target_changes(base.song(), doc.song());
        assert!(!changes.is_empty(), "{case}: the edit changed nothing");
        restore(&mut doc, &changes, case);
        assert_eq!(
            target_changes(base.song(), doc.song()),
            vec![],
            "{case}: restored text:\n{}",
            doc.text()
        );
        // A track comes back where it was.
        let order = |d: &SongDoc| d.song().tracks.keys().cloned().collect::<Vec<_>>();
        assert_eq!(order(&doc), order(&base), "{case}");
    }
}

/// The same for a file written with inline tables and arrays.
#[test]
fn put_restores_inline_forms() {
    let text = "sections = [{ name = \"a\", bars = 2 }, { name = \"b\", bars = 2 }]\n\
        automation = [{ param = \"kick.kick.decay\", at = 3, bars = 2, points = [[0, 0], [2, 1]] }]\n\n\
        [song]\nversion = 2\nname = \"t\"\nbpm = 120\n\n\
        [tracks]\nkick = { instrument = \"kick\", play = [{ at = 1, clip = \"k4\", bars = 2 }] }\n\
        hat = { instrument = \"hat_closed\" }\n\n\
        [clips]\nk4 = { kind = \"drum\", steps = \"x...x...x...X...\" }\n";
    let cases: Vec<(&str, Edit, Edit)> = vec![
        ("remove_track", nothing, |d| {
            d.remove_track("kick").map(drop)
        }),
        ("remove_clip", nothing, |d| d.remove_clip("k4").map(drop)),
        ("delete_section", nothing, |d| d.delete_section(0)),
        ("add_section", nothing, |d| d.insert_section(0, "intro", 1)),
        ("remove_lane", nothing, |d| {
            d.remove_lane("kick.kick.decay", 2).map(drop)
        }),
        // A chain of two comes back on a track with no `fx`.
        (
            "remove_effects",
            |d| {
                d.add_effect("hat", "eq", None, None)?;
                d.add_effect("hat", "distortion", None, None).map(drop)
            },
            |d| {
                d.remove_effect("hat", "eq")?;
                d.remove_effect("hat", "dist").map(drop)
            },
        ),
    ];
    restores(text, cases);
}

/// A put of what is already there writes nothing.
#[test]
fn put_of_the_current_value_changes_nothing() {
    let mut doc = fixture();
    for t in [
        "tracks.acid",
        "tracks.acid.instrument",
        "tracks.acid.fx",
        "clips.a1",
        "tracks.kick.play",
        "automation.acid.acid303.cutoff",
        "sections",
    ] {
        let t = target(t);
        let value = target_value(doc.song(), &t);
        doc.put(&t, &value).unwrap_or_else(|e| panic!("{t}: {e}"));
        assert_eq!(doc.text(), FIXTURE, "{t}");
    }
}

/// A temp dir holding a copy of the commented fixture as `song.toml`.
struct Song {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn song() -> Song {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.toml");
    fs::write(&path, FIXTURE).unwrap();
    Song { _dir: dir, path }
}

fn records(s: &Song) -> Vec<ActionRecord> {
    EditLog::for_song(&s.path).read_records().unwrap()
}

fn entries(s: &Song) -> Vec<EditEntry> {
    EditLog::for_song(&s.path).read_since(None).unwrap()
}

/// The log's lines that are entries (not records), parsed.
fn entry_lines(s: &Song) -> Vec<EditEntry> {
    let text = fs::read_to_string(s.path.with_file_name("edits.jsonl")).unwrap();
    text.lines()
        .filter(|l| !l.contains("\"record\":\"action\""))
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn summary(r: &ActionRecord) -> (Source, Vec<String>, Option<String>, Vec<String>) {
    (
        r.source,
        r.actions.clone(),
        r.run.clone(),
        names(&r.targets),
    )
}

fn cutoff(value: f64) -> Action {
    Action::SetParam {
        track: "acid".into(),
        param: "acid303.cutoff".into(),
        value,
        commit: true,
    }
}

fn set_value(path: &str, value: Value) -> Action {
    Action::SetValue {
        path: path.parse().unwrap(),
        value,
    }
}

#[test]
fn every_write_appends_a_record() {
    let s = song();
    let mut core = StudioCore::open(&s.path).unwrap();
    let t0 = Instant::now();

    // A knob burst: one record for the write.
    core.apply_edit(&cutoff(700.0), t0).unwrap();
    core.apply_edit(&cutoff(820.0), t0 + Duration::from_millis(10))
        .unwrap();
    let pan = Action::SetTrackMix {
        track: "hat".into(),
        gain_db: None,
        pan: Some(0.5),
        mute: None,
        solo: None,
    };
    core.apply_edit(&pan, t0 + Duration::from_millis(20))
        .unwrap();
    core.flush().unwrap();
    let r = records(&s);
    assert_eq!(r.len(), 1);
    assert_eq!(
        summary(&r[0]),
        (
            Source::Human,
            vec!["set_param".into(), "set_track_mix".into()],
            None,
            vec!["tracks.acid.acid303.cutoff".into(), "tracks.hat.pan".into()],
        )
    );
    assert_eq!(
        (&r[0].targets[0].old, &r[0].targets[0].new),
        (&json!(500), &json!(820))
    );
    let text = fs::read_to_string(&s.path).unwrap();
    assert_eq!(
        r[0].file_hash.as_deref(),
        Some(tune_studio::text_hash(&text).as_str())
    );

    // An external edit the studio logs.
    let edited = text.replace("bpm = 120", "bpm = 121");
    fs::write(&s.path, &edited).unwrap();
    core.on_disk_text(edited).expect("an external change");
    let r = records(&s);
    assert_eq!(r.len(), 2);
    assert_eq!(
        summary(&r[1]),
        (Source::External, vec![], None, vec!["song.bpm".into()])
    );

    // An undo, with the asker's source.
    core.undo(Source::Claude).unwrap();
    let r = records(&s);
    assert_eq!(r.len(), 3);
    assert_eq!(
        summary(&r[2]),
        (
            Source::Claude,
            vec!["undo".into()],
            None,
            vec!["song.bpm".into()]
        )
    );
    assert_eq!(
        (&r[2].targets[0].old, &r[2].targets[0].new),
        (&json!(121), &json!(120))
    );

    // An agent's actions in a run.
    let run = RunTag::new("r1", Some("open the filter")).unwrap();
    core.apply_now(
        &[set_value("tracks.acid.acid303.cutoff", json!(900))],
        &Origin::claude(Some(run)),
    )
    .unwrap();
    let r = records(&s);
    assert_eq!(r.len(), 4);
    assert_eq!(
        summary(&r[3]),
        (
            Source::Claude,
            vec!["set_value".into()],
            Some("r1".into()),
            vec!["tracks.acid.acid303.cutoff".into()]
        )
    );
    assert_eq!(r[3].run_title.as_deref(), Some("open the filter"));

    // `tune set`'s path.
    apply_to_file(
        &s.path,
        &[set_value("song.bpm", json!(125))],
        &Origin::claude(None),
    )
    .unwrap();
    let r = records(&s);
    assert_eq!(r.len(), 5);
    assert_eq!(
        summary(&r[4]),
        (
            Source::Claude,
            vec!["set_value".into()],
            None,
            vec!["song.bpm".into()]
        )
    );
    assert_eq!(r[4].run_title, None);

    // The entries read as before: every line that is not a record.
    let all = entries(&s);
    assert_eq!(all, entry_lines(&s));
    assert_eq!(all.len(), 6, "{all:#?}");
}

#[test]
fn redo_and_migrations_append_records() {
    let s = song();
    let mut core = StudioCore::open(&s.path).unwrap();
    core.apply_now(&[cutoff(820.0)], &Origin::human()).unwrap();
    core.undo(Source::Human).unwrap();
    core.redo(Source::Human).unwrap();
    let actions: Vec<Vec<String>> = records(&s).into_iter().map(|r| r.actions).collect();
    assert_eq!(
        actions,
        [vec!["set_param"], vec!["undo"], vec!["redo"]]
            .map(|a| a.into_iter().map(str::to_owned).collect::<Vec<_>>())
    );

    // The open-time migration and `migrate_file`: no target changes.
    for migrate in [true, false] {
        let s = song();
        fs::write(&s.path, include_str!("fixtures/commented-v1.toml")).unwrap();
        if migrate {
            tune_studio::migrate_file(&s.path, Source::Claude, false).unwrap();
        } else {
            StudioCore::open(&s.path).unwrap();
        }
        let r = records(&s);
        assert_eq!(r.len(), 1, "migrate_file: {migrate}");
        assert_eq!(r[0].actions, ["migrate"]);
        assert_eq!(r[0].targets, vec![]);
        assert_eq!(entries(&s).len(), 1, "the song.version entry");
    }
}

/// A write that changes nothing appends nothing.
#[test]
fn no_change_appends_nothing() {
    let s = song();
    let mut core = StudioCore::open(&s.path).unwrap();
    core.apply_now(&[cutoff(500.0)], &Origin::human()).unwrap();
    apply_to_file(
        &s.path,
        &[
            set_value("song.bpm", json!(130)),
            set_value("song.bpm", json!(120)),
        ],
        &Origin::claude(None),
    )
    .unwrap();
    assert!(!s.path.with_file_name("edits.jsonl").exists());
}

#[test]
fn old_logs_read_unchanged() {
    let s = song();
    // Lines as Task 3's code wrote them.
    let old = concat!(
        r#"{"ts":"2026-10-21T22:15:01.000Z","source":"human","path":"song.bpm","old":120,"new":121,"file_hash":"aa"}"#,
        "\n",
        r#"{"ts":"2026-10-21T22:15:02.000Z","source":"claude","path":"song.bpm","old":121,"new":122,"file_hash":"bb","action":"set_value","run":"r1","run_title":"t"}"#,
        "\n",
    );
    let file = s.path.with_file_name("edits.jsonl");
    fs::write(&file, old).unwrap();
    let log = EditLog::for_song(&s.path);
    let before = log.read_since(None).unwrap();
    assert_eq!(before.len(), 2);
    assert_eq!(before[1].run.as_deref(), Some("r1"));
    assert_eq!(log.read_records().unwrap(), vec![]);

    let record = pinned_record();
    log.append_write(&before[..1], Some(&record)).unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.starts_with(old), "{text}");
    assert_eq!(text.lines().count(), 4);
    assert_eq!(log.read_since(None).unwrap().len(), 3);
    assert_eq!(log.read_records().unwrap(), vec![record]);
}

const PINNED: &str = r#"{"ts":"2026-10-27T10:00:00.000Z","source":"claude","record":"action","actions":["set_value"],"run":"r1","run_title":"open the filter","targets":[{"target":"tracks.acid.acid303.cutoff","old":500,"new":820}],"file_hash":"…"}"#;

fn pinned_record() -> ActionRecord {
    ActionRecord {
        ts: "2026-10-27T10:00:00.000Z".parse().unwrap(),
        source: Source::Claude,
        actions: vec!["set_value".into()],
        run: Some("r1".into()),
        run_title: Some("open the filter".into()),
        targets: vec![TargetChange {
            target: target("tracks.acid.acid303.cutoff"),
            old: json!(500),
            new: json!(820),
        }],
        file_hash: Some("…".into()),
    }
}

#[test]
fn records_have_a_pinned_shape() {
    let record = pinned_record();
    assert_eq!(serde_json::to_string(&record).unwrap(), PINNED);
    assert_eq!(
        serde_json::from_str::<ActionRecord>(PINNED).unwrap(),
        record
    );
    // A record is not an entry, and an entry is not a record.
    assert!(serde_json::from_str::<EditEntry>(PINNED).is_err());
    let entry = r#"{"ts":"2026-10-21T22:15:01.000Z","source":"human","path":"song.bpm","old":120,"new":121}"#;
    assert!(serde_json::from_str::<ActionRecord>(entry).is_err());
    // Without a run, the run fields are left out.
    let bare = ActionRecord {
        run: None,
        run_title: None,
        ..record
    };
    let text = serde_json::to_string(&bare).unwrap();
    assert!(!text.contains("run"), "{text}");
}
