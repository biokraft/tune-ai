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

fn keys(song: &tune_song::Song) -> Vec<&str> {
    song.scenes.keys().map(String::as_str).collect()
}

fn refused<T: std::fmt::Debug>(result: Result<T, tune_song::EditOpError>) -> String {
    result.unwrap_err().to_string()
}

#[test]
fn scene_edits() {
    let song = parse_song(SESSION).unwrap();

    let added = tune_song::add_scene(&song, "jam", None).unwrap();
    let after = &added.arranged.song;
    assert_eq!(keys(after), ["groove", "break", "jam"]);
    assert!(after.scenes["jam"].slots.is_empty());
    assert_eq!(added.removed, tune_song::Removed::default());
    let first = tune_song::add_scene(&song, "jam", Some(0)).unwrap();
    assert_eq!(keys(&first.arranged.song), ["jam", "groove", "break"]);
    let middle = tune_song::add_scene(&song, "jam", Some(1)).unwrap();
    assert_eq!(keys(&middle.arranged.song), ["groove", "jam", "break"]);
    // An index equal to the count appends.
    let last = tune_song::add_scene(&song, "jam", Some(2)).unwrap();
    assert_eq!(keys(&last.arranged.song), ["groove", "break", "jam"]);

    let moved = tune_song::move_scene(&song, "break", 0).unwrap();
    assert_eq!(keys(&moved.arranged.song), ["break", "groove"]);
    assert_eq!(slots(&moved.arranged.song.scenes["break"]), [("hat", "h")]);
    let stay = tune_song::move_scene(&song, "groove", 0).unwrap();
    assert_eq!(stay.arranged.song, song);
    assert_eq!(keys(&stay.arranged.song), ["groove", "break"]);

    let renamed = tune_song::rename_scene(&song, "groove", "drop").unwrap();
    assert_eq!(keys(&renamed.arranged.song), ["drop", "break"]);
    assert_eq!(
        slots(&renamed.arranged.song.scenes["drop"]),
        [("kick", "k4"), ("acid", "a1")]
    );
    let same = tune_song::rename_scene(&song, "groove", "groove").unwrap();
    assert_eq!(same.arranged.song, song);

    let removed = tune_song::remove_scene(&song, "groove").unwrap();
    assert_eq!(keys(&removed.arranged.song), ["break"]);
    let slot = |track: &str, clip: &str| tune_song::RemovedSlot {
        scene: "groove".into(),
        track: track.into(),
        clip: clip.into(),
    };
    assert_eq!(
        removed.removed.slots,
        [slot("kick", "k4"), slot("acid", "a1")]
    );
    assert!(removed.removed.lanes.is_empty() && removed.removed.placements.is_empty());

    let set = tune_song::set_slot(&song, "break", "kick", Some("k4")).unwrap();
    assert_eq!(
        slots(&set.arranged.song.scenes["break"]),
        [("hat", "h"), ("kick", "k4")]
    );
    let set = tune_song::set_slot(&song, "break", "hat", Some("h")).unwrap();
    assert_eq!(set.arranged.song, song);
    let swapped = tune_song::set_slot(&song, "groove", "kick", Some("a1")).unwrap();
    assert_eq!(
        slots(&swapped.arranged.song.scenes["groove"]),
        [("kick", "a1"), ("acid", "a1")]
    );
    let added = tune_song::set_slot(&song, "groove", "hat", Some("h")).unwrap();
    assert_eq!(
        slots(&added.arranged.song.scenes["groove"]),
        [("kick", "k4"), ("acid", "a1"), ("hat", "h")]
    );
    let cleared = tune_song::set_slot(&song, "groove", "acid", None).unwrap();
    assert_eq!(
        slots(&cleared.arranged.song.scenes["groove"]),
        [("kick", "k4")]
    );
    let nothing = tune_song::set_slot(&song, "groove", "hat", None).unwrap();
    assert_eq!(nothing.arranged.song, song);

    // The refusals, verbatim.
    assert_eq!(
        refused(tune_song::add_scene(&song, "groove", None)),
        "scene `groove` already exists"
    );
    assert_eq!(
        refused(tune_song::add_scene(&song, "Jam", None)),
        "a scene id is lower-case letters, digits and `_`, got `Jam`"
    );
    assert_eq!(
        refused(tune_song::rename_scene(&song, "groove", "break")),
        "scene `break` already exists"
    );
    assert_eq!(
        refused(tune_song::rename_scene(&song, "groove", "Drop")),
        "a scene id is lower-case letters, digits and `_`, got `Drop`"
    );
    assert_eq!(
        refused(tune_song::remove_scene(&song, "drop")),
        "there is no scene `drop`"
    );
    assert_eq!(
        refused(tune_song::rename_scene(&song, "drop", "x")),
        "there is no scene `drop`"
    );
    assert_eq!(
        refused(tune_song::move_scene(&song, "drop", 0)),
        "there is no scene `drop`"
    );
    assert_eq!(
        refused(tune_song::set_slot(&song, "drop", "kick", None)),
        "there is no scene `drop`"
    );
    assert_eq!(
        refused(tune_song::set_slot(&song, "groove", "bass", Some("k4"))),
        "there is no track `bass`"
    );
    assert_eq!(
        refused(tune_song::set_slot(&song, "groove", "kick", Some("nope"))),
        "there is no clip `nope`"
    );
    assert_eq!(
        refused(tune_song::move_scene(&song, "groove", 2)),
        "index 2 is out of bounds (the song has 2 scenes)"
    );
    assert_eq!(
        refused(tune_song::add_scene(&song, "jam", Some(3))),
        "index 3 is out of bounds (the song has 2 scenes)"
    );
}
