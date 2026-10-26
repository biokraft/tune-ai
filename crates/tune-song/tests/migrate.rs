//! The v1 to v2 migration: the same song, every comment kept in order, the
//! file's table order kept, and v1 errors at their place in the v1 text.

use std::path::PathBuf;

use tune_song::{Migration, Parsed, SongError, migrate, parse};

const MINI: &str = include_str!("fixtures/mini.toml");
const MIXED: &str = include_str!("fixtures/mixed.toml");
const FIRST_ACID: &str = include_str!("fixtures/first-acid-v1.toml");
const FIRST_ACID_MIXED: &str = include_str!("fixtures/first-acid-mixed.toml");
const COMMENTED: &str = include_str!("../../tune-studio/tests/fixtures/commented-v1.toml");

/// The v1 fixtures that have a committed v2 copy in `fixtures/v2/`.
const FIXTURES: [(&str, &str); 4] = [
    ("mini", MINI),
    ("mixed", MIXED),
    ("first-acid-v1", FIRST_ACID),
    ("first-acid-mixed", FIRST_ACID_MIXED),
];

/// Every v1 source the migration is checked on.
const SOURCES: [(&str, &str); 5] = [
    ("mini", MINI),
    ("mixed", MIXED),
    ("first-acid-v1", FIRST_ACID),
    ("first-acid-mixed", FIRST_ACID_MIXED),
    ("commented-v1", COMMENTED),
];

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

/// The migrated text of `src`, which must be a v1 file.
fn migrated(src: &str) -> String {
    match migrate(src) {
        Ok(Migration::Migrated { from: 1, text }) => text,
        other => panic!("expected a migration from v1, got {other:?}"),
    }
}

/// The comments of a TOML text in order: each `#…` run to the end of its
/// line, trimmed, whether it fills the line or trails a value. A `#` inside
/// a string is not a comment.
fn comments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut quote: Option<char> = None;
        let mut escaped = false;
        for (i, c) in line.char_indices() {
            match quote {
                Some(q) => {
                    if escaped {
                        escaped = false;
                    } else if c == '\\' && q == '"' {
                        escaped = true;
                    } else if c == q {
                        quote = None;
                    }
                }
                None if c == '"' || c == '\'' => quote = Some(c),
                None if c == '#' => {
                    out.push(line[i..].trim().to_owned());
                    break;
                }
                None => {}
            }
        }
    }
    out
}

/// The table header lines of a TOML text, in order.
fn headers(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with('['))
        .collect()
}

#[test]
fn mini_migrates_to_this_text() {
    assert_eq!(
        migrate(MINI),
        Ok(Migration::Migrated {
            from: 1,
            text: MINI_V2.to_owned(),
        })
    );
}

#[test]
fn migrated_song_equals_the_v1_song() {
    for (name, src) in SOURCES {
        let old = parse(src).unwrap().song;
        let new = parse(&migrated(src));
        // `Song`'s `==` ignores the order of its maps (tracks set the mix
        // and stem order); the canonical text does not.
        let canonical = new.as_ref().map(|p| p.song.to_toml());
        assert_eq!(canonical, Ok(old.to_toml()), "{name}");
        assert_eq!(
            new,
            Ok(Parsed {
                song: old,
                version: 2,
            }),
            "{name}"
        );
    }
}

#[test]
fn migration_keeps_every_comment_in_order() {
    for (name, src) in SOURCES {
        let before = comments(src);
        assert!(!before.is_empty(), "{name} has comments to check");
        let after = comments(&migrated(src));
        let mut rest = after.iter();
        for c in &before {
            assert!(
                rest.any(|a| a == c),
                "{name}: `{c}` is missing or out of order in the migrated text"
            );
        }
    }
}

/// A one-track song whose first section is `section`.
fn with_section(section: &str) -> String {
    format!(
        r#"[song]
name = "c"
bpm = 120

[tracks.k]
instrument = "kick"

[patterns.k4]
track = "k"
steps = "x..."

[[sections]]
{section}
[[sections]]
name = "b"
bars = 2
"#
    )
}

#[test]
fn keeps_comments_on_removed_play_keys() {
    let after_bars =
        with_section("name = \"a\"\nbars = 2\n# drums only\nplay = { k = \"k4\" } # no acid\n");
    assert!(
        migrated(&after_bars).contains(
            "[[sections]]\nname = \"a\"\nbars = 2\n# drums only\n# no acid\n\n[[sections]]\n"
        ),
        "{}",
        migrated(&after_bars)
    );
    let first =
        with_section("# drums only\nplay = { k = \"k4\" } # no acid\nname = \"a\"\nbars = 2\n");
    assert!(
        migrated(&first).contains(
            "[[sections]]\n# drums only\n# no acid\nname = \"a\"\nbars = 2\n\n[[sections]]\n"
        ),
        "{}",
        migrated(&first)
    );
}

