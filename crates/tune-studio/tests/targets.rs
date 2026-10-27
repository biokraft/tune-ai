//! Undo targets: the song as targets, the changes between two songs, and
//! `put` writing a target back.

use serde_json::{Value, json};
use tune_dsp::acid303::ACID303_PARAMS;
use tune_studio::{
    EditError, SongDoc, SongPath, Target, TargetChange, target_changes, target_value,
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
