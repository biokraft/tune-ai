//! `tune analyze`: songs rendered in memory, WAV files, bar maps, PNGs.

use assert_cmd::Command;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn tune(dir: &Path) -> Command {
    let mut c = Command::cargo_bin("tune").unwrap();
    c.current_dir(dir);
    c
}

fn mini() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tune-song/tests/fixtures/mini.toml")
        .canonicalize()
        .unwrap()
}

fn json_of(out: &[u8]) -> Value {
    serde_json::from_slice(out).expect("stdout is JSON")
}

fn write_sine(path: &Path, sr: u32, channels: u16, secs: u32, dbfs: f64) {
    let spec = hound::WavSpec {
        channels,
        sample_rate: sr,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let amp = 10f64.powf(dbfs / 20.0);
    for i in 0..sr * secs {
        let s = (amp
            * (2.0 * std::f64::consts::PI * 1000.0 * f64::from(i) / f64::from(sr)).sin()
            * 32767.0) as i16;
        for _ in 0..channels {
            w.write_sample(s).unwrap();
        }
    }
    w.finalize().unwrap();
}

fn analyze_json(dir: &Path, args: &[&str]) -> Value {
    let out = tune(dir)
        .arg("analyze")
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    json_of(&out.stdout)
}

#[test]
fn analyze_wav_json() {
    let dir = tempfile::tempdir().unwrap();
    write_sine(&dir.path().join("t.wav"), 48_000, 2, 10, -23.0);
    let v = analyze_json(dir.path(), &["t.wav"]);
    assert_eq!(v["ok"], true);
    assert_eq!(v["source"]["kind"], "file");
    assert_eq!(v["source"]["bars"], Value::Null);
    let l = v["loudness"]["integrated_lufs"].as_f64().unwrap();
    assert!((l + 23.0).abs() < 0.1, "{l}");
    assert_eq!(v["png"], Value::Null);
}

#[test]
fn analyze_song_sections() {
    let dir = tempfile::tempdir().unwrap();
    let m = mini();
    let v = analyze_json(dir.path(), &[m.to_str().unwrap(), "--sections"]);
    assert_eq!(v["source"]["kind"], "song");
    assert_eq!(v["source"]["bars"], serde_json::json!([1, 4]));
    let s = v["sections"].as_array().unwrap();
    let names: Vec<&str> = s.iter().map(|x| x["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["a", "b"]);
    assert_eq!(s[0]["start_bar"], 1);
    assert_eq!(s[1]["start_bar"], 3);
}

#[test]
fn analyze_song_bars() {
    let dir = tempfile::tempdir().unwrap();
    let m = mini();
    let v = analyze_json(dir.path(), &[m.to_str().unwrap(), "--bars", "2..3"]);
    assert_eq!(v["source"]["bars"], serde_json::json!([2, 3]));
    assert_eq!(v["source"]["seconds"], 4.0);
}

#[test]
fn analyze_one_bar() {
    let dir = tempfile::tempdir().unwrap();
    let m = mini();
    let v = analyze_json(dir.path(), &[m.to_str().unwrap(), "--bars", "1..1"]);
    assert_eq!(v["loudness"]["short_term_max_lufs"], Value::Null);
}

#[test]
fn analyze_png_default_path() {
    let dir = tempfile::tempdir().unwrap();
    let m = mini();
    let v = analyze_json(dir.path(), &[m.to_str().unwrap(), "--png"]);
    assert_eq!(v["png"], "scratch/mini.png");
    assert!(dir.path().join("scratch/mini.png").is_file());
    let v = analyze_json(
        dir.path(),
        &[m.to_str().unwrap(), "--bars", "2..3", "--png"],
    );
    assert_eq!(v["png"], "scratch/mini-bars-2-3.png");
    assert!(dir.path().join("scratch/mini-bars-2-3.png").is_file());
    let v = analyze_json(dir.path(), &[m.to_str().unwrap(), "--png", "out/x.png"]);
    assert_eq!(v["png"], "out/x.png");
    assert!(dir.path().join("out/x.png").is_file());
}

#[test]
fn analyze_is_deterministic() {
    let m = mini();
    let run = || {
        let dir = tempfile::tempdir().unwrap();
        let out = tune(dir.path())
            .args([
                "analyze",
                m.to_str().unwrap(),
                "--sections",
                "--png",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(out.status.success());
        let png = std::fs::read(dir.path().join("scratch/mini.png")).unwrap();
        (out.stdout, png)
    };
    let (a, pa) = run();
    let (b, pb) = run();
    assert_eq!(a, b);
    assert_eq!(pa, pb);
}

#[test]
fn analyze_wav_with_song_map() {
    let dir = tempfile::tempdir().unwrap();
    let m = mini();
    let ms = m.to_str().unwrap();
    tune(dir.path())
        .args(["render", ms, "--bars", "3..4", "--out", "x.wav"])
        .assert()
        .success();
    let v = analyze_json(
        dir.path(),
        &["x.wav", "--song", ms, "--from-bar", "3", "--sections"],
    );
    assert_eq!(v["source"]["kind"], "file");
    let s = v["sections"].as_array().unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0]["name"], "b");
    assert_eq!(s[0]["start_bar"], 3);
    assert_eq!(s[0]["end_bar"], 4);
    let song = analyze_json(dir.path(), &[ms, "--bars", "3..4"]);
    let a = v["loudness"]["integrated_lufs"].as_f64().unwrap();
    let b = song["loudness"]["integrated_lufs"].as_f64().unwrap();
    assert!((a - b).abs() < 0.02, "{a} vs {b}");
}

#[test]
fn analyze_wav_past_the_song_end_is_noted() {
    let dir = tempfile::tempdir().unwrap();
    let m = mini_v2();
    let ms = m.to_str().unwrap();
    // mini is 4 bars of 2 s. 6 s from bar 3 holds bars 3-4 and 2 s past the end.
    write_sine(&dir.path().join("long.wav"), 48_000, 2, 6, -20.0);
    let v = analyze_json(dir.path(), &["long.wav", "--song", ms, "--from-bar", "3"]);
    assert_eq!(v["source"]["bars"], serde_json::json!([3, 4]));
    assert_eq!(v["source"]["seconds"], 4.0);
    assert_eq!(
        v["notes"],
        serde_json::json!([
            "the file runs 2.0 s past the song's last bar; that audio is ignored (check --from-bar)"
        ])
    );
    let out = tune(dir.path())
        .args(["analyze", "long.wav", "--song", ms, "--from-bar", "3"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.starts_with("note: the file runs 2.0 s past"), "{err}");

    // A file that fits leaves `notes` empty.
    write_sine(&dir.path().join("fit.wav"), 48_000, 2, 4, -20.0);
    let v = analyze_json(dir.path(), &["fit.wav", "--song", ms, "--from-bar", "3"]);
    assert_eq!(v["notes"], serde_json::json!([]));
}

#[test]
fn analyze_wav_ending_mid_bar_marks_the_section_partial() {
    let dir = tempfile::tempdir().unwrap();
    let m = mini();
    let ms = m.to_str().unwrap();
    // 3 s from bar 1: bar 1 whole, bar 2 cut in half; section a is bars 1-2.
    write_sine(&dir.path().join("short.wav"), 48_000, 2, 3, -20.0);
    let v = analyze_json(dir.path(), &["short.wav", "--song", ms, "--sections"]);
    let s = v["sections"].as_array().unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(
        (s[0]["start_bar"].as_u64(), s[0]["end_bar"].as_u64()),
        (Some(1), Some(2))
    );
    assert_eq!(s[0]["partial"], true);
}

fn fails_with(dir: &Path, args: &[&str], needle: &str) {
    let out = tune(dir)
        .arg("analyze")
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{args:?}");
    let v = json_of(&out.stdout);
    assert_eq!(v["ok"], false);
    let msg = v["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains(needle), "{args:?}: {msg}");
}

#[test]
fn analyze_errors() {
    let dir = tempfile::tempdir().unwrap();
    let m = mini();
    let ms = m.to_str().unwrap();
    write_sine(&dir.path().join("t.wav"), 48_000, 2, 2, -20.0);
    write_sine(&dir.path().join("t44.wav"), 44_100, 2, 2, -20.0);
    write_sine(&dir.path().join("six.wav"), 48_000, 6, 1, -20.0);
    fails_with(
        dir.path(),
        &[ms, "--song", ms],
        "--song maps a WAV onto a song; the input is already a song",
    );
    fails_with(
        dir.path(),
        &["t.wav", "--sections"],
        "--sections needs a bar map: analyse the song, or pass --song with the WAV",
    );
    fails_with(
        dir.path(),
        &["t.wav", "--bars", "1..2"],
        "--bars needs a bar map: analyse the song, or pass --song with the WAV",
    );
    fails_with(
        dir.path(),
        &["t44.wav", "--song", ms],
        "file is 44100 Hz but the song renders at 48000 Hz",
    );
    // 2 s at 120 BPM is exactly one bar: the file holds bar 3 only.
    fails_with(
        dir.path(),
        &["t.wav", "--song", ms, "--from-bar", "3", "--bars", "1..2"],
        "bars 1..2 are not inside the file (it holds bars 3..3)",
    );
    fails_with(dir.path(), &["six.wav"], "channels");
    fails_with(dir.path(), &["missing.wav"], "missing.wav");
}

#[test]
fn analyze_text_mentions_target() {
    let dir = tempfile::tempdir().unwrap();
    let m = mini();
    let out = tune(dir.path())
        .args(["analyze", m.to_str().unwrap(), "--sections"])
        .assert()
        .success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(s.contains("LUFS"), "{s}");
    assert!(s.contains("target -8..-6 LUFS"), "{s}");
    assert!(
        s.starts_with("mini: 0:08.0, 48000 Hz, stereo, bars 1-4"),
        "{s}"
    );
    for key in [
        "loudness",
        "peaks",
        "bands",
        "low end",
        "stereo",
        "brightness",
        "section a bars 1-2",
        "section b bars 3-4",
    ] {
        assert!(s.lines().any(|l| l.starts_with(key)), "{key}: {s}");
    }
}

// ---- tune compare ----

fn write_six(path: &Path, extra_500_dbfs: Option<f64>) {
    let sr = 48_000u32;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sr,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let tone = |hz: f64, db: f64, i: u32| {
        10f64.powf(db / 20.0)
            * (2.0 * std::f64::consts::PI * hz * f64::from(i) / f64::from(sr)).sin()
    };
    for i in 0..sr * 5 {
        let mut x = 0.0;
        for hz in [40.0, 150.0, 500.0, 2000.0, 6000.0, 14000.0] {
            x += tone(hz, -20.0, i);
        }
        if let Some(db) = extra_500_dbfs {
            x += tone(500.0, db, i);
        }
        w.write_sample((x * 32767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

fn compare_out(dir: &Path, args: &[&str]) -> std::process::Output {
    tune(dir)
        .env("HOME", dir)
        .arg("compare")
        .args(args)
        .output()
        .unwrap()
}

fn compare_json(dir: &Path, args: &[&str]) -> Value {
    let mut a = args.to_vec();
    a.push("--json");
    let out = compare_out(dir, &a);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    json_of(&out.stdout)
}

fn ref_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write_sine(&dir.path().join("ref.wav"), 48_000, 2, 10, -14.0);
    std::fs::write(
        dir.path().join("tune.local.toml"),
        "[refs]\nr = \"ref.wav\"\n",
    )
    .unwrap();
    dir
}

#[test]
fn compare_wav_to_wav() {
    let dir = tempfile::tempdir().unwrap();
    write_six(&dir.path().join("ref.wav"), None);
    write_six(&dir.path().join("ours.wav"), Some(-14.0));
    let v = compare_json(dir.path(), &["ours.wav", "ref.wav"]);
    assert_eq!(v["ok"], true);
    assert!(v["deltas"]["bands"]["low_mid"].as_f64().unwrap() > 3.0);
    let verdicts = v["verdicts"].as_array().unwrap();
    assert!(
        verdicts
            .iter()
            .any(|s| s.as_str().unwrap().ends_with("muddier")),
        "{verdicts:?}"
    );
    assert_eq!(v["reference"]["source"]["ref_name"], Value::Null);
    assert_eq!(v["reference"]["source"]["seconds_range"], Value::Null);
    assert_eq!(v["ours"]["source"]["kind"], "file");
    let text = String::from_utf8(compare_out(dir.path(), &["ours.wav", "ref.wav"]).stdout).unwrap();
    assert!(text.starts_with("ours ours"), "{text}");
    assert!(text.contains("\nref  ref"), "{text}");
    assert!(text.lines().any(|l| l.starts_with("- low-mid +")), "{text}");
    assert!(text.lines().last().unwrap().starts_with("png "), "{text}");
}

#[test]
fn compare_song_by_ref_name() {
    let dir = ref_dir();
    let m = mini();
    let v = compare_json(
        dir.path(),
        &[m.to_str().unwrap(), "--ref", "r", "--bars", "3..4"],
    );
    assert_eq!(v["reference"]["source"]["ref_name"], "r");
    assert_eq!(v["ours"]["source"]["bars"], serde_json::json!([3, 4]));
}

/// The reference's directory, as given and canonicalised (macOS temp dirs
/// live under a `/var` -> `/private/var` link).
fn dir_strings(dir: &Path) -> [String; 2] {
    [
        dir.display().to_string(),
        dir.canonicalize().unwrap().display().to_string(),
    ]
}

#[test]
fn compare_by_name_shows_the_configured_path() {
    let dir = ref_dir();
    std::fs::write(
        dir.path().join("tune.local.toml"),
        "[refs]\nr = \"~/ref.wav\"\nbad = \"~/bad.mp3\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("bad.mp3"), "not audio").unwrap();
    let m = mini();
    let ms = m.to_str().unwrap();
    let out = compare_out(dir.path(), &[ms, "--ref", "r", "--bars", "3..4", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json_of(&out.stdout);
    assert_eq!(v["reference"]["source"]["path"], "~/ref.wav");
    let stdout = String::from_utf8(out.stdout).unwrap();
    for d in dir_strings(dir.path()) {
        assert!(!stdout.contains(&d), "home path leaked: {stdout}");
    }

    let out = compare_out(dir.path(), &[ms, "--ref", "bad", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(all.contains("~/bad.mp3"), "{all}");
    for d in dir_strings(dir.path()) {
        assert!(!all.contains(&d), "home path leaked: {all}");
    }
}

#[test]
fn compare_png_default_path() {
    let dir = ref_dir();
    let m = mini();
    let v = compare_json(
        dir.path(),
        &[m.to_str().unwrap(), "--ref", "r", "--bars", "3..4", "--png"],
    );
    assert_eq!(v["png"], "scratch/compare-mini-bars-3-4-vs-r.png");
    let bytes = std::fs::read(dir.path().join("scratch/compare-mini-bars-3-4-vs-r.png")).unwrap();
    // PNG IHDR: width and height are big-endian u32 at bytes 16 and 20.
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 1600);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 1712);
}

#[test]
fn compare_ref_seconds() {
    let dir = ref_dir();
    let m = mini();
    let v = compare_json(
        dir.path(),
        &[m.to_str().unwrap(), "--ref", "r", "--ref-seconds", "2..4"],
    );
    assert_eq!(v["reference"]["source"]["seconds"], 2.0);
    assert_eq!(
        v["reference"]["source"]["seconds_range"],
        serde_json::json!([2.0, 4.0])
    );
}

#[test]
fn compare_errors() {
    let dir = ref_dir();
    let m = mini();
    let ms = m.to_str().unwrap();
    std::fs::write(
        dir.path().join("tune.local.toml"),
        "[refs]\nr = \"ref.wav\"\ngone = \"nope.wav\"\n",
    )
    .unwrap();
    let msg = |args: &[&str]| {
        let mut a = args.to_vec();
        a.push("--json");
        let out = compare_out(dir.path(), &a);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let v = json_of(&out.stdout);
        assert_eq!(v["ok"], false);
        v["errors"][0]["message"].as_str().unwrap().to_string()
    };
    assert!(msg(&[ms, "--ref", "x"]).contains("no reference named `x`"));
    assert!(msg(&[ms, "--ref", "gone"]).contains("which does not exist"));
    assert!(
        msg(&[ms, "--ref", "r", "--ref-seconds", "5..50"]).contains("the reference is 10.0 s long")
    );
    assert!(msg(&[ms, "--ref", "r", "--ref-seconds", "x"]).contains("bad --ref-seconds `x`"));
    for empty in ["5..2", "2..2"] {
        assert_eq!(
            msg(&[ms, "--ref", "r", "--ref-seconds", empty]),
            format!("bad --ref-seconds `{empty}`: A must be less than B")
        );
    }
    // compare has no --song, so a WAV as ours cannot take --bars.
    assert_eq!(
        msg(&["ref.wav", "--ref", "r", "--bars", "1..2"]),
        "--bars needs a song file as OURS: compare cannot map a WAV onto bars"
    );
    // Both forms together, or neither: clap rejects.
    let both = compare_out(dir.path(), &[ms, "ref.wav", "--ref", "r"]);
    assert_eq!(both.status.code(), Some(2));
    let neither = compare_out(dir.path(), &[ms]);
    assert_eq!(neither.status.code(), Some(2));
}

#[test]
fn compare_usage_puts_ours_first() {
    // `ours` is the first positional; a usage line that lists the reference
    // first would lead a reader to swap the two files.
    let dir = tempfile::tempdir().unwrap();
    let out = tune(dir.path())
        .args(["compare", "--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    let usage = help
        .lines()
        .find(|l| l.starts_with("Usage:"))
        .expect("usage line");
    let (ours, reference) = (usage.find("<OURS>"), usage.find("<REFERENCE"));
    assert!(
        matches!((ours, reference), (Some(o), Some(r)) if o < r),
        "{usage}"
    );
}

#[test]
fn analyze_by_ref_name() {
    let dir = ref_dir();
    std::fs::write(
        dir.path().join("tune.local.toml"),
        "[refs]\nr = \"~/ref.wav\"\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        tune(dir.path())
            .env("HOME", dir.path())
            .arg("analyze")
            .args(args)
            .output()
            .unwrap()
    };
    let out = run(&["--ref", "r", "--png", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json_of(&out.stdout);
    assert_eq!(v["source"]["path"], "~/ref.wav");
    assert_eq!(v["source"]["ref_name"], "r");
    assert_eq!(v["png"], "scratch/r.png");
    assert!(dir.path().join("scratch/r.png").is_file());
    let stdout = String::from_utf8(out.stdout).unwrap();
    for d in dir_strings(dir.path()) {
        assert!(!stdout.contains(&d), "home path leaked: {stdout}");
    }
    let text = String::from_utf8(run(&["--ref", "r"]).stdout).unwrap();
    assert!(text.starts_with("r: 0:10.0, 48000 Hz, stereo"), "{text}");

    // A file analysed by path has no ref name.
    let v = analyze_json(dir.path(), &["ref.wav"]);
    assert_eq!(v["source"]["ref_name"], Value::Null);

    // Exactly one of INPUT and --ref; an unknown name lists the known ones.
    assert_eq!(run(&["ref.wav", "--ref", "r"]).status.code(), Some(2));
    assert_eq!(run(&[]).status.code(), Some(2));
    let out = run(&["--ref", "x", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    let msg = json_of(&out.stdout)["errors"][0]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(msg.contains("no reference named `x`; known: r"), "{msg}");
}

fn mini_v2() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tune-song/tests/fixtures/v2/mini.toml")
        .canonicalize()
        .unwrap()
}

#[test]
fn analyze_sections_agree_on_v1_and_v2() {
    let dir = tempfile::tempdir().unwrap();
    let v1 = analyze_json(dir.path(), &[mini().to_str().unwrap(), "--sections"]);
    let v2 = analyze_json(dir.path(), &[mini_v2().to_str().unwrap(), "--sections"]);
    assert_eq!(v1["migrated_from"], 1);
    assert!(v1["notes"][0].as_str().unwrap().contains("tune migrate"));
    assert!(v2.get("migrated_from").is_none());
    assert_eq!(v2["notes"], serde_json::json!([]));
    let (a, b) = (
        v1["sections"].as_array().unwrap(),
        v2["sections"].as_array().unwrap(),
    );
    assert_eq!(a.len(), b.len());
    assert!(!a.is_empty());
    for (x, y) in a.iter().zip(b) {
        assert_eq!(x["name"], y["name"]);
        assert_eq!(x["start_bar"], y["start_bar"]);
        assert_eq!(x["end_bar"], y["end_bar"]);
        assert_eq!(x["integrated_lufs"], y["integrated_lufs"]);
    }
}

/// A v1 song compared with a reference is read as v2 in memory: `ours`
/// says so (`migrated_from` and a note), the reference does not, and text
/// mode prints the note on stderr.
#[test]
fn compare_notes_a_v1_song_under_ours() {
    let dir = tempfile::tempdir().unwrap();
    write_sine(&dir.path().join("ref.wav"), 48_000, 2, 4, -14.0);
    let m = mini();
    let song = m.to_str().unwrap();
    let v = compare_json(dir.path(), &[song, "ref.wav"]);
    assert_eq!(v["ours"]["migrated_from"], 1);
    let notes = v["ours"]["notes"].as_array().unwrap();
    assert!(
        notes
            .iter()
            .any(|n| n.as_str().unwrap().contains("tune migrate")),
        "{notes:?}"
    );
    assert!(v["reference"].get("migrated_from").is_none(), "{v}");
    assert!(v["reference"].get("notes").is_none(), "{v}");

    let out = compare_out(dir.path(), &[song, "ref.wav"]);
    assert!(out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.lines().any(|l| l.starts_with("note: ")
            && l.contains("is a version 1 song file")
            && l.contains("tune migrate")),
        "{stderr}"
    );
}
