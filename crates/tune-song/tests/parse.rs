//! Parsing and validation of song files.

use std::path::Path;

use tune_dsp::DistortionMode;
use tune_dsp::acid303::AcidPattern;
use tune_dsp::drums::DrumPattern;
use tune_song::{
    FxSpec, InstrumentKind, LoadError, PatternSteps, Song, SongError, load_song, parse_song,
};

const MINI: &str = include_str!("fixtures/mini.toml");

fn mini() -> Song {
    parse_song(MINI).unwrap_or_else(|e| panic!("mini.toml must parse: {e:?}"))
}

/// `MINI` with the first occurrence of `from` replaced by `to`.
fn swap(from: &str, to: &str) -> String {
    assert!(
        MINI.contains(from),
        "mutation target `{from}` not in mini.toml"
    );
    MINI.replacen(from, to, 1)
}

fn messages(errors: &[SongError]) -> String {
    errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn parses_mini() {
    let song = mini();
    assert_eq!(song.name, "mini");
    assert_eq!(song.bpm, 120.0);
    assert_eq!(song.seed, 7);
    assert_eq!(song.sample_rate, None);

    let ids: Vec<&str> = song.tracks.keys().map(String::as_str).collect();
    assert_eq!(ids, ["kick", "acid", "hat"]);

    let kick = &song.tracks["kick"];
    assert_eq!(kick.instrument, InstrumentKind::Kick);
    assert_eq!(kick.gain_db, -3.0);
    assert_eq!(kick.pan, 0.0);
    assert!(!kick.mute && !kick.solo);
    assert!(kick.params.is_empty());
    assert!(kick.fx.is_empty());

    let acid = &song.tracks["acid"];
    assert_eq!(acid.instrument, InstrumentKind::Acid303);
    assert_eq!(acid.gain_db, 0.0);
    let params: Vec<(&str, f64)> = acid.params.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(
        params,
        [
            ("acid303.cutoff", 500.0),
            ("acid303.resonance", 70.0),
            ("acid303.env_mod", 50.0),
            ("acid303.decay", 400.0),
            ("acid303.accent", 55.0),
        ]
    );
    assert_eq!(acid.fx.len(), 1);
    let FxSpec::Distortion { mode, params } = &acid.fx[0];
    assert_eq!(*mode, DistortionMode::Soft);
    assert_eq!(params.get("dist.drive"), Some(&12.0));
    assert_eq!(params.len(), 1);
    assert_eq!(acid.fx[0].type_name(), "distortion");

    let hat = &song.tracks["hat"];
    assert_eq!(hat.instrument, InstrumentKind::HatClosed);
    assert_eq!(hat.gain_db, -10.0);
    assert_eq!(hat.pan, 0.3);

    let pids: Vec<&str> = song.patterns.keys().map(String::as_str).collect();
    assert_eq!(pids, ["k4", "a1", "h"]);
    assert_eq!(song.patterns["k4"].track, "kick");
    assert_eq!(
        song.patterns["k4"].steps,
        PatternSteps::Drum(DrumPattern::parse("x...x...x...X...").unwrap())
    );
    assert_eq!(song.patterns["a1"].track, "acid");
    assert_eq!(
        song.patterns["a1"].steps,
        PatternSteps::Acid(AcidPattern::parse("C2 C2! C3~ C2 . D#2 C2~ G2!").unwrap())
    );
    assert_eq!(
        song.patterns["h"].steps,
        PatternSteps::Drum(DrumPattern::parse("..x...x...x...x.").unwrap())
    );

    assert_eq!(song.sections.len(), 2);
    let a = &song.sections[0];
    assert_eq!((a.name.as_str(), a.bars), ("a", 2));
    let play: Vec<(&str, &str)> = a
        .play
        .iter()
        .map(|(t, p)| (t.as_str(), p.as_str()))
        .collect();
    assert_eq!(play, [("kick", "k4"), ("hat", "h")]);
    assert!(a.automation.is_empty());

    let b = &song.sections[1];
    assert_eq!((b.name.as_str(), b.bars), ("b", 2));
    let play: Vec<(&str, &str)> = b
        .play
        .iter()
        .map(|(t, p)| (t.as_str(), p.as_str()))
        .collect();
    assert_eq!(play, [("kick", "k4"), ("acid", "a1"), ("hat", "h")]);
    assert_eq!(b.automation.len(), 1);
    let lane = &b.automation[0];
    assert_eq!(lane.track, "acid");
    assert_eq!(lane.param, "acid303.cutoff");
    assert_eq!(lane.points, [(0.0, 0.1), (2.0, 0.8)]);

    assert_eq!(song.total_bars(), 4);
    assert_eq!(song.section_start_bars(), [0, 2]);
}

#[test]
fn instrument_kind_names_round_trip() {
    for kind in [
        InstrumentKind::Acid303,
        InstrumentKind::Kick,
        InstrumentKind::Clap,
        InstrumentKind::HatClosed,
        InstrumentKind::HatOpen,
    ] {
        assert_eq!(InstrumentKind::from_name(kind.name()), Some(kind));
        assert_eq!(kind.is_drum(), kind != InstrumentKind::Acid303);
        assert!(
            kind.params()
                .iter()
                .all(|p| p.id.starts_with(&format!("{}.", kind.name())))
        );
    }
    assert_eq!(InstrumentKind::from_name("tuba"), None);
}

#[test]
fn integers_and_floats_accepted() {
    let int = parse_song(&swap("bpm = 120", "bpm = 145")).unwrap();
    let float = parse_song(&swap("bpm = 120", "bpm = 145.0")).unwrap();
    assert_eq!(int.bpm, 145.0);
    assert_eq!(int, float);
}

#[test]
fn lane_on_fx_param_resolves() {
    let src = swap(
        "param = \"acid.acid303.cutoff\"",
        "param = \"acid.dist.drive\"",
    );
    let song = parse_song(&src).unwrap();
    assert_eq!(song.sections[1].automation[0].param, "dist.drive");
}

/// Review Focus 5: many mistakes in one file (an unknown key typo, an error
/// inside an inline table and one inside `[[sections.automation]]`) are all
/// reported together, in line order, with 1-based positions.
#[test]
fn reports_all_errors_with_positions() {
    let src = r#"[song]
name = "errs"
bpm = 120
[tracks.kick]
instrument = "kick"
gian_db = -3
params = { "kick.tune" = 100 }

[patterns.k4]
track = "kick"
steps = "x...x...x...x..."
[[sections]]
name = "a"
play = { kick = "nope" }
bars = 2
[[sections.automation]]
param = "kick.kick.tune"
points = [[0, 0.1], [3, 0.5]]
"#;
    let lines: Vec<&str> = src.lines().collect();
    // 1-based column of `needle` on 1-based line `line`, counted in chars.
    let col = |line: usize, needle: &str| {
        let l = lines[line - 1];
        l[..l.find(needle).unwrap()].chars().count() + 1
    };

    let errors = parse_song(src).expect_err("four mistakes");
    let all = messages(&errors);
    let got: Vec<Option<usize>> = errors.iter().map(|e| e.line).collect();
    assert_eq!(got, [Some(6), Some(7), Some(14), Some(18)], "{all}");

    assert!(
        errors[0].message.contains("unknown field `gian_db`"),
        "{all}"
    );
    assert_eq!(errors[0].column, Some(1), "{all}");

    assert!(errors[1].message.contains("is out of range"), "{all}");
    assert!(errors[1].message.contains("kick.tune"), "{all}");
    assert_eq!(errors[1].column, Some(col(7, "100")), "{all}");

    assert!(
        errors[2].message.contains("unknown pattern `nope`"),
        "{all}"
    );
    assert_eq!(errors[2].column, Some(col(14, "\"nope\"")), "{all}");

    assert!(errors[3].message.contains("outside the section"), "{all}");
    assert_eq!(errors[3].column, Some(col(18, "3, 0.5")), "{all}");
}

#[test]
fn columns_count_chars_not_bytes() {
    let src = "[song]\nname = \"äöü\"; bpm = 120\n";
    // The `;` is a syntax error after three two-byte chars.
    let errors = parse_song(src).unwrap_err();
    assert_eq!(errors.len(), 1, "{}", messages(&errors));
    assert_eq!(errors[0].line, Some(2));
    assert_eq!(errors[0].column, Some("name = \"äöü\"".chars().count() + 1));
}

#[test]
fn syntax_error_has_position() {
    let errors = parse_song("[song\nname = 1").unwrap_err();
    assert_eq!(errors.len(), 1, "{}", messages(&errors));
    assert_eq!(errors[0].line, Some(1));
    assert!(errors[0].column.is_some());
}

#[test]
fn validation_rules() {
    let duplicated_fx = swap(
        "fx = [{ type = \"distortion\", mode = \"soft\", params = { \"dist.drive\" = 12 } }]",
        "fx = [{ type = \"distortion\" }, { type = \"distortion\", mode = \"hard\" }]",
    );
    let no_sections = MINI.split("[[sections]]").next().unwrap().to_owned();
    let rows: Vec<(String, &str)> = vec![
        (
            swap("track = \"hat\"", "track = \"ghost\""),
            "unknown track `ghost`",
        ),
        (
            swap("hat = \"h\" }", "snare = \"h\" }"),
            "unknown track `snare`",
        ),
        (
            swap("hat = \"h\" }", "hat = \"zz\" }"),
            "unknown pattern `zz`",
        ),
        (
            swap("kick = \"k4\", hat", "kick = \"h\", hat"),
            "pattern `h` belongs to track `hat`",
        ),
        (
            swap(
                "gain_db = -3",
                "gain_db = -3\nparams = { \"kick.nope\" = 1 }",
            ),
            "unknown parameter `kick.nope` for kick",
        ),
        (
            swap("\"acid303.cutoff\" = 500", "\"acid303.cutoff\" = 5000"),
            "is out of range",
        ),
        (
            swap("gain_db = -3", "gian_db = -3"),
            "unknown field `gian_db`",
        ),
        (
            swap("instrument = \"kick\"", "instrument = \"tuba\""),
            "unknown instrument `tuba`",
        ),
        (
            swap("\"x...x...x...X...\"", "\"x...x...o...X...\""),
            "bad step",
        ),
        (swap("C2 C2! C3~", "C2 H2! C3~"), "bad step"),
        (
            swap("steps = \"..x...x...x...x.\"", "steps = \"\""),
            "needs 1 to 64 steps",
        ),
        (
            swap(
                "steps = \"..x...x...x...x.\"",
                &format!("steps = \"{}\"", "x".repeat(65)),
            ),
            "needs 1 to 64 steps",
        ),
        (swap("bars = 2", "bars = 0"), "bars must be 1 to 1024"),
        (swap("bars = 2", "bars = 1025"), "bars must be 1 to 1024"),
        (swap("bpm = 120", "bpm = 400"), "bpm"),
        (swap("bpm = 120", "bpm = 39.5"), "bpm"),
        (swap("[2, 0.8]", "[3, 0.8]"), "outside the section"),
        (
            swap("[[0, 0.1], [2, 0.8]]", "[[1, 0.1], [0.5, 0.8]]"),
            "go backwards",
        ),
        (swap("[2, 0.8]", "[2, 1.5]"), "outside 0..=1"),
        (duplicated_fx, "only one distortion"),
        (
            swap("[tracks.hat]", "[tracks.Hat]"),
            "may only contain a-z, 0-9 and _",
        ),
        (
            swap("[patterns.k4]", "[patterns.k-4]"),
            "may only contain a-z, 0-9 and _",
        ),
        (swap("bars = 2", "bars = 1024"), "longer than 30 minutes"),
        (no_sections, "at least one section"),
    ];
    for (src, needle) in &rows {
        match parse_song(src) {
            Ok(_) => panic!("expected an error containing `{needle}`, got Ok"),
            Err(errors) => {
                let all = messages(&errors);
                assert!(all.contains(needle), "`{needle}` not in:\n{all}");
            }
        }
    }
}

#[test]
fn load_song_reads_the_fixture() {
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mini.toml"
    ));
    assert_eq!(load_song(path).unwrap(), mini());
}

#[test]
fn load_song_io_error() {
    let missing = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/missing.toml"
    ));
    match load_song(missing) {
        Err(LoadError::Io { path, .. }) => assert_eq!(path, missing),
        other => panic!("expected LoadError::Io, got {other:?}"),
    }
}

#[test]
fn load_song_invalid_error() {
    let dir = std::env::temp_dir().join(format!("tune-song-invalid-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.toml");
    std::fs::write(&path, swap("bpm = 120", "bpm = 1")).unwrap();
    let result = load_song(&path);
    std::fs::remove_dir_all(&dir).unwrap();
    match result {
        Err(LoadError::Invalid(errors)) => assert!(messages(&errors).contains("bpm")),
        other => panic!("expected LoadError::Invalid, got {other:?}"),
    }
}
