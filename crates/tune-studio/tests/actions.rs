//! The action layer: CLI arguments into actions, and what each action
//! names as its targets.

use serde_json::json;
use tune_studio::{Action, DrumHitDto, action_from_cli};

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
