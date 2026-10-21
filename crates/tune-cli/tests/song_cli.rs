use assert_cmd::Command;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn tune() -> Command {
    Command::cargo_bin("tune").unwrap()
}

fn mini() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tune-song/tests/fixtures/mini.toml")
}

fn json_of(out: &[u8]) -> Value {
    serde_json::from_slice(out).expect("stdout is JSON")
}

#[test]
fn validate_json_reports_shape() {
    let out = tune()
        .arg("validate")
        .arg(mini())
        .arg("--json")
        .assert()
        .success();
    let v = json_of(&out.get_output().stdout);
    assert_eq!(v["ok"], true);
    assert_eq!(v["name"], "mini");
    assert_eq!(v["bars"], 4);
    assert_eq!(v["seconds"], 8.0);
    assert_eq!(v["sections"], 2);
}

#[test]
fn validate_text_one_line() {
    let out = tune().arg("validate").arg(mini()).assert().success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(s, "ok: mini, 2 sections, 4 bars, 0:08.0 at 120 BPM\n");
}

fn broken_copy(dir: &Path) -> (PathBuf, usize) {
    let src = std::fs::read_to_string(mini()).unwrap();
    let bad = src.replace("acid = \"a1\"", "acid = \"nope\"");
    let line = bad.lines().position(|l| l.contains("\"nope\"")).unwrap() + 1;
    let path = dir.join("mini.toml");
    std::fs::write(&path, bad).unwrap();
    (path, line)
}

#[test]
fn validate_error_has_line_json_and_text() {
    let dir = tempfile::tempdir().unwrap();
    let (path, line) = broken_copy(dir.path());
    let out = tune()
        .arg("validate")
        .arg(&path)
        .arg("--json")
        .assert()
        .code(2);
    let v = json_of(&out.get_output().stdout);
    assert_eq!(v["ok"], false);
    assert_eq!(v["errors"][0]["line"], line);
    assert!(
        v["errors"][0]["file"]
            .as_str()
            .unwrap()
            .ends_with("mini.toml")
    );
    let out = tune().arg("validate").arg(&path).assert().code(2);
    let err = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(err.contains(&format!("mini.toml:{line}:")), "{err}");
}

#[test]
fn positionless_error_has_null_position() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.toml");
    std::fs::write(&path, "[song]\nname = \"x\"\nbpm = 120\n").unwrap();
    let out = tune()
        .arg("validate")
        .arg(&path)
        .arg("--json")
        .assert()
        .code(2);
    let v = json_of(&out.get_output().stdout);
    let e = v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["line"].is_null())
        .expect("a positionless error");
    assert!(e["column"].is_null());
    let out = tune().arg("validate").arg(&path).assert().code(2);
    let err = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(err.lines().any(|l| l.starts_with("error: ")), "{err}");
}

#[test]
fn info_json_sections_and_tracks() {
    let out = tune()
        .arg("info")
        .arg(mini())
        .arg("--json")
        .assert()
        .success();
    let v = json_of(&out.get_output().stdout);
    assert_eq!(v["sample_rate"], 48000);
    assert_eq!(v["sections"][1]["start_bar"], 3);
    assert_eq!(v["sections"][1]["end_bar"], 4);
    assert_eq!(v["sections"][1]["start_seconds"], 4.0);
    assert_eq!(v["sections"][1]["automation"][0], "acid.acid303.cutoff");
    assert_eq!(v["sections"][1]["play"]["acid"], "a1");
    assert_eq!(v["tracks"][1]["fx"][0], "distortion");
    assert_eq!(v["tracks"][1]["id"], "acid");
}

#[test]
fn info_text_has_rows() {
    let out = tune().arg("info").arg(mini()).assert().success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(s.starts_with("mini"), "{s}");
    assert!(s.contains("0:04.0"), "{s}");
    assert!(s.contains("acid.acid303.cutoff"), "{s}");
}

#[test]
fn params_json_lists_tables() {
    let out = tune().args(["params", "--json"]).assert().success();
    let v = json_of(&out.get_output().stdout);
    assert_eq!(v["instruments"]["kick"][0]["id"], "kick.tune");
    assert!(!v["effects"]["distortion"].as_array().unwrap().is_empty());
    for group in ["instruments", "effects"] {
        for (_, list) in v[group].as_object().unwrap() {
            for p in list.as_array().unwrap() {
                assert!(!p["help"].as_str().unwrap().is_empty(), "{p}");
                for k in ["label", "unit", "min", "max", "curve", "default"] {
                    assert!(p.get(k).is_some(), "{k} in {p}");
                }
            }
        }
    }
}

#[test]
fn params_text_mentions_ids() {
    let out = tune().arg("params").assert().success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(s.contains("kick.tune") && s.contains("dist.drive"));
}
