//! The action layer: CLI arguments into actions, and what each action
//! names as its targets.

use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use tune_song::parse_song;
use tune_studio::{
    Action, ActionsOutcome, DrumHitDto, Origin, SongDoc, StudioError, action_from_cli,
    apply_to_file,
};

const FIXTURE: &str = include_str!("fixtures/commented.toml");

fn args(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_owned()).collect()
}

fn cli(name: &str, xs: &[&str]) -> Result<Action, String> {
    action_from_cli(name, &args(xs))
}

#[test]
fn cli_arguments() {
    // An empty value is null: an optional argument left empty is `None`.
    assert_eq!(
        cli("set_section_play", &["section=1", "track=kick", "clip="]),
        Ok(Action::SetSectionPlay {
            section: 1,
            track: "kick".into(),
            clip: None,
        })
    );
    // A JSON argument takes `tune set`'s TOML value rules.
    let Ok(Action::SetValue { path, value }) = cli("set_value", &["path=song.bpm", "value=130"])
    else {
        panic!("set_value parses");
    };
    assert_eq!(path.to_string(), "song.bpm");
    assert_eq!(value, json!(130));
    // A text argument stays text, even when it reads as a number; a TOML
    // string literal is unquoted.
    let setting = |value: &str| {
        cli(
            "set_device_setting",
            &["track=acid", "device=dist", "setting=mode", value],
        )
    };
    let Ok(Action::SetDeviceSetting { value, .. }) = setting("value=2024") else {
        panic!("set_device_setting parses");
    };
    assert_eq!(value, "2024");
    let Ok(Action::SetDeviceSetting { value, .. }) = setting("value=\"hard\"") else {
        panic!("set_device_setting parses");
    };
    assert_eq!(value, "hard");
    // The first argument may go without its name.
    assert_eq!(cli("seek_bar", &["3"]), Ok(Action::SeekBar { bar: 3 }));
    assert_eq!(cli("stop", &[]), Ok(Action::Stop));
    // The Task 1 carry-over: a text action's literal case.
    assert_eq!(
        cli("add_track", &["track=bass", "instrument=acid303"]),
        Ok(Action::AddTrack {
            track: "bass".into(),
            instrument: "acid303".into(),
        })
    );
    assert_eq!(
        cli("add_track", &["bass", "instrument=acid303"]),
        cli("add_track", &["track=bass", "instrument=acid303"])
    );
    assert_eq!(
        cli("add_track", &["track=", "instrument=acid303"]),
        Err("`add_track` needs `track`".to_owned())
    );
    // The Task 1 carry-over: `rename_section`, whose first argument is a
    // number and the second a name.
    assert_eq!(
        cli("rename_section", &["index=1", "name=drop"]),
        Ok(Action::RenameSection {
            index: 1,
            name: "drop".into(),
        })
    );
    assert_eq!(
        cli("rename_section", &["1", "name=drop"]),
        cli("rename_section", &["index=1", "name=drop"])
    );
    assert_eq!(
        cli("rename_section", &["index=1"]),
        Err("`rename_section` needs `name`".to_owned())
    );
    assert_eq!(
        cli("add_effect", &["track=acid", "module=eq", "index=0"]),
        Ok(Action::AddEffect {
            track: "acid".into(),
            module: "eq".into(),
            id: None,
            index: Some(0),
        })
    );
    assert_eq!(
        cli("set_drum_step", &["clip=k4", "index=2", "hit=accent"]),
        Ok(Action::SetDrumStep {
            clip: "k4".into(),
            index: 2,
            hit: DrumHitDto::Accent,
        })
    );
    // An argument value may hold `=`; it splits at the first one.
    let Ok(Action::SetValue { value, .. }) = cli(
        "set_value",
        &["path=clips.k4.steps", "value=\"x...x...x...X...\""],
    ) else {
        panic!("set_value parses");
    };
    assert_eq!(value, json!("x...x...x...X..."));

    // The errors, verbatim.
    assert_eq!(
        cli("nope", &[]),
        Err("unknown action `nope` (see `tune actions`)".to_owned())
    );
    assert_eq!(
        cli("set_section_play", &["section=1", "track=kick", "foo=1"]),
        Err("`set_section_play` has no argument `foo` (it takes section, track, clip)".to_owned())
    );
    assert_eq!(
        cli(
            "set_section_play",
            &["section=1", "track=kick", "track=acid"]
        ),
        Err("`track` is given twice".to_owned())
    );
    assert_eq!(
        cli("set_section_play", &["1", "kick"]),
        Err("only the first argument (`section`) may be given without a name".to_owned())
    );
    assert_eq!(
        cli("set_section_play", &["1", "section=2"]),
        Err("`section` is given twice".to_owned())
    );
    assert_eq!(
        cli("set_section_play", &["section=1"]),
        Err("`set_section_play` needs `track`".to_owned())
    );
    assert_eq!(
        cli("stop", &["now"]),
        Err("`stop` has no argument `now` (it takes none)".to_owned())
    );
    let err = cli("seek_bar", &["bar=soon"]).unwrap_err();
    assert!(err.starts_with("bad arguments for `seek_bar`: "), "{err}");
    let err = cli("set_drum_step", &["clip=k4", "index=2", "hit=loud"]).unwrap_err();
    assert!(
        err.starts_with("bad arguments for `set_drum_step`: "),
        "{err}"
    );
    assert_eq!(
        cli(
            "set_param",
            &[
                "track=acid",
                "param=acid303.cutoff",
                "value=640",
                "commit=false"
            ]
        ),
        Err(
            "`set_param` with commit=false is a knob preview for the studio UI; give the value \
             without it"
                .to_owned()
        )
    );
    // Without `commit`, a knob value commits.
    assert_eq!(
        cli(
            "set_param",
            &["track=acid", "param=acid303.cutoff", "value=640"]
        ),
        Ok(Action::SetParam {
            track: "acid".into(),
            param: "acid303.cutoff".into(),
            value: 640.0,
            commit: true,
        })
    );
}

#[test]
fn targets_are_highlight_keys() {
    let knob = Action::SetParam {
        track: "acid".into(),
        param: "acid303.cutoff".into(),
        value: 640.0,
        commit: true,
    };
    assert_eq!(knob.targets(), ["tracks.acid.acid303.cutoff"]);
    let mix_knob = Action::SetParam {
        track: "mix".into(),
        param: "reverb.size".into(),
        value: 30.0,
        commit: true,
    };
    assert_eq!(mix_knob.targets(), ["mix.reverb.size"]);
    let step = Action::SetAcidStep {
        clip: "a1".into(),
        index: 3,
        note: Some(36),
        accent: false,
        slide: false,
    };
    assert_eq!(step.targets(), ["clips.a1.steps[3]"]);
    let mix = Action::SetTrackMix {
        track: "hat".into(),
        gain_db: Some(-3.0),
        pan: None,
        mute: Some(true),
        solo: None,
    };
    assert_eq!(mix.targets(), ["tracks.hat.gain_db", "tracks.hat.mute"]);
    let setting = Action::SetDeviceSetting {
        track: "acid".into(),
        device: "dist".into(),
        setting: "mode".into(),
        value: "hard".into(),
    };
    assert_eq!(setting.targets(), ["tracks.acid.dist.mode"]);
    assert_eq!(
        Action::SetSectionPlay {
            section: 2,
            track: "kick".into(),
            clip: None
        }
        .targets(),
        ["sections[2].play.kick"]
    );
    assert_eq!(
        Action::ResizeSection { index: 1, bars: 4 }.targets(),
        ["sections[1].bars"]
    );
    assert_eq!(Action::DeleteSection { index: 0 }.targets(), ["sections"]);
    assert!(Action::Play { from_bar: None }.targets().is_empty());
    assert!(Action::Undo.targets().is_empty());
}

/// Values that need care on the way out (quoting, nulls, arrays, inline
/// tables) read back as the same action.
#[test]
fn cli_args_read_back() {
    let setting = |value: &str| Action::SetDeviceSetting {
        track: "acid".into(),
        device: "dist".into(),
        setting: "mode".into(),
        value: value.into(),
    };
    let set_value = |path: &str, value| Action::SetValue {
        path: path.parse().expect("path"),
        value,
    };
    let cases = [
        (Action::SetLoop { bars: Some([2, 6]) }, vec!["bars=[2, 6]"]),
        (
            set_value("clips.k4.steps", json!("x...")),
            vec!["path=clips.k4.steps", "value=\"x...\""],
        ),
        (
            set_value("sections[1].play.acid", json!(null)),
            vec!["path=sections[1].play.acid", "value="],
        ),
        (
            set_value(
                "clips.lead.notes",
                json!([{"start": 0, "pitch": "C3", "len": 2.5}]),
            ),
            vec![
                "path=clips.lead.notes",
                "value=[{ start = 0, pitch = \"C3\", len = 2.5 }]",
            ],
        ),
        (
            setting("\"q\""),
            vec!["track=acid", "device=dist", "setting=mode", "value='\"q\"'"],
        ),
        (
            setting(""),
            vec!["track=acid", "device=dist", "setting=mode", "value=\"\""],
        ),
        (Action::Play { from_bar: None }, vec![]),
    ];
    for (action, want) in cases {
        let got = action.to_cli_args();
        assert_eq!(got, want, "{action:?}");
        assert_eq!(action_from_cli(action.name(), &got), Ok(action));
    }
}

