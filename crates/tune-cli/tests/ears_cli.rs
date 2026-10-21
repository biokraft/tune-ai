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
