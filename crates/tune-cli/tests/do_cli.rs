//! `tune do`, the run flags of `tune set` and the run suffix of `tune changes`,
//! against a copy of the commented fixture, with no studio open and through
//! one running in the test.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::TempDir;
use tune_studio::{
    Command as StudioCall, ControlServer, Event, Source, Studio, StudioCommand, StudioConfig,
};

struct Env {
    dir: TempDir,
    studio: TempDir,
    song: PathBuf,
}

fn fixture() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tune-studio/tests/fixtures/commented.toml");
    let song = dir.path().join("song.toml");
    fs::copy(src, &song).unwrap();
    Env {
        dir,
        studio: tempfile::tempdir().unwrap(),
        song,
    }
}

impl Env {
    fn tune(&self) -> Command {
        let mut c = Command::cargo_bin("tune").unwrap();
        c.env("TUNE_STUDIO_DIR", self.studio.path())
            .env_remove("TUNE_RUN")
            .env_remove("TUNE_RUN_TITLE");
        c
    }

    fn log(&self) -> String {
        fs::read_to_string(self.dir.path().join("edits.jsonl")).unwrap_or_default()
    }

    /// The log's entries: its lines without the action records.
    fn entries(&self) -> Vec<Value> {
        let log = self.log();
        let lines = log.lines().map(|l| serde_json::from_str(l).unwrap());
        lines
            .filter(|v: &Value| v.get("record").is_none())
            .collect()
    }

    fn last_entry(&self) -> Value {
        self.entries().pop().unwrap()
    }
}

fn json_out(cmd: &mut Command) -> Value {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// Runs a failing command and returns stdout and stderr together.
fn fails(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn do_sets_a_value_and_logs_the_run() {
    let e = fixture();
    let v = json_out(e.tune().args([
        "do",
        e.song.to_str().unwrap(),
        "set_value",
        "path=tracks.acid.acid303.cutoff",
        "value=820",
        "--run",
        "r1",
        "--run-title",
        "open the filter",
        "--json",
    ]));
    assert_eq!(v["ok"], true);
    assert_eq!(v["via"], "file");
    assert_eq!(
        v["changes"][0],
        json!({"path": "tracks.acid.acid303.cutoff", "old": 500, "new": 820})
    );
    let last = e.last_entry();
    assert_eq!(last["source"], "claude");
    assert_eq!(last["action"], "set_value");
    assert_eq!(last["run"], "r1");
    assert_eq!(last["run_title"], "open the filter");
    let text = fs::read_to_string(&e.song).unwrap();
    for c in ["# the body", "# the squelch", "# acid enters"] {
        assert!(text.contains(c), "lost comment {c}");
    }
}

#[test]
fn do_reads_the_run_from_the_environment() {
    let e = fixture();
    json_out(e.tune().env("TUNE_RUN", "r2").args([
        "do",
        e.song.to_str().unwrap(),
        "set_value",
        "path=song.bpm",
        "value=130",
        "--json",
    ]));
    assert_eq!(e.last_entry()["run"], "r2");
    json_out(e.tune().env("TUNE_RUN", "r2").args([
        "do",
        e.song.to_str().unwrap(),
        "set_value",
        "path=song.bpm",
        "value=131",
        "--run",
        "r3",
        "--json",
    ]));
    assert_eq!(e.last_entry()["run"], "r3");
}

/// `--run-title` names the run `TUNE_RUN` gives, for `set` and `do`.
#[test]
fn a_run_title_names_the_run_from_the_environment() {
    let e = fixture();
    let song = e.song.to_str().unwrap();
    let title = ["--run-title", "open the filter", "--json"];
    json_out(
        e.tune()
            .env("TUNE_RUN", "r9")
            .args(["set", song, "song.bpm=130"])
            .args(title),
    );
    let last = e.last_entry();
    assert_eq!(
        (&last["run"], &last["run_title"]),
        (&json!("r9"), &json!("open the filter"))
    );
    json_out(
        e.tune()
            .env("TUNE_RUN", "r9")
            .args(["do", song, "set_value", "path=song.bpm", "value=131"])
            .args(title),
    );
    let last = e.last_entry();
    assert_eq!(
        (&last["run"], &last["run_title"]),
        (&json!("r9"), &json!("open the filter"))
    );
}

const BAD_BATCH: &str = r#"[{"cmd":"set_value","args":{"path":"song.bpm","value":130}},{"cmd":"set_value","args":{"path":"tracks.nope.gain_db","value":1}}]"#;
const GOOD_BATCH: &str = r#"[{"cmd":"set_value","args":{"path":"song.bpm","value":130}},{"cmd":"set_value","args":{"path":"tracks.acid.gain_db","value":-2}}]"#;

#[test]
fn do_batch_is_all_or_nothing() {
    let e = fixture();
    let before = fs::read(&e.song).unwrap();
    let msg = fails(
        e.tune()
            .args(["do", e.song.to_str().unwrap(), "--batch", "--json"])
            .write_stdin(BAD_BATCH),
    );
    assert!(msg.contains("tracks.nope.gain_db"), "{msg}");
    assert_eq!(fs::read(&e.song).unwrap(), before);
    assert_eq!(e.log(), "");

    let v = json_out(
        e.tune()
            .args(["do", e.song.to_str().unwrap(), "--batch", "--json"])
            .write_stdin(GOOD_BATCH),
    );
    assert_eq!(v["changes"].as_array().unwrap().len(), 2);
    let entries = e.entries();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|x| x["action"] == "set_value"));
    assert_eq!(entries[0]["file_hash"], entries[1]["file_hash"]);
    assert_eq!(v["file_hash"], entries[0]["file_hash"]);
}

