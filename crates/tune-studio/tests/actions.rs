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
