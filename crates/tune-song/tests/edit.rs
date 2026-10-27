//! Track and device edits: the song each one makes, what it removes, and its
//! refusals.

use tune_song::{
    EditOpError, LaneOrigin, Registry, Song, add_effect, add_track, move_effect, parse_song,
    remove_effect, remove_track, rename_device, rename_track, set_instrument, track_table_toml,
};

const SONG: &str = r#"
[song]
version = 2
name = "t"
bpm = 120

[mix]
duck_source = "kick"

[tracks.kick]
instrument = "kick"

[tracks.acid]
instrument = "acid303"
fx = [{ type = "distortion", params = { "dist.drive" = 12 } }, { type = "eq" }]

[tracks.hat]
instrument = "hat_closed"

[clips.k4]
kind = "drum"
steps = "x...x...x...X..."

[[sections]]
name = "a"
bars = 4

[[automation]]
param = "acid.acid303.cutoff"
at = 1
bars = 2
points = [[0, 0.1], [2, 0.8]]

[[automation]]
param = "acid.dist.drive"
at = 2
bars = 2
points = [[0, 0.2], [2, 0.4]]

[[automation]]
param = "hat.send.delay"
at = 1
bars = 4
points = [[0, 0], [4, 1]]
"#;

fn song() -> Song {
    parse_song(SONG).expect("the song parses")
}

fn registry() -> &'static Registry {
    Registry::builtin()
}

fn ids(song: &Song) -> Vec<&str> {
    song.tracks.keys().map(String::as_str).collect()
}

fn fx_ids<'s>(song: &'s Song, track: &str) -> Vec<&'s str> {
    song.tracks[track]
        .fx
        .iter()
        .map(|d| d.id.as_str())
        .collect()
}

