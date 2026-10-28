//! `tune migrate`: v1 song files rewritten as v2, comments kept.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tune-song/tests/fixtures")
}

fn tune() -> Command {
    Command::cargo_bin("tune").unwrap()
}

/// A temp dir holding a copy of the v1 `mini.toml` as `song.toml`.
fn v1_copy() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.toml");
    fs::copy(fixtures().join("mini.toml"), &song).unwrap();
    (dir, song)
}

fn json_of(out: &[u8]) -> Value {
    serde_json::from_slice(out).expect("stdout is JSON")
}

fn log_lines(dir: &Path) -> Vec<Value> {
    fs::read_to_string(dir.join("edits.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn migrate_rewrites_a_v1_copy_and_logs_it() {
    let (dir, song) = v1_copy();
    let out = tune()
        .args(["migrate", song.to_str().unwrap(), "--json"])
        .assert()
        .success();
    let v = json_of(&out.get_output().stdout);
    assert_eq!(v["ok"], true);
    assert_eq!(v["migrated"], true);
    assert_eq!(v["from"], 1);
    assert_eq!(v["to"], 2);
    let hash = v["file_hash"].as_str().unwrap();
    assert_eq!(hash.len(), 16);
    assert!(hash.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    assert_eq!(
        fs::read_to_string(&song).unwrap(),
        fs::read_to_string(fixtures().join("v2/mini.toml")).unwrap()
    );
    let log = log_lines(dir.path());
    let last = log.last().expect("one log entry");
    assert_eq!(last["path"], "song.version");
    assert_eq!(last["source"], "claude");
    assert_eq!(last["old"], 1);
    assert_eq!(last["new"], 2);
    assert_eq!(last["file_hash"], hash);
}

#[test]
fn migrate_twice_is_a_no_op() {
    let (dir, song) = v1_copy();
    tune()
        .args(["migrate", song.to_str().unwrap()])
        .assert()
        .success();
    let bytes = fs::read(&song).unwrap();
    let lines = log_lines(dir.path()).len();
    let out = tune()
        .args(["migrate", song.to_str().unwrap(), "--json"])
        .assert()
        .success();
    let v = json_of(&out.get_output().stdout);
    assert_eq!(v["migrated"], false);
    assert_eq!(v["version"], 2);
    assert_eq!(fs::read(&song).unwrap(), bytes);
    assert_eq!(log_lines(dir.path()).len(), lines);
    let text = tune()
        .args(["migrate", song.to_str().unwrap()])
        .assert()
        .success();
    assert_eq!(
        String::from_utf8(text.get_output().stdout.clone()).unwrap(),
        format!("already version 2: {}\n", song.display())
    );
}

#[test]
fn migrate_dry_run_writes_nothing() {
    let (dir, song) = v1_copy();
    let before = fs::read(&song).unwrap();
    let out = tune()
        .args(["migrate", song.to_str().unwrap(), "--dry-run", "--json"])
        .assert()
        .success();
    let v = json_of(&out.get_output().stdout);
    assert_eq!(v["migrated"], true);
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["from"], 1);
    assert_eq!(v["to"], 2);
    assert_eq!(
        v["text"].as_str().unwrap(),
        fs::read_to_string(fixtures().join("v2/mini.toml")).unwrap()
    );
    assert_eq!(fs::read(&song).unwrap(), before);
    assert!(!dir.path().join("edits.jsonl").exists());
    let text = tune()
        .args(["migrate", song.to_str().unwrap(), "--dry-run"])
        .assert()
        .success();
    assert_eq!(
        String::from_utf8(text.get_output().stdout.clone()).unwrap(),
        fs::read_to_string(fixtures().join("v2/mini.toml")).unwrap()
    );
}

#[test]
fn migrate_text_mode_says_what_it_did() {
    let (_dir, song) = v1_copy();
    let out = tune()
        .args(["migrate", song.to_str().unwrap()])
        .assert()
        .success();
    assert_eq!(
        String::from_utf8(out.get_output().stdout.clone()).unwrap(),
        format!(
            "migrated {} from version 1 to 2 (comments kept)\n",
            song.display()
        )
    );
}

#[test]
fn migrate_invalid_v1_exits_2_with_positions() {
    let (_dir, song) = v1_copy();
    let bad = fs::read_to_string(&song)
        .unwrap()
        .replace("acid = \"a1\"", "acid = \"nope\"");
    fs::write(&song, &bad).unwrap();
    let out = tune()
        .args(["migrate", song.to_str().unwrap(), "--json"])
        .assert()
        .code(2);
    let v = json_of(&out.get_output().stdout);
    assert_eq!(v["ok"], false);
    assert!(v["errors"][0]["line"].is_u64());
    assert!(v["errors"][0]["column"].is_u64());
    assert_eq!(fs::read_to_string(&song).unwrap(), bad);
}
