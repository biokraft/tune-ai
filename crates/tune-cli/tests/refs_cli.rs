//! `tune refs`: listing references from a temp config, never the real one.

use assert_cmd::Command;
use serde_json::Value;
use std::path::Path;

fn tune(dir: &Path) -> Command {
    let mut c = Command::cargo_bin("tune").unwrap();
    c.current_dir(dir).env("HOME", dir);
    c
}

#[test]
fn refs_json_lists_entries() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("tune.local.toml"),
        "[refs]\nhere = \"here.mp3\"\ngone = \"~/gone.mp3\"\n",
    )
    .unwrap();
    std::fs::write(d.path().join("here.mp3"), "").unwrap();
    std::fs::create_dir(d.path().join("refs")).unwrap();
    std::fs::write(d.path().join("refs/extra.wav"), "").unwrap();
    let out = tune(d.path()).args(["refs", "--json"]).assert().success();
    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["ok"], true);
    assert!(v["root"].is_string());
    assert!(v["config"].is_string());
    let refs = v["refs"].as_array().unwrap();
    let got: Vec<(&str, bool, &str)> = refs
        .iter()
        .map(|r| {
            (
                r["name"].as_str().unwrap(),
                r["exists"].as_bool().unwrap(),
                r["from"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("here", true, "config"),
            ("gone", false, "config"),
            ("extra", true, "refs_dir")
        ]
    );
    assert_eq!(refs[1]["path"], "~/gone.mp3");
    assert!(refs[1]["resolved"].as_str().unwrap().ends_with("gone.mp3"));
}

#[test]
fn refs_text_marks_missing() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("tune.local.toml"),
        "[refs]\ngone = \"nope.mp3\"\n",
    )
    .unwrap();
    let out = tune(d.path()).arg("refs").assert().success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(text.trim_end(), "gone  missing  nope.mp3");
}

#[test]
fn refs_empty_is_ok() {
    let d = tempfile::tempdir().unwrap();
    let out = tune(d.path()).arg("refs").assert().success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(text.starts_with("no references"), "{text}");
}

#[test]
fn refs_bad_config_exits_2() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("tune.local.toml"), "[refs]\na = 1\n").unwrap();
    tune(d.path()).arg("refs").assert().code(2);
}