fn err<T>(r: Result<T, EditOpError>) -> String {
    match r {
        Ok(_) => panic!("expected a refusal"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn a_new_track_is_default_and_last() {
    let edited = add_track(&song(), registry(), "bass", "acid303").unwrap();
    let new = &edited.arranged.song;
    assert_eq!(ids(new), ["kick", "acid", "hat", "bass"]);
    let bass = &new.tracks["bass"];
    assert_eq!(bass.instrument.id, "acid303");
    assert_eq!(bass.instrument.module.name, "acid303");
    assert!(bass.instrument.params.is_empty() && bass.fx.is_empty() && bass.play.is_empty());
    assert_eq!(
        (bass.gain_db, bass.pan, bass.mute, bass.solo),
        (0.0, 0.0, false, false)
    );
    assert!(bass.strip.is_empty());
    assert_eq!(edited.removed, Default::default());
    // Every stretch is the old one.
    assert_eq!(
        edited.arranged.lanes,
        [
            LaneOrigin::Kept(0),
            LaneOrigin::Kept(1),
            LaneOrigin::Kept(2)
        ]
    );
    assert_eq!(new.automation, song().automation);
}

#[test]
fn the_track_table_lists_only_what_is_not_default() {
    let new = add_track(&song(), registry(), "bass", "acid303")
        .unwrap()
        .arranged
        .song;
    assert_eq!(
        track_table_toml("bass", &new.tracks["bass"]),
        "[tracks.bass]\ninstrument = \"acid303\"\n"
    );
    let full = &song().tracks["acid"];
    assert_eq!(
        track_table_toml("acid", full),
        "[tracks.acid]\ninstrument = \"acid303\"\nfx = [{ type = \"distortion\", mode = \"soft\", \
         params = { \"dist.drive\" = 12 } }, { type = \"eq\" }]\n"
    );
}

#[test]
fn track_id_rules() {
    let s = song();
    let add = |id: &str, module: &str| err(add_track(&s, registry(), id, module));
    assert_eq!(add("acid", "kick"), "track `acid` already exists");
    assert_eq!(add("mix", "kick"), "`mix` is reserved for the mix params");
    assert_eq!(
        add("Bass", "kick"),
        "a track id is lower-case letters, digits and `_`, got `Bass`"
    );
    assert_eq!(
        add("", "kick"),
        "a track id is lower-case letters, digits and `_`, got ``"
    );
    assert_eq!(
        add("bass", "acid3030"),
        "unknown instrument `acid3030` (expected acid303, kick, clap, hat_closed, hat_open or ride)"
    );
    assert_eq!(add("bass", "eq"), "`eq` is an effect, not an instrument");
    assert_eq!(err(remove_track(&s, "bass")), "there is no track `bass`");
    assert_eq!(
        err(rename_track(&s, "bass", "x")),
        "there is no track `bass`"
    );
    assert_eq!(
        err(rename_track(&s, "acid", "kick")),
        "track `kick` already exists"
    );
    assert_eq!(
        err(rename_track(&s, "acid", "mix")),
        "`mix` is reserved for the mix params"
    );
    assert_eq!(
        err(rename_track(&s, "acid", "A")),
        "a track id is lower-case letters, digits and `_`, got `A`"
    );
}

#[test]
fn removing_a_track_removes_its_lanes_and_duck_source() {
    let edited = remove_track(&song(), "acid").unwrap();
    assert_eq!(ids(&edited.arranged.song), ["kick", "hat"]);
    let gone: Vec<_> = edited
        .removed
        .lanes
        .iter()
        .map(|l| (l.track.as_str(), l.param.as_str()))
        .collect();
    assert_eq!(gone, [("acid", "acid303.cutoff"), ("acid", "dist.drive")]);
    assert!(edited.removed.placements.is_empty());
    assert_eq!(edited.removed.duck_source, None);
    assert_eq!(edited.arranged.lanes, [LaneOrigin::Kept(2)]);
    assert_eq!(edited.arranged.song.automation.len(), 1);

    let edited = remove_track(&song(), "kick").unwrap();
    assert_eq!(edited.removed.duck_source.as_deref(), Some("kick"));
    assert_eq!(edited.arranged.song.mix.duck_source, None);
}

#[test]
fn renaming_a_track_keeps_its_place_and_follows_references() {
    let edited = rename_track(&song(), "kick", "drum").unwrap();
    let new = &edited.arranged.song;
    assert_eq!(ids(new), ["drum", "acid", "hat"]);
    assert_eq!(new.mix.duck_source.as_deref(), Some("drum"));
    let edited = rename_track(&song(), "acid", "bass").unwrap();
    let new = &edited.arranged.song;
    assert_eq!(ids(new), ["kick", "bass", "hat"]);
    let lanes: Vec<_> = new
        .automation
        .iter()
        .map(|l| (&l.track[..], &l.param[..]))
        .collect();
    assert_eq!(
        lanes,
        [
            ("bass", "acid303.cutoff"),
            ("bass", "dist.drive"),
            ("hat", "send.delay")
        ]
    );
    assert_eq!(new.mix.duck_source.as_deref(), Some("kick"));
    assert_eq!(
        edited.arranged.lanes,
        [
            LaneOrigin::Kept(0),
            LaneOrigin::Kept(1),
            LaneOrigin::Kept(2)
        ]
    );
}

#[test]
fn effects_insert_move_and_remove() {
    let s = song();
    let eq0 = add_effect(&s, registry(), "acid", "eq", None, Some(0)).unwrap();
    // `eq` is taken by the second effect, so the new one is `eq2`.
    assert_eq!(fx_ids(&eq0.arranged.song, "acid"), ["eq2", "dist", "eq"]);
    let named = add_effect(&s, registry(), "hat", "eq", Some("tone"), None).unwrap();
    assert_eq!(fx_ids(&named.arranged.song, "hat"), ["tone"]);
    let plain = add_effect(&s, registry(), "hat", "eq", None, None).unwrap();
    assert_eq!(fx_ids(&plain.arranged.song, "hat"), ["eq"]);

    let moved = move_effect(&s, "acid", "dist", 1).unwrap();
    assert_eq!(fx_ids(&moved.arranged.song, "acid"), ["eq", "dist"]);
    assert_eq!(
        moved.arranged.song.tracks["acid"].fx[1].params["drive"], 12.0,
        "the params move with the device"
    );

    let removed = remove_effect(&s, "acid", "dist").unwrap();
    assert_eq!(fx_ids(&removed.arranged.song, "acid"), ["eq"]);
    let lanes: Vec<_> = removed.removed.lanes.iter().map(|l| &l.param[..]).collect();
    assert_eq!(lanes, ["dist.drive"]);
    assert_eq!(
        removed.arranged.lanes,
        [LaneOrigin::Kept(0), LaneOrigin::Kept(2)]
    );
}

#[test]
fn renaming_a_device_rewrites_its_lanes() {
    let s = song();
    let edited = rename_device(&s, "acid", "acid303", "bass303").unwrap();
    let new = &edited.arranged.song;
    assert_eq!(new.tracks["acid"].instrument.id, "bass303");
    assert_eq!(new.automation[0].param, "bass303.cutoff");
    assert_eq!(new.automation[1].param, "dist.drive");
    let edited = rename_device(&s, "acid", "dist", "drive2").unwrap();
    let new = &edited.arranged.song;
    assert_eq!(fx_ids(new, "acid"), ["drive2", "eq"]);
    assert_eq!(new.tracks["acid"].fx[0].params["drive"], 12.0);
    assert_eq!(new.automation[1].param, "drive2.drive");
    assert_eq!(new.automation[0].param, "acid303.cutoff");
}

#[test]
fn device_id_rules() {
    let s = song();
    let add = |id: Option<&str>, index| err(add_effect(&s, registry(), "acid", "eq", id, index));
    assert_eq!(
        add(Some("eq"), None),
        "track `acid` already has a device `eq`"
    );
    assert_eq!(
        add(Some("acid303"), None),
        "track `acid` already has a device `acid303`"
    );
    assert_eq!(
        add(Some("send"), None),
        "`send` and `duck` are reserved for the strip params"
    );
    assert_eq!(
        add(Some("duck"), None),
        "`send` and `duck` are reserved for the strip params"
    );
    assert_eq!(
        add(Some("Eq"), None),
        "a device id is lower-case letters, digits and `_`, got `Eq`"
    );
    assert_eq!(
        add(None, Some(3)),
        "index 3 is out of bounds (the chain has 2)"
    );
    assert_eq!(
        err(add_effect(&s, registry(), "acid", "reverb", None, None)),
        "unknown effect `reverb` (expected distortion or eq)"
    );
    assert_eq!(
        err(add_effect(&s, registry(), "acid", "kick", None, None)),
        "`kick` is an instrument, not an effect"
    );
    assert_eq!(
        err(add_effect(&s, registry(), "bass", "eq", None, None)),
        "there is no track `bass`"
    );
    assert_eq!(
        err(remove_effect(&s, "acid", "eq3")),
        "track `acid` has no device `eq3`"
    );
    assert_eq!(
        err(remove_effect(&s, "acid", "acid303")),
        "`acid303` is the instrument of track `acid`, not an effect"
    );
    assert_eq!(
        err(move_effect(&s, "acid", "dist", 2)),
        "index 2 is out of bounds (the chain has 2)"
    );
    assert_eq!(
        err(move_effect(&s, "acid", "eq3", 0)),
        "track `acid` has no device `eq3`"
    );
    assert_eq!(
        err(rename_device(&s, "acid", "eq3", "x")),
        "track `acid` has no device `eq3`"
    );
    assert_eq!(
        err(rename_device(&s, "acid", "dist", "eq")),
        "track `acid` already has a device `eq`"
    );
    assert_eq!(
        err(rename_device(&s, "acid", "dist", "send")),
        "`send` and `duck` are reserved for the strip params"
    );
    assert_eq!(
        err(rename_device(&s, "acid", "dist", "Dist")),
        "a device id is lower-case letters, digits and `_`, got `Dist`"
    );
}

#[test]
fn renaming_to_the_same_name_changes_nothing() {
    let s = song();
    assert_eq!(rename_track(&s, "acid", "acid").unwrap().arranged.song, s);
    assert_eq!(
        rename_device(&s, "acid", "dist", "dist")
            .unwrap()
            .arranged
            .song,
        s
    );
}

/// A song with a 303 that plays an acid clip and a notes clip, drives the
/// strip and an effect, and whose kick is the explicit duck source.
const SWAP: &str = r#"
[song]
version = 2
name = "t"
bpm = 120

[mix]
duck_source = "kick"

[tracks.kick]
instrument = "kick"
play = [{ at = 1, clip = "k4", bars = 2 }]

[tracks.acid]
instrument = "acid303"
params = { "acid303.cutoff" = 500, "send.delay" = -20 }
fx = [{ type = "eq" }]
play = [
  { at = 1, clip = "lead", bars = 1 },
  { at = 2, clip = "a1", bars = 1 },
]

[clips.k4]
kind = "drum"
steps = "x...x...x...X..."

[clips.a1]
kind = "acid"
steps = "C2 C2! C3~ C2 . D#2 C2~ G2!"

[clips.lead]
kind = "notes"
bars = 1
notes = [{ start = 0, pitch = "C3", len = 3 }]

[[sections]]
name = "a"
bars = 4

[[automation]]
param = "acid.acid303.cutoff"
at = 1
bars = 2
points = [[0, 0.1], [2, 0.8]]

[[automation]]
param = "acid.eq.low_cut"
at = 1
bars = 2
points = [[0, 0.1], [2, 0.8]]
"#;

fn swap_song() -> Song {
    parse_song(SWAP).expect("the song parses")
}

#[test]
fn swap_keeps_strip_params_and_note_clips() {
    let edited = set_instrument(&swap_song(), registry(), "acid", "kick").unwrap();
    let acid = &edited.arranged.song.tracks["acid"];
    assert_eq!(acid.instrument.module.name, "kick");
    assert_eq!(acid.instrument.id, "kick");
    assert!(acid.instrument.params.is_empty());
    assert_eq!(acid.strip["send.delay"], -20.0);
    assert_eq!(fx_ids(&edited.arranged.song, "acid"), ["eq"]);
    let kept: Vec<_> = acid.play.iter().map(|p| p.clip.as_str()).collect();
    assert_eq!(kept, ["lead"], "the notes clip stays, the acid clip goes");
    let removed: Vec<_> = edited
        .removed
        .placements
        .iter()
        .map(|(t, p)| (t.as_str(), p.clip.as_str(), p.start_bar))
        .collect();
    assert_eq!(removed, [("acid", "a1", 1)]);
    assert!(
        edited.arranged.song.clips.contains_key("a1"),
        "the clip stays in the pool"
    );
    // The instrument's stretch goes; the effect's stays.
    let gone: Vec<_> = edited.removed.lanes.iter().map(|l| &l.param[..]).collect();
    assert_eq!(gone, ["acid303.cutoff"]);
    assert_eq!(edited.arranged.lanes, [LaneOrigin::Kept(1)]);
    assert_eq!(edited.removed.duck_source, None);
}

#[test]
fn swap_to_the_same_module_changes_nothing() {
    let s = swap_song();
    let edited = set_instrument(&s, registry(), "acid", "acid303").unwrap();
    assert_eq!(edited.arranged.song, s);
    assert_eq!(edited.removed, Default::default());
}

#[test]
fn swap_clears_a_duck_source() {
    let s = swap_song();
    let edited = set_instrument(&s, registry(), "kick", "acid303").unwrap();
    assert_eq!(edited.removed.duck_source.as_deref(), Some("kick"));
    assert_eq!(edited.arranged.song.mix.duck_source, None);
    // The kick's drum placement cannot be played by a 303.
    assert_eq!(edited.removed.placements.len(), 1);
    // Another drum voice keeps the source.
    let edited = set_instrument(&s, registry(), "kick", "clap").unwrap();
    assert_eq!(edited.removed.duck_source, None);
    assert_eq!(
        edited.arranged.song.mix.duck_source.as_deref(),
        Some("kick")
    );
    assert!(edited.removed.placements.is_empty());
}

#[test]
fn swap_refusals() {
    let s = swap_song();
    let swap = |track: &str, module: &str| err(set_instrument(&s, registry(), track, module));
    assert_eq!(swap("bass", "kick"), "there is no track `bass`");
    assert_eq!(swap("acid", "eq"), "`eq` is an effect, not an instrument");
    assert_eq!(
        swap("acid", "acid3030"),
        "unknown instrument `acid3030` (expected acid303, kick, clap, hat_closed, hat_open or ride)"
    );
    let taken = SWAP.replacen(
        "fx = [{ type = \"eq\" }]",
        "fx = [{ type = \"eq\", id = \"kick\" }]",
        1,
    );
    let taken = taken.replace("acid.eq.low_cut", "acid.kick.low_cut");
    let taken = parse_song(&taken).unwrap();
    assert_eq!(
        err(set_instrument(&taken, registry(), "acid", "kick")),
        "track `acid` already has a device `kick`; rename it first"
    );
}
