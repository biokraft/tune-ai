//! The wire protocol, pinned by a golden file. The frontend tests read the
//! same `fixtures/protocol.json`, so a change to a command or an event shows
//! up on both sides at once. `UPDATE_GOLDEN=1` rewrites the file.

use std::collections::HashSet;

use serde_json::{Value, json};
use tune_studio::{
    Change, Command, DrumHitDto, ErrorView, Event, ExternalChange, SongPath, Source, StudioCore,
    StudioState,
};

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/protocol.json");
const COMMENTED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/commented.toml");

/// The wire name of a command. The exhaustive match makes a new variant a
/// compile error here until it gets a golden sample.
fn wire_name(c: &Command) -> &'static str {
    match c {
        Command::ListSongs => "list_songs",
        Command::GetState => "get_state",
        Command::OpenSong { .. } => "open_song",
        Command::Play { .. } => "play",
        Command::Stop => "stop",
        Command::SeekBar { .. } => "seek_bar",
        Command::SetLoop { .. } => "set_loop",
        Command::SetParam { .. } => "set_param",
        Command::SetFxMode { .. } => "set_fx_mode",
        Command::SetAcidStep { .. } => "set_acid_step",
        Command::SetDrumStep { .. } => "set_drum_step",
        Command::MoveSection { .. } => "move_section",
        Command::ResizeSection { .. } => "resize_section",
        Command::DuplicateSection { .. } => "duplicate_section",
        Command::DeleteSection { .. } => "delete_section",
        Command::SetSectionPlay { .. } => "set_section_play",
        Command::SetTrackMix { .. } => "set_track_mix",
        Command::Undo => "undo",
        Command::Redo => "redo",
        Command::ResolveOffered { .. } => "resolve_offered",
    }
}

fn sample_commands() -> Vec<Command> {
    vec![
        Command::ListSongs,
        Command::GetState,
        Command::OpenSong {
            path: "songs/mini/song.toml".into(),
        },
        Command::Play { from_bar: Some(4) },
        Command::Stop,
        Command::SeekBar { bar: 8 },
        Command::SetLoop { bars: Some([2, 6]) },
        Command::SetParam {
            track: "acid".into(),
            param: "acid303.cutoff".into(),
            value: 820.0,
            commit: true,
        },
        Command::SetFxMode {
            track: "acid".into(),
            device: "dist".into(),
            mode: "hard".into(),
        },
        Command::SetAcidStep {
            pattern: "a1".into(),
            index: 3,
            note: Some(36),
            accent: true,
            slide: false,
        },
        Command::SetDrumStep {
            pattern: "k4".into(),
            index: 5,
            hit: DrumHitDto::Accent,
        },
        Command::MoveSection { from: 0, to: 2 },
        Command::ResizeSection { index: 1, bars: 4 },
        Command::DuplicateSection { index: 1 },
        Command::DeleteSection { index: 0 },
        Command::SetSectionPlay {
            section: 1,
            track: "kick".into(),
            pattern: None,
        },
        Command::SetTrackMix {
            track: "hat".into(),
            gain_db: Some(-8.5),
            pan: None,
            mute: Some(true),
            solo: Some(false),
        },
        Command::Undo,
        Command::Redo,
        Command::ResolveOffered { reapply: true },
    ]
}

fn change(path: &str, old: Value, new: Value) -> Change {
    Change {
        path: path.parse::<SongPath>().expect("sample path"),
        old,
        new,
    }
}

/// The state of the commented fixture song, with a fixed path (the real one
/// is a temp dir).
fn sample_state() -> StudioState {
    // A copy: opening a song may write it.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("song.toml");
    std::fs::copy(COMMENTED, &path).expect("copy the fixture");
    let core = StudioCore::open(&path).expect("open the fixture");
    StudioState {
        path: Some("songs/mini/song.toml".into()),
        song: Some(core.view()),
        playing: false,
        cursor_bar: 0,
        loop_bars: Some([0, 2]),
        can_undo: true,
        can_redo: false,
        offered: vec![change("tracks.acid.acid303.cutoff", json!(500), json!(820))],
        file_errors: vec![ErrorView {
            line: Some(12),
            column: Some(5),
            message: "expected a value".into(),
        }],
        migrated_from: Some(1),
    }
}

fn sample_events() -> Vec<Event> {
    vec![
        Event::State(sample_state()),
        Event::Playhead {
            step: 37,
            bar: 2,
            beat: 1,
            playing: true,
        },
        Event::Applied {
            source: Source::Human,
            changes: vec![change("tracks.acid.acid303.cutoff", json!(500), json!(820))],
        },
        Event::ExternalChange(ExternalChange {
            source: Source::Claude,
            changes: vec![change("sections[1].bars", json!(2), json!(4))],
            replaced: false,
            merged: vec![change("tracks.hat.pan", json!(0.3), json!(0.4))],
            offered: vec![change("tracks.acid.acid303.cutoff", json!(820), json!(500))],
            errors: vec![],
        }),
        Event::Error {
            message: "cannot write the song file".into(),
        },
    ]
}

fn golden_value() -> Value {
    let commands: Vec<Value> = sample_commands()
        .iter()
        .map(|c| serde_json::to_value(c).expect("serialize a command"))
        .collect();
    let events: Vec<Value> = sample_events()
        .iter()
        .map(|e| serde_json::to_value(e).expect("serialize an event"))
        .collect();
    json!({ "commands": commands, "events": events })
}

fn read_golden() -> Value {
    let text = std::fs::read_to_string(GOLDEN).expect("read protocol.json");
    serde_json::from_str(&text).expect("protocol.json is JSON")
}

#[test]
fn golden_matches() {
    let now = golden_value();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        let mut text = serde_json::to_string_pretty(&now).expect("pretty");
        text.push('\n');
        std::fs::write(GOLDEN, text).expect("write protocol.json");
    }
    assert_eq!(
        now,
        read_golden(),
        "the wire format changed; review the diff, then rerun with UPDATE_GOLDEN=1"
    );
}

#[test]
fn commands_round_trip() {
    let golden = read_golden();
    let commands = golden["commands"].as_array().expect("commands array");
    assert_eq!(commands.len(), sample_commands().len());
    for raw in commands {
        let cmd: Command = serde_json::from_value(raw.clone())
            .unwrap_or_else(|e| panic!("`{raw}` does not parse: {e}"));
        assert_eq!(&serde_json::to_value(&cmd).expect("reserialize"), raw);
    }
}

#[test]
fn every_command_variant_has_a_sample() {
    let names: HashSet<&str> = sample_commands().iter().map(wire_name).collect();
    assert_eq!(names.len(), sample_commands().len(), "one sample each");
    for c in sample_commands() {
        assert_eq!(serde_json::to_value(&c).unwrap()["cmd"], wire_name(&c));
    }
}

#[test]
fn every_event_has_a_distinct_tag() {
    let tags: HashSet<String> = sample_events()
        .iter()
        .map(|e| serde_json::to_value(e).unwrap()["event"].to_string())
        .collect();
    assert_eq!(tags.len(), 5);
}