#[test]
fn tables_keep_their_order() {
    // The v1 headers, renamed: the same tables in the same order.
    let renamed = |src: &str| -> Vec<String> {
        headers(src)
            .into_iter()
            .map(|h| {
                h.replace("[patterns.", "[clips.")
                    .replace("[[sections.automation]]", "[[automation]]")
            })
            .collect()
    };
    let text = migrated(FIRST_ACID);
    let got = headers(&text);
    assert_eq!(got, renamed(FIRST_ACID));
    // Spelled out for first-acid: [song], the five tracks, the clips in
    // pattern order, then each section followed by its stretches.
    let lanes = [0, 2, 2, 2, 2, 2, 1, 0];
    let mut want = vec!["[song]".to_owned()];
    for t in ["kick", "clap", "hat_closed", "hat_open", "acid"] {
        want.push(format!("[tracks.{t}]"));
    }
    for c in [
        "kick4",
        "hats8",
        "open_off",
        "clap24",
        "acid_main",
        "acid_jump",
    ] {
        want.push(format!("[clips.{c}]"));
    }
    for n in lanes {
        want.push("[[sections]]".to_owned());
        want.extend(std::iter::repeat_n("[[automation]]".to_owned(), n));
    }
    assert_eq!(got, want);
    // With a [mix] table, it stays second.
    let mixed = migrated(FIRST_ACID_MIXED);
    assert_eq!(headers(&mixed), renamed(FIRST_ACID_MIXED));
    assert_eq!(headers(&mixed)[..2], ["[song]", "[mix]"]);
}

#[test]
fn migrates_inline_forms() {
    let src = r#"sections = [{ name = "a", bars = 2, play = { k = "k4" }, automation = [{ param = "k.kick.tune", points = [[0, 0.5]] }] }, { name = "b", bars = 3, automation = [{ param = "k.kick.decay", points = [[0, 0.2], [3, 0.9]] }] }]
patterns.k4 = { track = "k", steps = "x..." }

[song]
name = "inline"
bpm = 120

[tracks.k]
instrument = "kick"
"#;
    let old = parse(src).unwrap().song;
    let text = migrated(src);
    assert_eq!(
        parse(&text),
        Ok(Parsed {
            song: old,
            version: 2,
        }),
        "{text}"
    );
    assert!(
        text.starts_with(
            "sections = [{ name = \"a\", bars = 2 }, { name = \"b\", bars = 3 }]\n\
             clips.k4 = { kind = \"drum\", steps = \"x...\" }\n"
        ),
        "{text}"
    );
    // The stretches of inline sections go after the last table, in section
    // order.
    assert!(
        text.ends_with(
            "[tracks.k]\ninstrument = \"kick\"\nplay = [{ at = 1, clip = \"k4\", bars = 2 }]\n\n\
             [[automation]]\nparam = \"k.kick.tune\"\nat = 1\nbars = 2\npoints = [[0, 0.5]]\n\n\
             [[automation]]\nparam = \"k.kick.decay\"\nat = 3\nbars = 3\npoints = [[0, 0.2], [3, 0.9]]\n"
        ),
        "{text}"
    );
}

#[test]
fn migration_is_idempotent() {
    for (name, src) in SOURCES {
        assert_eq!(migrate(&migrated(src)), Ok(Migration::Current), "{name}");
    }
}

#[test]
fn invalid_v1_reports_its_own_positions() {
    let src = r#"[song]
name = "bad"
# the tempo
bpm = 400

[tracks.k]
instrument = "kick"

[patterns.k4]
track = "k"
steps = "x..."

[[sections]]
name = "a"
bars = 2
play = { k = "k4" }
"#;
    let errors = migrate(src).unwrap_err();
    assert_eq!(errors.len(), 1, "{errors:?}");
    let SongError { line, column, .. } = errors[0];
    assert_eq!((line, column), (Some(4), Some(7)), "{errors:?}");
}

#[test]
fn v2_fixtures_are_the_migration_output() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v2");
    let update = std::env::var_os("UPDATE_FIXTURES").is_some_and(|v| v == "1");
    for (name, src) in FIXTURES {
        let path = dir.join(format!("{name}.toml"));
        let text = migrated(src);
        if update {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &text).unwrap();
        }
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (run with UPDATE_FIXTURES=1)", path.display()));
        assert_eq!(committed, text, "{name}");
    }
}

#[test]
fn inline_song_header_keeps_its_spacing() {
    for song in [
        r#"song = { name = "c", bpm = 120, version = 1 }"#,
        r#"song = { name = "c", version = 1, bpm = 120 }"#,
        r#"song = { name = "c", bpm = 120 }"#,
    ] {
        let src = format!(
            "{song}\n\n[tracks.k]\ninstrument = \"kick\"\n\n[patterns.k4]\ntrack = \"k\"\nsteps = \"x...\"\n\n[[sections]]\nname = \"a\"\nbars = 2\nplay = {{ k = \"k4\" }}\n"
        );
        let text = migrated(&src);
        assert!(
            text.starts_with("song = { version = 2, name = \"c\", bpm = 120 }\n"),
            "{song}:\n{text}"
        );
    }
}
