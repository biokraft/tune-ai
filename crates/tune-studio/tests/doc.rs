//! In-place song edits: paths, comment preservation, validation and section
//! operations.

use serde_json::{Value, json};
use tune_song::{SectionPlay, placements_toml};
use tune_studio::{EditError, SongDoc, SongPath};

/// The commented fixture in format 2: `migrate(COMMENTED_V1)`.
const COMMENTED: &str = include_str!("fixtures/commented.toml");
const COMMENTED_V1: &str = include_str!("fixtures/commented-v1.toml");

/// The five comments the fixture adds to the mini song.
const COMMENTS: [&str; 5] = [
    "# the body\n[tracks.kick]",
    "gain_db = -3 # headroom\n",
    "# the squelch\n[tracks.acid]",
    "# acid enters\n[[sections]]",
    "# the filter opens over the section\n[[automation]]",
];

/// Every comment line of `text`, in order.
fn comment_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|l| l.find('#').map(|i| &l[i..]))
        .collect()
}

/// The table headers of `text` from `from` on, in order.
fn headers_from<'a>(text: &'a str, from: &str) -> Vec<&'a str> {
    let start = text
        .find(from)
        .unwrap_or_else(|| panic!("`{from}` in:\n{text}"));
    text[start..]
        .lines()
        .filter(|l| l.starts_with('['))
        .collect()
}

fn doc() -> SongDoc {
    SongDoc::parse(COMMENTED).expect("fixture parses")
}

fn path(s: &str) -> SongPath {
    s.parse()
        .unwrap_or_else(|e| panic!("`{s}` should parse: {e}"))
}

fn names(doc: &SongDoc) -> Vec<String> {
    doc.song().sections.iter().map(|s| s.name.clone()).collect()
}

/// Lines of `after` that differ from `before` (both must have equal length).
fn changed_lines<'a>(before: &str, after: &'a str) -> Vec<&'a str> {
    let (b, a): (Vec<_>, Vec<_>) = (before.lines().collect(), after.lines().collect());
    assert_eq!(b.len(), a.len(), "line count changed:\n{after}");
    b.iter()
        .zip(&a)
        .filter(|(x, y)| x != y)
        .map(|(_, y)| *y)
        .collect()
}

#[test]
fn paths_round_trip() {
    for s in [
        "song.bpm",
        "tracks.kick.gain_db",
        "tracks.kick.pan",
        "tracks.kick.mute",
        "tracks.kick.solo",
        "tracks.acid.acid303.cutoff",
        "tracks.acid.dist.drive",
        "tracks.acid.dist.mode",
        "tracks.acid",
        "clips.a1.steps",
        "song.version",
        "sections[2].bars",
        "sections[0].name",
        "sections[1].lanes",
        "sections[1].play.acid",
        "sections",
    ] {
        assert_eq!(path(s).to_string(), s);
    }
    assert_eq!(
        path("tracks.acid.acid303.cutoff"),
        SongPath::TrackParam {
            track: "acid".into(),
            param: "acid303.cutoff".into()
        }
    );
    assert_eq!(path("sections[2].bars"), SongPath::SectionBars(2));
    for bad in ["foo.bar", "sections[x].bars", "tracks.Acid.gain_db"] {
        assert!(bad.parse::<SongPath>().is_err(), "`{bad}` should not parse");
    }
}

#[test]
fn paths_serialize_as_strings() {
    let p = path("sections[1].play.acid");
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(v, json!("sections[1].play.acid"));
    assert_eq!(serde_json::from_value::<SongPath>(v).unwrap(), p);
}

#[test]
fn parse_round_trips_text() {
    let d = doc();
    assert_eq!(d.text(), COMMENTED);
    assert_eq!(d.migrated_from(), None);
}

#[test]
fn v1_text_is_migrated_in_memory() {
    let d = SongDoc::parse(COMMENTED_V1).expect("the v1 fixture parses");
    assert_eq!(d.migrated_from(), Some(1));
    let text = d.text();
    assert!(text.contains("[song]\nversion = 2\n"), "{text}");
    assert_eq!(comment_lines(&text), comment_lines(COMMENTED_V1));
    assert_eq!(text, COMMENTED, "the committed v2 fixture is the migration");
    assert_eq!(d.song(), doc().song());
    assert_eq!(d.get(&path("song.version")), Some(json!(2)));
}

#[test]
fn clip_steps_path() {
    let old: SongPath = "patterns.k4.steps".parse().unwrap();
    let new: SongPath = "clips.k4.steps".parse().unwrap();
    assert_eq!(old, new);
    assert_eq!(old.to_string(), "clips.k4.steps");
    let mut d = doc();
    d.set(&new, &json!("x.x.x.x.x.x.x.x.")).unwrap();
    assert_eq!(
        changed_lines(COMMENTED, &d.text()),
        ["steps = \"x.x.x.x.x.x.x.x.\""]
    );
    let k4 = d.text().find("[clips.k4]").unwrap();
    assert!(
        d.text()[k4..].starts_with("[clips.k4]\nkind = \"drum\"\nsteps = \"x.x.x.x.x.x.x.x.\"\n"),
        "{}",
        d.text()
    );
}

#[test]
fn set_param_preserves_comments() {
    let mut d = doc();
    d.set(&path("tracks.acid.acid303.cutoff"), &json!(820))
        .unwrap();
    let text = d.text();
    let changed = changed_lines(COMMENTED, &text);
    assert_eq!(changed.len(), 1, "{text}");
    assert!(
        changed[0].contains("\"acid303.cutoff\" = 820"),
        "{}",
        changed[0]
    );
    for c in COMMENTS {
        assert!(text.contains(c), "lost `{c}`:\n{text}");
    }
    assert_eq!(d.song().tracks["acid"].instrument.params["cutoff"], 820.0);
}

#[test]
fn set_keeps_trailing_comment() {
    let mut d = doc();
    d.set(&path("tracks.kick.gain_db"), &json!(-4.5)).unwrap();
    let text = d.text();
    assert_eq!(
        changed_lines(COMMENTED, &text),
        ["gain_db = -4.5 # headroom"]
    );
}

#[test]
fn set_param_inserts_missing_key() {
    let mut d = doc();
    d.set(&path("tracks.kick.kick.tune"), &json!(60)).unwrap();
    assert_eq!(d.song().tracks["kick"].instrument.params["tune"], 60.0);
    assert!(
        d.text().contains("params = { \"kick.tune\" = 60 }"),
        "{}",
        d.text()
    );
    // A second param lands in the same inline table.
    d.set(&path("tracks.kick.kick.decay"), &json!(300)).unwrap();
    assert_eq!(d.song().tracks["kick"].instrument.params["decay"], 300.0);
    assert_eq!(d.song().tracks["kick"].instrument.params["tune"], 60.0);
    for c in COMMENTS {
        assert!(d.text().contains(c), "lost `{c}`:\n{}", d.text());
    }
}

#[test]
fn set_fx_param() {
    let mut d = doc();
    d.set(&path("tracks.acid.dist.drive"), &json!(20)).unwrap();
    assert_eq!(d.song().tracks["acid"].fx[0].params["drive"], 20.0);
    d.set(&path("tracks.acid.dist.mix"), &json!(0.5)).unwrap();
    assert_eq!(d.song().tracks["acid"].fx[0].params["mix"], 0.5);
}

#[test]
fn set_rejects_invalid() {
    let mut d = doc();
    let err = d
        .set(&path("tracks.acid.acid303.cutoff"), &json!(99999))
        .unwrap_err();
    match &err {
        EditError::Invalid(errors) => assert!(
            errors.iter().any(|e| e.message.contains("is out of range")),
            "{errors:?}"
        ),
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert_eq!(d.text(), COMMENTED);
    assert_eq!(d.song().tracks["acid"].instrument.params["cutoff"], 500.0);
}

#[test]
fn integers_written_without_fraction() {
    let mut d = doc();
    d.set(&path("tracks.hat.gain_db"), &json!(-6.0)).unwrap();
    d.set(&path("tracks.hat.pan"), &json!(0.25)).unwrap();
    let text = d.text();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.contains(&"gain_db = -6"), "{text}");
    assert!(lines.contains(&"pan = 0.25"), "{text}");
    d.set(&path("song.bpm"), &json!(138.0)).unwrap();
    assert!(d.text().lines().any(|l| l == "bpm = 138"), "{}", d.text());
}

