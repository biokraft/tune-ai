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

fn is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).unwrap().next().is_none()
}

fn wav_spec(path: &Path) -> (u16, u16, u32) {
    let r = hound::WavReader::open(path).unwrap();
    (r.spec().channels, r.spec().bits_per_sample, r.duration())
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

#[test]
fn render_whole_song() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.wav");
    let a = tune()
        .arg("render")
        .arg(mini())
        .arg("--out")
        .arg(&out)
        .arg("--json")
        .assert()
        .success();
    let v = json_of(&a.get_output().stdout);
    assert_eq!(v["ok"], true);
    assert_eq!(v["frames"], 384_000);
    assert!(v["peak_dbfs"].as_f64().unwrap() <= -1.0);
    assert!(v["pre_clip_peak_dbfs"].is_number());
    assert!(v["knee_samples"].is_number());
    assert_eq!(v["files"].as_array().unwrap().len(), 1);
    assert_eq!(wav_spec(&out), (2, 24, 384_000));
}

#[test]
fn render_text_line() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.wav");
    let a = tune()
        .arg("render")
        .arg(mini())
        .arg("--out")
        .arg(&out)
        .assert()
        .success();
    let s = String::from_utf8(a.get_output().stdout.clone()).unwrap();
    assert!(s.starts_with("rendered bars 1..4 (0:08.0) to "), "{s}");
    assert!(s.contains("dBFS, pre-clip peak"), "{s}");
}

#[test]
fn render_bar_range() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.wav");
    let a = tune()
        .arg("render")
        .arg(mini())
        .args(["--bars", "2..3", "--json", "--out"])
        .arg(&out)
        .assert()
        .success();
    assert_eq!(json_of(&a.get_output().stdout)["frames"], 192_000);
    assert_eq!(wav_spec(&out).2, 192_000);
}

#[test]
fn render_stems() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("stems");
    let a = tune()
        .arg("render")
        .arg(mini())
        .args(["--stems", "--json", "--out"])
        .arg(&out)
        .assert()
        .success();
    let v = json_of(&a.get_output().stdout);
    assert_eq!(v["files"].as_array().unwrap().len(), 4);
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 4);
}

#[test]
fn render_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.wav"), dir.path().join("b.wav"));
    for p in [&a, &b] {
        tune()
            .arg("render")
            .arg(mini())
            .arg("--out")
            .arg(p)
            .assert()
            .success();
    }
    assert_eq!(std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
}

fn render_fails(extra: &[&str], out_name: &str, needle: &str) {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join(out_name);
    let a = tune()
        .arg("render")
        .arg(mini())
        .args(extra)
        .arg("--out")
        .arg(&out)
        .assert()
        .code(2);
    let err = String::from_utf8(a.get_output().stderr.clone()).unwrap();
    assert!(err.contains(needle), "{err}");
    let a = tune()
        .arg("render")
        .arg(mini())
        .args(extra)
        .arg("--json")
        .arg("--out")
        .arg(&out)
        .assert()
        .code(2);
    let v = json_of(&a.get_output().stdout);
    assert_eq!(v["ok"], false);
    assert!(v["errors"][0]["message"].as_str().unwrap().contains(needle));
    assert!(is_empty(dir.path()));
}

#[test]
fn render_bad_args_exit_2_and_write_nothing() {
    render_fails(&["--bars", "0..2"], "x.wav", "bars start at 1");
    render_fails(&["--bars", "3..9"], "x.wav", "past the end (4 bars)");
    render_fails(&["--bars", "3..2"], "x.wav", "B < A");
    render_fails(&["--stems"], "x.wav", "--stems writes a directory");
    render_fails(&[], "x.flac", ".wav");
}

#[test]
fn render_missing_song_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let a = tune()
        .args(["render", "nope.toml", "--json", "--out"])
        .arg(dir.path().join("x.wav"))
        .assert()
        .code(2);
    let v = json_of(&a.get_output().stdout);
    assert_eq!(v["ok"], false);
    assert!(is_empty(dir.path()));
}

#[test]
fn render_303_json() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("d.wav");
    let a = tune()
        .args(["render-303", "--demo", "--json", "--out"])
        .arg(&out)
        .assert()
        .success();
    let v = json_of(&a.get_output().stdout);
    assert_eq!(v["ok"], true);
    assert!(v["file"].as_str().unwrap().ends_with("d.wav"));
    assert!(v["seconds"].is_number() && v["peak_dbfs"].is_number());
    assert_eq!(v["clipped_samples"], 0);
}
