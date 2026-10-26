//! Parsing and validation of song files.

use std::path::Path;

use tune_dsp::acid303::AcidPattern;
use tune_dsp::drums::DrumPattern;
use tune_modules::{ACID303, EQ, HAT_CLOSED, KICK, RIDE};
use tune_song::{
    Clip, ClipKind, Device, Lane, LoadError, MasterMode, Mix, MixUnit, Placement, Registry,
    SectionPlay, Song, SongError, lane_value, load_song, parse_song, parse_song_with,
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
    assert_eq!(kick.instrument, Device::new("kick", &KICK));
    assert_eq!(kick.gain_db, -3.0);
    assert_eq!(kick.pan, 0.0);
    assert!(!kick.mute && !kick.solo);
    assert!(kick.strip.is_empty());
    assert!(kick.fx.is_empty());

    let acid = &song.tracks["acid"];
    assert_eq!(acid.instrument.module, &ACID303);
    assert_eq!(acid.gain_db, 0.0);
    let params: Vec<(&str, f64)> = acid
        .instrument
        .params
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(
        params,
        [
            ("cutoff", 500.0),
            ("resonance", 70.0),
            ("env_mod", 50.0),
            ("decay", 400.0),
            ("accent", 55.0),
        ]
    );
    assert!(acid.strip.is_empty());
    assert_eq!(acid.fx.len(), 1);
    let dist = &acid.fx[0];
    assert_eq!(dist.module.name, "distortion");
    assert_eq!(dist.setting("mode"), Some("soft"));
    assert_eq!(dist.params.get("drive"), Some(&12.0));
    assert_eq!(dist.params.len(), 1);
    assert_eq!(dist.param_id("drive"), "dist.drive");

    let hat = &song.tracks["hat"];
    assert_eq!(hat.instrument.module, &HAT_CLOSED);
    assert_eq!(hat.gain_db, -10.0);
    assert_eq!(hat.pan, 0.3);

    let pids: Vec<&str> = song.clips.keys().map(String::as_str).collect();
    assert_eq!(pids, ["k4", "a1", "h"]);
    assert_eq!(
        song.clips["k4"],
        Clip::Drum(DrumPattern::parse("x...x...x...X...").unwrap())
    );
    assert_eq!(
        song.clips["a1"],
        Clip::Acid(AcidPattern::parse("C2 C2! C3~ C2 . D#2 C2~ G2!").unwrap())
    );
    assert_eq!(
        song.clips["h"],
        Clip::Drum(DrumPattern::parse("..x...x...x...x.").unwrap())
    );

    let sections: Vec<(&str, u32)> = song
        .sections
        .iter()
        .map(|s| (s.name.as_str(), s.bars))
        .collect();
    assert_eq!(sections, [("a", 2), ("b", 2)]);
    assert_eq!(song.section_play(0, "acid"), SectionPlay::Silent);
    assert_eq!(song.section_play(1, "acid"), SectionPlay::Clip("a1"));
    assert_eq!(song.section_play(0, "kick"), SectionPlay::Clip("k4"));
    assert!(song.lanes_in(0).is_empty());
    let lanes = song.lanes_in(1);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].track, "acid");
    assert_eq!(lanes[0].param, "acid303.cutoff");
    assert_eq!(lanes[0].points, [(0.0, 0.1), (2.0, 0.8)]);

    assert_eq!(song.total_bars(), 4);
    assert_eq!(song.section_start_bars(), [0, 2]);
}

#[test]
fn v1_play_becomes_placements_and_lane_stretches() {
    let song = mini();
    let p = |start_bar, bars, clip: &str| Placement {
        start_bar,
        bars,
        clip: clip.to_owned(),
        offset_bars: 0,
    };
    assert_eq!(song.tracks["kick"].play, [p(0, 2, "k4"), p(2, 2, "k4")]);
    assert_eq!(song.tracks["acid"].play, [p(2, 2, "a1")]);
    assert_eq!(song.tracks["hat"].play, [p(0, 2, "h"), p(2, 2, "h")]);
    assert_eq!(
        song.automation,
        [Lane {
            track: "acid".into(),
            param: "acid303.cutoff".into(),
            start_bar: 2,
            bars: 2,
            points: vec![(0.0, 0.1), (2.0, 0.8)],
        }]
    );
}