#[test]
fn set_mixer_fields() {
    let mut d = doc();
    d.set(&path("tracks.kick.pan"), &json!(-0.5)).unwrap();
    d.set(&path("tracks.kick.mute"), &json!(true)).unwrap();
    d.set(&path("tracks.hat.solo"), &json!(true)).unwrap();
    let song = d.song();
    assert_eq!(song.tracks["kick"].pan, -0.5);
    assert!(song.tracks["kick"].mute);
    assert!(song.tracks["hat"].solo);
    assert!(matches!(
        d.set(&path("tracks.kick.mute"), &json!(1)),
        Err(EditError::BadValue { .. })
    ));
    assert!(matches!(
        d.set(&path("tracks.kick.gain_db"), &json!("loud")),
        Err(EditError::BadValue { .. })
    ));
}

#[test]
fn play_null_removes_key() {
    let mut d = doc();
    d.set(&path("sections[1].play.acid"), &Value::Null).unwrap();
    let song = d.song();
    let play = ["kick", "acid", "hat"].map(|t| song.section_play(1, t));
    assert_eq!(
        play,
        [
            SectionPlay::Clip("k4"),
            SectionPlay::Silent,
            SectionPlay::Clip("h")
        ]
    );
    // The acid plays nowhere now, so it has no `play` key.
    let acid = &d.text()[d.text().find("[tracks.acid]").unwrap()..];
    let acid = &acid[..acid.find("\n\n").unwrap()];
    assert!(!acid.contains("play"), "{acid}");
    d.set(&path("sections[0].play.acid"), &json!("a1")).unwrap();
    assert_eq!(d.song().section_play(0, "acid"), SectionPlay::Clip("a1"));
    assert!(
        d.text()
            .contains("fx = [{ type = \"distortion\", mode = \"soft\", params = { \"dist.drive\" = 12 } }]\nplay = [{ at = 1, clip = \"a1\", bars = 2 }]\n"),
        "{}",
        d.text()
    );
}

/// Review Focus 1 and F9: the stretch under section `b` and the comment
/// above it travel with `b`.
#[test]
fn move_section_moves_its_automation_tables_with_it() {
    let mut d = doc();
    d.move_section(1, 0).unwrap();
    assert_eq!(names(&d), ["b", "a"]);
    let text = d.text();
    let b = "# acid enters\n[[sections]]\nname = \"b\"\nbars = 2\n\n\
             # the filter opens over the section\n[[automation]]\n\
             param = \"acid.acid303.cutoff\"\nat = 1\nbars = 2\n\
             points = [[0, 0.1], [2, 0.8]]\n\n[[sections]]\nname = \"a\"\nbars = 2\n";
    assert!(text.contains(b), "{text}");
    let song = d.song();
    assert_eq!(song.lanes_in(0).len(), 1);
    assert_eq!(song.lanes_in(0)[0].param, "acid303.cutoff");
    assert!(song.lanes_in(1).is_empty());
    assert_eq!(song.section_play(0, "acid"), SectionPlay::Clip("a1"));
    assert_eq!(song.section_play(1, "acid"), SectionPlay::Silent);
    assert!(
        text.contains("play = [{ at = 1, clip = \"a1\", bars = 2 }]"),
        "{text}"
    );
    for c in COMMENTS {
        assert!(text.contains(c), "lost `{c}`:\n{text}");
    }
    // Moving back restores the file byte for byte.
    d.move_section(0, 1).unwrap();
    assert_eq!(d.text(), COMMENTED);
}

#[test]
fn move_section_bounds() {
    let mut d = doc();
    assert!(matches!(
        d.move_section(2, 0),
        Err(EditError::OutOfBounds { index: 2, len: 2 })
    ));
    assert!(matches!(
        d.move_section(0, 5),
        Err(EditError::OutOfBounds { index: 5, len: 2 })
    ));
}

#[test]
fn duplicate_and_delete() {
    let mut d = doc();
    d.duplicate_section(0).unwrap();
    assert_eq!(names(&d), ["a", "a", "b"]);
    d.delete_section(2).unwrap();
    assert_eq!(names(&d), ["a", "a"]);
    d.delete_section(0).unwrap();
    assert_eq!(names(&d), ["a"]);
    let err = d.delete_section(0).unwrap_err();
    assert!(matches!(err, EditError::LastSection));
    assert_eq!(err.to_string(), "a song needs at least one section");
}

#[test]
fn duplicate_carries_lanes_in_order() {
    let mut d = doc();
    d.duplicate_section(1).unwrap();
    assert_eq!(names(&d), ["a", "b", "b"]);
    let song = d.song();
    assert_eq!(song.lanes_in(1).len(), 1);
    assert_eq!(song.lanes_in(2).len(), 1);
    assert_eq!(song.lanes_in(2)[0].start_bar, 4);
    assert_eq!(song.section_play(2, "acid"), SectionPlay::Clip("a1"));
    // Each copy's lane follows its own section.
    assert_eq!(
        headers_from(&d.text(), "# acid enters"),
        [
            "[[sections]]",
            "[[automation]]",
            "[[sections]]",
            "[[automation]]"
        ]
    );
}

#[test]
fn duplicate_section_copies_lane_comments() {
    let mut d = doc();
    d.duplicate_section(1).unwrap();
    let text = d.text();
    let copy = "# acid enters\n[[sections]]\nname = \"b\"\nbars = 2\n\n\
                # the filter opens over the section\n[[automation]]\n\
                param = \"acid.acid303.cutoff\"\nat = 5\nbars = 2\n\
                points = [[0, 0.1], [2, 0.8]]\n";
    assert!(text.ends_with(copy), "{text}");
    assert_eq!(
        text.matches("# the filter opens over the section").count(),
        2
    );
    assert_eq!(text.matches("# acid enters").count(), 2);
    assert!(
        text.contains("play = [\n  { at = 3, clip = \"a1\", bars = 2 },\n  { at = 5, clip = \"a1\", bars = 2 },\n]"),
        "{text}"
    );
}

#[test]
fn resize_section_uses_section_semantics() {
    let mut d = doc();
    d.set(&path("sections[1].bars"), &json!(4)).unwrap();
    assert_eq!(d.song().total_bars(), 6);
    let text = d.text();
    // The placements and the stretch that ran to the end of `b` grow with
    // it; the stretch's points stay as they are.
    assert!(
        text.contains("play = [{ at = 3, clip = \"a1\", bars = 4 }]"),
        "{text}"
    );
    assert!(
        text.contains("  { at = 3, clip = \"k4\", bars = 4 },\n"),
        "{text}"
    );
    assert!(
        text.contains("at = 3\nbars = 4\npoints = [[0, 0.1], [2, 0.8]]"),
        "{text}"
    );
    // Growing `a` moves `b` and everything in it.
    d.set(&path("sections[0].bars"), &json!(3)).unwrap();
    let text = d.text();
    assert!(
        text.contains("play = [{ at = 4, clip = \"a1\", bars = 4 }]"),
        "{text}"
    );
    assert!(
        text.contains("  { at = 1, clip = \"k4\", bars = 3 },\n"),
        "{text}"
    );
    assert!(
        text.contains("at = 4\nbars = 4\npoints = [[0, 0.1], [2, 0.8]]"),
        "{text}"
    );
    for c in COMMENTS {
        assert!(text.contains(c), "lost `{c}`:\n{text}");
    }
}

