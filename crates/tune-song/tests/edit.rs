//! Track and device edits: the song each one makes, what it removes, and its
//! refusals.

use tune_dsp::DrumPattern;
use tune_song::{
    Clip, EditOpError, Lane, LaneOrigin, Placement, Registry, Song, add_clip, add_effect, add_lane,
    add_placement, add_track, clip_table_toml, duplicate_clip, move_effect, move_placement,
    parse_song, remove_clip, remove_effect, remove_lane, remove_placement, remove_track,
    rename_clip, rename_device, rename_track, resize_placement, set_instrument, set_lane_points,
    track_table_toml,
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

// ----- clips -----

fn clip_ids(song: &Song) -> Vec<&str> {
    song.clips.keys().map(String::as_str).collect()
}

fn drum(steps: &str) -> Clip {
    Clip::Drum(DrumPattern::parse(steps).unwrap())
}

#[test]
fn a_new_clip_goes_last() {
    let s = swap_song();
    let edited = add_clip(&s, "k8", drum("x.x.x.x.x.x.x.x.")).unwrap();
    assert_eq!(clip_ids(&edited.arranged.song), ["k4", "a1", "lead", "k8"]);
    assert_eq!(edited.arranged.song.clips["k8"], drum("x.x.x.x.x.x.x.x."));
    assert_eq!(edited.arranged.song.tracks, s.tracks);
    assert_eq!(edited.arranged.lanes.len(), s.automation.len());
    assert_eq!(edited.removed, Default::default());
}

#[test]
fn clip_id_rules() {
    let s = swap_song();
    let clip = || drum("x...");
    assert_eq!(err(add_clip(&s, "k4", clip())), "clip `k4` already exists");
    assert_eq!(
        err(add_clip(&s, "K9", clip())),
        "a clip id is lower-case letters, digits and `_`, got `K9`"
    );
    assert_eq!(err(remove_clip(&s, "k9")), "there is no clip `k9`");
    assert_eq!(err(rename_clip(&s, "k9", "k8")), "there is no clip `k9`");
    assert_eq!(err(rename_clip(&s, "k4", "a1")), "clip `a1` already exists");
    assert_eq!(
        err(rename_clip(&s, "k4", "K8")),
        "a clip id is lower-case letters, digits and `_`, got `K8`"
    );
    assert_eq!(err(duplicate_clip(&s, "k9", "k8")), "there is no clip `k9`");
    assert_eq!(
        err(duplicate_clip(&s, "k4", "a1")),
        "clip `a1` already exists"
    );
    assert_eq!(
        err(duplicate_clip(&s, "k4", "k4")),
        "clip `k4` already exists"
    );
}

#[test]
fn removing_a_clip_removes_and_lists_its_placements() {
    let s = swap_song();
    let edited = remove_clip(&s, "a1").unwrap();
    let song = &edited.arranged.song;
    assert_eq!(clip_ids(song), ["k4", "lead"]);
    let plays: Vec<_> = song.tracks["acid"]
        .play
        .iter()
        .map(|p| p.clip.as_str())
        .collect();
    assert_eq!(plays, ["lead"]);
    let removed: Vec<_> = edited
        .removed
        .placements
        .iter()
        .map(|(t, p)| (t.as_str(), p.clip.as_str(), p.start_bar, p.bars))
        .collect();
    assert_eq!(removed, [("acid", "a1", 1, 1)]);
    assert!(edited.removed.lanes.is_empty());
    assert_eq!(edited.removed.duck_source, None);
    assert_eq!(edited.arranged.lanes.len(), 2);
}

#[test]
fn renaming_a_clip_keeps_its_place_and_follows_its_placements() {
    let s = swap_song();
    let edited = rename_clip(&s, "a1", "bass1").unwrap();
    let song = &edited.arranged.song;
    assert_eq!(clip_ids(song), ["k4", "bass1", "lead"]);
    assert_eq!(song.clips["bass1"], s.clips["a1"]);
    let plays: Vec<_> = song.tracks["acid"]
        .play
        .iter()
        .map(|p| p.clip.as_str())
        .collect();
    assert_eq!(plays, ["lead", "bass1"]);
    assert_eq!(edited.removed, Default::default());
    // The same name changes nothing.
    assert_eq!(rename_clip(&s, "a1", "a1").unwrap().arranged.song, s);
}

#[test]
fn a_duplicate_stands_right_after_the_original() {
    let s = swap_song();
    let edited = duplicate_clip(&s, "k4", "k4b").unwrap();
    let song = &edited.arranged.song;
    assert_eq!(clip_ids(song), ["k4", "k4b", "a1", "lead"]);
    assert_eq!(song.clips["k4b"], s.clips["k4"]);
    assert_eq!(song.tracks, s.tracks);
}

#[test]
fn clip_tables_are_written_as_to_toml_writes_them() {
    let s = swap_song();
    assert_eq!(
        clip_table_toml("k4", &s.clips["k4"]),
        "[clips.k4]\nkind = \"drum\"\nsteps = \"x... x... x... X...\"\n"
    );
    assert_eq!(
        clip_table_toml("lead", &s.clips["lead"]),
        "[clips.lead]\nkind = \"notes\"\nbars = 1\n\
         notes = [{ start = 0, pitch = \"C3\", len = 3 }]\n"
    );
    // Every table of a song's text is one of these.
    let whole = s.to_toml();
    for (id, clip) in &s.clips {
        assert!(whole.contains(&clip_table_toml(id, clip)), "{id}");
    }
}

const PLACED: &str = r#"
[song]
version = 2
name = "t"
bpm = 120

[tracks.kick]
instrument = "kick"
play = [{ at = 1, clip = "k4", bars = 2 }, { at = 5, clip = "k4", bars = 2 }]

[clips.k4]
kind = "drum"
steps = "x...x...x...X..."

[[sections]]
name = "a"
bars = 8
"#;

fn placed() -> Song {
    parse_song(PLACED).expect("the song parses")
}

/// A track's placements as `(0-based start bar, bars, offset)`.
fn spans(song: &Song, track: &str) -> Vec<(u32, u32, u32)> {
    song.tracks[track]
        .play
        .iter()
        .map(|p| (p.start_bar, p.bars, p.offset_bars))
        .collect()
}

fn placement(start_bar: u32, bars: u32) -> Placement {
    Placement {
        start_bar,
        bars,
        clip: "k4".into(),
        offset_bars: 0,
    }
}

#[test]
fn a_placement_goes_in_by_time() {
    let s = placed();
    let edited = add_placement(&s, "kick", placement(3, 1)).unwrap();
    let song = &edited.arranged.song;
    assert_eq!(spans(song, "kick"), [(0, 2, 0), (3, 1, 0), (4, 2, 0)]);
    assert_eq!(edited.removed, Default::default());
    assert_eq!(edited.arranged.lanes.len(), s.automation.len());
    // A tie goes after the placement already there (the validator then
    // reports the overlap at the new one).
    let tie = add_placement(&s, "kick", placement(0, 1)).unwrap();
    assert_eq!(
        spans(&tie.arranged.song, "kick"),
        [(0, 2, 0), (0, 1, 0), (4, 2, 0)]
    );
    assert_eq!(
        add_placement(&s, "bass", placement(0, 1)).unwrap_err(),
        EditOpError::NoSuchTrack("bass".into())
    );
}

#[test]
fn a_placement_moves_and_resizes_by_its_start() {
    let s = placed();
    let moved = move_placement(&s, "kick", 4, 2).unwrap();
    assert_eq!(spans(&moved.arranged.song, "kick"), [(0, 2, 0), (2, 2, 0)]);
    // Past the other one: the list stays in time order.
    let moved = move_placement(&s, "kick", 0, 6).unwrap();
    let play = &moved.arranged.song.tracks["kick"].play;
    assert_eq!(play.iter().map(|p| p.start_bar).collect::<Vec<_>>(), [4, 6]);
    let resized = resize_placement(&s, "kick", 4, 1).unwrap();
    assert_eq!(
        spans(&resized.arranged.song, "kick"),
        [(0, 2, 0), (4, 1, 0)]
    );
    // A move to where it is changes nothing.
    assert_eq!(move_placement(&s, "kick", 4, 4).unwrap().arranged.song, s);
}

#[test]
fn removing_a_placement_lists_it() {
    let s = placed();
    let edited = remove_placement(&s, "kick", 4).unwrap();
    assert_eq!(spans(&edited.arranged.song, "kick"), [(0, 2, 0)]);
    assert_eq!(
        edited.removed.placements,
        [("kick".to_owned(), placement(4, 2))]
    );
}

#[test]
fn a_missing_placement_is_named_one_based() {
    let s = placed();
    for err in [
        move_placement(&s, "kick", 1, 2).unwrap_err(),
        resize_placement(&s, "kick", 1, 2).unwrap_err(),
        remove_placement(&s, "kick", 1).unwrap_err(),
    ] {
        assert_eq!(err.to_string(), "track `kick` has no placement at bar 2");
    }
    assert_eq!(
        remove_placement(&s, "bass", 0).unwrap_err(),
        EditOpError::NoSuchTrack("bass".into())
    );
}

const LANED: &str = r#"
[song]
version = 2
name = "t"
bpm = 120

[tracks.kick]
instrument = "kick"

[tracks.acid]
instrument = "acid303"

[clips.k4]
kind = "drum"
steps = "x...x...x...X..."

[[sections]]
name = "a"
bars = 2

[[sections]]
name = "b"
bars = 2

[[sections]]
name = "c"
bars = 2

[[automation]]
param = "acid.acid303.cutoff"
at = 1
bars = 2
points = [[0, 0.1], [2, 0.8]]

[[automation]]
param = "acid.acid303.resonance"
at = 3
bars = 2
points = [[0, 0.2], [2, 0.4]]

[[automation]]
param = "acid.acid303.cutoff"
at = 5
bars = 2
points = [[0, 0.5], [2, 0.6]]
"#;

fn laned() -> Song {
    parse_song(LANED).expect("the song parses")
}

fn reverb_lane(start_bar: u32) -> Lane {
    Lane {
        track: "mix".into(),
        param: "reverb.size".into(),
        start_bar,
        bars: 1,
        points: vec![(0.0, 0.2), (1.0, 0.6)],
    }
}

/// `(param, 0-based start bar)` of each stretch, in order.
fn stretches(song: &Song) -> Vec<(String, u32)> {
    song.automation
        .iter()
        .map(|l| (format!("{}.{}", l.track, l.param), l.start_bar))
        .collect()
}

#[test]
fn a_new_stretch_goes_after_the_ones_of_its_section() {
    let s = laned();
    // Bar index 1 is in section a, 2 is the first bar of section b (the
    // exact boundary), 4 the first of section c.
    for (start, at) in [(1, 1), (2, 2), (3, 2), (4, 3), (5, 3)] {
        let edited = add_lane(&s, reverb_lane(start)).unwrap();
        let mut want: Vec<LaneOrigin> = (0..3).map(LaneOrigin::Kept).collect();
        want.insert(at, LaneOrigin::New);
        assert_eq!(edited.arranged.lanes, want, "start {start}");
        assert_eq!(edited.arranged.song.automation[at], reverb_lane(start));
        assert_eq!(edited.removed, Default::default());
    }
    let edited = add_lane(&s, reverb_lane(2)).unwrap();
    assert_eq!(
        stretches(&edited.arranged.song),
        [
            ("acid.acid303.cutoff".to_owned(), 0),
            ("acid.acid303.resonance".to_owned(), 2),
            ("mix.reverb.size".to_owned(), 2),
            ("acid.acid303.cutoff".to_owned(), 4),
        ]
    );
}

#[test]
fn the_first_stretch_of_a_song_is_new() {
    let mut s = laned();
    s.automation.clear();
    let edited = add_lane(&s, reverb_lane(0)).unwrap();
    assert_eq!(edited.arranged.lanes, [LaneOrigin::New]);
    assert_eq!(edited.arranged.song.automation, [reverb_lane(0)]);
}

#[test]
fn a_new_stretch_is_not_checked_here() {
    // Overlaps, unknown params and points out of range are the validator's.
    let s = laned();
    let mut lane = reverb_lane(0);
    lane.track = "acid".into();
    lane.param = "acid303.cutoff".into();
    lane.points = vec![(0.0, 7.0)];
    let edited = add_lane(&s, lane.clone()).unwrap();
    assert_eq!(edited.arranged.song.automation.len(), 4);
    assert!(edited.arranged.song.automation.contains(&lane));
}

#[test]
fn removing_a_stretch_lists_it_and_keeps_the_others() {
    let s = laned();
    let edited = remove_lane(&s, "acid", "acid303.cutoff", 0).unwrap();
    assert_eq!(
        stretches(&edited.arranged.song),
        [
            ("acid.acid303.resonance".to_owned(), 2),
            ("acid.acid303.cutoff".to_owned(), 4)
        ]
    );
    assert_eq!(
        edited.arranged.lanes,
        [LaneOrigin::Kept(1), LaneOrigin::Kept(2)]
    );
    assert_eq!(edited.removed.lanes, [s.automation[0].clone()]);
    assert_eq!(edited.removed.placements, []);
    // The other stretch of the same param is picked by its start.
    let edited = remove_lane(&s, "acid", "acid303.cutoff", 4).unwrap();
    assert_eq!(edited.removed.lanes, [s.automation[2].clone()]);
    assert_eq!(
        edited.arranged.lanes,
        [LaneOrigin::Kept(0), LaneOrigin::Kept(1)]
    );
}

#[test]
fn a_stretch_is_found_only_by_its_exact_start() {
    let s = laned();
    // The first cutoff stretch covers bar indices 0 and 1 and ends where
    // bar index 2 begins: neither the bar inside it nor the one at its end
    // is its start.
    for (bar, shown) in [(1, 2), (2, 3), (3, 4)] {
        let message = format!("there is no lane `acid.acid303.cutoff` at bar {shown}");
        assert_eq!(
            remove_lane(&s, "acid", "acid303.cutoff", bar)
                .unwrap_err()
                .to_string(),
            message
        );
        assert_eq!(
            set_lane_points(&s, "acid", "acid303.cutoff", bar, vec![(0.0, 0.5)])
                .unwrap_err()
                .to_string(),
            message
        );
    }
    assert_eq!(
        remove_lane(&s, "acid", "acid303.nope", 0).unwrap_err(),
        EditOpError::NoSuchLane {
            track: "acid".into(),
            param: "acid303.nope".into(),
            bar: 0
        }
    );
}

#[test]
fn new_points_replace_only_the_points() {
    let s = laned();
    let points = vec![(0.0, 0.3), (1.0, 0.9), (1.0, 0.1), (2.0, 0.5)];
    let edited = set_lane_points(&s, "acid", "acid303.cutoff", 4, points.clone()).unwrap();
    let song = &edited.arranged.song;
    assert_eq!(song.automation[2].points, points);
    assert_eq!(
        Lane {
            points: s.automation[2].points.clone(),
            ..song.automation[2].clone()
        },
        s.automation[2]
    );
    assert_eq!(song.automation[..2], s.automation[..2]);
    assert_eq!(
        edited.arranged.lanes,
        [
            LaneOrigin::Kept(0),
            LaneOrigin::Kept(1),
            LaneOrigin::Kept(2)
        ]
    );
    assert_eq!(edited.removed, Default::default());
    assert_eq!(song.tracks, s.tracks);
}
