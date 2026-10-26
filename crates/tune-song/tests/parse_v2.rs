//! Reading song format version 2: version dispatch, device ids, the clip
//! pool, placements and top-level automation, with positioned errors.

use tune_song::{Lane, Parsed, SongError, parse, parse_song};

const MINI: &str = include_str!("fixtures/mini.toml");

/// `fixtures/mini.toml` written in version 2 (what the migration makes of it).
const MINI_V2: &str = r#"# Shared fixture: tune-song, tune-engine and tune-cli tests all read this file.

[song]
version = 2
name = "mini"
bpm = 120
seed = 7

[tracks.kick]
instrument = "kick"
gain_db = -3
play = [
  { at = 1, clip = "k4", bars = 2 },
  { at = 3, clip = "k4", bars = 2 },
]

[tracks.acid]
instrument = "acid303"
params = { "acid303.cutoff" = 500, "acid303.resonance" = 70, "acid303.env_mod" = 50, "acid303.decay" = 400, "acid303.accent" = 55 }
fx = [{ type = "distortion", mode = "soft", params = { "dist.drive" = 12 } }]
play = [{ at = 3, clip = "a1", bars = 2 }]

[tracks.hat]
instrument = "hat_closed"
gain_db = -10
pan = 0.3
play = [
  { at = 1, clip = "h", bars = 2 },
  { at = 3, clip = "h", bars = 2 },
]

[clips.k4]
kind = "drum"
steps = "x...x...x...X..."

[clips.a1]
kind = "acid"
steps = "C2 C2! C3~ C2 . D#2 C2~ G2!"

[clips.h]
kind = "drum"
steps = "..x...x...x...x."

[[sections]]
name = "a"
bars = 2

[[sections]]
name = "b"
bars = 2

[[automation]]
param = "acid.acid303.cutoff"
at = 3
bars = 2
points = [[0, 0.1], [2, 0.8]]
"#;

const KICK_PLAY: &str = r#"play = [
  { at = 1, clip = "k4", bars = 2 },
  { at = 3, clip = "k4", bars = 2 },
]"#;

const ACID_FX: &str =
    r#"fx = [{ type = "distortion", mode = "soft", params = { "dist.drive" = 12 } }]"#;

/// `src` with the one occurrence of `from` replaced by `to`.
fn swap_in(src: &str, from: &str, to: &str) -> String {
    assert_eq!(src.matches(from).count(), 1, "`{from}` must occur once");
    src.replacen(from, to, 1)
}

/// `MINI_V2` with the one occurrence of `from` replaced by `to`.
fn swap(from: &str, to: &str) -> String {
    swap_in(MINI_V2, from, to)
}

fn parsed(src: &str) -> Parsed {
    parse(src).unwrap_or_else(|e| panic!("must parse: {}\n{src}", messages(&e)))
}

