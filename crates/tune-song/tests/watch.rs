//! The song watcher against real temp dirs and real timing.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread::sleep;
use std::time::Duration;

use tune_song::{RawReload, Reload, SongWatcher};

const MINI: &str = include_str!("fixtures/mini.toml");
const WAIT: Duration = Duration::from_secs(5);

struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    rx: Receiver<Reload>,
    _watcher: SongWatcher,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.toml");
    fs::write(&path, MINI).unwrap();
    let (tx, rx) = mpsc::channel();
    let watcher = SongWatcher::start(&path, tx).unwrap();
    // Let the OS watcher settle before the test touches the file.
    sleep(Duration::from_millis(300));
    Fixture {
        _dir: dir,
        path,
        rx,
        _watcher: watcher,
    }
}

fn with_bpm(bpm: u32) -> String {
    let out = MINI.replacen("bpm = 120", &format!("bpm = {bpm}"), 1);
    assert_ne!(out, MINI);
    out
}

fn expect_bpm(rx: &Receiver<Reload>, bpm: f64) {
    match rx.recv_timeout(WAIT) {
        Ok(Reload::Ok(song)) => assert_eq!(song.bpm, bpm),
        other => panic!("expected Ok reload, got {other:?}"),
    }
}

#[test]
fn valid_edit_reports_ok() {
    let f = fixture();
    fs::write(&f.path, with_bpm(121)).unwrap();
    expect_bpm(&f.rx, 121.0);
}

#[test]
fn atomic_rename_save() {
    let f = fixture();
    let tmp = f.path.with_file_name("song.toml.tmp");
    fs::write(&tmp, with_bpm(122)).unwrap();
    fs::rename(&tmp, &f.path).unwrap();
    expect_bpm(&f.rx, 122.0);
}

#[test]
fn partial_write_then_complete() {
    let f = fixture();
    let full = with_bpm(123);
    let half = full.len() / 2;
    let mut file = fs::File::create(&f.path).unwrap(); // truncates
    file.write_all(&full.as_bytes()[..half]).unwrap();
    file.flush().unwrap();
    sleep(Duration::from_millis(200));
    file.write_all(&full.as_bytes()[half..]).unwrap();
    file.flush().unwrap();
    drop(file);

    let mut invalid = 0;
    loop {
        match f.rx.recv_timeout(WAIT) {
            Ok(Reload::Invalid(_)) => invalid += 1,
            Ok(Reload::Ok(song)) => {
                assert_eq!(song.bpm, 123.0);
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(invalid <= 1, "{invalid} Invalid events before Ok");
    // Nothing trails the final Ok.
    assert!(f.rx.recv_timeout(Duration::from_millis(500)).is_err());
}

#[test]
fn identical_rewrite_ignored() {
    let f = fixture();
    fs::write(&f.path, MINI).unwrap();
    assert!(f.rx.recv_timeout(Duration::from_secs(1)).is_err());
}

#[test]
fn broken_toml_reports_invalid() {
    let f = fixture();
    fs::write(&f.path, "[song\n").unwrap();
    match f.rx.recv_timeout(WAIT) {
        Ok(Reload::Invalid(errors)) => assert_eq!(errors[0].line, Some(1)),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn drop_stops_the_watcher_thread() {
    let f = fixture();
    let Fixture {
        rx,
        _watcher,
        path,
        _dir,
    } = f;
    drop(_watcher);
    fs::write(&path, with_bpm(130)).unwrap();
    // Sender gone: the channel disconnects instead of timing out.
    assert!(matches!(
        rx.recv_timeout(WAIT),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[test]
fn raw_reports_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.toml");
    fs::write(&path, MINI).unwrap();
    let (tx, rx) = mpsc::channel();
    let _watcher = SongWatcher::start_raw(&path, tx).unwrap();
    sleep(Duration::from_millis(300));

    // Raw reports do not parse: invalid text comes through as it is.
    fs::write(&path, "[song\n").unwrap();
    match rx.recv_timeout(WAIT) {
        Ok(RawReload::Changed(text)) => assert_eq!(text, "[song\n"),
        other => panic!("expected Changed, got {other:?}"),
    }
    let text = with_bpm(121);
    fs::write(&path, &text).unwrap();
    match rx.recv_timeout(WAIT) {
        Ok(RawReload::Changed(got)) => assert_eq!(got, text),
        other => panic!("expected Changed, got {other:?}"),
    }
    // The same bytes again report nothing.
    fs::write(&path, &text).unwrap();
    assert!(rx.recv_timeout(Duration::from_secs(1)).is_err());
}
