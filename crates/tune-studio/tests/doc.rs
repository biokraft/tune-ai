//! In-place song edits: paths, comment preservation, validation and section
//! moves.

use serde_json::{Value, json};
use tune_song::SectionPlay;
use tune_studio::{EditError, SongDoc, SongPath};

const COMMENTED: &str = include_str!("fixtures/commented.toml");

/// The four comments the fixture adds to the mini song.
const COMMENTS: [&str; 4] = [
    "# the body\n[tracks.kick]",
    "gain_db = -3 # headroom\n",
    "# the squelch\n[tracks.acid]",
    "# acid enters\n[[sections]]",
];

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
        "patterns.a1.steps",
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
    assert_eq!(doc().text(), COMMENTED);
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
    d.set(&path("sections[0].play.acid"), &json!("a1")).unwrap();
    assert_eq!(d.song().section_play(0, "acid"), SectionPlay::Clip("a1"));
    assert!(
        d.text()
            .contains("play = { kick = \"k4\", hat = \"h\", acid = \"a1\" }"),
        "{}",
        d.text()
    );
}

#[test]
fn move_section_carries_comments_and_lanes() {
    let mut d = doc();
    d.move_section(1, 0).unwrap();
    assert_eq!(names(&d), ["b", "a"]);
    let text = d.text();
    let comment = text.find("# acid enters").expect("comment kept");
    let first = text.find("[[sections]]").unwrap();
    assert!(
        comment < first,
        "comment must lead the first section:\n{text}"
    );
    assert!(
        text.contains("# acid enters\n[[sections]]\nname = \"b\""),
        "{text}"
    );
    let song = d.song();
    assert_eq!(song.lanes_in(0).len(), 1);
    assert_eq!(song.lanes_in(0)[0].param, "acid303.cutoff");
    assert!(song.lanes_in(1).is_empty());
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
    // Each copy's lane follows its own header.
    let text = d.text();
    let tail = &text[text.find("# acid enters").unwrap()..];
    let order: Vec<&str> = tail.lines().filter(|l| l.starts_with("[[")).collect();
    assert_eq!(
        order,
        [
            "[[sections]]",
            "[[sections.automation]]",
            "[[sections]]",
            "[[sections.automation]]"
        ]
    );
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
    d.set(&path("patterns.h.steps"), &json!("x.x.x.x.x.x.x.x."))
        .unwrap();
    // Read back in the canonical notation (groups of four).
    assert_eq!(
        d.get(&path("patterns.h.steps")),
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
        "patterns.zz.steps",
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
    for p in ["sections", "tracks.acid", "sections[1].lanes"] {
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

#[test]
fn move_section_with_interleaved_tables() {
    // A pattern table between the sections, and a comment on a lane.
    let src = "[song]\nname = \"x\"\nbpm = 120\n\n[tracks.acid]\ninstrument = \"acid303\"\n\n\
               [[sections]]\nname = \"a\"\nbars = 1\n\n# lane of a\n[[sections.automation]]\n\
               param = \"acid.acid303.cutoff\"\npoints = [[0, 0.5]]\n\n[patterns.p]\ntrack = \"acid\"\n\
               steps = \"C2\"\n\n[[sections]]\nname = \"b\"\nbars = 2\nplay = { acid = \"p\" }\n";
    let mut d = SongDoc::parse(src).unwrap();
    d.move_section(0, 1).unwrap();
    assert_eq!(names(&d), ["b", "a"]);
    let song = d.song();
    assert!(song.lanes_in(0).is_empty());
    assert_eq!(song.lanes_in(1).len(), 1);
    assert_eq!(song.section_play(0, "acid"), SectionPlay::Clip("p"));
    let text = d.text();
    assert!(
        text.contains("# lane of a\n[[sections.automation]]"),
        "{text}"
    );
    d.move_section(1, 0).unwrap();
    assert_eq!(d.text(), src);
}

#[test]
fn inline_sections_array() {
    let src = "sections = [{ name = \"a\", bars = 1 }, { name = \"b\", bars = 2 }]\n\n\
               [song]\nname = \"x\"\nbpm = 120\n";
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

const MIXED: &str = include_str!("../../tune-song/tests/fixtures/mixed.toml");

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
    let src = "# tracks first\n[tracks.k]\ninstrument = \"kick\"\n\n\
               [song]\nname = \"x\"\nbpm = 120\n\n\
               # the end\n[[sections]]\nname = \"a\"\nbars = 1\nplay = { k = \"p\" }\n\n\
               [patterns.p]\ntrack = \"k\"\nsteps = \"x...\"\n";
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
    assert_eq!(
        d.text(),
        COMMENTED.replacen(
            "pan = 0.3\n",
            "pan = 0.3\nparams = { \"send.delay\" = -14 }\n",
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
    assert_eq!(view["tracks"][0]["plays"], json!(["drum"]));
    assert_eq!(view["tracks"][1]["plays"], json!(["acid"]));
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
fx = [{ type = "eq" }, { type = "eq", id = "eq2", params = { "eq2.low_cut" = 200 } }]
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

#[test]
fn v2_files_refuse_arrangement_edits_for_now() {
    let mut d = SongDoc::parse(V2).expect("v2 parses");
    assert_eq!(d.version(), 2);
    for (p, v) in [
        ("sections[0].bars", json!(2)),
        ("sections[1].play.acid", json!("a1")),
        ("sections[1].play.acid", Value::Null),
        ("patterns.a1.steps", json!("C2")),
    ] {
        assert_eq!(
            d.set(&path(p), &v),
            Err(EditError::NotYetOnV2(p.to_owned())),
            "{p}"
        );
    }
    assert_eq!(
        d.move_section(0, 1).unwrap_err().to_string(),
        "`moving a section` is not yet supported on v2 files"
    );
    assert!(matches!(
        d.duplicate_section(0),
        Err(EditError::NotYetOnV2(_))
    ));
    assert!(matches!(d.delete_section(0), Err(EditError::NotYetOnV2(_))));
    assert_eq!(d.text(), V2);

    // Edits that touch no arrangement still work.
    d.set(&path("sections[1].name"), &json!("drop")).unwrap();
    d.set(&path("tracks.acid.eq2.low_cut"), &json!(300))
        .unwrap();
    assert_eq!(d.song().tracks["acid"].fx[1].params["low_cut"], 300.0);
    assert!(d.song().tracks["acid"].fx[0].params.is_empty());
}
