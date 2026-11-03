//! The wire protocol, pinned by a golden file. The frontend tests read the
//! same `fixtures/protocol.json`, so a change to a command or an event shows
//! up on both sides at once. `UPDATE_GOLDEN=1` rewrites the file.

use std::collections::HashSet;

use serde_json::{Value, json};
use tune_studio::{
    ACTIONS, Action, Change, Command, DrumHitDto, ErrorView, Event, ExternalChange, RunTag,
    SongPath, Source, StudioCommand, StudioCore, StudioState, action_from_cli, find_action,
};

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/protocol.json");
const COMMENTED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/commented.toml");

/// The wire name of a command. The exhaustive match makes a new variant a
/// compile error here until it gets a golden sample.
fn wire_name(c: &Command) -> &'static str {
    match c {
        Command::Studio(s) => match s {
            StudioCommand::ListSongs => "list_songs",
            StudioCommand::GetState => "get_state",
            StudioCommand::OpenSong { .. } => "open_song",
            StudioCommand::ResolveOffered { .. } => "resolve_offered",
        },
        Command::Action(a) => match a {
            Action::Play { .. } => "play",
            Action::Stop => "stop",
            Action::SeekBar { .. } => "seek_bar",
            Action::SetLoop { .. } => "set_loop",
            Action::LaunchClip { .. } => "launch_clip",
            Action::LaunchScene { .. } => "launch_scene",
            Action::StopClip { .. } => "stop_clip",
            Action::BackToArrangement { .. } => "back_to_arrangement",
            Action::Undo => "undo",
            Action::Redo => "redo",
            Action::SetValue { .. } => "set_value",
            Action::SetParam { .. } => "set_param",
            Action::SetDeviceSetting { .. } => "set_device_setting",
            Action::SetAcidStep { .. } => "set_acid_step",
            Action::SetDrumStep { .. } => "set_drum_step",
            Action::MoveSection { .. } => "move_section",
            Action::ResizeSection { .. } => "resize_section",
            Action::DuplicateSection { .. } => "duplicate_section",
            Action::DeleteSection { .. } => "delete_section",
            Action::SetSectionPlay { .. } => "set_section_play",
            Action::SetTrackMix { .. } => "set_track_mix",
            Action::AddTrack { .. } => "add_track",
            Action::RemoveTrack { .. } => "remove_track",
            Action::RenameTrack { .. } => "rename_track",
            Action::AddEffect { .. } => "add_effect",
            Action::RemoveEffect { .. } => "remove_effect",
            Action::MoveEffect { .. } => "move_effect",
            Action::RenameDevice { .. } => "rename_device",
            Action::SetInstrument { .. } => "set_instrument",
            Action::AddClip { .. } => "add_clip",
            Action::RemoveClip { .. } => "remove_clip",
            Action::RenameClip { .. } => "rename_clip",
            Action::DuplicateClip { .. } => "duplicate_clip",
            Action::AddPlacement { .. } => "add_placement",
            Action::MovePlacement { .. } => "move_placement",
            Action::ResizePlacement { .. } => "resize_placement",
            Action::RemovePlacement { .. } => "remove_placement",
            Action::AddSection { .. } => "add_section",
            Action::RenameSection { .. } => "rename_section",
            Action::AddScene { .. } => "add_scene",
            Action::RemoveScene { .. } => "remove_scene",
            Action::RenameScene { .. } => "rename_scene",
            Action::MoveScene { .. } => "move_scene",
            Action::SetSlot { .. } => "set_slot",
            Action::AddLane { .. } => "add_lane",
            Action::RemoveLane { .. } => "remove_lane",
            Action::SetLanePoints { .. } => "set_lane_points",
            Action::UndoRun { .. } => "undo_run",
        },
    }
}

/// Why a command cannot be given to `tune do`, or `None` when it can. The
/// exhaustive match names every exemption (spec §8.1); there are no others.
fn exemption(c: &Command) -> Option<&'static str> {
    match c {
        Command::Studio(s) => Some(match s {
            StudioCommand::GetState => "the CLI reads the file and `tune info`",
            StudioCommand::ListSongs | StudioCommand::OpenSong { .. } => {
                "the CLI names the song file"
            }
            StudioCommand::ResolveOffered { .. } => "the conflict prompt only a human sees",
        }),
        Command::Action(Action::SetParam { commit: false, .. }) => {
            Some("a knob preview during a drag; the agent sets the final value")
        }
        Command::Action(_) => None,
    }
}