#[test]
fn a_batch_with_a_live_action_is_refused() {
    let e = fixture();
    let before = fs::read(&e.song).unwrap();
    let batch = r#"[{"cmd":"stop"},{"cmd":"set_value","args":{"path":"song.bpm","value":130}}]"#;
    let msg = fails(
        e.tune()
            .args(["do", e.song.to_str().unwrap(), "--batch"])
            .write_stdin(batch),
    );
    assert!(
        msg.contains("a batch holds song edits only; run `stop` on its own"),
        "{msg}"
    );
    assert_eq!(fs::read(&e.song).unwrap(), before);
}

#[test]
fn live_action_without_a_studio_fails() {
    let e = fixture();
    let song = e.song.to_str().unwrap();
    for (args, name) in [
        (vec!["play"], "play"),
        (vec!["undo"], "undo"),
        (vec!["seek_bar", "3"], "seek_bar"),
    ] {
        let mut cmd = e.tune();
        cmd.arg("do").arg(song).args(&args).arg("--json");
        let msg = fails(&mut cmd);
        let want = format!(
            "`{name}` needs the studio: open the song with `tune serve <song>` or the app first"
        );
        assert!(msg.contains(&want), "{msg}");
    }
}

#[test]
fn bad_runs_are_refused() {
    let e = fixture();
    let song = e.song.to_str().unwrap();
    let before = fs::read(&e.song).unwrap();
    let msg = fails(e.tune().args([
        "do",
        song,
        "set_value",
        "path=song.bpm",
        "value=130",
        "--run",
        "has space",
    ]));
    assert!(
        msg.contains("run id must be 1 to 64 letters, digits or `_.:-`, got `has space`"),
        "{msg}"
    );
    let msg = fails(e.tune().args([
        "do",
        song,
        "set_value",
        "path=song.bpm",
        "value=130",
        "--run-title",
        "x",
    ]));
    assert!(msg.contains("--run-title needs a run"), "{msg}");
    assert_eq!(fs::read(&e.song).unwrap(), before);
    assert_eq!(e.log(), "");
}

/// A `TUNE_STUDIO_DIR` the command cannot use is refused, not taken as
/// "no studio": a studio started with a good one may have the song open.
#[test]
fn an_unusable_studio_dir_is_refused() {
    let e = fixture();
    let song = e.song.to_str().unwrap();
    let text = fs::read_to_string(&e.song).unwrap();
    let relative = |c: &mut Command| {
        c.env("TUNE_STUDIO_DIR", "studios");
    };
    let mut cmd = e.tune();
    relative(&mut cmd);
    let err = fails(cmd.args(["do", song, "set_value", "path=song.bpm", "value=130"]));
    assert!(
        err.contains("TUNE_STUDIO_DIR must be an absolute path"),
        "{err}"
    );
    let mut cmd = e.tune();
    relative(&mut cmd);
    let err = fails(cmd.args(["set", song, "song.bpm=130", "--json"]));
    assert!(
        err.contains("TUNE_STUDIO_DIR must be an absolute path"),
        "{err}"
    );
    assert_eq!(fs::read_to_string(&e.song).unwrap(), text);
    assert_eq!(e.log(), "");
}

