use assert_cmd::Command;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// One studio directory per test process, so no test reads the real `~/.tune`.
fn studio_dir() -> &'static Path {
    static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    DIR.get_or_init(|| tempfile::tempdir().unwrap()).path()
}

fn tune() -> Command {
    let mut c = Command::cargo_bin("tune").unwrap();
    c.env("TUNE_STUDIO_DIR", studio_dir());
    c
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
fn info_v2_shows_placement_offsets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.toml");
    std::fs::write(
        &path,
        r#"[song]
version = 2
name = "v2"
bpm = 120

[tracks.acid]
instrument = "acid303"
play = [{ at = 2, clip = "a1", bars = 1, offset = 1 }]

[clips.a1]
kind = "acid"
steps = "C2 C3"

[[sections]]
name = "a"
bars = 2
"#,
    )
    .unwrap();
    let out = tune().arg("info").arg(&path).assert().success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(s.contains("play: acid=a1@2+1(offset=1)"), "{s}");
    let out = tune()
        .arg("info")
        .arg(&path)
        .arg("--json")
        .assert()
        .success();
    let v = json_of(&out.get_output().stdout);
    assert_eq!(
        v["sections"][0]["play"]["acid"],
        json!([{ "at": 2, "clip": "a1", "bars": 1, "offset": 1 }])
    );
}

#[test]
fn params_json_lists_tables() {
    let out = tune().args(["params", "--json"]).assert().success();
    let v = json_of(&out.get_output().stdout);
    assert_eq!(v["instruments"]["kick"][0]["id"], "kick.tune");
    assert_eq!(v["plays"]["kick"], json!(["drum", "notes"]));
    assert_eq!(v["plays"]["acid303"], json!(["acid", "notes"]));
    assert!(!v["effects"]["distortion"].as_array().unwrap().is_empty());
    assert_eq!(
        v["settings"]["distortion"][0]["values"],
        json!(["soft", "hard", "overdrive"])
    );
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

fn render_stderr(src: &Path) -> String {
    let dir = tempfile::tempdir().unwrap();
    let a = tune()
        .arg("render")
        .arg(src)
        .arg("--out")
        .arg(dir.path().join("x.wav"))
        .assert()
        .success();
    String::from_utf8(a.get_output().stderr.clone()).unwrap()
}

#[test]
fn clipper_note_for_knee_and_warning_for_limiting() {
    let err = render_stderr(&mini());
    assert!(err.contains("note: soft clipper shaped "), "{err}");
    assert!(!err.contains("warning:"), "{err}");

    let dir = tempfile::tempdir().unwrap();
    let hot = dir.path().join("hot.toml");
    let src = std::fs::read_to_string(mini()).unwrap();
    let src = src
        .replace("gain_db = -3", "gain_db = 12")
        .replace("gain_db = -10", "gain_db = 12")
        .replace(
            "[tracks.acid]\ninstrument = \"acid303\"",
            "[tracks.acid]\ninstrument = \"acid303\"\ngain_db = 12",
        );
    std::fs::write(&hot, src).unwrap();
    let err = render_stderr(&hot);
    assert!(err.contains("warning: master clipper engaged on "), "{err}");
}

fn mixed() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tune-song/tests/fixtures/mixed.toml")
}

fn render_json(src: &Path) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.wav");
    let a = tune()
        .arg("render")
        .arg(src)
        .arg("--out")
        .arg(&out)
        .arg("--json")
        .assert()
        .success();
    json_of(&a.get_output().stdout)
}

fn spec_ids(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn params_lists_mix_and_strip() {
    let out = tune().args(["params", "--json"]).assert().success();
    let v = json_of(&out.get_output().stdout);
    let strip = spec_ids(&v["strip"]);
    for id in ["send.delay", "duck.depth"] {
        assert!(strip.contains(&id.to_string()), "{id} in {strip:?}");
    }
    for (unit, id) in [
        ("delay", "delay.time"),
        ("reverb", "reverb.size"),
        ("master", "master.ceiling"),
    ] {
        assert!(spec_ids(&v["mix"][unit]).contains(&id.to_string()), "{id}");
    }
    assert!(spec_ids(&v["effects"]["eq"]).contains(&"eq.low_cut".to_string()));
    assert!(spec_ids(&v["instruments"]["ride"]).contains(&"ride.decay".to_string()));
    let text = tune().arg("params").assert().success();
    let s = String::from_utf8(text.get_output().stdout.clone()).unwrap();
    for h in ["strip\n", "mix delay\n", "mix reverb\n", "mix master\n"] {
        assert!(
            s.contains(&format!("\n{h}")) || s.starts_with(h),
            "{h} in {s}"
        );
    }
}

#[test]
fn render_reports_limiter() {
    let v = render_json(&mixed());
    assert_eq!(v["master"], "limit");
    assert!(v["max_reduction_db"].as_f64().unwrap() > 0.0);
    assert!(v["peak_dbfs"].as_f64().unwrap() <= -1.5 + 1e-9, "{v}");
}

#[test]
fn render_mini_reports_clip() {
    let v = render_json(&mini());
    assert_eq!(v["master"], "clip");
    assert_eq!(v["max_reduction_db"].as_f64().unwrap(), 0.0);
    for k in [
        "ok",
        "files",
        "frames",
        "seconds",
        "peak_dbfs",
        "pre_clip_peak_dbfs",
        "knee_samples",
    ] {
        assert!(v.get(k).is_some(), "{k}");
    }
}

#[test]
fn limit_mode_does_not_print_clipper_lines() {
    let err = render_stderr(&mixed());
    assert!(!err.contains("clipper"), "{err}");
}

/// Render stdout and stderr, minus the v1 song's migration note (the
/// limiter and clipper messages are what these tests look at).
fn render_text(src: &Path) -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    let a = tune()
        .arg("render")
        .arg(src)
        .arg("--out")
        .arg(dir.path().join("x.wav"))
        .assert()
        .success();
    let o = a.get_output();
    let err = String::from_utf8(o.stderr.clone()).unwrap();
    let err: String = err
        .lines()
        .filter(|l| !l.contains("tune migrate"))
        .map(|l| format!("{l}\n"))
        .collect();
    (String::from_utf8(o.stdout.clone()).unwrap(), err)
}