/// One sample of every studio command, then one of every action in catalog
/// order.
fn sample_commands() -> Vec<Command> {
    let studio = [
        StudioCommand::ListSongs,
        StudioCommand::GetState,
        StudioCommand::OpenSong {
            path: "songs/mini/song.toml".into(),
        },
        StudioCommand::ResolveOffered { reapply: true },
    ];
    studio
        .into_iter()
        .map(Command::Studio)
        .chain(sample_actions().into_iter().map(Command::Action))
        .collect()
}

fn sample_actions() -> Vec<Action> {
    vec![
        Action::Play { from_bar: Some(4) },
        Action::Stop,
        Action::SeekBar { bar: 8 },
        Action::SetLoop { bars: Some([2, 6]) },
        Action::LaunchClip {
            track: "acid".into(),
            scene: "groove".into(),
        },
        Action::LaunchScene {
            scene: "groove".into(),
        },
        Action::StopClip {
            track: "hat".into(),
        },
        Action::BackToArrangement { track: None },
        Action::Undo,
        Action::Redo,
        Action::SetValue {
            path: "tracks.acid.acid303.cutoff".parse().expect("sample path"),
            value: json!(820),
        },
        Action::SetParam {
            track: "acid".into(),
            param: "acid303.cutoff".into(),
            value: 820.0,
            commit: true,
        },
        Action::SetDeviceSetting {
            track: "acid".into(),
            device: "dist".into(),
            setting: "mode".into(),
            value: "hard".into(),
        },
        Action::SetAcidStep {
            clip: "a1".into(),
            index: 3,
            note: Some(36),
            accent: true,
            slide: false,
        },
        Action::SetDrumStep {
            clip: "k4".into(),
            index: 5,
            hit: DrumHitDto::Accent,
        },
        Action::MoveSection { from: 0, to: 2 },
        Action::ResizeSection { index: 1, bars: 4 },
        Action::DuplicateSection { index: 1 },
        Action::DeleteSection { index: 0 },
        Action::SetSectionPlay {
            section: 1,
            track: "kick".into(),
            clip: None,
        },
        Action::SetTrackMix {
            track: "hat".into(),
            gain_db: Some(-8.5),
            pan: None,
            mute: Some(true),
            solo: Some(false),
        },
        Action::AddTrack {
            track: "bass".into(),
            instrument: "acid303".into(),
        },
        Action::RemoveTrack {
            track: "bass".into(),
        },
        Action::RenameTrack {
            track: "acid".into(),
            to: "bass".into(),
        },
        Action::AddEffect {
            track: "acid".into(),
            module: "eq".into(),
            id: None,
            index: Some(0),
        },
        Action::RemoveEffect {
            track: "acid".into(),
            device: "dist".into(),
        },
        Action::MoveEffect {
            track: "acid".into(),
            device: "dist".into(),
            index: 1,
        },
        Action::RenameDevice {
            track: "acid".into(),
            device: "dist".into(),
            to: "drive2".into(),
        },
        Action::SetInstrument {
            track: "acid".into(),
            module: "kick".into(),
        },
        Action::AddClip {
            clip: "lead".into(),
            kind: "notes".into(),
            steps: None,
            bars: Some(2),
            notes: Some(json!([{ "start": 0, "pitch": "C3", "len": 2 }])),
        },
        Action::RemoveClip { clip: "k4".into() },
        Action::RenameClip {
            clip: "a1".into(),
            to: "bass1".into(),
        },
        Action::DuplicateClip {
            clip: "h".into(),
            to: "h2".into(),
        },
        Action::AddPlacement {
            track: "acid".into(),
            clip: "a1".into(),
            at: 1,
            bars: 2,
            offset: None,
        },
        Action::MovePlacement {
            track: "acid".into(),
            at: 3,
            to: 1,
        },
        Action::ResizePlacement {
            track: "acid".into(),
            at: 3,
            bars: 1,
        },
        Action::RemovePlacement {
            track: "kick".into(),
            at: 3,
        },
        Action::AddSection {
            name: "break".into(),
            bars: 2,
            index: Some(1),
        },
        Action::RenameSection {
            index: 1,
            name: "drop".into(),
        },
        Action::AddScene {
            scene: "jam".into(),
            index: Some(1),
        },
        Action::RemoveScene {
            scene: "groove".into(),
        },
        Action::RenameScene {
            scene: "groove".into(),
            to: "drop".into(),
        },
        Action::MoveScene {
            scene: "break".into(),
            index: 0,
        },
        Action::SetSlot {
            scene: "groove".into(),
            track: "acid".into(),
            clip: Some("a1".into()),
        },
        Action::AddLane {
            param: "mix.reverb.size".into(),
            at: 1,
            bars: 4,
            points: vec![[0.0, 0.2], [4.0, 0.6]],
        },
        Action::RemoveLane {
            param: "acid.acid303.cutoff".into(),
            at: 3,
        },
        Action::SetLanePoints {
            param: "acid.acid303.cutoff".into(),
            at: 3,
            points: vec![[0.0, 0.3], [2.0, 0.9]],
        },
        Action::UndoRun { run: "r1".into() },
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
        // One sample per event: Claude's, so the golden pins the run's form.
        Event::Applied {
            source: Source::Claude,
            changes: vec![change("song.bpm", json!(120), json!(130))],
            run: Some(RunTag::new("r1", Some("open the filter")).expect("a run")),
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
    for (raw, sample) in commands.iter().zip(sample_commands()) {
        let cmd: Command = serde_json::from_value(raw.clone())
            .unwrap_or_else(|e| panic!("`{raw}` does not parse: {e}"));
        // A studio command parses as one, an action as an action.
        assert_eq!(cmd, sample);
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

#[test]
fn every_protocol_action_is_reachable_from_tune_do() {
    let preview = Command::Action(Action::SetParam {
        track: "acid".into(),
        param: "acid303.cutoff".into(),
        value: 640.0,
        commit: false,
    });
    for cmd in sample_commands().into_iter().chain([preview.clone()]) {
        if let Some(reason) = exemption(&cmd) {
            assert!(!reason.is_empty());
            continue;
        }
        let Command::Action(a) = &cmd else {
            panic!("{cmd:?} is neither exempt nor an action");
        };
        let args = a.to_cli_args();
        assert_eq!(
            action_from_cli(a.name(), &args).as_ref(),
            Ok(a),
            "{} {args:?}",
            a.name()
        );
    }
    // The knob preview is exempt, and `tune do` refuses it.
    assert!(exemption(&preview).is_some());
    let Command::Action(a) = &preview else {
        unreachable!()
    };
    assert!(action_from_cli(a.name(), &a.to_cli_args()).is_err());
}

#[test]
fn the_catalog_matches_the_enum() {
    let actions = sample_actions();
    let names: Vec<&str> = ACTIONS.iter().map(|s| s.name).collect();
    let samples: Vec<&str> = actions.iter().map(Action::name).collect();
    assert_eq!(names, samples);
    for a in &actions {
        let spec = a.spec();
        assert_eq!(spec.name, a.name());
        assert_eq!(find_action(a.name()).map(|s| s.name), Some(a.name()));
        let wire = serde_json::to_value(a).expect("serialize an action");
        let keys: Vec<String> = wire
            .get("args")
            .and_then(Value::as_object)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        let spec_names: Vec<&str> = spec.args.iter().map(|g| g.name).collect();
        for k in &keys {
            assert!(
                spec_names.contains(&k.as_str()),
                "`{}` sends `{k}`, which its spec lacks",
                a.name()
            );
        }
        for arg in spec.args.iter().filter(|g| g.required) {
            assert!(
                keys.iter().any(|k| k == arg.name),
                "`{}` lacks its required `{}`",
                a.name(),
                arg.name
            );
        }
    }
    assert!(find_action("nope").is_none());
}