// ----- tracks and devices -----

/// A temp dir holding a copy of `text` as `song.toml`.
struct Song {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn song_with(text: &str) -> Song {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("song.toml");
    fs::write(&path, text).expect("write song");
    Song { _dir: dir, path }
}

impl Song {
    fn apply(&self, actions: &[Action]) -> Result<ActionsOutcome, StudioError> {
        apply_to_file(&self.path, actions, &Origin::claude(None))
    }

    fn text(&self) -> String {
        fs::read_to_string(&self.path).expect("read song")
    }

    /// Applies one action that must work.
    fn ok(&self, action: Action) -> ActionsOutcome {
        self.apply(&[action]).unwrap_or_else(|e| panic!("{e}"))
    }

    /// Applies one action that must be refused, leaving the file as it was.
    fn refused(&self, action: Action) -> String {
        let before = self.text();
        let err = self.apply(&[action]).expect_err("refused").to_string();
        assert_eq!(self.text(), before, "a refused action writes nothing");
        err
    }

    fn song(&self) -> tune_song::Song {
        parse_song(&self.text()).expect("the file parses")
    }
}

fn fixture() -> Song {
    song_with(FIXTURE)
}

fn add_track(track: &str, instrument: &str) -> Action {
    Action::AddTrack {
        track: track.into(),
        instrument: instrument.into(),
    }
}

fn add_effect(track: &str, module: &str, id: Option<&str>, index: Option<usize>) -> Action {
    Action::AddEffect {
        track: track.into(),
        module: module.into(),
        id: id.map(Into::into),
        index,
    }
}

fn set_value(path: &str, value: Value) -> Action {
    Action::SetValue {
        path: path.parse().expect("path"),
        value,
    }
}

fn comment_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|l| l.find('#').map(|i| &l[i..]))
        .collect()
}

/// Asserts that every comment line of `before` not in `except` is in
/// `after`, in order.
fn comments_kept(before: &str, after: &str, except: &[&str]) {
    let want: Vec<&str> = comment_lines(before)
        .into_iter()
        .filter(|c| !except.contains(c))
        .collect();
    let mut have = comment_lines(after).into_iter();
    for c in &want {
        assert!(
            have.any(|h| h == *c),
            "comment `{c}` lost or out of order:\n{after}"
        );
    }
}

fn track_ids(song: &tune_song::Song) -> Vec<&str> {
    song.tracks.keys().map(String::as_str).collect()
}

#[test]
fn add_track_appends_a_default_track() {
    let s = fixture();
    let out = s.ok(add_track("bass", "acid303"));
    assert_eq!(out.results, [Value::Null]);
    let text = s.text();
    assert!(
        text.contains("]\n\n[tracks.bass]\ninstrument = \"acid303\"\n\n[clips.k4]"),
        "{text}"
    );
    assert_eq!(
        text.replacen("\n[tracks.bass]\ninstrument = \"acid303\"\n", "", 1),
        FIXTURE
    );
    comments_kept(FIXTURE, &text, &[]);
    let song = s.song();
    assert_eq!(track_ids(&song), ["kick", "acid", "hat", "bass"]);
    let _ = tune_engine::CompiledSong::new(&song, 48_000);
}

#[test]
fn track_id_rules() {
    let s = fixture();
    assert_eq!(
        s.refused(add_track("acid", "kick")),
        "track `acid` already exists"
    );
    assert_eq!(
        s.refused(Action::RemoveTrack {
            track: "bass".into()
        }),
        "there is no track `bass`"
    );
    assert_eq!(
        s.refused(add_track("mix", "kick")),
        "`mix` is reserved for the mix params"
    );
    assert_eq!(
        s.refused(add_track("Bass", "kick")),
        "a track id is lower-case letters, digits and `_`, got `Bass`"
    );
    assert_eq!(
        s.refused(add_track("bass", "acid3030")),
        "unknown instrument `acid3030` (expected acid303, kick, clap, hat_closed, hat_open or ride)"
    );
    assert_eq!(
        s.refused(add_track("bass", "eq")),
        "`eq` is an effect, not an instrument"
    );
    assert_eq!(
        s.refused(Action::RenameTrack {
            track: "acid".into(),
            to: "kick".into()
        }),
        "track `kick` already exists"
    );
}

#[test]
fn remove_track_cleans_up() {
    let s = fixture();
    let out = s.ok(Action::RemoveTrack {
        track: "acid".into(),
    });
    assert_eq!(
        out.results,
        [
            json!({"removed": {"lanes": [{"param": "acid.acid303.cutoff", "at": 3, "bars": 2,
            "points": [[0, 0.1], [2, 0.8]]}], "placements": [], "duck_source": null}})
        ]
    );
    let text = s.text();
    for gone in [
        "[tracks.acid]",
        "# the squelch",
        "[[automation]]",
        "# the filter opens over the section",
    ] {
        assert!(!text.contains(gone), "`{gone}` left in:\n{text}");
    }
    // The kick and hat tables are byte-identical.
    let table = |text: &str, id: &str| {
        let start = text.find(&format!("[tracks.{id}]")).expect("table");
        let rest = &text[start..];
        let end = rest.find("\n\n").map_or(rest.len(), |i| i + 1);
        rest[..end].to_owned()
    };
    for id in ["kick", "hat"] {
        assert_eq!(table(&text, id), table(FIXTURE, id), "{id}");
    }
    comments_kept(
        FIXTURE,
        &text,
        &["# the squelch", "# the filter opens over the section"],
    );
    assert_eq!(track_ids(&s.song()), ["kick", "hat"]);
}

#[test]
fn removing_the_duck_source_clears_it() {
    let with_source = FIXTURE.replacen(
        "\n# the body",
        "\n[mix]\nduck_source = \"kick\"\n\n# the body",
        1,
    );
    let s = song_with(&with_source);
    let out = s.ok(Action::RemoveTrack {
        track: "kick".into(),
    });
    assert_eq!(out.results[0]["removed"]["duck_source"], json!("kick"));
    assert!(!s.text().contains("duck_source"), "{}", s.text());

    let ducking = with_source.replacen(
        "params = { \"acid303.cutoff\" = 500,",
        "params = { \"duck.depth\" = 6, \"acid303.cutoff\" = 500,",
        1,
    );
    let s = song_with(&ducking);
    let err = s.refused(Action::RemoveTrack {
        track: "kick".into(),
    });
    assert!(err.contains("`duck.depth` needs a kick to follow"), "{err}");
}

#[test]
fn rename_track_rewrites_references() {
    let s = fixture();
    s.ok(Action::RenameTrack {
        track: "acid".into(),
        to: "bass".into(),
    });
    let text = s.text();
    assert!(
        text.contains("# the squelch\n[tracks.bass]\ninstrument = \"acid303\""),
        "{text}"
    );
    assert!(text.contains("param = \"bass.acid303.cutoff\""), "{text}");
    assert_eq!(track_ids(&s.song()), ["kick", "bass", "hat"]);
    comments_kept(FIXTURE, &text, &[]);
    // Only the renamed lines differ.
    let changed: Vec<_> = text
        .lines()
        .zip(FIXTURE.lines())
        .filter(|(a, b)| a != b)
        .map(|(a, _)| a)
        .collect();
    assert_eq!(
        changed,
        ["[tracks.bass]", "param = \"bass.acid303.cutoff\""]
    );
}

#[test]
fn effects_insert_at_their_index() {
    let s = fixture();
    let out = s.ok(add_effect("acid", "eq", None, Some(0)));
    assert_eq!(out.results, [json!({"id": "eq"})]);
    let text = s.text();
    assert!(
        text.contains(
            "fx = [{ type = \"eq\" }, { type = \"distortion\", mode = \"soft\", params = { \
             \"dist.drive\" = 12 } }]"
        ),
        "{text}"
    );
    let out = s.ok(add_effect("acid", "eq", None, None));
    assert_eq!(out.results, [json!({"id": "eq2"})]);
    assert!(
        text_fx(&s.text()).ends_with("{ type = \"eq\", id = \"eq2\" }]"),
        "{}",
        s.text()
    );
    s.ok(Action::MoveEffect {
        track: "acid".into(),
        device: "dist".into(),
        index: 2,
    });
    let song = s.song();
    let chain: Vec<_> = song.tracks["acid"]
        .fx
        .iter()
        .map(|d| d.id.as_str())
        .collect();
    assert_eq!(chain, ["eq", "eq2", "dist"]);
    assert_eq!(song.tracks["acid"].fx[2].params["drive"], 12.0);
    comments_kept(FIXTURE, &s.text(), &[]);
}