fn messages(errors: &[SongError]) -> String {
    errors
        .iter()
        .map(SongError::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The 1-based line and char column of the `skip`-th char of the one
/// occurrence of `needle` in `src`.
fn pos(src: &str, needle: &str, skip: usize) -> (usize, usize) {
    assert_eq!(src.matches(needle).count(), 1, "`{needle}` must occur once");
    let offset = src.find(needle).unwrap();
    let before = &src[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[line_start..].chars().count() + 1 + skip)
}

/// `src` fails with exactly one error: `message` at the `skip`-th char of
/// `needle`.
#[track_caller]
fn fails_at(src: &str, needle: &str, skip: usize, message: &str) {
    let errors = parse(src).expect_err("must fail");
    let (line, column) = pos(src, needle, skip);
    assert_eq!(
        errors,
        [SongError {
            line: Some(line),
            column: Some(column),
            message: message.to_owned(),
        }],
        "{}\n{src}",
        messages(&errors)
    );
}

#[test]
fn a_v2_song_equals_its_v1_original() {
    assert_eq!(parsed(MINI_V2).song, parse_song(MINI).unwrap());
}

#[test]
fn version_is_reported() {
    assert_eq!(parsed(MINI).version, 1);
    assert_eq!(parsed(MINI_V2).version, 2);
    let explicit = MINI.replacen("[song]\n", "[song]\nversion = 1\n", 1);
    let p = parsed(&explicit);
    assert_eq!(p.version, 1);
    assert_eq!(p.song, parse_song(MINI).unwrap());
}

#[test]
fn unknown_version_is_rejected() {
    fails_at(
        &swap("version = 2", "version = 3"),
        "version = 3",
        10,
        "unknown song format version 3 (this tune reads versions 1 and 2)",
    );
    fails_at(
        &swap("version = 2", "version = 2.0"),
        "version = 2.0",
        10,
        "`version` must be an integer, found float",
    );
}

#[test]
fn v1_shapes_in_a_v2_file_get_hints() {
    let src = format!("{MINI_V2}\n[patterns.x]\ntrack = \"kick\"\nsteps = \"x...\"\n");
    fails_at(
        &src,
        "[patterns.x]",
        1,
        "a version 2 file keeps patterns as [clips.<id>] with `kind`",
    );
    fails_at(
        &swap(
            "name = \"b\"\nbars = 2\n",
            "name = \"b\"\nbars = 2\nplay = { kick = \"k4\" }\n",
        ),
        "play = { kick",
        0,
        "in version 2, what a track plays lives in the track's `play` list",
    );
    let src = swap(
        "name = \"b\"\nbars = 2\n",
        "name = \"b\"\nbars = 2\n\n[[sections.automation]]\nparam = \"acid.acid303.cutoff\"\npoints = [[0, 0.5]]\n",
    );
    fails_at(
        &src,
        "[[sections.automation]]",
        11,
        "in version 2, automation is top-level [[automation]] with `at` and `bars`",
    );
}

#[test]
fn devices_by_id() {
    let src = swap(
        ACID_FX,
        r#"fx = [{ type = "eq" }, { type = "eq", id = "eq2", params = { "eq2.low_cut" = 200 } }]"#,
    ) + "\n[[automation]]\nparam = \"acid.eq2.low_cut\"\nat = 1\nbars = 4\npoints = [[0, 0.2], [4, 0.6]]\n";
    let song = parsed(&src).song;
    let acid = &song.tracks["acid"];
    let ids: Vec<&str> = acid.fx.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, ["eq", "eq2"]);
    assert!(acid.fx[0].params.is_empty());
    assert_eq!(acid.fx[1].params["low_cut"], 200.0);
    assert_eq!(
        song.automation[1],
        Lane {
            track: "acid".into(),
            param: "eq2.low_cut".into(),
            start_bar: 0,
            bars: 4,
            points: vec![(0.0, 0.2), (4.0, 0.6)],
        }
    );
}

#[test]
fn instrument_table_form() {
    let src = swap(
        "instrument = \"acid303\"",
        "instrument = { type = \"acid303\", id = \"bass\" }",
    );
    let src = swap_in(
        &src,
        r#"params = { "acid303.cutoff" = 500, "acid303.resonance" = 70, "acid303.env_mod" = 50, "acid303.decay" = 400, "acid303.accent" = 55 }"#,
        r#"params = { "bass.cutoff" = 800, "send.delay" = -12 }"#,
    );
    let src = swap_in(&src, "acid.acid303.cutoff", "acid.bass.cutoff");
    let song = parsed(&src).song;
    let acid = &song.tracks["acid"];
    assert_eq!(acid.instrument.id, "bass");
    assert_eq!(acid.instrument.module.name, "acid303");
    assert_eq!(acid.instrument.params["cutoff"], 800.0);
    assert_eq!(acid.instrument.params.len(), 1);
    assert_eq!(acid.strip["send.delay"], -12.0);
    assert_eq!(song.automation[0].param, "bass.cutoff");
}

#[test]
fn device_rules() {
    let fx = |entries: &str| swap(ACID_FX, &format!("fx = [{entries}]"));
    fails_at(
        &fx(r#"{ type = "eq", id = "Eq" }"#),
        "\"Eq\"",
        0,
        "device id `Eq` may only contain a-z, 0-9 and _",
    );
    fails_at(
        &fx(r#"{ type = "eq" }, { type = "eq" }"#),
        "{ type = \"eq\" }]",
        9,
        "track `acid` already has a device `eq` (give the second one an `id`, e.g. `id = \"eq2\"`)",
    );
    // An explicit id that collides gets a different hint: it already has one.
    fails_at(
        &fx(r#"{ type = "eq" }, { type = "eq", id = "eq" }"#),
        "\"eq\" }]",
        0,
        "track `acid` already has a device `eq` (give the second one a different `id`)",
    );
    fails_at(
        &fx(r#"{ type = "eq", id = "send" }"#),
        "\"send\"",
        0,
        "`send` is reserved for the strip and cannot be a device id",
    );
    fails_at(
        &fx(r#"{ type = "eq", id = "duck" }"#),
        "\"duck\"",
        0,
        "`duck` is reserved for the strip and cannot be a device id",
    );
    fails_at(
        &fx(r#"{ type = "eq", id = "eq2", params = { "eq.low_cut" = 200 } }"#),
        "\"eq.low_cut\"",
        0,
        "`eq.low_cut` is not a param of fx `eq2`: its params are `eq2.<param>`",
    );
    fails_at(
        &swap("instrument = \"kick\"", "instrument = \"dist\""),
        "\"dist\"",
        0,
        "`dist` is an effect, not an instrument",
    );
    let bass = swap(
        "instrument = \"acid303\"",
        "instrument = { type = \"acid303\", id = \"bass\" }",
    )
    .replacen("acid.acid303.cutoff", "acid.bass.cutoff", 1)
    .replacen(
        r#"params = { "acid303.cutoff" = 500, "acid303.resonance" = 70, "acid303.env_mod" = 50, "acid303.decay" = 400, "acid303.accent" = 55 }"#,
        r#"params = { "acid303.cutoff" = 500 }"#,
        1,
    );
    fails_at(
        &bass,
        "\"acid303.cutoff\"",
        0,
        "`acid303.cutoff` is not a param of instrument `bass`: its params are `bass.<param>`",
    );
}

#[test]
fn clip_rules() {
    fails_at(
        &swap("kind = \"acid\"", "kind = \"notes\""),
        "\"notes\"",
        0,
        "unknown clip kind `notes` (expected acid or drum)",
    );
    fails_at(
        &swap(
            "steps = \"C2 C2! C3~ C2 . D#2 C2~ G2!\"",
            "steps = \"x...\"",
        ),
        "\"x...\"",
        0,
        "clip `a1` has a bad step: step 0: `x...` is not a note or rest (expected e.g. `C2`, `D#3!~` or `.`)",
    );
}

#[test]
fn placement_rules() {
    let kick = |play: &str| swap(KICK_PLAY, play);
    fails_at(
        &kick(r#"play = [{ at = 3, clip = "k4", bars = 2 }, { at = 1, clip = "k4", bars = 1 }]"#),
        "at = 1, clip = \"k4\", bars = 1",
        5,
        "placements on track `kick` must be in time order",
    );
    fails_at(
        &kick(r#"play = [{ at = 1, clip = "k4", bars = 2 }, { at = 2, clip = "k4", bars = 2 }]"#),
        "at = 2,",
        5,
        "placement at bar 2 on track `kick` overlaps the one at bar 1",
    );
    fails_at(
        &kick(r#"play = [{ at = 3, clip = "k4", bars = 4 }]"#),
        "bars = 4",
        7,
        "placement on track `kick` ends at bar 6, past the song's end (bar 4)",
    );
    fails_at(
        &kick(r#"play = [{ at = 0, clip = "k4", bars = 2 }]"#),
        "at = 0",
        5,
        "`at` is a 1-based bar: it must be at least 1",
    );
    fails_at(
        &kick(r#"play = [{ at = 1, clip = "a1", bars = 2 }]"#),
        "at = 1, clip = \"a1\"",
        15,
        "clip `a1` is an acid clip; track `kick` (kick) plays drum clips",
    );
    fails_at(
        &kick(r#"play = [{ at = 1, clip = "x", bars = 2 }]"#),
        "\"x\"",
        0,
        "unknown clip `x`",
    );
}

/// `MINI_V2` with one more cutoff stretch after the existing one (bars 3-4).
fn with_lane(at: i64, bars: i64, points: &str) -> String {
    format!(
        "{MINI_V2}\n[[automation]]\nparam = \"acid.acid303.cutoff\"\nat = {at}\nbars = {bars}\npoints = {points}\n"
    )
}

#[test]
fn automation_rules() {
    // The existing stretch covers bars 3-4.
    fails_at(
        &with_lane(2, 2, "[[0, 0.5]]"),
        "at = 2",
        5,
        "automation for `acid.acid303.cutoff` at bars 2-3 overlaps the stretch at bars 3-4",
    );
    fails_at(
        &with_lane(1, 2, "[[3, 0.5]]"),
        "[[3, 0.5]]",
        2,
        "bar 3 is outside the stretch (0..=2)",
    );
    fails_at(
        &with_lane(3, 4, "[[0, 0.5]]").replacen("at = 3\nbars = 2", "at = 1\nbars = 2", 1),
        "bars = 4",
        7,
        "automation at bars 3-6 runs past the song's end (bar 4)",
    );
    fails_at(
        &with_lane(0, 1, "[[0, 0.5]]"),
        "at = 0",
        5,
        "`at` is a 1-based bar: it must be at least 1",
    );
}

#[test]
fn stretches_may_leave_gaps_and_come_in_any_order() {
    // Bars 3-4 (in the file), then bar 1: bar 2 is a gap.
    let song = parsed(&with_lane(1, 1, "[[0, 0.3], [1, 0.4]]")).song;
    let spans: Vec<(u32, u32)> = song
        .automation
        .iter()
        .map(|l| (l.start_bar, l.bars))
        .collect();
    assert_eq!(spans, [(2, 2), (0, 1)]);
}

#[test]
fn automation_tables_may_sit_between_sections() {
    let lane = "[[automation]]\nparam = \"acid.acid303.cutoff\"\nat = 3\nbars = 2\npoints = [[0, 0.1], [2, 0.8]]\n";
    let src = swap(&format!("\n{lane}"), "");
    let src = swap_in(
        &src,
        "[[sections]]\nname = \"b\"",
        &format!("{lane}\n[[sections]]\nname = \"b\""),
    );
    let song = parsed(&src).song;
    assert_eq!(song.sections.len(), 2);
    assert_eq!(song.automation.len(), 1);
    assert_eq!(song, parse_song(MINI).unwrap());
}

#[test]
fn empty_sections_report_only_the_missing_section() {
    // Placements and an automation stretch exist, but no section: the one
    // error is the real one, not "past the song's end (bar 0)" beside it.
    let head = MINI_V2.split("[[sections]]").next().unwrap();
    let tail = &MINI_V2[MINI_V2.find("[[automation]]").unwrap()..];
    let src = format!("sections = []\n{head}{tail}");
    let errors = parse(&src).expect_err("must fail");
    assert_eq!(
        errors,
        [SongError {
            line: None,
            column: None,
            message: "a song needs at least one section ([[sections]])".to_owned(),
        }],
        "{}",
        messages(&errors)
    );
}
