//! Scenes: `[scenes.<id>]` tables with the clip each track's slot holds.

use tune_song::{SongError, parse_song};

const SESSION: &str = include_str!("fixtures/session.toml");
const MINI: &str = include_str!("fixtures/mini.toml");
const MINI_V2: &str = include_str!("fixtures/v2/mini.toml");

fn slots(scene: &tune_song::Scene) -> Vec<(&str, &str)> {
    scene
        .slots
        .iter()
        .map(|(t, c)| (t.as_str(), c.as_str()))
        .collect()
}

#[test]
fn scenes_parse_in_file_order() {
    let song = parse_song(SESSION).unwrap();
    assert_eq!(song.scenes.keys().collect::<Vec<_>>(), ["groove", "break"]);
    assert_eq!(
        slots(&song.scenes["groove"]),
        [("kick", "k4"), ("acid", "a1")]
    );
    assert_eq!(slots(&song.scenes["break"]), [("hat", "h")]);
    assert!(parse_song(MINI_V2).unwrap().scenes.is_empty());
}

/// The one error of `src`, with the text it points at.
fn error_of(src: &str) -> SongError {
    let mut errors = parse_song(src).unwrap_err();
    assert_eq!(errors.len(), 1, "{errors:?}");
    errors.remove(0)
}

/// Replaces `from` with `to` in the session fixture, expects one error with
/// `message` and checks its position is that of `at` within the new text.
fn rule(from: &str, to: &str, at: &str, message: &str) {
    assert_eq!(SESSION.matches(from).count(), 1, "{from}");
    let src = SESSION.replace(from, to);
    let e = error_of(&src);
    assert_eq!(e.message, message);
    let offset = src.find(at).unwrap();
    let line = src[..offset].matches('\n').count() + 1;
    let column = offset - src[..offset].rfind('\n').map_or(0, |i| i + 1) + 1;
    assert_eq!((e.line, e.column), (Some(line), Some(column)), "{e:?}");
}

#[test]
fn scene_rules() {
    rule(
        "slots = { kick = \"k4\", acid = \"a1\" }",
        "slots = { bass = \"k4\" }",
        "bass",
        "unknown track `bass`",
    );
    rule(
        "acid = \"a1\" }",
        "acid = \"nope\" }",
        "\"nope\"",
        "unknown clip `nope`",
    );
    rule(
        "acid = \"a1\" }",
        "acid = \"k4\" }",
        "\"k4\" }\n\n# hats",
        "clip `k4` is a drum clip; track `acid` (acid303) plays acid clips",
    );
    rule(
        "[scenes.break]",
        "[scenes.Drop]",
        "Drop",
        "scene id `Drop` may only contain a-z, 0-9 and _",
    );
    let e = error_of(&SESSION.replace("[scenes.break]\n", "[scenes.break]\nname = \"x\"\n"));
    assert!(e.message.contains("name"), "{}", e.message);
    assert!(e.message.contains("unknown"), "{}", e.message);

    let v1 = format!("{MINI}\n[scenes.x]\n");
    let e = error_of(&v1);
    assert_eq!(
        e.message,
        "this looks like a version 2 song: add `version = 2` to [song]"
    );
}

#[test]
fn scenes_round_trip() {
    let song = parse_song(SESSION).unwrap();
    let text = song.to_toml();
    assert!(
        text.contains(
            "\"\n\n[scenes.groove]\nslots = { kick = \"k4\", acid = \"a1\" }\n\n[scenes.break]\nslots = { hat = \"h\" }\n\n[[sections]]"
        ),
        "{text}"
    );
    assert!(
        text.find("[clips.h]") < text.find("[scenes.groove]"),
        "{text}"
    );
    let back = parse_song(&text).unwrap();
    assert_eq!(back, song);
    assert_eq!(back.scenes.keys().collect::<Vec<_>>(), ["groove", "break"]);

    let empty = SESSION.replace(
        "[scenes.break]\nslots = { hat = \"h\" }\n",
        "[scenes.empty]\n",
    );
    let song = parse_song(&empty).unwrap();
    assert!(song.scenes["empty"].slots.is_empty());
    let text = song.to_toml();
    assert!(text.contains("\n[scenes.empty]\n\n[[sections]]"), "{text}");
    assert_eq!(parse_song(&text).unwrap(), song);
}
