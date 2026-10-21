//! The committed songs under `songs/` stay valid, and `first-acid` keeps the
//! arrangement it was drafted with.

use assert_cmd::Command;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn tune() -> Command {
    Command::cargo_bin("tune").unwrap()
}

fn songs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../songs")
}

fn json_of(out: &[u8]) -> Value {
    serde_json::from_slice(out).expect("stdout is JSON")
}

/// Every `songs/<name>/song.toml`, sorted by name.
fn song_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(songs_dir())
        .expect("songs/ exists")
        .map(|e| e.unwrap().path().join("song.toml"))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    files
}

#[test]
fn every_song_validates() {
    let files = song_files();
    assert!(!files.is_empty(), "no songs/*/song.toml found");
    for song in files {
        let out = tune()
            .arg("validate")
            .arg(&song)
            .arg("--json")
            .output()
            .unwrap();
        let v = json_of(&out.stdout);
        assert!(
            out.status.success() && v["ok"] == true,
            "{} does not validate: {v}",
            song.display()
        );
    }
}

#[test]
fn first_acid_arrangement() {
    let song = songs_dir().join("first-acid/song.toml");
    let out = tune()
        .arg("info")
        .arg(&song)
        .arg("--json")
        .assert()
        .success();
    let v = json_of(&out.get_output().stdout);
    assert_eq!(v["bpm"], 145.0);
    assert_eq!(v["sample_rate"], 48_000);
    assert_eq!(v["bars"], 144);

    let expected: [(&str, u64, u64); 8] = [
        ("intro", 16, 1),
        ("intro_acid", 16, 17),
        ("build", 32, 33),
        ("breakdown", 16, 65),
        ("drop", 16, 81),
        ("drop_peak", 16, 97),
        ("outro", 16, 113),
        ("outro_drums", 16, 129),
    ];
    let sections = v["sections"].as_array().expect("sections array");
    let got: Vec<(&str, u64, u64)> = sections
        .iter()
        .map(|s| {
            (
                s["name"].as_str().unwrap(),
                s["bars"].as_u64().unwrap(),
                s["start_bar"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(got, expected);

    // 2304 steps at 145 BPM and 48 kHz: round(2304 * 48000 * 15 / 145) =
    // 11 440 552 frames, about 238.345 s.
    let seconds = v["seconds"].as_f64().unwrap();
    assert!((238.3..238.4).contains(&seconds), "seconds = {seconds}");
}