/// Without `HOME` (and no `TUNE_STUDIO_DIR`) no studio can be found: the
/// edit goes into the file, with one warning.
#[test]
fn no_home_edits_the_file_with_a_warning() {
    let e = fixture();
    let song = e.song.to_str().unwrap();
    for args in [
        vec!["do", song, "set_value", "path=song.bpm", "value=130"],
        vec!["set", song, "song.bpm=131"],
    ] {
        let out = e
            .tune()
            .env_remove("TUNE_STUDIO_DIR")
            .env_remove("HOME")
            .args(&args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            err.matches("HOME is not set; a running studio cannot be found")
                .count(),
            1,
            "{err}"
        );
    }
    assert!(fs::read_to_string(&e.song).unwrap().contains("bpm = 131"));
}

#[test]
fn set_takes_a_run() {
    let e = fixture();
    json_out(e.tune().args([
        "set",
        e.song.to_str().unwrap(),
        "song.bpm=130",
        "--run",
        "r4",
        "--json",
    ]));
    let last = e.last_entry();
    assert_eq!(last["run"], "r4");
    assert_eq!(last["action"], "set_value");
}

#[test]
fn changes_show_the_run() {
    let e = fixture();
    json_out(e.tune().args([
        "do",
        e.song.to_str().unwrap(),
        "set_value",
        "path=tracks.acid.acid303.cutoff",
        "value=820",
        "--run",
        "r1",
        "--json",
    ]));
    let out = e
        .tune()
        .args(["changes", e.song.to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().last().unwrap();
    assert!(line.ends_with(" (run r1)"), "{line}");
    let v = json_out(
        e.tune()
            .args(["changes", e.song.to_str().unwrap(), "--json"]),
    );
    assert_eq!(v["changes"][0]["run"], "r1");
    assert_eq!(v["changes"][0]["action"], "set_value");
}

#[test]
fn do_prints_results_and_no_changes() {
    let e = fixture();
    let out = e
        .tune()
        .args([
            "do",
            e.song.to_str().unwrap(),
            "set_value",
            "path=song.bpm",
            "value=120",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "no changes");
    assert_eq!(e.log(), "");
}

#[test]
fn do_goes_through_a_running_studio() {
    let e = fixture();
    let studio = Studio::spawn(
        StudioConfig {
            audio: Box::new(tune_audio::NullOut::new()),
            songs_root: e.dir.path().to_owned(),
        },
        Some(e.song.clone()),
    )
    .unwrap();
    let events = studio.subscribe();
    let _control = ControlServer::start(studio.clone(), e.studio.path()).unwrap();
    let song = e.song.to_str().unwrap();

    let v = json_out(e.tune().args([
        "do",
        song,
        "set_value",
        "path=song.bpm",
        "value=130",
        "--run",
        "r5",
        "--json",
    ]));
    assert_eq!(v["via"], "studio");
    assert_eq!(
        v["changes"][0],
        json!({"path": "song.bpm", "old": 120, "new": 130})
    );
    let applied = loop {
        if let Event::Applied { source, run, .. } =
            events.recv_timeout(Duration::from_secs(10)).unwrap()
        {
            break (source, run);
        }
    };
    assert_eq!(applied.0, Source::Claude);
    assert_eq!(applied.1.unwrap().id, "r5");
    assert_eq!(e.last_entry()["run"], "r5");

    let v = json_out(e.tune().args(["do", song, "play", "--json"]));
    assert_eq!(v["via"], "studio");
    let state = studio
        .call(StudioCall::Studio(StudioCommand::GetState))
        .unwrap();
    assert_eq!(state["playing"], true);

    let v = json_out(e.tune().args(["set", song, "song.bpm=128", "--json"]));
    assert_eq!(v["via"], "studio");
    assert!(fs::read_to_string(&e.song).unwrap().contains("bpm = 128"));
    studio.shutdown();
}

#[test]
fn swap_from_the_cli() {
    let e = fixture();
    let v = json_out(e.tune().args([
        "do",
        e.song.to_str().unwrap(),
        "set_instrument",
        "track=acid",
        "module=kick",
        "--json",
    ]));
    assert_eq!(v["results"][0]["removed"]["placements"][0]["clip"], "a1");
    assert!(
        fs::read_to_string(&e.song)
            .unwrap()
            .contains("instrument = \"kick\"")
    );

    // Text mode says what went, on a fresh copy.
    let e = fixture();
    let out = e
        .tune()
        .args([
            "do",
            e.song.to_str().unwrap(),
            "set_instrument",
            "track=acid",
            "module=kick",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("removed placement a1 on acid at bar 3"),
        "{text}"
    );
    assert!(
        text.contains("removed lane acid.acid303.cutoff at bar 3"),
        "{text}"
    );
}

#[test]
fn clips_from_the_cli() {
    let e = fixture();
    let song = e.song.to_str().unwrap();
    let v = json_out(e.tune().args([
        "do",
        song,
        "add_clip",
        "clip=lead",
        "kind=notes",
        "bars=2",
        "notes=[{ start = 0, pitch = \"C3\", len = 2 }]",
        "--json",
    ]));
    assert_eq!(v["changes"][0]["path"], "clips.lead.bars");
    assert!(
        fs::read_to_string(&e.song)
            .unwrap()
            .contains("notes = [{ start = 0, pitch = \"C3\", len = 2 }]")
    );

    // Text mode says which placements went with the clip.
    let out = e
        .tune()
        .args(["do", song, "remove_clip", "clip=k4"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("removed placement k4 on kick at bar 1"),
        "{text}"
    );
    assert!(
        text.contains("removed placement k4 on kick at bar 3"),
        "{text}"
    );
}

#[test]
fn placements_and_sections_from_the_cli() {
    let e = fixture();
    let song = e.song.to_str().unwrap();
    let v = json_out(e.tune().args([
        "do",
        song,
        "add_section",
        "break",
        "bars=2",
        "index=1",
        "--json",
    ]));
    assert_eq!(v["changes"][0]["path"], "sections");
    let text = fs::read_to_string(&e.song).unwrap();
    assert!(text.contains("name = \"break\""), "{text}");
    assert!(
        text.contains("{ at = 5, clip = \"k4\", bars = 2 }"),
        "{text}"
    );

    // Text mode says which placement went.
    let out = e
        .tune()
        .args(["do", song, "remove_placement", "kick", "at=5"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("removed placement k4 on kick at bar 5"),
        "{text}"
    );

    // A refusal names the bar as the producer counts it, and exits 2.
    let err = fails(
        e.tune()
            .args(["do", song, "move_placement", "acid", "at=2", "to=1"]),
    );
    assert!(
        err.contains("track `acid` has no placement at bar 2"),
        "{err}"
    );
}

#[test]
fn lanes_from_the_cli() {
    let e = fixture();
    let song = e.song.to_str().unwrap();
    // The points go in as a TOML array, quoted for the shell.
    let v = json_out(e.tune().args([
        "do",
        song,
        "add_lane",
        "param=mix.reverb.size",
        "at=1",
        "bars=2",
        "points=[[0, 0.2], [2, 0.6]]",
        "--json",
    ]));
    assert_eq!(v["changes"][0]["path"], "sections[0].lanes");
    let text = fs::read_to_string(&e.song).unwrap();
    assert!(
        text.contains(
            "param = \"mix.reverb.size\"\nat = 1\nbars = 2\npoints = [[0, 0.2], [2, 0.6]]"
        ),
        "{text}"
    );

    // Text mode says which stretch went.
    let out = e
        .tune()
        .args(["do", song, "remove_lane", "acid.acid303.cutoff", "at=3"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("removed lane acid.acid303.cutoff at bar 3"),
        "{text}"
    );

    // A refusal exits 2 with the lane named as the producer counts bars.
    let err = fails(e.tune().args([
        "do",
        song,
        "set_lane_points",
        "mix.reverb.size",
        "at=2",
        "points=[[0, 1]]",
    ]));
    assert!(
        err.contains("there is no lane `mix.reverb.size` at bar 2"),
        "{err}"
    );
}

#[test]
fn undo_run_from_the_cli() {
    let e = fixture();
    let song = e.song.to_str().unwrap();
    let edits = [
        ["path=tracks.acid.acid303.cutoff", "value=820"],
        ["path=song.bpm", "value=130"],
    ];
    for edit in edits {
        json_out(
            e.tune()
                .args(["do", song, "set_value"])
                .args(edit)
                .args(["--run", "r1", "--json"]),
        );
    }
    let v = json_out(e.tune().args(["do", song, "undo_run", "r1", "--json"]));
    assert_eq!(v["via"], "file");
    let undo = &v["results"][0];
    let mut reverted: Vec<&str> = undo["reverted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["target"].as_str().unwrap())
        .collect();
    reverted.sort_unstable();
    assert_eq!(reverted, ["song.bpm", "tracks.acid.acid303.cutoff"]);
    assert_eq!(undo["kept"], json!([]));
    let fixture_text = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tune-studio/tests/fixtures/commented.toml"),
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&e.song).unwrap(), fixture_text);
}

/// With the studio closed, a placement added by hand between two of the
/// run's writes to the same list stays, and the undo lists it as kept.
#[test]
fn undo_run_keeps_a_placement_added_by_hand_between_its_writes() {
    let e = fixture();
    let song = e.song.to_str().unwrap();
    let run = ["--run", "r1", "--json"];
    json_out(
        e.tune()
            .args([
                "do",
                song,
                "add_placement",
                "track=acid",
                "clip=a1",
                "at=1",
                "bars=1",
            ])
            .args(run),
    );
    let text = fs::read_to_string(&e.song).unwrap();
    let mine = "  { at = 2, clip = \"a1\", bars = 1 }, # mine\n";
    let first = "  { at = 1, clip = \"a1\", bars = 1 },\n";
    assert!(text.contains(first), "{text}");
    fs::write(&e.song, text.replacen(first, &format!("{first}{mine}"), 1)).unwrap();
    json_out(
        e.tune()
            .args([
                "do",
                song,
                "resize_placement",
                "track=acid",
                "at=3",
                "bars=1",
            ])
            .args(run),
    );
    let before = fs::read_to_string(&e.song).unwrap();
    let v = json_out(e.tune().args(["do", song, "undo_run", "r1", "--json"]));
    let undo = &v["results"][0];
    assert_eq!(undo["reverted"], json!([]));
    let kept = undo["kept"].as_array().unwrap();
    assert!(
        kept.iter()
            .any(|k| k["item"]["at"] == 2 && k["by"].is_null() && k["reason"] == "changed_later"),
        "{undo}"
    );
    let after = fs::read_to_string(&e.song).unwrap();
    assert_eq!(after, before);
    // The run's resize rewrote the list (dropping the comment); the
    // placement itself is still there.
    assert!(
        after.contains("{ at = 2, clip = \"a1\", bars = 1 }"),
        "{after}"
    );
}

#[test]
fn undo_run_says_what_it_kept() {
    let e = fixture();
    let song = e.song.to_str().unwrap();
    for edit in ["tracks.acid.acid303.cutoff=820", "song.bpm=130"] {
        json_out(e.tune().args(["set", song, edit, "--run", "r1", "--json"]));
    }
    let text = fs::read_to_string(&e.song).unwrap();
    fs::write(&e.song, text.replace("= 820", "= 900")).unwrap();
    let out = e
        .tune()
        .args(["do", song, "undo_run", "r1"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "undid run r1: 1 reverted, 1 kept\n\
         kept tracks.acid.acid303.cutoff: changed later outside the studio"
    );
    let err = fails(e.tune().args(["do", song, "undo_run", "zz"]));
    assert!(
        err.contains("no edits of run `zz` in the edit log"),
        "{err}"
    );
}