#[test]
fn section_play_and_lanes_derive_from_the_arrangement() {
    let mut song = mini();
    let acid = song.tracks.get_mut("acid").unwrap();
    // Bar 3 only (inside section `b`, bars 2..4).
    acid.play = vec![Placement {
        start_bar: 2,
        bars: 1,
        clip: "a1".into(),
        offset_bars: 0,
    }];
    match song.section_play(1, "acid") {
        SectionPlay::Other(ps) => assert_eq!(ps, [&song.tracks["acid"].play[0]]),
        other => panic!("expected Other, got {other:?}"),
    }
    assert_eq!(song.section_play(0, "acid"), SectionPlay::Silent);
    // One placement over the whole song covers both sections.
    song.tracks.get_mut("acid").unwrap().play = vec![Placement {
        start_bar: 0,
        bars: 4,
        clip: "a1".into(),
        offset_bars: 0,
    }];
    assert_eq!(song.section_play(0, "acid"), SectionPlay::Clip("a1"));
    assert_eq!(song.section_play(1, "acid"), SectionPlay::Clip("a1"));
    assert_eq!(song.section_play(1, "nope"), SectionPlay::Silent);

    // A stretch over bars 1..3 overlaps both sections.
    song.automation[0].start_bar = 1;
    assert_eq!(song.lanes_in(0).len(), 1);
    assert_eq!(song.lanes_in(1).len(), 1);
    song.automation[0].start_bar = 0;
    assert_eq!(song.lanes_in(0).len(), 1);
    assert!(song.lanes_in(1).is_empty());
}

#[test]
fn lane_value_shapes() {
    let ramp = [(0.0, 0.0), (1.0, 1.0)];
    assert_eq!(lane_value(&ramp, 0.0), 0.0);
    assert_eq!(lane_value(&ramp, 0.5), 0.5);
    assert_eq!(lane_value(&ramp, 1.0), 1.0);
    assert_eq!(lane_value(&ramp, 1.7), 1.0);
    let single = [(0.5, 0.3)];
    assert_eq!(lane_value(&single, 0.0), 0.3);
    assert_eq!(lane_value(&single, 0.9), 0.3);
    let jump = [(1.0, 0.2), (1.0, 0.9), (2.0, 0.5)];
    assert_eq!(lane_value(&jump, 0.99), 0.2);
    assert_eq!(lane_value(&jump, 1.0), 0.9);
    assert!((lane_value(&jump, 1.5) - 0.7).abs() < 1e-15);
    assert_eq!(lane_value(&jump, 3.0), 0.5);
}

