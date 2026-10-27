//! `tune set` and `tune changes` against a copy of the commented fixture.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;
use tune_studio::text_hash;

fn fixture() -> (TempDir, PathBuf) {
    fixture_of("commented.toml")
}

/// A temp copy of the studio fixture `name` as `song.toml`.
fn fixture_of(name: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tune-studio/tests/fixtures")
        .join(name);
    let song = dir.path().join("song.toml");
    fs::copy(src, &song).unwrap();
    (dir, song)
}

/// One studio directory per test process, so no test reads the real `~/.tune`.
fn studio_dir() -> &'static Path {
    static DIR: std::sync::OnceLock<TempDir> = std::sync::OnceLock::new();
    DIR.get_or_init(|| tempfile::tempdir().unwrap()).path()
}

fn tune() -> Command {
    let mut c = Command::cargo_bin("tune").unwrap();
    c.env("TUNE_STUDIO_DIR", studio_dir());
    c
}

fn json_out(cmd: &mut Command) -> Value {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn set_writes_and_logs_claude() {
    let (dir, song) = fixture();
    let v = json_out(tune().args([
        "set",
        song.to_str().unwrap(),
        "tracks.acid.acid303.cutoff=820",
        "--json",
    ]));
    assert_eq!(v["ok"], true);
    assert!(v.get("migrated_from").is_none(), "{v}");
    assert_eq!(v["changes"][0]["old"], 500);
    assert_eq!(v["changes"][0]["new"], 820);
    let text = fs::read_to_string(&song).unwrap();
    for c in ["# the body", "# headroom", "# the squelch", "# acid enters"] {
        assert!(text.contains(c), "lost comment {c}");
    }
    assert_eq!(v["file_hash"], text_hash(&text));
    let log = fs::read_to_string(dir.path().join("edits.jsonl")).unwrap();
    let last: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    assert_eq!(last["source"], "claude");
    assert_eq!(last["file_hash"], text_hash(&text));
}

#[test]
fn set_text_output() {
    let (_d, song) = fixture();
    let out = tune()
        .args([
            "set",
            song.to_str().unwrap(),
            "tracks.acid.acid303.cutoff=820",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "set tracks.acid.acid303.cutoff 500 -> 820"
    );
}

#[test]
fn set_is_all_or_nothing() {
    let (dir, song) = fixture();
    let before = fs::read(&song).unwrap();
    let out = tune()
        .args([
            "set",
            song.to_str().unwrap(),
            "tracks.acid.acid303.cutoff=820",
            "tracks.acid.acid303.resonance=999",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("is out of range"));
    assert_eq!(fs::read(&song).unwrap(), before);
    assert!(!dir.path().join("edits.jsonl").exists());
    assert!(!dir.path().join(".song.toml.tune-tmp").exists());
}

#[test]
fn set_empty_value_removes_play() {
    let (_d, song) = fixture();
    let v = json_out(tune().args([
        "set",
        song.to_str().unwrap(),
        "sections[1].play.acid=",
        "--json",
    ]));
    assert_eq!(v["changes"][0]["old"], "a1");
    assert_eq!(v["changes"][0]["new"], Value::Null);
    assert!(!fs::read_to_string(&song).unwrap().contains("clip = \"a1\""));
}

/// Re-setting the clip a placement already plays, on a section that
/// placement enters mid-clip, restarts the clip there: a change, logged.
#[test]
fn set_same_clip_on_a_phase_shifted_section_is_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.toml");
    let k3 = "x...".repeat(12);
    fs::write(
        &song,
        format!(
            "[song]\nversion = 2\nname = \"phase\"\nbpm = 120\nseed = 1\n\n\
             [tracks.k]\ninstrument = \"kick\"\nplay = [{{ at = 1, clip = \"k3\", bars = 4 }}]\n\n\
             [clips.k3]\nkind = \"drum\"\nsteps = \"{k3}\"\n\n\
             [[sections]]\nname = \"a\"\nbars = 2\n\n[[sections]]\nname = \"b\"\nbars = 2\n"
        ),
    )
    .unwrap();
    let v = json_out(tune().args([
        "set",
        song.to_str().unwrap(),
        "sections[1].play.k=k3",
        "--json",
    ]));
    assert_eq!(v["changes"][0]["path"], "sections[1].play.k");
    assert_eq!(
        v["changes"][0]["old"],
        serde_json::json!([{ "at": 1, "clip": "k3", "bars": 4, "offset": 0 }])
    );
    assert_eq!(v["changes"][0]["new"], "k3");
    let text = fs::read_to_string(&song).unwrap();
    assert!(
        text.contains("{ at = 3, clip = \"k3\", bars = 2 }"),
        "{text}"
    );
    let log = fs::read_to_string(dir.path().join("edits.jsonl")).unwrap();
    let last: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    assert_eq!(
        (&last["source"], &last["path"]),
        (&"claude".into(), &"sections[1].play.k".into())
    );
    assert_eq!(last["file_hash"], text_hash(&text));
}

/// Resizing a late section of a long song splits a placement that runs
/// into it far into its clip; the split's offset is still valid.
#[test]
fn set_bars_splits_a_long_placement() {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.toml");
    fs::write(
        &song,
        "[song]\nversion = 2\nname = \"long\"\nbpm = 200\nseed = 1\n\n\
         [tracks.k]\ninstrument = \"kick\"\nplay = [{ at = 1, clip = \"k4\", bars = 1210 }]\n\n\
         [clips.k4]\nkind = \"drum\"\nsteps = \"x...x...x...x...\"\n\n\
         [[sections]]\nname = \"a\"\nbars = 1000\n\n[[sections]]\nname = \"b\"\nbars = 200\n\n\
         [[sections]]\nname = \"c\"\nbars = 10\n",
    )
    .unwrap();
    let v = json_out(tune().args([
        "set",
        song.to_str().unwrap(),
        "sections[2].bars=8",
        "--json",
    ]));
    assert_eq!(v["changes"][0]["path"], "sections[2].bars");
    let text = fs::read_to_string(&song).unwrap();
    assert!(
        text.contains("{ at = 1201, clip = \"k4\", bars = 8, offset = 1200 }"),
        "{text}"
    );
}

#[test]
fn set_on_v1_migrates_and_says_so() {
    let (dir, song) = fixture_of("commented-v1.toml");
    let v = json_out(tune().args([
        "set",
        song.to_str().unwrap(),
        "tracks.acid.acid303.cutoff=820",
        "--json",
    ]));
    assert_eq!(v["migrated_from"], 1);
    assert_eq!(v["changes"][0]["path"], "song.version");
    assert_eq!(
        (&v["changes"][0]["old"], &v["changes"][0]["new"]),
        (&1.into(), &2.into())
    );
    assert_eq!(v["changes"][1]["path"], "tracks.acid.acid303.cutoff");
    let text = fs::read_to_string(&song).unwrap();
    let parsed = tune_song::parse(&text).expect("the file parses");
    assert_eq!(parsed.version, 2);
    assert!(text.contains("# the filter opens over the section\n[[automation]]"));
    let log = fs::read_to_string(dir.path().join("edits.jsonl")).unwrap();
    let first: Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    assert_eq!(
        (&first["source"], &first["path"]),
        (&"claude".into(), &"song.version".into())
    );

    // Text mode says it too.
    let (_d, song) = fixture_of("commented-v1.toml");
    let out = tune()
        .args([
            "set",
            song.to_str().unwrap(),
            "tracks.acid.acid303.cutoff=820",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "migrated the file from version 1 to 2\nset tracks.acid.acid303.cutoff 500 -> 820"
    );
}

#[test]
fn set_noop_on_v1_writes_nothing() {
    let (dir, song) = fixture_of("commented-v1.toml");
    let before = fs::read(&song).unwrap();
    let v = json_out(tune().args([
        "set",
        song.to_str().unwrap(),
        "tracks.acid.acid303.cutoff=500",
        "--json",
    ]));
    assert_eq!(v["changes"], serde_json::json!([]));
    assert!(v.get("migrated_from").is_none(), "{v}");
    assert_eq!(
        v["file_hash"],
        text_hash(&String::from_utf8(before.clone()).unwrap())
    );
    assert_eq!(fs::read(&song).unwrap(), before, "the file stays v1");
    assert!(!dir.path().join("edits.jsonl").exists());
}

#[test]
fn set_bare_string_mode() {
    let (_d, song) = fixture();
    let v = json_out(tune().args([
        "set",
        song.to_str().unwrap(),
        "tracks.acid.dist.mode=hard",
        "--json",
    ]));
    assert_eq!(v["changes"][0]["old"], "soft");
    assert_eq!(v["changes"][0]["new"], "hard");
}

#[test]
fn set_unknown_path() {
    let (_d, song) = fixture();
    let out = tune()
        .args(["set", song.to_str().unwrap(), "tracks.acid.cutof=1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("does not exist in this song"));
}

#[test]
fn set_param_typo_names_the_track() {
    let (_d, song) = fixture();
    let out = tune()
        .args(["set", song.to_str().unwrap(), "tracks.acid.acid303.cutof=1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(
            "`acid303.cutof` is not a parameter of track `acid` (acid303); see `tune params --json`"
        ),
        "{stderr}"
    );
}

fn write_log(dir: &Path) {
    let lines = [
        r#"{"ts":"2026-10-21T10:00:01.000Z","source":"human","path":"song.bpm","old":120,"new":121}"#,
        r#"{"ts":"2026-10-21T10:00:02.000Z","source":"claude","path":"song.bpm","old":121,"new":122,"file_hash":"abc"}"#,
        r#"{"ts":"2026-10-21T10:00:03.000Z","source":"human","path":"song.bpm","old":122,"new":123}"#,
    ];
    fs::write(dir.join("edits.jsonl"), lines.join("\n") + "\n").unwrap();
}

#[test]
fn changes_since() {
    let (dir, song) = fixture();
    write_log(dir.path());
    let s = song.to_str().unwrap();
    let v = json_out(tune().args(["changes", s, "--since", "2026-10-21T10:00:01Z", "--json"]));
    assert_eq!(v["changes"].as_array().unwrap().len(), 2);
    assert_eq!(v["until"], "2026-10-21T10:00:03.000Z");
    let v = json_out(tune().args([
        "changes",
        s,
        "--since",
        "2026-10-21T10:00:01Z",
        "--source",
        "human",
        "--json",
    ]));
    assert_eq!(v["changes"].as_array().unwrap().len(), 1);
    let v = json_out(tune().args(["changes", s, "--json"]));
    assert_eq!(v["changes"].as_array().unwrap().len(), 3);
}

#[test]
fn changes_text_line() {
    let (dir, song) = fixture();
    write_log(dir.path());
    let out = tune()
        .args(["changes", song.to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next().unwrap();
    // HH:MM:SS local time, then source, path and the change.
    assert!(first.ends_with(" human song.bpm 120 -> 121"), "{first}");
    assert_eq!(first.split(' ').next().unwrap().len(), 8);
}

#[test]
fn changes_missing_log() {
    let (_d, song) = fixture();
    let v = json_out(tune().args(["changes", song.to_str().unwrap(), "--json"]));
    assert_eq!(v["changes"], serde_json::json!([]));
    assert_eq!(v["until"], Value::Null);
}

fn mixed_copy() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tune-song/tests/fixtures/v2/mixed.toml");
    let song = dir.path().join("song.toml");
    fs::copy(src, &song).unwrap();
    (dir, song)
}

#[test]
fn set_mix_paths() {
    let (_d, song) = mixed_copy();
    let s = song.to_str().unwrap();
    let v = json_out(tune().args([
        "set",
        s,
        "mix.delay.feedback=50",
        "tracks.acid.send.delay=-10",
        "mix.master=clip",
        "--json",
    ]));
    assert_eq!(v["changes"].as_array().unwrap().len(), 3);
    json_out(tune().args(["validate", s, "--json"]));
    let c = json_out(tune().args(["changes", s, "--json"]));
    let list = c["changes"].as_array().unwrap();
    assert_eq!(list.len(), 3, "{c}");
    assert!(list.iter().all(|e| e["source"] == "claude"), "{c}");
}

#[test]
fn set_mix_errors() {
    let (_d, song) = mixed_copy();
    let s = song.to_str().unwrap();
    let out = tune()
        .args(["set", s, "mix.delay.fedback=1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("is not a parameter of track `mix`"), "{e}");
    let out = tune().args(["set", s, "mix.master=loud"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("unknown master mode"), "{e}");
}

#[test]
fn set_clip_notes_from_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.toml");
    fs::write(
        &song,
        r#"[song]
version = 2
name = "notes"
bpm = 120

[tracks.acid]
instrument = "acid303"
play = [{ at = 1, clip = "lead", bars = 1 }]

[clips.lead]
kind = "notes"
bars = 1
notes = [
  { start = 0, pitch = "C3", len = 3 },
  { start = 2.5, pitch = "D#3", len = 2, vel = 120 },
]

[[sections]]
name = "a"
bars = 1
"#,
    )
    .unwrap();
    let v = json_out(tune().args([
        "set",
        song.to_str().unwrap(),
        "clips.lead.notes=[{ start = 0, pitch = \"C3\", len = 2 }]",
        "--json",
    ]));
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["changes"][0]["path"], "clips.lead.notes");
    let text = fs::read_to_string(&song).unwrap();
    assert!(
        text.contains("notes = [{ start = 0, pitch = \"C3\", len = 2 }]"),
        "{text}"
    );
    let v = json_out(tune().args(["changes", song.to_str().unwrap(), "--json"]));
    let entries = v["changes"].as_array().unwrap();
    assert!(
        entries
            .iter()
            .any(|e| e["path"] == "clips.lead.notes" && e["source"] == "claude"),
        "{v}"
    );
}