#[test]
fn set_section_play_writes_placements() {
    let mut d = doc();
    d.set(&path("sections[0].play.acid"), &json!("a1")).unwrap();
    let play = &d.song().tracks["acid"].play;
    assert_eq!(play.len(), 2);
    let expected = format!("\nplay = {}\n", placements_toml(play));
    assert!(d.text().contains(&expected), "{}", d.text());
    assert_eq!(
        changed_lines(
            &COMMENTED.replacen(
                "play = [{ at = 3, clip = \"a1\", bars = 2 }]",
                "x\nx\nx\nx",
                1
            ),
            &d.text()
        ),
        [
            "play = [",
            "  { at = 1, clip = \"a1\", bars = 2 },",
            "  { at = 3, clip = \"a1\", bars = 2 },",
            "]"
        ]
    );
}

/// Review Focus 5: the clip-kind rule refuses the edit with the
/// validator's message, and the text stays as it was.
#[test]
fn set_section_play_rejects_an_incompatible_clip() {
    let mut d = doc();
    let err = d
        .set(&path("sections[0].play.acid"), &json!("k4"))
        .unwrap_err();
    match &err {
        EditError::Invalid(errors) => assert!(
            errors.iter().any(|e| e
                .message
                .contains("clip `k4` is a drum clip; track `acid` (acid303) plays acid clips")),
            "{errors:?}"
        ),
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert_eq!(d.text(), COMMENTED);
    let err = d
        .set(&path("sections[0].play.acid"), &json!(3))
        .unwrap_err();
    assert!(matches!(err, EditError::BadValue { .. }), "{err:?}");
}

/// An edit of a document, for tables of edits.
type Op = fn(&mut SongDoc) -> Result<(), EditError>;

/// Review Focus 1: no section operation loses a comment.
#[test]
fn section_ops_keep_every_comment() {
    let ops: [(&str, Op); 9] = [
        ("move 1 to 0", |d| d.move_section(1, 0)),
        ("move 0 to 1", |d| d.move_section(0, 1)),
        ("duplicate 0", |d| d.duplicate_section(0)),
        ("duplicate 1", |d| d.duplicate_section(1)),
        ("delete 0", |d| d.delete_section(0)),
        ("grow 0", |d| d.set(&path("sections[0].bars"), &json!(5))),
        ("shrink 1", |d| d.set(&path("sections[1].bars"), &json!(1))),
        ("play a1 in 0", |d| {
            d.set(&path("sections[0].play.acid"), &json!("a1"))
        }),
        ("silence 1", |d| {
            d.set(&path("sections[1].play.kick"), &Value::Null)
        }),
    ];
    // The fixture, and the fixture with a commented stretch across the
    // `a`/`b` edge, which most ops split there.
    let crossing = COMMENTED.replacen(
        "[[sections]]\nname = \"a\"\nbars = 2\n",
        "[[sections]]\nname = \"a\"\nbars = 2\n\n\
         # a sweep across the edge\n[[automation]] # on the header\n\
         param = \"acid.acid303.resonance\" # the param\nat = 2 # starts in a\n\
         bars = 2 # ends in b\npoints = [[0, 0.2], [2, 0.6]] # it rises\n",
        1,
    );
    assert_ne!(crossing, COMMENTED);
    for src in [COMMENTED, crossing.as_str()] {
        for (name, op) in ops {
            let mut d = SongDoc::parse(src).unwrap();
            op(&mut d).unwrap_or_else(|e| panic!("{name}: {e}"));
            let text = d.text();
            assert_ne!(text, src, "{name} changed nothing");
            for c in comment_lines(src) {
                assert!(text.contains(c), "{name} lost `{c}`:\n{text}");
            }
        }
    }
}

/// A stretch whose first piece an op drops keeps its table, comments
/// included, for the piece that is left.
#[test]
fn a_cut_stretch_keeps_its_table() {
    let src = COMMENTED.replacen(
        "[[sections]]\nname = \"a\"\nbars = 2\n",
        "[[sections]]\nname = \"a\"\nbars = 2\n\n\
         # a sweep across the edge\n[[automation]] # on the header\n\
         param = \"acid.acid303.resonance\" # the param\nat = 2 # starts in a\n\
         bars = 2 # ends in b\npoints = [[0, 0.2], [2, 0.6]] # it rises\n",
        1,
    );
    let mut d = SongDoc::parse(&src).unwrap();
    d.delete_section(0).unwrap();
    assert!(
        d.text().contains(
            "# a sweep across the edge\n[[automation]] # on the header\n\
             param = \"acid.acid303.resonance\" # the param\nat = 1 # starts in a\n\
             bars = 1 # ends in b\npoints = [[0, 0.4], [1, 0.6]] # it rises\n"
        ),
        "{}",
        d.text()
    );
}

#[test]
fn bars_out_of_range_read_like_the_validator() {
    let mut d = doc();
    for (bars, message) in [
        (0, "bars must be 1 to 1024, got 0"),
        (2000, "bars must be 1 to 1024, got 2000"),
        (-3, "bars must be 1 to 1024, got -3"),
    ] {
        let err = d.set(&path("sections[0].bars"), &json!(bars)).unwrap_err();
        assert_eq!(
            err,
            EditError::BadValue {
                path: "sections[0].bars".into(),
                message: message.into()
            }
        );
        assert_eq!(d.text(), COMMENTED);
    }
}

#[test]
fn bars_and_mode() {
    let mut d = doc();
    d.set(&path("sections[0].bars"), &json!(8)).unwrap();
    assert_eq!(d.song().total_bars(), 10);
    d.set(&path("tracks.acid.dist.mode"), &json!("hard"))
        .unwrap();
    assert_eq!(d.song().tracks["acid"].fx[0].setting("mode"), Some("hard"));
    assert!(matches!(
        d.set(&path("sections[0].bars"), &json!(2.5)),
        Err(EditError::BadValue { .. })
    ));
}

#[test]
fn names_and_steps() {
    let mut d = doc();
    d.set(&path("sections[0].name"), &json!("intro")).unwrap();
    assert_eq!(names(&d), ["intro", "b"]);
    d.set(&path("clips.h.steps"), &json!("x.x.x.x.x.x.x.x."))
        .unwrap();
    // Read back in the canonical notation (groups of four).
    assert_eq!(
        d.get(&path("clips.h.steps")),
        Some(json!("x.x. x.x. x.x. x.x."))
    );
}

#[test]
fn no_such_target() {
    let mut d = doc();
    let err = d
        .set(&path("tracks.kick.dist.drive"), &json!(10))
        .unwrap_err();
    assert!(matches!(err, EditError::NoSuchTarget(_)), "{err:?}");
    assert_eq!(
        err.to_string(),
        "`tracks.kick.dist.drive` does not exist in this song"
    );
    for p in [
        "tracks.clap.gain_db",
        "sections[9].bars",
        "clips.zz.steps",
        "tracks.kick.dist.mode",
    ] {
        let err = d.set(&path(p), &json!(1)).unwrap_err();
        assert!(matches!(err, EditError::NoSuchTarget(_)), "{p}: {err:?}");
    }
    assert_eq!(d.text(), COMMENTED);
}

#[test]
fn read_only() {
    let mut d = doc();
    for p in [
        "sections",
        "tracks.acid",
        "sections[1].lanes",
        "song.version",
    ] {
        let err = d.set(&path(p), &json!([])).unwrap_err();
        assert!(matches!(err, EditError::ReadOnly(_)), "{p}: {err:?}");
    }
}

#[test]
fn get_effective() {
    let d = doc();
    assert_eq!(d.get(&path("tracks.acid.acid303.tuning")), Some(json!(440)));
    assert_eq!(d.get(&path("tracks.acid.acid303.cutoff")), Some(json!(500)));
    assert_eq!(d.get(&path("tracks.acid.dist.drive")), Some(json!(12)));
    assert_eq!(d.get(&path("tracks.acid.dist.mode")), Some(json!("soft")));
    assert_eq!(d.get(&path("tracks.hat.pan")), Some(json!(0.3)));
    assert_eq!(d.get(&path("tracks.acid.gain_db")), Some(json!(0)));
    assert_eq!(d.get(&path("tracks.acid.mute")), Some(json!(false)));
    assert_eq!(d.get(&path("song.bpm")), Some(json!(120)));
    assert_eq!(d.get(&path("sections[1].bars")), Some(json!(2)));
    assert_eq!(d.get(&path("sections[0].play.acid")), Some(Value::Null));
    assert_eq!(d.get(&path("sections[1].play.acid")), Some(json!("a1")));
    assert_eq!(d.get(&path("sections")), Some(json!(["a:2", "b:2"])));
    assert_eq!(
        d.get(&path("sections[1].lanes")),
        Some(
            json!([{ "track": "acid", "param": "acid303.cutoff", "at": 3, "bars": 2, "points": [[0, 0.1], [2, 0.8]] }])
        )
    );
    assert_eq!(
        d.get(&path("tracks.acid")),
        Some(json!({ "instrument": "acid303", "fx": [{ "id": "dist", "type": "distortion" }] }))
    );
    assert_eq!(d.get(&path("tracks.kick.dist.drive")), None);
    assert_eq!(d.get(&path("tracks.clap.gain_db")), None);
    assert_eq!(d.get(&path("sections[5].bars")), None);
}

#[test]
fn section_values_are_derived() {
    let d = SongDoc::parse(include_str!("../../tune-song/tests/fixtures/mini.toml")).unwrap();
    assert_eq!(d.get(&path("sections[1].play.acid")), Some(json!("a1")));
    assert_eq!(d.get(&path("sections[0].play.acid")), Some(Value::Null));
    assert_eq!(
        d.get(&path("sections[1].lanes")),
        Some(json!([{
            "track": "acid",
            "param": "acid303.cutoff",
            "at": 3,
            "bars": 2,
            "points": [[0, 0.1], [2, 0.8]]
        }]))
    );
    assert_eq!(d.get(&path("sections[0].lanes")), Some(json!([])));

    // Shapes a v1 file cannot hold: a placement on part of a section, and
    // a stretch across a section edge.
    let mut song = d.song().clone();
    song.tracks.get_mut("acid").unwrap().play = vec![tune_song::Placement {
        start_bar: 3,
        bars: 1,
        clip: "a1".into(),
        offset_bars: 1,
    }];
    song.automation[0].start_bar = 1;
    let value = |path: &SongPath| tune_studio::effective_value(&song, path);
    assert_eq!(
        value(&path("sections[1].play.acid")),
        Some(json!([{ "at": 4, "clip": "a1", "bars": 1, "offset": 1 }]))
    );
    let lane = json!([{
        "track": "acid",
        "param": "acid303.cutoff",
        "at": 2,
        "bars": 2,
        "points": [[0, 0.1], [2, 0.8]]
    }]);
    assert_eq!(value(&path("sections[0].lanes")), Some(lane.clone()));
    assert_eq!(value(&path("sections[1].lanes")), Some(lane));
}

/// The section and automation tables are laid out as one block from the
/// first section; a table that sat between sections follows the block.
#[test]
fn move_section_with_interleaved_tables() {
    // A clip table between the sections, a stretch after the last section
    // (the canonical layout) and a comment on it.
    let src = "[song]\nversion = 2\nname = \"x\"\nbpm = 120\n\n\
               [tracks.acid]\ninstrument = \"acid303\"\nplay = [{ at = 2, clip = \"p\", bars = 2 }]\n\n\
               [[sections]]\nname = \"a\"\nbars = 1\n\n\
               [clips.p]\nkind = \"acid\"\nsteps = \"C2\"\n\n\
               [[sections]]\nname = \"b\"\nbars = 2\n\n\
               # lane of a\n[[automation]]\nparam = \"acid.acid303.cutoff\"\nat = 1\nbars = 1\n\
               points = [[0, 0.5]]\n";
    let mut d = SongDoc::parse(src).unwrap();
    let before = d.song().clone();
    d.move_section(0, 1).unwrap();
    assert_eq!(names(&d), ["b", "a"]);
    let song = d.song();
    assert!(song.lanes_in(0).is_empty());
    assert_eq!(song.lanes_in(1).len(), 1);
    assert_eq!(song.section_play(0, "acid"), SectionPlay::Clip("p"));
    let text = d.text();
    assert_eq!(
        headers_from(&text, "[[sections]]"),
        [
            "[[sections]]",
            "[[sections]]",
            "[[automation]]",
            "[clips.p]"
        ]
    );
    assert!(
        text.contains("name = \"a\"\nbars = 1\n\n# lane of a\n[[automation]]\n"),
        "{text}"
    );
    d.move_section(1, 0).unwrap();
    assert_eq!(d.song(), &before);
    assert_eq!(
        headers_from(&d.text(), "[[sections]]"),
        [
            "[[sections]]",
            "[[automation]]",
            "[[sections]]",
            "[clips.p]"
        ]
    );
}

#[test]
fn inline_sections_array() {
    let src = "sections = [{ name = \"a\", bars = 1 }, { name = \"b\", bars = 2 }]\n\n\
               [song]\nversion = 2\nname = \"x\"\nbpm = 120\n";
    let mut d = SongDoc::parse(src).unwrap();
    d.move_section(1, 0).unwrap();
    assert!(
        d.text()
            .starts_with("sections = [{ name = \"b\", bars = 2 }, { name = \"a\", bars = 1 }]\n"),
        "{}",
        d.text()
    );
    d.duplicate_section(1).unwrap();
    assert_eq!(names(&d), ["b", "a", "a"]);
    d.set(&path("sections[2].bars"), &json!(4)).unwrap();
    d.delete_section(0).unwrap();
    assert!(
        d.text()
            .starts_with("sections = [{ name = \"a\", bars = 1 }, { name = \"a\", bars = 4 }]\n"),
        "{}",
        d.text()
    );
}

// ----- the mixer: `mix.*` paths and strip params -----

const MIXED: &str = include_str!("../../tune-song/tests/fixtures/v2/mixed.toml");

fn mixed() -> SongDoc {
    SongDoc::parse(MIXED).expect("mixed fixture parses")
}

/// The canonical writer gives back the same song from the edited text.
fn assert_canonical_round_trip(d: &SongDoc) {
    let canonical = d.song().to_toml();
    assert_eq!(
        &tune_song::parse_song(&canonical).expect("canonical text parses"),
        d.song(),
        "{canonical}"
    );
}

#[test]
fn mix_paths_round_trip() {
    for s in [
        "mix.master",
        "mix.duck_source",
        "mix.delay.feedback",
        "mix.master.gain",
        "tracks.acid.send.delay",
        "tracks.acid.eq.low_cut",
    ] {
        assert_eq!(path(s).to_string(), s);
    }
    assert_eq!(path("mix.master"), SongPath::MixMaster);
    assert_eq!(path("mix.duck_source"), SongPath::MixDuckSource);
    assert_eq!(
        path("mix.delay.feedback"),
        SongPath::MixParam {
            param: "delay.feedback".into()
        }
    );
    assert_eq!(
        path("tracks.acid.send.delay"),
        SongPath::TrackParam {
            track: "acid".into(),
            param: "send.delay".into()
        }
    );
    assert_eq!(
        SongPath::param("mix", "reverb.size"),
        path("mix.reverb.size")
    );
    assert_eq!(
        SongPath::param("acid", "send.reverb"),
        path("tracks.acid.send.reverb")
    );
    for bad in [
        "mix",
        "mix.",
        "mix.delay",
        "mix.delay.",
        "mix..feedback",
        "mix.delay.feed.back",
        "mix.Delay.feedback",
    ] {
        assert!(bad.parse::<SongPath>().is_err(), "`{bad}` should not parse");
    }
    let err = "nope".parse::<SongPath>().unwrap_err();
    assert!(err.contains("mix.<param id>"), "{err}");
}

/// Review Focus 5: a commented file with no `[mix]` gains one right after
/// `[song]`, and nothing else in the file moves.
#[test]
fn set_mix_param_creates_mix_table() {
    let mut d = doc();
    d.set(&path("mix.master.gain"), &json!(6)).unwrap();
    let expected = COMMENTED.replacen(
        "seed = 7\n",
        "seed = 7\n\n[mix]\nparams = { \"master.gain\" = 6 }\n",
        1,
    );
    assert_ne!(expected, COMMENTED);
    assert_eq!(d.text(), expected);
    assert_eq!(d.song().mix.params["master.gain"], 6.0);
    assert_eq!(d.get(&path("mix.master.gain")), Some(json!(6)));
    assert_canonical_round_trip(&d);
    // The text is a valid song on its own, with the comments.
    let reparsed = SongDoc::parse(&d.text()).expect("edited text parses");
    assert_eq!(reparsed.song(), d.song());

    // More mix values land in the same table.
    d.set(&path("mix.delay.feedback"), &json!(30)).unwrap();
    d.set(&path("mix.master"), &json!("limit")).unwrap();
    let expected = COMMENTED.replacen(
        "seed = 7\n",
        "seed = 7\n\n[mix]\nparams = { \"master.gain\" = 6, \"delay.feedback\" = 30 }\n\
         master = \"limit\"\n",
        1,
    );
    assert_eq!(d.text(), expected);
    assert_eq!(d.get(&path("mix.master")), Some(json!("limit")));
    assert_eq!(d.get(&path("mix.delay.feedback")), Some(json!(30)));
    // An id left out reads as its default.
    assert_eq!(d.get(&path("mix.reverb.size")), Some(json!(25)));
    assert_canonical_round_trip(&d);

    // An unknown mix id is refused with the usual message.
    let before = d.text();
    let err = d.set(&path("mix.delay.fedback"), &json!(30)).unwrap_err();
    assert_eq!(
        err,
        EditError::UnknownParam {
            track: "mix".into(),
            instrument: "mix",
            param: "delay.fedback".into(),
        }
    );
    assert_eq!(
        err.to_string(),
        "`delay.fedback` is not a parameter of track `mix` (mix); see `tune params --json`"
    );
    assert_eq!(d.get(&path("mix.delay.fedback")), None);
    assert_eq!(d.text(), before);
}

/// `[mix]` follows `[song]` wherever `[song]` sits in the file.
#[test]
fn mix_table_follows_song() {
    let src = "# tracks first\n[tracks.k]\ninstrument = \"kick\"\nplay = [{ at = 1, clip = \"p\", bars = 1 }]\n\n\
               [song]\nversion = 2\nname = \"x\"\nbpm = 120\n\n\
               # the end\n[[sections]]\nname = \"a\"\nbars = 1\n\n\
               [clips.p]\nkind = \"drum\"\nsteps = \"x...\"\n";
    let mut d = SongDoc::parse(src).unwrap();
    d.set(&path("mix.master"), &json!("limit")).unwrap();
    assert_eq!(
        d.text(),
        src.replacen("bpm = 120\n", "bpm = 120\n\n[mix]\nmaster = \"limit\"\n", 1)
    );
}

/// Review Focus 5: a strip param on a track without a `params` key.
#[test]
fn set_strip_param_on_track_without_params() {
    let mut d = doc();
    d.set(&path("tracks.hat.send.delay"), &json!(-14)).unwrap();
    // The new key goes last, after the hat's `play`.
    assert_eq!(
        d.text(),
        COMMENTED.replacen(
            "  { at = 3, clip = \"h\", bars = 2 },\n]\n",
            "  { at = 3, clip = \"h\", bars = 2 },\n]\nparams = { \"send.delay\" = -14 }\n",
            1
        )
    );
    assert_eq!(d.song().tracks["hat"].strip["send.delay"], -14.0);
    assert_eq!(d.get(&path("tracks.hat.send.delay")), Some(json!(-14)));
    assert_eq!(d.get(&path("tracks.hat.send.reverb")), Some(json!(-60)));
    assert_canonical_round_trip(&d);

    // On a track with an inline `params`, the strip id joins it.
    d.set(&path("tracks.acid.duck.depth"), &json!(6)).unwrap();
    assert!(
        d.text()
            .contains("\"acid303.accent\" = 55, \"duck.depth\" = 6 }"),
        "{}",
        d.text()
    );
    assert_eq!(d.song().tracks["acid"].strip["duck.depth"], 6.0);
    for c in COMMENTS {
        assert!(d.text().contains(c), "lost `{c}`:\n{}", d.text());
    }
}

#[test]
fn set_eq_param() {
    let mut d = mixed();
    assert_eq!(d.get(&path("tracks.acid.eq.low_cut")), Some(json!(85)));
    d.set(&path("tracks.acid.eq.low_cut"), &json!(120)).unwrap();
    assert_eq!(
        d.text(),
        MIXED.replacen("\"eq.low_cut\" = 85", "\"eq.low_cut\" = 120", 1)
    );
    assert_eq!(d.song().tracks["acid"].fx[1].params["low_cut"], 120.0);
    assert_eq!(d.get(&path("tracks.acid.eq.low_cut")), Some(json!(120)));
    // An eq id the file leaves out is inserted into the eq entry.
    d.set(&path("tracks.acid.eq.high_cut"), &json!(9000))
        .unwrap();
    assert!(
        d.text().contains(
            "{ type = \"eq\", params = { \"eq.low_cut\" = 120, \"eq.high_cut\" = 9000 } }"
        ),
        "{}",
        d.text()
    );
    // The kick has no eq.
    assert_eq!(
        d.set(&path("tracks.kick.eq.low_cut"), &json!(120)),
        Err(EditError::NoSuchTarget("tracks.kick.eq.low_cut".into()))
    );
}

#[test]
fn set_master_mode_bad_value() {
    let mut d = doc();
    let err = d.set(&path("mix.master"), &json!("loud")).unwrap_err();
    assert!(matches!(err, EditError::Invalid(_)), "{err:?}");
    assert!(err.to_string().contains("unknown master mode"), "{err}");
    assert_eq!(d.text(), COMMENTED, "a refused edit changes nothing");
    assert_eq!(d.get(&path("mix.master")), Some(json!("clip")));
    assert!(matches!(
        d.set(&path("mix.master"), &json!(1)),
        Err(EditError::BadValue { .. })
    ));
}

/// Review Focus 5: `mix.duck_source=` clears the key, falling back to the
/// first kick.
#[test]
fn clear_duck_source() {
    let src = MIXED.replacen(
        "master = \"limit\"\n",
        "master = \"limit\"\nduck_source = \"hat\"\n",
        1,
    );
    assert_ne!(src, MIXED);
    let mut d = SongDoc::parse(&src).unwrap();
    assert_eq!(d.get(&path("mix.duck_source")), Some(json!("hat")));
    d.set(&path("mix.duck_source"), &Value::Null).unwrap();
    assert_eq!(d.text(), MIXED);
    assert_eq!(d.song().mix.duck_source, None);
    assert_eq!(d.get(&path("mix.duck_source")), Some(json!("kick")));
    // Setting it writes the key back into `[mix]`.
    d.set(&path("mix.duck_source"), &json!("hat")).unwrap();
    assert_eq!(d.text(), MIXED.replacen(
        "params = { \"delay.feedback\" = 40, \"reverb.size\" = 30, \"master.gain\" = 6 }\n",
        "params = { \"delay.feedback\" = 40, \"reverb.size\" = 30, \"master.gain\" = 6 }\nduck_source = \"hat\"\n",
        1,
    ));

    // Clearing on a file without `[mix]` adds nothing.
    let mut d = doc();
    d.set(&path("mix.duck_source"), &Value::Null).unwrap();
    assert_eq!(d.text(), COMMENTED);
    assert_eq!(d.get(&path("mix.duck_source")), Some(json!("kick")));
    assert!(matches!(
        d.set(&path("mix.duck_source"), &json!(3)),
        Err(EditError::BadValue { .. })
    ));
}

/// A device setting is addressed like a param: `tracks.<t>.<device>.<setting>`.
#[test]
fn fx_setting_path_is_generic() {
    let mut d = mixed();
    let before = d.text();
    d.set(&path("tracks.acid.dist.mode"), &json!("hard"))
        .unwrap();
    assert_eq!(
        d.text(),
        before.replacen("mode = \"soft\"", "mode = \"hard\"", 1)
    );
    assert_eq!(d.get(&path("tracks.acid.dist.mode")), Some(json!("hard")));
    assert_eq!(d.song().tracks["acid"].fx[0].settings, ["hard"]);

    let before = d.text();
    let err = d
        .set(&path("tracks.acid.eq.mode"), &json!("hard"))
        .unwrap_err();
    assert!(matches!(err, EditError::UnknownParam { .. }), "{err:?}");
    assert_eq!(d.get(&path("tracks.acid.eq.mode")), None);
    assert_eq!(d.text(), before);
}

#[test]
fn views_list_clips_and_what_tracks_play() {
    let view = serde_json::to_value(tune_studio::SongView::from(doc().song())).unwrap();
    let first = &view["patterns"][0];
    assert_eq!(first["kind"], "drum");
    assert_eq!(first["id"], "k4");
    assert!(first.get("track").is_none(), "a clip has no track: {first}");
    assert_eq!(view["tracks"][0]["plays"], json!(["drum", "notes"]));
    assert_eq!(view["tracks"][1]["plays"], json!(["acid", "notes"]));
}

/// A version 2 song with a second eq on the acid.
const V2: &str = r#"[song]
version = 2
name = "v2"
bpm = 120

[tracks.kick]
instrument = "kick"
play = [{ at = 1, clip = "k4", bars = 2 }]

[tracks.acid]
instrument = "acid303"
fx = [{ type = "eq", params = { "eq.low_cut" = 85 } }, { type = "eq", id = "eq2" }]
play = [{ at = 2, clip = "a1", bars = 1 }]

[clips.k4]
kind = "drum"
steps = "x...x...x...x..."

[clips.a1]
kind = "acid"
steps = "C2 C3"

[[sections]]
name = "a"
bars = 1

[[sections]]
name = "b"
bars = 1
"#;

/// Review Focus 3: a param of the second eq reaches that device and no
/// other.
#[test]
fn set_param_on_second_eq() {
    let mut d = SongDoc::parse(V2).expect("v2 parses");
    d.set(&path("tracks.acid.eq2.low_cut"), &json!(200))
        .unwrap();
    let text = d.text();
    assert_eq!(
        changed_lines(V2, &text),
        [
            "fx = [{ type = \"eq\", params = { \"eq.low_cut\" = 85 } }, { type = \"eq\", id = \"eq2\", params = { \"eq2.low_cut\" = 200 } }]"
        ]
    );
    assert_eq!(text.matches("eq2.low_cut").count(), 1, "{text}");
    assert_eq!(d.get(&path("tracks.acid.eq2.low_cut")), Some(json!(200)));
    assert_eq!(d.get(&path("tracks.acid.eq.low_cut")), Some(json!(85)));
    let fx = &d.song().tracks["acid"].fx;
    assert_eq!(fx[1].params["low_cut"], 200.0);
    assert_eq!(fx[0].params["low_cut"], 85.0);
}

#[test]
fn a_numbered_device_id_the_track_lacks_is_no_such_target() {
    let mut d = SongDoc::parse(V2).expect("v2 parses");
    // The kick has no eq2 (nor any eq); a typo'd name stays unknown.
    assert_eq!(
        d.set(&path("tracks.kick.eq2.low_cut"), &json!(120)),
        Err(EditError::NoSuchTarget("tracks.kick.eq2.low_cut".into()))
    );
    assert!(matches!(
        d.set(&path("tracks.acid.eq2.low_cutt"), &json!(120)),
        Err(EditError::UnknownParam { .. })
    ));
}

fn view_of(text: &str) -> Value {
    let d = SongDoc::parse(text).expect("song parses");
    serde_json::to_value(tune_studio::SongView::from(d.song())).unwrap()
}

/// Review Focus 3: every device in the views carries its id, and a second
/// eq's params carry that id, so the UI can tell the two apart.
#[test]
fn views_carry_device_ids() {
    let view = view_of(V2);
    let acid = &view["tracks"][1];
    assert_eq!(acid["id"], "acid");
    assert_eq!(acid["instrument"], "acid303");
    assert_eq!(acid["instrument_id"], "acid303");
    let fx = acid["fx"].as_array().unwrap();
    assert_eq!(fx[0]["id"], "eq");
    assert_eq!(fx[1]["id"], "eq2");
    assert_eq!(fx[1]["kind"], "eq");
    let ids: Vec<&str> = fx[1]["params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert!(!ids.is_empty());
    assert!(ids.iter().all(|id| id.starts_with("eq2.")), "{ids:?}");
    // A table-form instrument keeps its own id.
    let named = V2.replacen(
        "instrument = \"acid303\"",
        "instrument = { type = \"acid303\", id = \"bass\" }",
        1,
    );
    let acid = &view_of(&named)["tracks"][1];
    assert_eq!(acid["instrument"], "acid303");
    assert_eq!(acid["instrument_id"], "bass");
    assert!(
        acid["params"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["id"].as_str().unwrap().starts_with("bass.")),
        "{acid}"
    );
}

/// A placement that covers only half of section `b` is not what any
/// section plays; the aligned ones name their section.
#[test]
fn loose_placements_have_no_section() {
    let from = "play = [{ at = 3, clip = \"a1\", bars = 2 }]";
    assert!(COMMENTED.contains(from));
    let text = COMMENTED.replacen(
        from,
        "play = [{ at = 4, clip = \"a1\", bars = 1, offset = 1 }]",
        1,
    );
    let view = view_of(&text);
    assert_eq!(
        view["tracks"][1]["placements"],
        json!([{ "start_bar": 3, "bars": 1, "clip": "a1", "offset": 1, "in_section": null }])
    );
    assert_eq!(
        view["tracks"][0]["placements"],
        json!([
            { "start_bar": 0, "bars": 2, "clip": "k4", "offset": 0, "in_section": 0 },
            { "start_bar": 2, "bars": 2, "clip": "k4", "offset": 0, "in_section": 1 },
        ])
    );
    assert!(view["sections"][1]["play"].get("acid").is_none());
    // One placement over both sections is shown by the first; the second
    // plays it mid-clip, so it is loose there.
    let text = COMMENTED.replacen(from, "play = [{ at = 1, clip = \"a1\", bars = 4 }]", 1);
    let view = view_of(&text);
    assert_eq!(view["tracks"][1]["placements"][0]["in_section"], 0);
    assert_eq!(view["sections"][0]["play"]["acid"], "a1");
    assert!(view["sections"][1]["play"].get("acid").is_none());
    // With an offset, no section shows it.
    let text = COMMENTED.replacen(
        from,
        "play = [{ at = 1, clip = \"a1\", bars = 4, offset = 1 }]",
        1,
    );
    let view = view_of(&text);
    assert_eq!(
        view["tracks"][1]["placements"][0]["in_section"],
        Value::Null
    );
    assert!(view["sections"][0]["play"].get("acid").is_none());
}

/// A song with one 4-bar section, then another, and one cutoff stretch.
fn four_bar_sections(at: u32, bars: u32) -> String {
    format!(
        r#"[song]
version = 2
name = "lanes"
bpm = 120

[tracks.acid]
instrument = "acid303"
play = [{{ at = 1, clip = "a1", bars = 8 }}]

[clips.a1]
kind = "acid"
steps = "C2 C3"

[[sections]]
name = "a"
bars = 4

[[sections]]
name = "b"
bars = 4

[[automation]]
param = "acid.acid303.cutoff"
at = {at}
bars = {bars}
points = [[0, 0.1], [2, 0.8]]
"#
    )
}

/// Every stretch that overlaps a section is listed there, its points
/// shifted to the section start and its part of the section in `from`/`to`.
#[test]
fn lane_views_carry_their_extent() {
    // The second half of section `a`.
    let view = view_of(&four_bar_sections(3, 2));
    assert_eq!(
        view["sections"][0]["lanes"],
        json!([{
            "track": "acid",
            "param": "acid303.cutoff",
            "points": [[2.0, 0.1], [4.0, 0.8]],
            "from": 2.0,
            "to": 4.0,
        }])
    );
    assert_eq!(view["sections"][1]["lanes"], json!([]));
    // Across the edge: each section shows its part.
    let view = view_of(&four_bar_sections(3, 4));
    let lanes = |i: usize| view["sections"][i]["lanes"][0].clone();
    assert_eq!(lanes(0)["points"], json!([[2.0, 0.1], [4.0, 0.8]]));
    assert_eq!(
        (lanes(0)["from"].clone(), lanes(0)["to"].clone()),
        (json!(2.0), json!(4.0))
    );
    assert_eq!(lanes(1)["points"], json!([[-2.0, 0.1], [0.0, 0.8]]));
    assert_eq!(
        (lanes(1)["from"].clone(), lanes(1)["to"].clone()),
        (json!(0.0), json!(2.0))
    );
}

/// A stretch split at a section edge writes the value there as a short
/// decimal, not with float noise (`0.1 + 0.4 * 4 / 8` is
/// `0.30000000000000004`), and the doc's song equals its text.
#[test]
fn a_split_lane_value_is_written_rounded() {
    let text = four_bar_sections(1, 8).replacen("[[0, 0.1], [2, 0.8]]", "[[0, 0.1], [8, 0.5]]", 1);
    let mut d = SongDoc::parse(&text).unwrap();
    d.set(&path("sections[0].bars"), &json!(5)).unwrap();
    let out = d.text();
    assert!(out.contains("points = [[0, 0.1], [4, 0.3]]"), "{out}");
    assert!(out.contains("points = [[0, 0.3], [4, 0.5]]"), "{out}");
    assert!(!out.contains("0.30000000000000004"), "{out}");
    assert_eq!(SongDoc::parse(&out).unwrap().song(), d.song());
}

/// A version 2 song with a drum clip and a two-note notes clip, with a
/// comment on each key the note-clip tests edit.
const NOTE_SONG: &str = r#"[song]
version = 2
name = "notes"
bpm = 120

[tracks.kick]
instrument = "kick"
play = [{ at = 1, clip = "k4", bars = 1 }]

[tracks.acid]
instrument = "acid303"
play = [{ at = 1, clip = "lead", bars = 1 }]

[clips.k4]
kind = "drum"
steps = "x...x...x...x..."

[clips.lead]
kind = "notes"
bars = 1 # one bar
notes = [
  { start = 0, pitch = "C3", len = 3 },
  { start = 2.5, pitch = "D#3", len = 2, vel = 120 },
]

[[sections]]
name = "a"
bars = 1
"#;

#[test]
fn clip_notes_paths() {
    for p in ["clips.lead.notes", "clips.lead.bars"] {
        assert_eq!(path(p).to_string(), p);
    }
    for bad in ["clips.lead.note", "clips..notes", "patterns.lead.notes"] {
        assert!(bad.parse::<SongPath>().is_err(), "{bad}");
    }

    let mut d = SongDoc::parse(NOTE_SONG).unwrap();
    assert_eq!(
        d.get(&path("clips.lead.notes")),
        Some(json!([
            { "start": 0, "pitch": "C3", "len": 3, "vel": 100 },
            { "start": 2.5, "pitch": "D#3", "len": 2, "vel": 120 },
        ]))
    );
    assert_eq!(d.get(&path("clips.lead.bars")), Some(json!(1)));
    // Null for the other kinds, and no steps for a notes clip.
    assert_eq!(d.get(&path("clips.k4.notes")), Some(Value::Null));
    assert_eq!(d.get(&path("clips.k4.bars")), Some(Value::Null));
    assert_eq!(d.get(&path("clips.lead.steps")), Some(Value::Null));
    assert_eq!(d.get(&path("clips.nope.notes")), None);

    let before = d.text();
    d.set(
        &path("clips.lead.notes"),
        &json!([
            { "start": 0, "pitch": "C3", "len": 2 },
            { "start": 4, "pitch": "G3", "len": 1, "vel": 120 },
        ]),
    )
    .unwrap();
    let after = d.text();
    assert_eq!(
        after,
        before.replace(
            "[\n  { start = 0, pitch = \"C3\", len = 3 },\n  { start = 2.5, pitch = \"D#3\", len = 2, vel = 120 },\n]",
            "[\n  { start = 0, pitch = \"C3\", len = 2 },\n  { start = 4, pitch = \"G3\", len = 1, vel = 120 },\n]",
        )
    );
    assert_ne!(after, before);
    assert!(after.contains("bars = 1 # one bar"));

    // Bars: an integer, comment kept.
    d.set(&path("clips.lead.bars"), &json!(2)).unwrap();
    assert!(d.text().contains("bars = 2 # one bar"), "{}", d.text());
    assert_eq!(d.get(&path("clips.lead.bars")), Some(json!(2)));
}

#[test]
fn clip_notes_refuse_bad_values_and_kinds() {
    let mut d = SongDoc::parse(NOTE_SONG).unwrap();
    let before = d.text();
    let err = d
        .set(
            &path("clips.lead.notes"),
            &json!([{ "start": 0.3, "pitch": "C3", "len": 1 }]),
        )
        .unwrap_err();
    assert!(matches!(err, EditError::Invalid(_)), "{err:?}");
    assert!(err.to_string().contains("off the grid"), "{err}");
    assert_eq!(d.text(), before);

    let msg = |d: &mut SongDoc, p: &str, v: Value| match d.set(&path(p), &v).unwrap_err() {
        EditError::BadValue { message, .. } => message,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        msg(&mut d, "clips.k4.notes", json!([])),
        "clip `k4` is a drum clip: it has no notes"
    );
    assert_eq!(
        msg(&mut d, "clips.k4.bars", json!(2)),
        "clip `k4` is a drum clip: it has no bars"
    );
    assert_eq!(
        msg(&mut d, "clips.lead.steps", json!("x...")),
        "clip `lead` is a notes clip: it has no steps (set clips.lead.notes)"
    );
    assert_eq!(
        msg(&mut d, "clips.lead.notes", json!("C3")),
        "expected an array of notes, each {start, pitch, len, vel}"
    );
    assert_eq!(
        msg(
            &mut d,
            "clips.lead.notes",
            json!([{ "start": 0, "pitch": "C3", "len": 1, "foo": 1 }])
        ),
        "a note has start, pitch, len and vel, not `foo`"
    );
    assert_eq!(d.text(), before);
    assert!(matches!(
        d.set(&path("clips.nope.notes"), &json!([])).unwrap_err(),
        EditError::NoSuchTarget(_)
    ));
}

#[test]
fn views_show_note_clips() {
    let doc = SongDoc::parse(NOTE_SONG).expect("note song parses");
    let view = serde_json::to_value(tune_studio::SongView::from(doc.song())).unwrap();
    assert_eq!(
        view["patterns"][1],
        json!({
            "kind": "notes",
            "id": "lead",
            "bars": 1,
            "notes": [
                {"start": 0.0, "pitch": 48, "len": 3.0, "vel": 100},
                {"start": 2.5, "pitch": 51, "len": 2.0, "vel": 120},
            ],
        })
    );
}

const SESSION: &str = include_str!("../../tune-song/tests/fixtures/session.toml");

fn session() -> SongDoc {
    SongDoc::parse(SESSION).expect("session fixture parses")
}

#[test]
fn scene_paths_parse_and_print() {
    for s in ["scenes", "scenes.groove", "scenes.groove.slots.acid"] {
        assert_eq!(path(s).to_string(), s);
    }
    assert_eq!(
        path("scenes.groove.slots.acid"),
        SongPath::SceneSlot {
            scene: "groove".into(),
            track: "acid".into()
        }
    );
    assert!(path("scenes").is_read_only());
    assert!(path("scenes.groove").is_read_only());
    assert!(!path("scenes.groove.slots.acid").is_read_only());
    for bad in [
        "scenes.Groove.slots.acid",
        "scenes.groove.acid",
        "scenes.groove.slots",
        "scenes.groove.slots.",
        "scenes.groove.slots.Acid",
        "scenes..slots.acid",
        "scenes.",
    ] {
        let err = bad.parse::<SongPath>().expect_err(bad);
        assert!(err.contains("scenes.<scene>.slots.<track>"), "{err}");
    }
}

#[test]
fn scene_values() {
    let d = session();
    let get = |s: &str| d.get(&path(s));
    assert_eq!(get("scenes"), Some(json!(["groove", "break"])));
    assert_eq!(
        get("scenes.groove"),
        Some(json!({ "slots": { "kick": "k4", "acid": "a1" } }))
    );
    assert_eq!(get("scenes.groove.slots.acid"), Some(json!("a1")));
    assert_eq!(get("scenes.groove.slots.hat"), Some(Value::Null));
    assert_eq!(get("scenes.nope.slots.acid"), None);
    assert_eq!(get("scenes.nope"), None);
    let plain = SongDoc::parse(COMMENTED).unwrap();
    assert_eq!(plain.get(&path("scenes")), Some(json!([])));
}

#[test]
fn set_a_slot_in_place() {
    let mut d = session();
    d.set(&path("scenes.groove.slots.hat"), &json!("h"))
        .unwrap();
    assert_eq!(
        d.text(),
        SESSION.replace(
            "slots = { kick = \"k4\", acid = \"a1\" }",
            "slots = { kick = \"k4\", acid = \"a1\", hat = \"h\" }"
        )
    );
    assert_eq!(d.song().scenes["groove"].slots["hat"], "h");

    // Replacing an existing slot keeps the key's place.
    let mut d = session();
    d.set(&path("scenes.groove.slots.kick"), &json!("k4"))
        .unwrap();
    assert_eq!(d.text(), SESSION);

    let mut d = session();
    d.set(&path("scenes.groove.slots.acid"), &Value::Null)
        .unwrap();
    assert_eq!(
        d.text(),
        SESSION.replace(
            "slots = { kick = \"k4\", acid = \"a1\" }",
            "slots = { kick = \"k4\" }"
        )
    );
    // The empty string clears too.
    let mut e = session();
    e.set(&path("scenes.groove.slots.acid"), &json!(""))
        .unwrap();
    assert_eq!(e.text(), d.text());

    let mut d = session();
    d.set(&path("scenes.break.slots.hat"), &Value::Null)
        .unwrap();
    assert_eq!(d.text(), SESSION.replace("slots = { hat = \"h\" }\n", ""));
    assert!(d.text().contains("# hats alone\n[scenes.break]\n"));
    assert!(d.song().scenes["break"].slots.is_empty());

    // A scene without `slots` gets them, and clearing the last one takes them away again.
    let bare = SESSION.replace("slots = { hat = \"h\" }\n", "");
    let mut d = SongDoc::parse(&bare).unwrap();
    d.set(&path("scenes.break.slots.hat"), &json!("h")).unwrap();
    assert_eq!(d.text(), SESSION);
    d.set(&path("scenes.break.slots.hat"), &Value::Null)
        .unwrap();
    assert_eq!(d.text(), bare);
    // Clearing an empty slot changes nothing.
    d.set(&path("scenes.break.slots.hat"), &Value::Null)
        .unwrap();
    assert_eq!(d.text(), bare);
}

#[test]
fn set_a_slot_in_a_slots_table() {
    let tabled = SESSION.replace(
        "slots = { kick = \"k4\", acid = \"a1\" }",
        "\n[scenes.groove.slots]\nkick = \"k4\" # the floor\nacid = \"a1\"",
    );
    let tabled = tabled.replace("# jam rows\n\n", "# jam rows\n");
    let mut d = SongDoc::parse(&tabled).unwrap();
    d.set(&path("scenes.groove.slots.hat"), &json!("h"))
        .unwrap();
    assert_eq!(
        d.text(),
        tabled.replace("acid = \"a1\"\n", "acid = \"a1\"\nhat = \"h\"\n")
    );
    d.set(&path("scenes.groove.slots.kick"), &Value::Null)
        .unwrap();
    assert!(
        d.text().contains("acid = \"a1\"\nhat = \"h\"\n"),
        "{}",
        d.text()
    );
    assert!(!d.text().contains("the floor"), "{}", d.text());
    assert_eq!(d.song().scenes["groove"].slots.len(), 2);
}

#[test]
fn slot_errors() {
    let mut d = session();
    let err = d
        .set(&path("scenes.nope.slots.acid"), &json!("a1"))
        .unwrap_err();
    assert_eq!(
        err,
        EditError::NoSuchTarget("scenes.nope.slots.acid".into())
    );
    let err = d
        .set(&path("scenes.groove.slots.acid"), &json!("k4"))
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("clip `k4` is a drum clip; track `acid` (acid303) plays acid clips"),
        "{err}"
    );
    let err = d
        .set(&path("scenes.groove.slots.bass"), &json!("k4"))
        .unwrap_err();
    assert!(err.to_string().contains("unknown track `bass`"), "{err}");
    let err = d
        .set(&path("scenes.groove.slots.hat"), &json!(3))
        .unwrap_err();
    assert!(matches!(err, EditError::BadValue { .. }), "{err}");
    let err = d.set(&path("scenes.groove"), &json!({})).unwrap_err();
    assert!(matches!(err, EditError::ReadOnly(_)), "{err}");
    assert_eq!(d.text(), SESSION);
}

#[test]
fn clearing_an_absent_slot_changes_no_text() {
    let inline = SESSION.replace("slots = { hat = \"h\" }", "slots = {}");
    let tabled = SESSION.replace("slots = { hat = \"h\" }", "[scenes.break.slots] # none");
    for text in [inline, tabled] {
        let mut d = SongDoc::parse(&text).unwrap();
        d.set(&path("scenes.break.slots.hat"), &Value::Null)
            .unwrap();
        assert_eq!(d.text(), text);
    }
}

#[test]
fn clearing_the_last_slot_of_a_table_keeps_the_table() {
    let tabled = SESSION.replace(
        "slots = { hat = \"h\" }",
        "[scenes.break.slots] # the hats\nhat = \"h\"",
    );
    let mut d = SongDoc::parse(&tabled).unwrap();
    d.set(&path("scenes.break.slots.hat"), &Value::Null)
        .unwrap();
    assert_eq!(d.text(), tabled.replace("hat = \"h\"\n", ""));
    assert!(d.text().contains("[scenes.break.slots] # the hats"));
    assert!(d.song().scenes["break"].slots.is_empty());
}

#[test]
fn clearing_a_non_last_inline_slot_keeps_the_rest() {
    let mut d = session();
    d.set(&path("scenes.groove.slots.kick"), &Value::Null)
        .unwrap();
    assert_eq!(
        d.text(),
        SESSION.replace(
            "slots = { kick = \"k4\", acid = \"a1\" }",
            "slots = { acid = \"a1\" }"
        )
    );
}