/// The `fx = […]` line of the acid track.
fn text_fx(text: &str) -> &str {
    text.lines()
        .find(|l| l.starts_with("fx = "))
        .expect("an fx line")
}

#[test]
fn the_first_effect_of_a_track_is_added_before_its_play_list() {
    let s = fixture();
    s.ok(add_effect("hat", "eq", Some("tone"), None));
    let text = s.text();
    assert!(
        text.contains("pan = 0.3\nfx = [{ type = \"eq\", id = \"tone\" }]\nplay = ["),
        "{text}"
    );
}

#[test]
fn remove_effect_removes_its_lanes() {
    let with_lane = format!(
        "{FIXTURE}\n# drive\n[[automation]]\nparam = \"acid.dist.drive\"\nat = 1\nbars = 2\npoints = [[0, 0.2], [2, 0.4]]\n"
    );
    let s = song_with(&with_lane);
    let out = s.ok(Action::RemoveEffect {
        track: "acid".into(),
        device: "dist".into(),
    });
    assert_eq!(
        out.results,
        [
            json!({"removed": {"lanes": [{"param": "acid.dist.drive", "at": 1, "bars": 2,
            "points": [[0, 0.2], [2, 0.4]]}], "placements": [], "duck_source": null}})
        ]
    );
    let text = s.text();
    assert!(!text.contains("fx ="), "{text}");
    assert!(!text.contains("dist.drive"), "{text}");
    assert!(!text.contains("# drive"), "{text}");
    // The other stretch stays.
    assert!(text.contains("acid.acid303.cutoff"), "{text}");
}

#[test]
fn rename_device_rewrites_params_and_lanes() {
    let s = fixture();
    s.ok(Action::RenameDevice {
        track: "acid".into(),
        device: "acid303".into(),
        to: "bass303".into(),
    });
    let text = s.text();
    assert!(
        text.contains("instrument = { type = \"acid303\", id = \"bass303\" }"),
        "{text}"
    );
    assert!(
        text.contains("params = { \"bass303.cutoff\" = 500, \"bass303.resonance\" = 70,"),
        "{text}"
    );
    assert!(text.contains("param = \"acid.bass303.cutoff\""), "{text}");
    s.ok(set_value("tracks.acid.bass303.cutoff", json!(600)));
    assert_eq!(s.song().tracks["acid"].instrument.params["cutoff"], 600.0);

    s.ok(Action::RenameDevice {
        track: "acid".into(),
        device: "dist".into(),
        to: "drive2".into(),
    });
    let text = s.text();
    assert!(
        text.contains(
            "{ type = \"distortion\", id = \"drive2\", mode = \"soft\", params = { \
             \"drive2.drive\" = 12 } }"
        ),
        "{text}"
    );
    comments_kept(FIXTURE, &text, &[]);
}

#[test]
fn device_id_rules() {
    let s = fixture();
    assert_eq!(
        s.refused(add_effect("acid", "eq", Some("dist"), None)),
        "track `acid` already has a device `dist`"
    );
    assert_eq!(
        s.refused(add_effect("acid", "eq", Some("send"), None)),
        "`send` and `duck` are reserved for the strip params"
    );
    assert_eq!(
        s.refused(add_effect("acid", "eq", Some("Eq"), None)),
        "a device id is lower-case letters, digits and `_`, got `Eq`"
    );
    assert_eq!(
        s.refused(add_effect("acid", "reverb", None, None)),
        "unknown effect `reverb` (expected distortion or eq)"
    );
    assert_eq!(
        s.refused(add_effect("acid", "kick", None, None)),
        "`kick` is an instrument, not an effect"
    );
    assert_eq!(
        s.refused(add_effect("acid", "eq", None, Some(4))),
        "index 4 is out of bounds (the chain has 1)"
    );
    assert_eq!(
        s.refused(Action::RemoveEffect {
            track: "acid".into(),
            device: "eq3".into()
        }),
        "track `acid` has no device `eq3`"
    );
    assert_eq!(
        s.refused(Action::RenameDevice {
            track: "acid".into(),
            device: "dist".into(),
            to: "acid303".into()
        }),
        "track `acid` already has a device `acid303`"
    );
}

#[test]
fn a_batch_builds_on_itself() {
    let s = fixture();
    let batch = vec![
        add_track("bass", "acid303"),
        add_effect("bass", "eq", None, None),
        set_value("tracks.bass.eq.low_cut", json!(120)),
    ];
    let out = s.apply(&batch).expect("applies");
    let hash = out.file_hash.clone().expect("written once");
    assert_eq!(out.results.len(), 3);
    let log = tune_studio::EditLog::for_song(&s.path)
        .read_since(None)
        .expect("read log");
    // Every change is logged under its action: a new effect also brings its
    // params into being.
    let mut actions: Vec<_> = log.iter().map(|e| e.action.as_deref()).collect();
    actions.dedup();
    assert_eq!(
        actions,
        [Some("add_track"), Some("add_effect"), Some("set_value")]
    );
    assert!(log.iter().all(|e| e.file_hash.as_deref() == Some(&hash)));
    let song = s.song();
    let bass = &song.tracks["bass"];
    assert_eq!(bass.fx[0].id, "eq");
    assert_eq!(bass.fx[0].params["low_cut"], 120.0);

    // One bad action in the batch writes nothing.
    let s = fixture();
    let mut bad = batch;
    bad.push(set_value("tracks.bass.eq.nope", json!(1)));
    assert!(s.apply(&bad).is_err());
    assert_eq!(s.text(), FIXTURE);
}

#[test]
fn track_and_device_actions_name_their_targets() {
    assert_eq!(add_track("bass", "kick").targets(), ["tracks.bass"]);
    assert_eq!(
        Action::RemoveTrack {
            track: "acid".into()
        }
        .targets(),
        ["tracks.acid"]
    );
    assert_eq!(
        Action::RenameTrack {
            track: "acid".into(),
            to: "bass".into()
        }
        .targets(),
        ["tracks.acid"]
    );
    assert_eq!(
        add_effect("acid", "eq", None, None).targets(),
        ["tracks.acid.fx"]
    );
    assert_eq!(
        Action::MoveEffect {
            track: "acid".into(),
            device: "dist".into(),
            index: 0
        }
        .targets(),
        ["tracks.acid.fx"]
    );
    let rename = |device: &str| Action::RenameDevice {
        track: "acid".into(),
        device: device.into(),
        to: "x".into(),
    };
    assert_eq!(rename("acid303").targets(), ["tracks.acid.instrument"]);
    assert_eq!(rename("dist").targets(), ["tracks.acid.fx"]);
}