#[test]
fn instruments_come_from_the_registry() {
    let registry = Registry::builtin();
    for module in registry.instruments() {
        assert_eq!(module.plays(ClipKind::Drum), module.name != "acid303");
        let src = swap(
            "instrument = \"hat_closed\"",
            &format!("instrument = \"{}\"", module.name),
        )
        .replacen(
            "\"..x...x...x...x.\"",
            if module.plays(ClipKind::Drum) {
                "\"..x...x...x...x.\""
            } else {
                "\"C2\""
            },
            1,
        );
        let song = parse_song_with(&src, registry).unwrap_or_else(|e| panic!("{}", messages(&e)));
        assert_eq!(
            song.tracks["hat"].instrument,
            Device::new(module.name, module)
        );
    }
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
    assert_eq!(song.automation[0].param, "dist.drive");
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
        (swap("[tracks.hat]", "[tracks.mix]"), "`mix` is reserved"),
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

#[test]
fn ride_track_parses() {
    let src = MINI
        .replacen(
            "[patterns.k4]",
            "[tracks.ride]\ninstrument = \"ride\"\nparams = { \"ride.decay\" = 2000 }\n\n[patterns.r]\ntrack = \"ride\"\nsteps = \"x.x.x.x.x.x.x.x.\"\n\n[patterns.k4]",
            1,
        )
        .replacen("play = { kick = \"k4\", hat = \"h\" }", "play = { kick = \"k4\", hat = \"h\", ride = \"r\" }", 1);
    let song = parse_song(&src).unwrap_or_else(|e| panic!("{e:?}"));
    let ride = &song.tracks["ride"];
    assert_eq!(ride.instrument.module, &RIDE);
    assert_eq!(ride.instrument.params["decay"], 2000.0);

    let bad = src.replacen("\"ride.decay\" = 2000", "\"ride.nope\" = 1", 1);
    let errors = parse_song(&bad).unwrap_err();
    let text = messages(&errors);
    assert!(
        text.contains("unknown parameter `ride.nope` for ride"),
        "{text}"
    );
    assert!(text.contains("ride.tone"), "{text}");
}

const MIXED: &str = include_str!("fixtures/mixed.toml");

fn must_parse(src: &str) -> Song {
    parse_song(src).unwrap_or_else(|e| panic!("must parse: {}\n{src}", messages(&e)))
}

#[test]
fn mixed_fixture_parses() {
    let song = must_parse(MIXED);
    assert_eq!(song.mix.master, MasterMode::Limit);
    assert_eq!(song.mix.duck_source, None);
    assert_eq!(song.duck_source(), Some("kick"));
    let mix: Vec<(&str, f64)> = song
        .mix
        .params
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(
        mix,
        [
            ("delay.feedback", 40.0),
            ("reverb.size", 30.0),
            ("master.gain", 6.0)
        ]
    );

    let acid = &song.tracks["acid"];
    assert_eq!(acid.strip["send.delay"], -12.0);
    assert_eq!(acid.strip["send.reverb"], -18.0);
    assert_eq!(acid.strip["duck.depth"], 6.0);
    assert_eq!(acid.instrument.params["cutoff"], 500.0);
    let types: Vec<&str> = acid.fx.iter().map(|d| d.module.name).collect();
    assert_eq!(types, ["distortion", "eq"]);
    let eq = &acid.fx[1];
    assert_eq!(eq.params.get("low_cut"), Some(&85.0));
    assert_eq!(eq.params.len(), 1);
    assert!(eq.settings.is_empty());
    assert_eq!(acid.device("eq"), Some(eq));
    assert_eq!(acid.device("acid303"), Some(&acid.instrument));
    assert_eq!(acid.device("nope"), None);
    assert_eq!(song.tracks["hat"].strip["send.reverb"], -20.0);
    assert_eq!(song.tracks["ride"].instrument.module, &RIDE);

    let lanes: Vec<(&str, &str)> = song
        .automation
        .iter()
        .map(|l| (l.track.as_str(), l.param.as_str()))
        .collect();
    assert_eq!(
        lanes,
        [
            ("acid", "acid303.cutoff"),
            ("acid", "send.delay"),
            ("mix", "reverb.size")
        ]
    );

    assert!(song.bus_active(MixUnit::Delay));
    assert!(song.bus_active(MixUnit::Reverb));
    assert!(song.bus_active(MixUnit::Master));
}

#[test]
fn mini_has_default_mix() {
    let song = mini();
    assert_eq!(song.mix, Mix::default());
    assert!(song.mix.is_default());
    assert_eq!(song.mix.master, MasterMode::Clip);
    assert!(!song.bus_active(MixUnit::Delay));
    assert!(!song.bus_active(MixUnit::Reverb));
    assert!(song.bus_active(MixUnit::Master));
    assert_eq!(song.duck_source(), Some("kick"));
}

#[test]
fn mix_names_round_trip() {
    for mode in MasterMode::ALL {
        assert_eq!(MasterMode::from_name(mode.name()), Some(mode));
    }
    assert_eq!(MasterMode::from_name("loud"), None);
    let names: Vec<&str> = MixUnit::ALL.iter().map(|u| u.name()).collect();
    assert_eq!(names, ["delay", "reverb", "master"]);
    for unit in MixUnit::ALL {
        for (i, spec) in unit.params().iter().enumerate() {
            assert!(spec.id.starts_with(&format!("{}.", unit.name())));
            let (u, index, found) = tune_song::find_mix_param(spec.id).unwrap();
            assert_eq!((u, index, found.id), (unit, i, spec.id));
        }
    }
    assert!(tune_song::find_mix_param("delay.fedback").is_none());
    assert!(tune_song::find_mix_param("send.delay").is_none());
    assert_eq!(tune_song::MIX_TRACK_ID, "mix");
}

#[test]
fn lane_only_send_is_active() {
    let src = swap(
        "param = \"acid.acid303.cutoff\"",
        "param = \"acid.send.delay\"",
    );
    let song = must_parse(&src);
    assert!(!song.tracks["acid"].strip.contains_key("send.delay"));
    assert_eq!(song.automation[0].param, "send.delay");
    assert!(song.bus_active(MixUnit::Delay));
    assert!(!song.bus_active(MixUnit::Reverb));
}

/// `(line, column, message)` of every error, in the order reported.
fn positioned(src: &str) -> Vec<(usize, usize, String)> {
    let errors = parse_song(src).expect_err("expected errors");
    errors
        .into_iter()
        .map(|e| (e.line.unwrap(), e.column.unwrap(), e.message))
        .collect()
}

/// 1-based column of `needle` on 1-based `line` of `src`, counted in chars.
fn col(src: &str, line: usize, needle: &str) -> usize {
    let l = src.lines().nth(line - 1).unwrap();
    l[..l
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` not on line {line}"))]
        .chars()
        .count()
        + 1
}

/// Review Focus 3: every new rule reports at the offending value with
/// line:col. Three files, because `duck_source` is one key (a duplicate stops
/// parsing) and a self-duck needs the kick to be the source.
#[test]
fn mix_errors_have_positions() {
    let mix_known = "delay.time, delay.feedback, delay.low_cut, delay.high_cut, delay.width, \
                     reverb.size, reverb.damp, reverb.pre_delay, reverb.low_cut, reverb.width, \
                     master.gain, master.ceiling, master.release";
    let src = r#"[song]
name = "errs"
bpm = 120

[mix]
master = "loud"
loudness = 3
params = { "delay.fedback" = 40, "delay.feedback" = 95 }

[tracks.kick]
instrument = "kick"
params = { "duck.depth" = 6 }

[tracks.acid]
instrument = "acid303"
params = { "send.chorus" = -12 }
fx = [{ type = "eq", mode = "soft" }]

[patterns.a1]
track = "acid"
steps = "C2"

[[sections]]
name = "a"
bars = 2
play = { acid = "a1" }

[[sections.automation]]
param = "mix.delay.fedback"
points = [[0, 0.1]]
"#;
    let acid_known: Vec<&str> = ACID303
        .params
        .iter()
        .chain(tune_dsp::STRIP_PARAMS)
        .map(|s| s.id)
        .collect();
    let expected = vec![
        (
            6,
            col(src, 6, "\"loud\""),
            "unknown master mode `loud` (expected clip or limit)".to_owned(),
        ),
        (
            7,
            1,
            "unknown field `loudness` in [mix], expected one of `master`, `duck_source`, `params`"
                .to_owned(),
        ),
        (
            8,
            col(src, 8, "\"delay.fedback\""),
            format!("unknown parameter `delay.fedback` for mix (known: {mix_known})"),
        ),
        (
            8,
            col(src, 8, "95"),
            "value 95 for `delay.feedback` is out of range 0..=90".to_owned(),
        ),
        (
            12,
            col(src, 12, "6 }"),
            "track `kick` is the duck source and cannot duck itself".to_owned(),
        ),
        (
            16,
            col(src, 16, "\"send.chorus\""),
            format!(
                "unknown parameter `send.chorus` for acid303 (known: {})",
                acid_known.join(", ")
            ),
        ),
        (17, col(src, 17, "\"soft\""), "eq has no mode".to_owned()),
        (
            29,
            col(src, 29, "\"mix.delay.fedback\""),
            format!("unknown parameter `delay.fedback` for mix (known: {mix_known})"),
        ),
    ];
    assert_eq!(positioned(src), expected);

    let unknown = swap("seed = 7\n", "seed = 7\n\n[mix]\nduck_source = \"kik\"\n");
    let line = unknown.lines().position(|l| l.contains("kik")).unwrap() + 1;
    assert_eq!(
        positioned(&unknown),
        [(
            line,
            col(&unknown, line, "\"kik\""),
            "unknown track `kik`".to_owned()
        )]
    );

    let melodic = swap("seed = 7\n", "seed = 7\n\n[mix]\nduck_source = \"acid\"\n");
    let line = melodic
        .lines()
        .position(|l| l.contains("duck_source"))
        .unwrap()
        + 1;
    assert_eq!(
        positioned(&melodic),
        [(
            line,
            col(&melodic, line, "\"acid\""),
            "duck_source `acid` must be a drum track (ducking follows its hits)".to_owned()
        )]
    );
}

#[test]
fn duck_needs_a_source() {
    const NEEDS: &str =
        "`duck.depth` needs a kick to follow: add a kick track or set duck_source in [mix]";
    let src = swap("instrument = \"kick\"", "instrument = \"clap\"").replacen(
        "\"acid303.accent\" = 55",
        "\"acid303.accent\" = 55, \"duck.depth\" = 6",
        1,
    );
    let line = src.lines().position(|l| l.contains("duck.depth")).unwrap() + 1;
    assert_eq!(
        positioned(&src),
        [(line, col(&src, line, "6 }"), NEEDS.to_owned())]
    );

    // A lane on `duck.depth` needs a source too.
    let lane = swap("instrument = \"kick\"", "instrument = \"clap\"").replacen(
        "param = \"acid.acid303.cutoff\"",
        "param = \"acid.duck.depth\"",
        1,
    );
    let line = lane.lines().position(|l| l.contains("duck.depth")).unwrap() + 1;
    assert_eq!(
        positioned(&lane),
        [(line, col(&lane, line, "\"acid.duck"), NEEDS.to_owned())]
    );

    // Depth 0 is off and needs nothing.
    let off = swap("instrument = \"kick\"", "instrument = \"clap\"").replacen(
        "\"acid303.accent\" = 55",
        "\"acid303.accent\" = 55, \"duck.depth\" = 0",
        1,
    );
    assert_eq!(must_parse(&off).duck_source(), None);
}

#[test]
fn explicit_duck_source_wins() {
    let src = swap("seed = 7\n", "seed = 7\n\n[mix]\nduck_source = \"hat\"\n").replacen(
        "[tracks.hat]",
        "[tracks.kick2]\ninstrument = \"kick\"\n\n[tracks.hat]",
        1,
    );
    let song = must_parse(&src);
    assert_eq!(song.mix.duck_source.as_deref(), Some("hat"));
    assert_eq!(song.duck_source(), Some("hat"));
    assert_eq!(song.mix.master, MasterMode::Clip);
    assert!(!song.mix.is_default());
}

#[test]
fn second_eq_is_rejected() {
    let src = swap(
        "fx = [{ type = \"distortion\", mode = \"soft\", params = { \"dist.drive\" = 12 } }]",
        "fx = [{ type = \"eq\" }, { type = \"eq\", params = { \"eq.low_cut\" = 85 } }]",
    );
    let all = messages(&parse_song(&src).unwrap_err());
    assert!(all.contains("track `acid` may have only one eq"), "{all}");

    let lane = swap(
        "fx = [{ type = \"distortion\", mode = \"soft\", params = { \"dist.drive\" = 12 } }]",
        "fx = [{ type = \"eq\" }]",
    )
    .replacen(
        "param = \"acid.acid303.cutoff\"",
        "param = \"acid.eq.mid_gain\"",
        1,
    );
    let song = must_parse(&lane);
    assert_eq!(song.tracks["acid"].fx, [Device::new("eq", &EQ)]);
    assert_eq!(song.automation[0].param, "eq.mid_gain");
}

/// Review Focus 3: a lane on the duck source's own `duck.depth` would make
/// it duck itself, just like a static depth.
#[test]
fn duck_lane_on_source_is_self_duck() {
    let src = swap(
        "param = \"acid.acid303.cutoff\"",
        "param = \"kick.duck.depth\"",
    );
    let line = src.lines().position(|l| l.contains("kick.duck")).unwrap() + 1;
    assert_eq!(
        positioned(&src),
        [(
            line,
            col(&src, line, "\"kick.duck"),
            "track `kick` is the duck source and cannot duck itself".to_owned()
        )]
    );

    // With another source named, the kick may duck.
    let other = src.replacen(
        "seed = 7\n",
        "seed = 7\n\n[mix]\nduck_source = \"hat\"\n",
        1,
    );
    assert_eq!(must_parse(&other).duck_source(), Some("hat"));
}

/// An invalid explicit `duck_source` reports only itself: no self-duck error
/// is derived from it, for a static depth or a lane.
#[test]
fn invalid_duck_source_drives_no_self_duck() {
    const NOT_DRUM: &str = "duck_source `acid` must be a drum track (ducking follows its hits)";
    let named = swap("seed = 7\n", "seed = 7\n\n[mix]\nduck_source = \"acid\"\n");
    let line = named
        .lines()
        .position(|l| l.contains("duck_source"))
        .unwrap()
        + 1;
    let expected = [(line, col(&named, line, "\"acid\""), NOT_DRUM.to_owned())];

    let depth = named.replacen(
        "\"acid303.accent\" = 55",
        "\"acid303.accent\" = 55, \"duck.depth\" = 6",
        1,
    );
    assert_eq!(positioned(&depth), expected);

    let lane = named.replacen(
        "param = \"acid.acid303.cutoff\"",
        "param = \"acid.duck.depth\"",
        1,
    );
    assert_eq!(positioned(&lane), expected);
}

#[test]
fn tracks_have_devices_with_default_ids() {
    let song = mini();
    let acid = &song.tracks["acid"];
    assert_eq!(acid.instrument.id, "acid303");
    assert_eq!(acid.instrument.module.name, "acid303");
    let ids: Vec<&str> = acid.fx.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, ["dist"]);
    assert_eq!(acid.fx[0].settings, ["soft"]);
    assert_eq!(
        acid.fx[0].params,
        indexmap::indexmap! {"drive".to_owned() => 12.0}
    );
    assert_eq!(acid.instrument.params["cutoff"], 500.0);

    let song = must_parse(MIXED);
    let acid = &song.tracks["acid"];
    let ids: Vec<&str> = acid.fx.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, ["dist", "eq"]);
    let strip: Vec<(&str, f64)> = acid.strip.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(
        strip,
        [
            ("send.delay", -12.0),
            ("send.reverb", -18.0),
            ("duck.depth", 6.0)
        ]
    );
}

#[test]
fn next_device_id_numbers_repeats() {
    let song = must_parse(MIXED);
    let mut acid = song.tracks["acid"].clone();
    assert_eq!(acid.next_device_id(&EQ), "eq2");
    acid.fx.push(Device::new("eq2", &EQ));
    assert_eq!(acid.next_device_id(&EQ), "eq3");
    assert_eq!(mini().tracks["acid"].next_device_id(&EQ), "eq");
}

/// v1 instruments, fx types and settings are checked against the registry,
/// with the messages v1 always had.
#[test]
fn device_rules_name_what_is_allowed() {
    const DIST: &str =
        "fx = [{ type = \"distortion\", mode = \"soft\", params = { \"dist.drive\" = 12 } }]";
    let rows = [
        (
            swap("instrument = \"kick\"", "instrument = \"eq\""),
            "unknown instrument `eq` (expected one of acid303, kick, clap, hat_closed, hat_open, ride)",
        ),
        (
            swap(DIST, "fx = [{ type = \"chorus\" }]"),
            "unknown fx type `chorus` (expected one of distortion, eq)",
        ),
        (
            swap(DIST, "fx = [{ type = \"distortion\", mode = \"fuzz\" }]"),
            "unknown distortion mode `fuzz` (expected soft, hard or overdrive)",
        ),
        (
            swap(DIST, "fx = [{ type = \"distortion\", depth = 1 }]"),
            "unknown field `depth` in fx 1 of [tracks.acid], expected one of `type`, `mode`, `params`",
        ),
        (
            swap(
                DIST,
                "fx = [{ type = \"distortion\", params = { \"eq.low_cut\" = 85 } }]",
            ),
            "unknown parameter `eq.low_cut` for distortion (known: dist.drive, dist.mix, dist.output)",
        ),
    ];
    for (src, expected) in &rows {
        let errors = parse_song(src).expect_err(expected);
        let got: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(got, [*expected]);
    }
    let hard = must_parse(&swap(
        DIST,
        "fx = [{ type = \"distortion\", mode = \"hard\" }]",
    ));
    assert_eq!(hard.tracks["acid"].fx[0].settings, ["hard"]);
}

#[test]
fn patterns_become_clips_with_kinds() {
    let song = mini();
    let ids: Vec<&str> = song.clips.keys().map(String::as_str).collect();
    assert_eq!(ids, ["k4", "a1", "h"]);
    let kinds: Vec<ClipKind> = song.clips.values().map(Clip::kind).collect();
    assert_eq!(kinds, [ClipKind::Drum, ClipKind::Acid, ClipKind::Drum]);
    assert_eq!(song.clips["k4"].steps_text(), "x... x... x... X...");
}

#[test]
fn the_v1_writer_names_the_playing_track_as_owner() {
    let song = mini();
    let again = parse_song(&song.to_toml()).unwrap();
    assert_eq!(again, song);
    // `h` is played by the hat track, not the first drum track.
    assert!(song.to_toml().contains("[patterns.h]\ntrack = \"hat\""));
}
