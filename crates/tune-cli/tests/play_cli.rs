//! `tune play` end to end. Every test uses `--device null`: no audio plays.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::Value;

fn mini() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tune-song/tests/fixtures/mini.toml")
}

struct Player {
    child: Child,
    lines: Receiver<String>,
}

impl Player {
    fn spawn(args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tune"))
            .arg("play")
            .args(args)
            .args(["--device", "null", "--json"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn tune play");
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self { child, lines }
    }

    /// The next stdout line as JSON, within `secs` seconds.
    fn event_within(&self, secs: u64) -> Value {
        match self.lines.recv_timeout(Duration::from_secs(secs)) {
            Ok(line) => serde_json::from_str(&line).unwrap_or_else(|_| panic!("not JSON: {line}")),
            Err(RecvTimeoutError::Timeout) => panic!("no event within {secs} s"),
            Err(RecvTimeoutError::Disconnected) => panic!("tune play closed stdout"),
        }
    }

    fn event(&self) -> Value {
        self.event_within(5)
    }

    fn expect(&self, name: &str) -> Value {
        let v = self.event();
        assert_eq!(v["event"], name, "unexpected event: {v}");
        v
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_exit(child: &mut Child, within: Duration) -> std::process::ExitStatus {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(start.elapsed() < within, "process did not exit in time");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn hot_reload_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("mini.toml");
    let original = std::fs::read_to_string(mini()).unwrap();
    std::fs::write(&song, &original).unwrap();

    let mut p = Player::spawn(&[song.to_str().unwrap(), "--loop", "1..4"]);
    let playing = p.expect("playing");
    assert_eq!(playing["bar"], 1);
    assert_eq!(playing["device"], "null");
    assert_eq!(playing["sample_rate"], 48_000);

    std::fs::write(&song, original.replace("bpm = 120", "bpm = 121")).unwrap();
    let reloaded = p.expect("reloaded");
    assert!(reloaded["bar"].as_u64().unwrap() >= 1);

    std::fs::write(&song, "[song\n").unwrap();
    let failed = p.expect("reload_error");
    assert_eq!(failed["errors"][0]["line"], 1);

    std::fs::write(&song, &original).unwrap();
    p.expect("reloaded");

    let pid = p.child.id().to_string();
    let status = Command::new("kill").args(["-INT", &pid]).status().unwrap();
    assert!(status.success());
    let stopped = p.event_within(2);
    assert_eq!(stopped["event"], "stopped");
    assert!(stopped["bar"].as_u64().unwrap() >= 1);
    let status = wait_exit(&mut p.child, Duration::from_secs(2));
    assert_eq!(status.code(), Some(0));
}

#[test]
fn plays_to_the_end() {
    let mut p = Player::spawn(&[mini().to_str().unwrap(), "--from-bar", "4"]);
    let playing = p.expect("playing");
    assert_eq!(playing["bar"], 4);
    assert_eq!(playing["bpm"], 120.0);
    let finished = p.event_within(4);
    assert_eq!(finished["event"], "finished");
    let status = wait_exit(&mut p.child, Duration::from_secs(2));
    assert_eq!(status.code(), Some(0));
}

#[test]
fn from_bar_past_end() {
    let out = Command::new(env!("CARGO_BIN_EXE_tune"))
        .args(["play"])
        .arg(mini())
        .args(["--from-bar", "9", "--device", "null"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("past the end"), "stderr: {err}");
}