#[test]
fn the_catalog_lists_the_track_and_device_actions() {
    for name in [
        "add_track",
        "remove_track",
        "rename_track",
        "add_effect",
        "remove_effect",
        "move_effect",
        "rename_device",
    ] {
        let spec = tune_studio::find_action(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(spec.kind, tune_studio::ActionKind::Edit);
    }
    let help = tune_studio::find_action("rename_track")
        .expect("listed")
        .help;
    assert!(help.contains("noise"), "{help}");
}

/// The text writers and the model agree, whatever the form of the file.
#[test]
fn edits_work_on_a_file_in_inline_and_one_line_forms() {
    let text = "[song]\nversion = 2\nname = \"t\"\nbpm = 120\n\n[tracks]\nkick = { instrument = \"kick\", fx = [\n  { type = \"eq\" },\n  { type = \"distortion\" },\n] }\n\n[[sections]]\nname = \"a\"\nbars = 1\n";
    let s = song_with(text);
    s.ok(add_track("bass", "acid303"));
    s.ok(Action::RenameTrack {
        track: "kick".into(),
        to: "drum".into(),
    });
    s.ok(add_effect("drum", "eq", None, Some(1)));
    s.ok(Action::MoveEffect {
        track: "drum".into(),
        device: "dist".into(),
        index: 0,
    });
    s.ok(Action::RemoveEffect {
        track: "drum".into(),
        device: "eq".into(),
    });
    s.ok(Action::RenameDevice {
        track: "drum".into(),
        device: "kick".into(),
        to: "thump".into(),
    });
    s.ok(Action::RemoveTrack {
        track: "bass".into(),
    });
    let song = s.song();
    assert_eq!(track_ids(&song), ["drum"]);
    assert_eq!(song.tracks["drum"].instrument.id, "thump");
    SongDoc::parse(&s.text()).expect("parses as a doc");
}

// ----- forms the writer cannot edit yet -----

/// m14: an effect chain written as `[[tracks.<t>.fx]]` tables is a valid
/// song, but the writer edits inline `fx` arrays only: adding, removing or
/// moving an effect is refused with a plain message, not an internal error.
#[test]
fn an_fx_array_of_tables_is_refused_plainly() {
    let text = FIXTURE.replace(
        "fx = [{ type = \"distortion\", mode = \"soft\", params = { \"dist.drive\" = 12 } }]\n\
         play = [{ at = 3, clip = \"a1\", bars = 2 }]\n",
        "play = [{ at = 3, clip = \"a1\", bars = 2 }]\n\n\
         [[tracks.acid.fx]]\ntype = \"distortion\"\nmode = \"soft\"\n\
         params = { \"dist.drive\" = 12 }\n\n\
         [[tracks.acid.fx]]\ntype = \"eq\"\n",
    );
    let s = song_with(&text);
    assert_eq!(s.song().tracks["acid"].fx.len(), 2, "{text}");
    let want = "this file writes `tracks.acid.fx` as [[tracks.acid.fx]] tables; \
                the studio can't edit that form yet, so change it by hand";
    let actions = [
        Action::AddEffect {
            track: "acid".into(),
            module: "eq".into(),
            id: Some("eq2".into()),
            index: None,
        },
        Action::RemoveEffect {
            track: "acid".into(),
            device: "eq".into(),
        },
        Action::MoveEffect {
            track: "acid".into(),
            device: "eq".into(),
            index: 0,
        },
    ];
    for action in actions {
        assert_eq!(s.refused(action.clone()), want, "{action:?}");
    }
}

/// m14: a root inline `tracks = { … }` table (a key of the root table,
/// so before `[song]`) is a valid song, but a new track cannot be written
/// into it: refused plainly.
#[test]
fn a_root_inline_tracks_table_is_refused_plainly() {
    let text = "tracks = { kick = { instrument = \"kick\", play = [{ at = 1, clip = \"k\", bars = 1 }] } }\n\n\
                [song]\nversion = 2\nname = \"x\"\nbpm = 120\n\n\
                [clips.k]\nkind = \"drum\"\nsteps = \"x...\"\n\n\
                [[sections]]\nname = \"a\"\nbars = 1\n";
    let s = song_with(text);
    assert_eq!(s.song().tracks.len(), 1);
    assert_eq!(
        s.refused(add_track("bass", "acid303")),
        "this file writes `tracks` as an inline table (`tracks = { ... }`); \
         the studio can't edit that form yet, so change it by hand"
    );
}

// ----- set_instrument -----

fn set_instrument(track: &str, module: &str) -> Action {
    Action::SetInstrument {
        track: track.into(),
        module: module.into(),
    }
}

/// The lines of `[tracks.<id>]`, up to the next table header.
fn track_lines(text: &str, id: &str) -> Vec<String> {
    let header = format!("[tracks.{id}]");
    text.lines()
        .skip_while(|l| *l != header)
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .map(str::to_owned)
        .collect()
}

#[test]
fn swap_cleans_up_and_lists_it() {
    let s = fixture();
    let out = s.ok(set_instrument("acid", "kick"));
    assert_eq!(
        out.results,
        [json!({"removed": {
            "lanes": [{"param": "acid.acid303.cutoff", "at": 3, "bars": 2,
                "points": [[0, 0.1], [2, 0.8]]}],
            "placements": [{"track": "acid", "at": 3, "clip": "a1", "bars": 2, "offset": 0}],
            "duck_source": null,
        }})]
    );
    let text = s.text();
    let acid = track_lines(&text, "acid");
    assert!(
        acid.contains(&"instrument = \"kick\"".to_owned()),
        "{acid:?}"
    );
    assert!(
        !acid
            .iter()
            .any(|l| l.starts_with("params") || l.starts_with("play")),
        "{acid:?}"
    );
    assert!(
        acid.iter()
            .any(|l| l.starts_with("fx = [{ type = \"distortion\"")),
        "the fx line stays: {acid:?}"
    );
    assert!(!text.contains("[[automation]]"), "{text}");
    assert!(text.contains("[clips.a1]"), "the clip stays in the pool");
    comments_kept(FIXTURE, &text, &["# the filter opens over the section"]);
    let song = s.song();
    assert_eq!(song.tracks["acid"].instrument.module.name, "kick");
}

#[test]
fn swap_to_the_same_module_writes_nothing() {
    let s = fixture();
    let out = s.ok(set_instrument("acid", "acid303"));
    assert!(out.changes.is_empty());
    assert_eq!(s.text(), FIXTURE);
}

#[test]
fn swap_keeps_strip_params_in_the_params_table() {
    let with_send = FIXTURE.replacen(
        "params = { \"acid303.cutoff\" = 500,",
        "params = { \"send.delay\" = -20, \"acid303.cutoff\" = 500,",
        1,
    );
    let s = song_with(&with_send);
    s.ok(set_instrument("acid", "kick"));
    let acid = track_lines(&s.text(), "acid");
    assert!(
        acid.contains(&"params = { \"send.delay\" = -20 }".to_owned()),
        "{acid:?}"
    );
    assert_eq!(s.song().tracks["acid"].strip["send.delay"], -20.0);
}

#[test]
fn swap_refusals_leave_the_file() {
    let s = fixture();
    assert_eq!(
        s.refused(set_instrument("bass", "kick")),
        "there is no track `bass`"
    );
    assert_eq!(
        s.refused(set_instrument("acid", "eq")),
        "`eq` is an effect, not an instrument"
    );
}

#[test]
fn swap_is_one_undo_step() {
    let s = fixture();
    let mut core = tune_studio::StudioCore::open(&s.path).expect("opens");
    core.apply_now(&[set_instrument("acid", "kick")], &Origin::claude(None))
        .expect("swaps");
    assert_ne!(s.text(), FIXTURE);
    core.undo(tune_studio::Source::Human).expect("undoes");
    assert_eq!(s.text(), FIXTURE);
}

#[test]
fn swap_names_its_target() {
    assert_eq!(
        set_instrument("acid", "kick").targets(),
        ["tracks.acid.instrument"]
    );
    let spec = tune_studio::find_action("set_instrument").expect("listed");
    assert_eq!(spec.kind, tune_studio::ActionKind::Edit);
    assert_eq!(
        cli("set_instrument", &["track=acid", "module=kick"]),
        Ok(set_instrument("acid", "kick"))
    );
    assert_eq!(
        set_instrument("acid", "kick").to_cli_args(),
        ["track=acid", "module=kick"]
    );
}

#[test]
fn swap_handles_a_device_table_and_inline_forms() {
    // An instrument table with its own id: its params go, whatever it was called.
    let renamed = FIXTURE
        .replacen(
            "instrument = \"acid303\"",
            "instrument = { type = \"acid303\", id = \"bass303\" }",
            1,
        )
        .replacen("\"acid303.", "\"bass303.", 5)
        .replace("acid.acid303.cutoff", "acid.bass303.cutoff");
    let s = song_with(&renamed);
    s.ok(set_instrument("acid", "hat_open"));
    let acid = track_lines(&s.text(), "acid");
    assert!(
        acid.contains(&"instrument = \"hat_open\"".to_owned()),
        "{acid:?}"
    );
    assert_eq!(s.song().tracks["acid"].instrument.id, "hat_open");

    // An inline track keeps its other keys' spacing.
    let text = "[song]\nversion = 2\nname = \"t\"\nbpm = 120\n\n[tracks]\nacid = { instrument = \"acid303\", params = { \"acid303.cutoff\" = 500, \"send.delay\" = -20 }, gain_db = -3 }\n\n[[sections]]\nname = \"a\"\nbars = 1\n";
    let s = song_with(text);
    s.ok(set_instrument("acid", "kick"));
    assert!(
        s.text().contains(
            "acid = { instrument = \"kick\", params = { \"send.delay\" = -20 }, gain_db = -3 }"
        ),
        "{}",
        s.text()
    );
}

// ----- clips -----

fn add_clip(clip: &str, kind: &str) -> Action {
    add_clip_with(clip, kind, None, None, None)
}

fn add_clip_with(
    clip: &str,
    kind: &str,
    steps: Option<&str>,
    bars: Option<u32>,
    notes: Option<Value>,
) -> Action {
    Action::AddClip {
        clip: clip.into(),
        kind: kind.into(),
        steps: steps.map(Into::into),
        bars,
        notes,
    }
}

/// The lines of `[clips.<id>]`, up to the next table header.
fn clip_lines(text: &str, id: &str) -> Vec<String> {
    let header = format!("[clips.{id}]");
    text.lines()
        .skip_while(|l| *l != header)
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .map(str::to_owned)
        .collect()
}

#[test]
fn add_clip_kinds() {
    let s = fixture();
    s.ok(add_clip_with(
        "k8",
        "drum",
        Some("x.x.x.x.x.x.x.x."),
        None,
        None,
    ));
    let text = s.text();
    // Right after the last clip table.
    assert!(
        text.contains(
            "steps = \"..x...x...x...x.\"\n\n[clips.k8]\nkind = \"drum\"\n\
             steps = \"x.x.x.x.x.x.x.x.\"\n\n[[sections]]"
        ),
        "{text}"
    );
    comments_kept(FIXTURE, &text, &[]);

    // A notes clip through the CLI: `notes` is the TOML array `tune set` reads.
    let lead = cli(
        "add_clip",
        &[
            "clip=lead",
            "kind=notes",
            "bars=2",
            "notes=[{ start = 0, pitch = \"C3\", len = 2 }]",
        ],
    )
    .expect("parses");
    s.ok(lead);
    let text = s.text();
    assert_eq!(
        clip_lines(&text, "lead"),
        [
            "kind = \"notes\"",
            "bars = 2",
            "notes = [{ start = 0, pitch = \"C3\", len = 2 }]",
            ""
        ]
    );
    assert_eq!(s.song().clips["lead"].steps(), 32);

    // Defaults: 16 rests, one bar, no notes.
    s.ok(add_clip("a2", "acid"));
    s.ok(add_clip("d2", "drum"));
    s.ok(add_clip("n2", "notes"));
    let text = s.text();
    assert_eq!(
        clip_lines(&text, "a2"),
        [
            "kind = \"acid\"",
            "steps = \". . . . . . . . . . . . . . . .\"",
            ""
        ]
    );
    assert_eq!(
        clip_lines(&text, "d2"),
        ["kind = \"drum\"", "steps = \"................\"", ""]
    );
    assert_eq!(
        clip_lines(&text, "n2"),
        ["kind = \"notes\"", "bars = 1", ""]
    );
    let song = s.song();
    let ids: Vec<&str> = song.clips.keys().map(String::as_str).collect();
    assert_eq!(ids, ["k4", "a1", "h", "k8", "lead", "a2", "d2", "n2"]);
}

#[test]
fn a_clip_goes_after_the_tracks_when_there_are_none() {
    let text = "[song]\nversion = 2\nname = \"t\"\nbpm = 120\n\n[tracks.kick]\ninstrument = \"kick\"\n\n[[sections]]\nname = \"a\"\nbars = 1\n";
    let s = song_with(text);
    s.ok(add_clip("k4", "drum"));
    assert_eq!(
        s.text(),
        text.replace(
            "\n[[sections]]",
            "\n[clips.k4]\nkind = \"drum\"\nsteps = \"................\"\n\n[[sections]]"
        )
    );
}

#[test]
fn add_clip_rules() {
    let s = fixture();
    assert_eq!(
        s.refused(add_clip("x", "chords")),
        "unknown clip kind `chords` (expected acid, drum or notes)"
    );
    assert_eq!(
        s.refused(add_clip("k4", "drum")),
        "clip `k4` already exists"
    );
    assert_eq!(
        s.refused(add_clip("K9", "drum")),
        "a clip id is lower-case letters, digits and `_`, got `K9`"
    );
    let steps_on_notes = s.refused(add_clip_with("n", "notes", Some("x..."), None, None));
    assert!(
        steps_on_notes.contains(
            "a notes clip has no `steps`: its length is `bars` and its notes are `notes`"
        ),
        "{steps_on_notes}"
    );
    let bars_on_drum = s.refused(add_clip_with("d", "drum", None, Some(2), None));
    assert!(
        bars_on_drum.contains("`bars` and `notes` are for a notes clip (kind = \"notes\")"),
        "{bars_on_drum}"
    );
    let bad_steps = s.refused(add_clip_with("d", "drum", Some("x.q"), None, None));
    assert!(
        bad_steps.starts_with("the edit would make the song invalid:\n"),
        "{bad_steps}"
    );
    let position = bad_steps.lines().nth(1).expect("an error line");
    assert!(
        position.chars().next().is_some_and(|c| c.is_ascii_digit()) && position.contains(':'),
        "a positioned error: {position}"
    );
    let bad_note = s.refused(add_clip_with(
        "n",
        "notes",
        None,
        None,
        Some(json!([{"start": 0, "pitch": "H9", "len": 2}])),
    ));
    assert!(bad_note.contains("pitch"), "{bad_note}");
}

#[test]
fn remove_clip_removes_its_placements() {
    let s = fixture();
    let out = s.ok(Action::RemoveClip { clip: "k4".into() });
    assert_eq!(
        out.results,
        [json!({"removed": {
            "lanes": [],
            "placements": [
                {"track": "kick", "at": 1, "clip": "k4", "bars": 2, "offset": 0},
                {"track": "kick", "at": 3, "clip": "k4", "bars": 2, "offset": 0},
            ],
            "duck_source": null,
        }})]
    );
    let text = s.text();
    assert!(!text.contains("[clips.k4]"), "{text}");
    assert!(
        !track_lines(&text, "kick")
            .iter()
            .any(|l| l.starts_with("play") || l.contains("k4")),
        "{text}"
    );
    comments_kept(FIXTURE, &text, &[]);
    assert_eq!(
        s.refused(Action::RemoveClip { clip: "k9".into() }),
        "there is no clip `k9`"
    );
}

#[test]
fn rename_clip_rewrites_placements() {
    let s = fixture();
    s.ok(Action::RenameClip {
        clip: "a1".into(),
        to: "bass1".into(),
    });
    let text = s.text();
    assert!(
        text.contains(
            "steps = \"x...x...x...X...\"\n\n[clips.bass1]\nkind = \"acid\"\n\
             steps = \"C2 C2! C3~ C2 . D#2 C2~ G2!\"\n\n[clips.h]"
        ),
        "{text}"
    );
    assert!(
        text.contains("play = [{ at = 3, clip = \"bass1\", bars = 2 }]"),
        "{text}"
    );
    assert!(!text.contains("a1"), "{text}");
    comments_kept(FIXTURE, &text, &[]);
    assert_eq!(
        s.refused(Action::RenameClip {
            clip: "bass1".into(),
            to: "h".into()
        }),
        "clip `h` already exists"
    );
}

#[test]
fn duplicate_clip_copies_after_the_original() {
    let s = fixture();
    s.ok(Action::DuplicateClip {
        clip: "h".into(),
        to: "h2".into(),
    });
    let text = s.text();
    assert!(
        text.contains(
            "[clips.h]\nkind = \"drum\"\nsteps = \"..x...x...x...x.\"\n\n\
             [clips.h2]\nkind = \"drum\"\nsteps = \"..x...x...x...x.\"\n\n[[sections]]"
        ),
        "{text}"
    );
    // The copy does not take the original's comments.
    let commented = FIXTURE.replacen(
        "[clips.h]\nkind = \"drum\"",
        "[clips.h] # the hats\nkind = \"drum\" # four on the floor?",
        1,
    );
    let s = song_with(&commented);
    s.ok(Action::DuplicateClip {
        clip: "h".into(),
        to: "h2".into(),
    });
    let text = s.text();
    assert_eq!(text.matches("# the hats").count(), 1, "{text}");
    assert_eq!(text.matches("# four on the floor?").count(), 1, "{text}");
    assert_eq!(
        s.refused(Action::DuplicateClip {
            clip: "h".into(),
            to: "k4".into()
        }),
        "clip `k4` already exists"
    );
}

#[test]
fn clip_actions_name_their_targets_and_read_back() {
    assert_eq!(add_clip("k8", "drum").targets(), ["clips.k8"]);
    assert_eq!(
        Action::RemoveClip { clip: "k4".into() }.targets(),
        ["clips.k4"]
    );
    let rename = Action::RenameClip {
        clip: "a1".into(),
        to: "bass1".into(),
    };
    assert_eq!(rename.targets(), ["clips.a1", "clips.bass1"]);
    let dup = Action::DuplicateClip {
        clip: "h".into(),
        to: "h2".into(),
    };
    assert_eq!(dup.targets(), ["clips.h", "clips.h2"]);
    for name in ["add_clip", "remove_clip", "rename_clip", "duplicate_clip"] {
        let spec = tune_studio::find_action(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(spec.kind, tune_studio::ActionKind::Edit);
    }
    let full = Action::AddClip {
        clip: "lead".into(),
        kind: "notes".into(),
        steps: None,
        bars: Some(2),
        notes: Some(json!([{"start": 0, "pitch": "C3", "len": 2}])),
    };
    for action in [full, add_clip("k8", "drum"), rename, dup] {
        let got = action.to_cli_args();
        assert_eq!(action_from_cli(action.name(), &got), Ok(action));
    }
}

#[test]
fn clip_edits_work_on_inline_files_and_are_one_undo_step() {
    let text = "[song]\nversion = 2\nname = \"t\"\nbpm = 120\n\n[tracks]\nkick = { instrument = \"kick\", play = [{ at = 1, clip = \"k4\", bars = 1 }] }\n\n[clips]\nk4 = { kind = \"drum\", steps = \"x...x...x...X...\" }\n\n[[sections]]\nname = \"a\"\nbars = 1\n";
    let s = song_with(text);
    s.ok(add_clip("k8", "drum"));
    s.ok(Action::DuplicateClip {
        clip: "k4".into(),
        to: "k5".into(),
    });
    s.ok(Action::RenameClip {
        clip: "k4".into(),
        to: "main".into(),
    });
    s.ok(Action::RemoveClip { clip: "k8".into() });
    let song = s.song();
    let ids: Vec<&str> = song.clips.keys().map(String::as_str).collect();
    assert_eq!(ids, ["main", "k5"]);
    assert_eq!(song.tracks["kick"].play[0].clip, "main");

    let s = fixture();
    let mut core = tune_studio::StudioCore::open(&s.path).expect("opens");
    core.apply_now(
        &[Action::RemoveClip { clip: "k4".into() }],
        &Origin::claude(None),
    )
    .expect("removes");
    assert_ne!(s.text(), FIXTURE);
    core.undo(tune_studio::Source::Human).expect("undoes");
    assert_eq!(s.text(), FIXTURE);
}

// ----- placements and sections -----

fn add_placement(track: &str, clip: &str, at: u32, bars: u32) -> Action {
    Action::AddPlacement {
        track: track.into(),
        clip: clip.into(),
        at,
        bars,
        offset: None,
    }
}

fn move_placement(track: &str, at: u32, to: u32) -> Action {
    Action::MovePlacement {
        track: track.into(),
        at,
        to,
    }
}

fn add_section(name: &str, bars: u32, index: Option<usize>) -> Action {
    Action::AddSection {
        name: name.into(),
        bars,
        index,
    }
}

#[test]
fn placements_by_start_bar() {
    let s = fixture();
    s.ok(add_placement("acid", "a1", 1, 2));
    let text = s.text();
    assert!(
        text.contains(
            "play = [\n  { at = 1, clip = \"a1\", bars = 2 },\n  { at = 3, clip = \"a1\", bars = 2 },\n]"
        ),
        "{text}"
    );
    comments_kept(FIXTURE, &text, &[]);

    // Offset is written when it is not 0.
    let s = fixture();
    s.ok(Action::AddPlacement {
        track: "acid".into(),
        clip: "a1".into(),
        at: 1,
        bars: 1,
        offset: Some(1),
    });
    assert!(
        s.text()
            .contains("{ at = 1, clip = \"a1\", bars = 1, offset = 1 }"),
        "{}",
        s.text()
    );

    let s = fixture();
    s.ok(move_placement("acid", 3, 1));
    assert!(
        s.text()
            .contains("play = [{ at = 1, clip = \"a1\", bars = 2 }]"),
        "{}",
        s.text()
    );

    let s = fixture();
    s.ok(Action::ResizePlacement {
        track: "acid".into(),
        at: 3,
        bars: 1,
    });
    assert!(
        s.text()
            .contains("play = [{ at = 3, clip = \"a1\", bars = 1 }]"),
        "{}",
        s.text()
    );

    let s = fixture();
    let out = s.ok(Action::RemovePlacement {
        track: "kick".into(),
        at: 3,
    });
    assert!(
        s.text()
            .contains("play = [{ at = 1, clip = \"k4\", bars = 2 }]"),
        "{}",
        s.text()
    );
    assert_eq!(
        out.results,
        [json!({"removed": {
            "lanes": [],
            "placements": [
                {"track": "kick", "at": 3, "clip": "k4", "bars": 2, "offset": 0},
            ],
            "duck_source": null,
        }})]
    );
    comments_kept(FIXTURE, &s.text(), &[]);
}

#[test]
fn placement_rules() {
    let s = fixture();
    let err = s.refused(add_placement("acid", "a1", 4, 1));
    assert!(
        err.contains("placement at bar 4 on track `acid` overlaps the one at bar 3"),
        "{err}"
    );
    let err = s.refused(add_placement("acid", "k4", 1, 1));
    assert!(
        err.contains("clip `k4` is a drum clip; track `acid` (acid303) plays acid clips"),
        "{err}"
    );
    let err = s.refused(add_placement("acid", "a1", 5, 2));
    assert!(err.contains("past the song's end"), "{err}");
    let err = s.refused(add_placement("acid", "zz", 1, 1));
    assert!(err.contains("unknown clip `zz`"), "{err}");
    assert_eq!(
        s.refused(add_placement("bass", "a1", 1, 1)),
        "there is no track `bass`"
    );
    assert_eq!(
        s.refused(add_placement("acid", "a1", 0, 1)),
        "bars are 1-based: `at` must be at least 1"
    );
    assert_eq!(
        s.refused(move_placement("acid", 3, 0)),
        "bars are 1-based: `to` must be at least 1"
    );
    for action in [
        move_placement("acid", 2, 1),
        Action::ResizePlacement {
            track: "acid".into(),
            at: 2,
            bars: 1,
        },
        Action::RemovePlacement {
            track: "acid".into(),
            at: 2,
        },
    ] {
        assert_eq!(s.refused(action), "track `acid` has no placement at bar 2");
    }
    assert_eq!(
        s.refused(Action::RemovePlacement {
            track: "acid".into(),
            at: 0
        }),
        "bars are 1-based: `at` must be at least 1"
    );
    // A move onto another placement is the validator's overlap.
    let err = s.refused(move_placement("kick", 3, 2));
    assert!(err.contains("overlaps the one at bar 1"), "{err}");
}

#[test]
fn add_section_shifts_what_follows() {
    let s = fixture();
    s.ok(add_section("break", 2, Some(1)));
    let text = s.text();
    assert!(
        text.contains(
            "[[sections]]\nname = \"a\"\nbars = 2\n\n[[sections]]\nname = \"break\"\nbars = 2\n\n\
             # acid enters\n[[sections]]\nname = \"b\"\nbars = 2\n"
        ),
        "{text}"
    );
    let song = s.song();
    let sections: Vec<_> = song
        .sections
        .iter()
        .map(|s| (s.name.as_str(), s.bars))
        .collect();
    assert_eq!(sections, [("a", 2), ("break", 2), ("b", 2)]);
    assert!(
        text.contains("play = [{ at = 5, clip = \"a1\", bars = 2 }]"),
        "{text}"
    );
    assert!(text.contains("at = 5\nbars = 2\npoints"), "{text}");
    assert_eq!(
        text.matches("{ at = 1, clip = \"k4\", bars = 2 }").count(),
        1
    );
    assert!(
        text.contains("{ at = 5, clip = \"k4\", bars = 2 }"),
        "{text}"
    );
    assert!(
        text.contains("{ at = 5, clip = \"h\", bars = 2 }"),
        "{text}"
    );
    comments_kept(FIXTURE, &text, &[]);

    // No index appends and moves nothing.
    let s = fixture();
    s.ok(add_section("outro", 4, None));
    let (before, after) = (parse_song(FIXTURE).expect("parses"), s.song());
    assert_eq!(after.tracks, before.tracks);
    assert_eq!(after.automation, before.automation);
    let names: Vec<_> = after.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["a", "b", "outro"]);
    assert_eq!(after.sections[2].bars, 4);
    comments_kept(FIXTURE, &s.text(), &[]);
}

#[test]
fn add_section_splits_a_crossing_placement_in_the_file() {
    let text = FIXTURE.replace(
        "play = [{ at = 3, clip = \"a1\", bars = 2 }]",
        "play = [{ at = 1, clip = \"a1\", bars = 4 }]",
    );
    let s = song_with(&text);
    s.ok(add_section("break", 1, Some(1)));
    assert!(
        s.text().contains(
            "play = [\n  { at = 1, clip = \"a1\", bars = 2 },\n  { at = 4, clip = \"a1\", bars = 2, offset = 2 },\n]"
        ),
        "{}",
        s.text()
    );
}

#[test]
fn add_section_rules() {
    let s = fixture();
    assert_eq!(
        s.refused(add_section("x", 1, Some(3))),
        "index 3 is out of bounds (there are 2)"
    );
    let err = s.refused(add_section("x", 0, None));
    assert!(err.contains("bars must be 1 to 1024, got 0"), "{err}");
    let err = s.refused(add_section("x", 1025, Some(0)));
    assert!(err.contains("bars must be 1 to 1024, got 1025"), "{err}");
    // A name is any string; quotes are escaped.
    s.ok(add_section("the \"drop\"", 1, Some(0)));
    assert_eq!(s.song().sections[0].name, "the \"drop\"");
}

#[test]
fn rename_section() {
    let s = fixture();
    s.ok(Action::RenameSection {
        index: 1,
        name: "drop".into(),
    });
    assert_eq!(
        s.text(),
        FIXTURE.replacen("name = \"b\"", "name = \"drop\"", 1)
    );
    assert_eq!(
        s.refused(Action::RenameSection {
            index: 5,
            name: "x".into()
        }),
        "`sections[5].name` does not exist in this song"
    );
}

#[test]
fn placement_and_section_actions_name_their_targets_and_read_back() {
    let remove = Action::RemovePlacement {
        track: "kick".into(),
        at: 3,
    };
    let resize = Action::ResizePlacement {
        track: "acid".into(),
        at: 3,
        bars: 1,
    };
    let with_offset = Action::AddPlacement {
        track: "acid".into(),
        clip: "a1".into(),
        at: 1,
        bars: 1,
        offset: Some(1),
    };
    for action in [
        add_placement("acid", "a1", 1, 2),
        move_placement("acid", 3, 1),
        resize.clone(),
        remove.clone(),
    ] {
        let track = if matches!(action, Action::RemovePlacement { .. }) {
            "kick"
        } else {
            "acid"
        };
        assert_eq!(action.targets(), [format!("tracks.{track}.play")]);
    }
    assert_eq!(add_section("break", 2, Some(1)).targets(), ["sections"]);
    assert_eq!(add_section("outro", 4, None).targets(), ["sections"]);
    let rename = Action::RenameSection {
        index: 1,
        name: "drop".into(),
    };
    assert_eq!(rename.targets(), ["sections[1].name"]);
    for name in [
        "add_placement",
        "move_placement",
        "resize_placement",
        "remove_placement",
        "add_section",
        "rename_section",
    ] {
        let spec = tune_studio::find_action(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(spec.kind, tune_studio::ActionKind::Edit);
    }
    for action in [
        add_placement("acid", "a1", 1, 2),
        with_offset,
        move_placement("acid", 3, 1),
        resize,
        remove,
        add_section("break", 2, Some(1)),
        add_section("outro", 4, None),
        rename,
    ] {
        let got = action.to_cli_args();
        assert_eq!(action_from_cli(action.name(), &got), Ok(action));
    }
    assert_eq!(
        cli("add_section", &["outro", "bars=4"]),
        Ok(add_section("outro", 4, None))
    );
}

#[test]
fn placement_and_section_edits_work_on_inline_files_and_are_one_undo_step() {
    let text = "sections = [{ name = \"a\", bars = 2 }, { name = \"b\", bars = 2 }]\n\n\
        [song]\nversion = 2\nname = \"t\"\nbpm = 120\n\n\
        [tracks]\nkick = { instrument = \"kick\", play = [{ at = 1, clip = \"k4\", bars = 2 }] }\n\n\
        [clips]\nk4 = { kind = \"drum\", steps = \"x...x...x...X...\" }\n";
    let s = song_with(text);
    s.ok(add_section("break", 1, Some(1)));
    s.ok(add_placement("kick", "k4", 3, 2));
    s.ok(move_placement("kick", 3, 4));
    s.ok(Action::ResizePlacement {
        track: "kick".into(),
        at: 4,
        bars: 1,
    });
    s.ok(Action::RenameSection {
        index: 1,
        name: "gap".into(),
    });
    let song = s.song();
    let names: Vec<_> = song.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["a", "gap", "b"]);
    let play: Vec<_> = song.tracks["kick"]
        .play
        .iter()
        .map(|p| (p.start_bar, p.bars))
        .collect();
    assert_eq!(play, [(0, 2), (3, 1)]);

    let s = fixture();
    let mut core = tune_studio::StudioCore::open(&s.path).expect("opens");
    core.apply_now(&[add_section("break", 2, Some(1))], &Origin::claude(None))
        .expect("adds");
    assert_ne!(s.text(), FIXTURE);
    core.undo(tune_studio::Source::Human).expect("undoes");
    assert_eq!(s.text(), FIXTURE);
}

// ----- lanes -----

const MINI: &str = include_str!("../../tune-song/tests/fixtures/v2/mini.toml");

/// The commented fixture's lane table, comment and all.
const LANE_TABLE: &str = "# the filter opens over the section\n[[automation]]\n\
    param = \"acid.acid303.cutoff\"\nat = 3\nbars = 2\npoints = [[0, 0.1], [2, 0.8]]\n";

fn add_lane(param: &str, at: u32, bars: u32, points: &[[f64; 2]]) -> Action {
    Action::AddLane {
        param: param.into(),
        at,
        bars,
        points: points.to_vec(),
    }
}

fn remove_lane(param: &str, at: u32) -> Action {
    Action::RemoveLane {
        param: param.into(),
        at,
    }
}

fn set_lane_points(param: &str, at: u32, points: &[[f64; 2]]) -> Action {
    Action::SetLanePoints {
        param: param.into(),
        at,
        points: points.to_vec(),
    }
}

/// The stretches of a song as `(param, 1-based at, bars)`.
fn lanes_of(song: &tune_song::Song) -> Vec<(String, u32, u32)> {
    song.automation
        .iter()
        .map(|l| (format!("{}.{}", l.track, l.param), l.start_bar + 1, l.bars))
        .collect()
}

#[test]
fn add_lane_writes_a_table() {
    let s = fixture();
    let out = s.ok(add_lane("mix.reverb.size", 1, 4, &[[0.0, 0.2], [4.0, 0.6]]));
    assert_eq!(out.results, [Value::Null]);
    let text = s.text();
    // The new table follows the section it starts in; the old lane's table
    // and its comment are untouched.
    assert!(
        text.contains(
            "[[sections]]\nname = \"a\"\nbars = 2\n\n[[automation]]\nparam = \"mix.reverb.size\"\n\
             at = 1\nbars = 4\npoints = [[0, 0.2], [4, 0.6]]\n\n# acid enters\n[[sections]]"
        ),
        "{text}"
    );
    assert!(text.ends_with(LANE_TABLE), "{text}");
    comments_kept(FIXTURE, &text, &[]);
    assert_eq!(
        lanes_of(&s.song()),
        [
            ("mix.reverb.size".to_owned(), 1, 4),
            ("acid.acid303.cutoff".to_owned(), 3, 2)
        ]
    );

    // A stretch of another param in the old lane's section goes after it.
    let s = fixture();
    s.ok(add_lane(
        "acid.acid303.resonance",
        3,
        2,
        &[[0.0, 0.5], [2.0, 0.5]],
    ));
    let text = s.text();
    assert!(text.starts_with(&FIXTURE[..FIXTURE.len() - 1]), "{text}");
    assert!(
        text.ends_with(
            "\n[[automation]]\nparam = \"acid.acid303.resonance\"\nat = 3\nbars = 2\n\
             points = [[0, 0.5], [2, 0.5]]\n"
        ),
        "{text}"
    );
}

#[test]
fn the_first_stretch_creates_the_automation_list() {
    let lanes_at = MINI
        .find("\n[[automation]]")
        .expect("the fixture has a lane");
    let s = song_with(&MINI[..lanes_at]);
    s.ok(add_lane(
        "acid.acid303.cutoff",
        3,
        2,
        &[[0.0, 0.1], [2.0, 0.8]],
    ));
    assert_eq!(s.text(), MINI);
    assert_eq!(
        lanes_of(&s.song()),
        [("acid.acid303.cutoff".to_owned(), 3, 2)]
    );

    // Tracks and clips written inline: the list still follows the sections.
    let text = "[song]\nversion = 2\nname = \"t\"\nbpm = 120\n\n[tracks]\nkick = { instrument = \"kick\" }\n\n[clips]\nk4 = { kind = \"drum\", steps = \"x...x...x...X...\" }\n\n[[sections]]\nname = \"a\"\nbars = 2\n";
    let s = song_with(text);
    s.ok(add_lane("kick.kick.decay", 1, 1, &[[0.0, 0.0], [1.0, 1.0]]));
    assert_eq!(
        s.text(),
        format!(
            "{text}\n[[automation]]\nparam = \"kick.kick.decay\"\nat = 1\nbars = 1\n\
             points = [[0, 0], [1, 1]]\n"
        )
    );
}

#[test]
fn add_lane_into_an_inline_automation_array() {
    let head = "automation = [{ param = \"kick.kick.decay\", at = 5, bars = 2, points = [[0, 0], [2, 1]] }]\n\n";
    let rest = "[song]\nversion = 2\nname = \"t\"\nbpm = 120\n\n[tracks]\nkick = { instrument = \"kick\" }\n\n[[sections]]\nname = \"a\"\nbars = 4\n\n[[sections]]\nname = \"b\"\nbars = 4\n";
    let s = song_with(&format!("{head}{rest}"));
    s.ok(add_lane("mix.reverb.size", 1, 2, &[[0.0, 0.2], [2.0, 0.6]]));
    let text = s.text();
    // The new entry comes first (section a), the old one is untouched.
    assert!(
        text.starts_with(
            "automation = [{ param = \"mix.reverb.size\", at = 1, bars = 2, points = [[0, 0.2], [2, 0.6]] }, \
             { param = \"kick.kick.decay\", at = 5, bars = 2, points = [[0, 0], [2, 1]] }]"
        ),
        "{text}"
    );
    assert!(text.ends_with(rest), "{text}");
    assert_eq!(
        lanes_of(&s.song()),
        [
            ("mix.reverb.size".to_owned(), 1, 2),
            ("kick.kick.decay".to_owned(), 5, 2)
        ]
    );
    let mut want = parse_song(&format!("{head}{rest}")).expect("parses");
    want.automation.insert(
        0,
        tune_song::Lane {
            track: "mix".into(),
            param: "reverb.size".into(),
            start_bar: 0,
            bars: 2,
            points: vec![(0.0, 0.2), (2.0, 0.6)],
        },
    );
    assert_eq!(s.song(), want);
}

#[test]
fn lane_rules() {
    let s = fixture();
    let points = [[0.0, 0.2], [2.0, 0.6]];
    // Exactly on top of the old stretch (bars 3-4), one bar before it and one
    // bar into it. The validator names the later table of the two.
    for (at, bars, message) in [
        (3, 2, "at bars 3-4 overlaps the stretch at bars 3-4"),
        (2, 2, "at bars 3-4 overlaps the stretch at bars 2-3"),
        (4, 1, "at bars 4-4 overlaps the stretch at bars 3-4"),
    ] {
        let points = [[0.0, 0.2], [f64::from(bars), 0.6]];
        let err = s.refused(add_lane("acid.acid303.cutoff", at, bars, &points));
        assert!(
            err.contains(&format!("automation for `acid.acid303.cutoff` {message}")),
            "{err}"
        );
    }
    // A stretch that ends where the old one starts (bars 1-2, the old one
    // starts at bar 3) does not overlap.
    s.ok(add_lane("acid.acid303.cutoff", 1, 2, &points));
    let err = s.refused(add_lane("acid.acid303.nope", 1, 2, &points));
    assert!(
        err.contains("unknown parameter `acid303.nope` for acid303"),
        "{err}"
    );
    let err = s.refused(add_lane("acid.acid303.resonance", 1, 2, &[[0.0, 1.5]]));
    assert!(err.contains("value 1.5 is outside 0..=1"), "{err}");
    let err = s.refused(add_lane("acid.acid303.resonance", 1, 2, &[[3.0, 0.5]]));
    assert!(
        err.contains("bar 3 is outside the stretch (0..=2)"),
        "{err}"
    );
    let err = s.refused(add_lane("acid.acid303.resonance", 4, 2, &points));
    assert!(err.contains("runs past the song's end"), "{err}");
    let err = s.refused(add_lane("acid.acid303.resonance", 1, 2, &[]));
    assert!(err.contains("a lane needs at least one point"), "{err}");
    assert_eq!(
        s.refused(add_lane("acid.acid303.resonance", 0, 2, &points)),
        "bars are 1-based: `at` must be at least 1"
    );
    assert_eq!(
        s.refused(remove_lane("acid.acid303.cutoff", 0)),
        "bars are 1-based: `at` must be at least 1"
    );
    assert_eq!(
        s.refused(set_lane_points("acid.acid303.cutoff", 0, &points)),
        "bars are 1-based: `at` must be at least 1"
    );
    let err = s.refused(add_lane("acid.acid303.resonance", 1, 0, &points));
    assert!(err.contains("bars must be at least 1, got 0"), "{err}");

    // A stretch is addressed by the bar it starts at.
    let s = fixture();
    for at in [1, 2, 4, 5] {
        let message = format!("there is no lane `acid.acid303.cutoff` at bar {at}");
        assert_eq!(s.refused(remove_lane("acid.acid303.cutoff", at)), message);
        assert_eq!(
            s.refused(set_lane_points("acid.acid303.cutoff", at, &points)),
            message
        );
    }
    assert_eq!(
        s.refused(remove_lane("acid.acid303.resonance", 3)),
        "there is no lane `acid.acid303.resonance` at bar 3"
    );
    // A param needs its track.
    let err = s.refused(add_lane("cutoff", 1, 1, &points));
    assert!(err.contains("`<track>.<param id>`"), "{err}");
    let err = s.refused(remove_lane("cutoff", 3));
    assert!(err.contains("`<track>.<param id>`"), "{err}");
}

#[test]
fn remove_and_set_points() {
    let s = fixture();
    let out = s.ok(remove_lane("acid.acid303.cutoff", 3));
    let text = s.text();
    assert!(!text.contains("[[automation]]"), "{text}");
    assert!(!text.contains("the filter opens"), "{text}");
    assert_eq!(text, FIXTURE.replace(&format!("\n{LANE_TABLE}"), ""));
    comments_kept(FIXTURE, &text, &["# the filter opens over the section"]);
    assert_eq!(
        out.results,
        [json!({"removed": {
            "lanes": [{
                "param": "acid.acid303.cutoff",
                "at": 3,
                "bars": 2,
                "points": [[0, 0.1], [2, 0.8]],
            }],
            "placements": [],
            "duck_source": null,
        }})]
    );
    assert!(s.song().automation.is_empty());

    let s = fixture();
    let out = s.ok(set_lane_points(
        "acid.acid303.cutoff",
        3,
        &[[0.0, 0.3], [2.0, 0.9]],
    ));
    assert_eq!(out.results, [Value::Null]);
    assert_eq!(
        s.text(),
        FIXTURE.replace(
            "points = [[0, 0.1], [2, 0.8]]",
            "points = [[0, 0.3], [2, 0.9]]"
        )
    );
    // Points out of range are the validator's.
    let err = s.refused(set_lane_points("acid.acid303.cutoff", 3, &[[0.0, 2.0]]));
    assert!(err.contains("value 2 is outside 0..=1"), "{err}");
}

#[test]
fn lane_actions_name_their_targets_and_read_back() {
    let add = add_lane("mix.reverb.size", 1, 4, &[[0.0, 0.2], [4.0, 0.6]]);
    let remove = remove_lane("acid.acid303.cutoff", 3);
    let set = set_lane_points("acid.acid303.cutoff", 3, &[[0.0, 0.3], [2.0, 0.9]]);
    assert_eq!(add.targets(), ["automation.mix.reverb.size"]);
    assert_eq!(remove.targets(), ["automation.acid.acid303.cutoff"]);
    assert_eq!(set.targets(), ["automation.acid.acid303.cutoff"]);
    for name in ["add_lane", "remove_lane", "set_lane_points"] {
        let spec = tune_studio::find_action(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(spec.kind, tune_studio::ActionKind::Edit);
    }
    assert_eq!(
        set.to_cli_args(),
        [
            "param=acid.acid303.cutoff",
            "at=3",
            "points=[[0.0, 0.3], [2.0, 0.9]]"
        ]
    );
    for action in [add, remove, set] {
        let got = action.to_cli_args();
        assert_eq!(action_from_cli(action.name(), &got), Ok(action));
    }
    // The CLI reads whole numbers and decimals alike.
    assert_eq!(
        cli(
            "set_lane_points",
            &[
                "param=acid.acid303.cutoff",
                "at=3",
                "points=[[0, 0.3], [2, 0.9]]"
            ]
        ),
        Ok(set_lane_points(
            "acid.acid303.cutoff",
            3,
            &[[0.0, 0.3], [2.0, 0.9]]
        ))
    );
}

#[test]
fn lane_edits_are_one_undo_step() {
    let s = fixture();
    let mut core = tune_studio::StudioCore::open(&s.path).expect("opens");
    core.apply_now(
        &[add_lane("mix.reverb.size", 1, 4, &[[0.0, 0.2], [4.0, 0.6]])],
        &Origin::claude(None),
    )
    .expect("adds");
    assert_ne!(s.text(), FIXTURE);
    core.undo(tune_studio::Source::Human).expect("undoes");
    assert_eq!(s.text(), FIXTURE);
}