fn mixed_with_gain(db: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("hot.toml");
    std::fs::copy(mixed(), &song).unwrap();
    tune()
        .args([
            "set",
            song.to_str().unwrap(),
            &format!("mix.master.gain={db}"),
        ])
        .assert()
        .success();
    (dir, song)
}

#[test]
fn limiter_note_in_text_mode() {
    let (out, err) = render_text(&mixed());
    assert!(
        err.starts_with("note: limiter reduced peaks by up to "),
        "{err}"
    );
    assert!(err.trim_end().ends_with(" dB"), "{err}");
    assert!(!err.contains("warning:"), "{err}");
    assert!(out.contains("limiter reduced peaks by "), "{out}");
}

#[test]
fn limiter_warning_in_text_mode() {
    let (_d, song) = mixed_with_gain("12");
    let (_out, err) = render_text(&song);
    assert!(
        err.starts_with("warning: limiter reduced peaks by "),
        "{err}"
    );
    assert!(err.contains(" dB: lower mix.master.gain"), "{err}");
    assert!(!err.contains("note:"), "{err}");
}

#[test]
fn limit_text_line_leads_with_reduction() {
    let (out, _) = render_text(&mixed());
    assert!(out.starts_with("rendered bars 1.."), "{out}");
    assert!(out.contains("dBFS, limiter reduced peaks by "), "{out}");
    assert!(!out.contains("pre-clip"), "{out}");
}

fn v2(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tune-song/tests/fixtures/v2")
        .join(name)
}

#[test]
fn validate_v1_says_it_migrated_in_memory() {
    let out = tune()
        .arg("validate")
        .arg(mini())
        .arg("--json")
        .assert()
        .success();
    assert_eq!(json_of(&out.get_output().stdout)["migrated_from"], 1);
    let out = tune().arg("validate").arg(mini()).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("tune migrate"), "{stderr}");
}

#[test]
fn validate_v2_has_no_migration_note() {
    let out = tune()
        .arg("validate")
        .arg(v2("mini.toml"))
        .arg("--json")
        .assert()
        .success();
    assert!(
        json_of(&out.get_output().stdout)
            .get("migrated_from")
            .is_none()
    );
    let out = tune()
        .arg("validate")
        .arg(v2("mini.toml"))
        .assert()
        .success();
    assert!(out.get_output().stderr.is_empty());
}

#[test]
fn info_lists_devices_and_placements() {
    let out = tune()
        .arg("info")
        .arg(v2("mixed.toml"))
        .arg("--json")
        .assert()
        .success();
    let v = json_of(&out.get_output().stdout);
    assert!(v.get("migrated_from").is_none());
    let acid = v["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "acid")
        .unwrap();
    assert_eq!(
        acid["devices"]["fx"],
        json!([{"id": "dist", "type": "distortion"}, {"id": "eq", "type": "eq"}])
    );
    assert_eq!(
        acid["devices"]["instrument"],
        json!({"id": "acid303", "type": "acid303"})
    );
    assert_eq!(acid["instrument"], "acid303");
    assert_eq!(
        acid["placements"][0],
        json!({"at": 3, "clip": "a1", "bars": 2, "offset": 0})
    );
    assert_eq!(
        v["sections"][1]["play"],
        json!({"kick": "k4", "acid": "a1", "hat": "h"})
    );
}

#[test]
fn info_text_shows_a_device_id_that_differs_from_its_module_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let src = std::fs::read_to_string(v2("mixed.toml")).unwrap().replace(
        "{ type = \"eq\", params = { \"eq.low_cut\" = 85 } }]",
        "{ type = \"eq\", params = { \"eq.low_cut\" = 85 } }, { type = \"eq\", id = \"eq2\" }]",
    );
    assert!(src.contains("eq2"));
    let song = dir.path().join("song.toml");
    std::fs::write(&song, &src).unwrap();
    let out = tune().arg("info").arg(&song).assert().success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(text.contains("fx: distortion eq eq(eq2)"), "{text}");
}

