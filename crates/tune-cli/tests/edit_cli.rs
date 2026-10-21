//! `tune set` and `tune changes` against a copy of the commented fixture.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;
use tune_studio::text_hash;

fn fixture() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tune-studio/tests/fixtures/commented.toml");
    let song = dir.path().join("song.toml");
    fs::copy(src, &song).unwrap();
    (dir, song)
}

fn tune() -> Command {
    Command::cargo_bin("tune").unwrap()
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
    assert!(!fs::read_to_string(&song).unwrap().contains("acid = \"a1\""));
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