#[test]
fn render_v1_reports_migration() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("out.wav");
    let out = tune()
        .arg("render")
        .arg(mini())
        .arg("--out")
        .arg(&wav)
        .arg("--json")
        .assert()
        .success();
    assert_eq!(json_of(&out.get_output().stdout)["migrated_from"], 1);
    let out = tune()
        .arg("render")
        .arg(v2("mini.toml"))
        .arg("--out")
        .arg(&wav)
        .arg("--json")
        .assert()
        .success();
    assert!(
        json_of(&out.get_output().stdout)
            .get("migrated_from")
            .is_none()
    );
}

#[test]
fn a_note_clip_song_validates_and_renders() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.toml");
    std::fs::write(
        &path,
        r#"[song]
version = 2
name = "notes"
bpm = 120

[tracks.kick]
instrument = "kick"
play = [{ at = 1, clip = "k4", bars = 2 }]

[tracks.acid]
instrument = "acid303"
play = [{ at = 1, clip = "lead", bars = 2 }]

[clips.k4]
kind = "drum"
steps = "x...x...x...x..."

[clips.lead]
kind = "notes"
bars = 2
notes = [
  { start = 0, pitch = "C2", len = 3 },
  { start = 4, pitch = "C3", len = 2, vel = 120 },
  { start = 8, pitch = "G2", len = 4 },
  { start = 16, pitch = "D#2", len = 4 },
  { start = 24, pitch = "F2", len = 4 },
]

[clips.stab]
kind = "notes"
bars = 1
notes = [{ start = 0, pitch = "C3", len = 1 }]

[[sections]]
name = "a"
bars = 2
"#,
    )
    .unwrap();
    tune().arg("validate").arg(&path).assert().success();

    let out = tune()
        .arg("info")
        .arg(&path)
        .arg("--json")
        .assert()
        .success();
    let v = json_of(&out.get_output().stdout);
    assert_eq!(
        v["clips"],
        json!([
            { "id": "k4", "kind": "drum", "steps": 16 },
            { "id": "lead", "kind": "notes", "steps": 32, "bars": 2, "notes": 5 },
            { "id": "stab", "kind": "notes", "steps": 16, "bars": 1, "notes": 1 },
        ])
    );
    let out = tune().arg("info").arg(&path).assert().success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        s.contains("clips: k4 drum 16 steps, lead notes 2 bars 5 notes, stab notes 1 bar 1 note"),
        "{s}"
    );

    let stems = dir.path().join("stems");
    let out = tune()
        .arg("render")
        .arg(&path)
        .args(["--stems", "--json", "--out"])
        .arg(&stems)
        .assert()
        .success();
    let v = json_of(&out.get_output().stdout);
    let files = v["files"].as_array().unwrap();
    let acid = files
        .iter()
        .map(|f| PathBuf::from(f.as_str().unwrap()))
        .find(|f| f.file_name().unwrap().to_string_lossy().contains("acid"))
        .unwrap_or_else(|| panic!("no acid stem in {v}"));
    let acid = if acid.is_absolute() {
        acid
    } else {
        stems.join(acid.file_name().unwrap())
    };
    let peak = hound::WavReader::open(&acid)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap().abs())
        .fold(0.0_f32, f32::max);
    let dbfs = 20.0 * f64::from(peak).log10();
    assert!(dbfs > -60.0, "acid stem peak {dbfs} dBFS");
}

fn session() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tune-song/tests/fixtures/session.toml")
}

#[test]
fn info_lists_scenes() {
    let out = tune()
        .arg("info")
        .arg(session())
        .arg("--json")
        .assert()
        .success();
    let v = json_of(&out.get_output().stdout);
    assert_eq!(
        v["scenes"],
        json!([
            {"id": "groove", "slots": {"kick": "k4", "acid": "a1"}},
            {"id": "break", "slots": {"hat": "h"}},
        ])
    );
    let out = tune().arg("info").arg(session()).assert().success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        text.lines()
            .any(|l| l == "scenes: groove (kick k4, acid a1), break (hat h)"),
        "{text}"
    );

    let out = tune()
        .arg("info")
        .arg(v2("mini.toml"))
        .arg("--json")
        .assert()
        .success();
    assert_eq!(json_of(&out.get_output().stdout)["scenes"], json!([]));
    let out = tune().arg("info").arg(v2("mini.toml")).assert().success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(!text.contains("scenes:"), "{text}");

    let out = tune()
        .arg("validate")
        .arg(session())
        .arg("--json")
        .assert()
        .success();
    assert_eq!(json_of(&out.get_output().stdout)["ok"], true);
}
