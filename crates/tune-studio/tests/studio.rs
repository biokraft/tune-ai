//! The studio actor end to end: commands in, events out. A capture device
//! parks the live processor so each test pulls frames by hand (no audio is
//! ever played), and a real watcher runs on temp dirs.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tune_engine::{AudioOut, CompiledSong, LiveProcessor, RenderOptions, render};
use tune_song::parse_song;
use tune_studio::{
    Action, Command, EditLog, Event, Origin, RunTag, SongPath, Source, StudioCommand, StudioConfig,
    StudioError, StudioHandle, StudioState, set_paths,
};

const SESSION: &str = include_str!("../../tune-song/tests/fixtures/session.toml");
const MINI: &str = include_str!("../../tune-song/tests/fixtures/v2/mini.toml");
/// The longest any test waits for one thing.
// Generous: FSEvents delivery can take seconds while the machine is busy
// (e.g. under `task verify`), and a deadline only costs time on failure.
const WAIT: Duration = Duration::from_secs(10);
const RATE: u32 = 48_000;
/// Frames per `process` call when pulling, like a small device buffer.
const BLOCK: usize = 512;

type Slot = Arc<Mutex<Option<LiveProcessor>>>;

/// An audio device that parks the processor for the test to pull.
struct CaptureOut {
    slot: Slot,
}

impl AudioOut for CaptureOut {
    fn sample_rate(&mut self, wanted: u32) -> Result<u32, String> {
        Ok(wanted)
    }

    fn start(&mut self, processor: LiveProcessor) -> Result<Box<dyn Send>, String> {
        *lock(&self.slot) = Some(processor);
        Ok(Box::new(Parked(self.slot.clone())))
    }
}

/// Dropping it (the studio stopping playback) takes the processor away.
struct Parked(Slot);

impl Drop for Parked {
    fn drop(&mut self) {
        lock(&self.0).take();
    }
}

fn lock(slot: &Slot) -> std::sync::MutexGuard<'_, Option<LiveProcessor>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A studio on a temp copy of a song, subscribed, with its watcher armed.
struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    slot: Slot,
    studio: StudioHandle,
    events: Receiver<Event>,
}

fn fixture(text: &str) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("song.toml");
    fs::write(&path, text).expect("write song");
    let slot = Slot::default();
    let config = StudioConfig {
        audio: Box::new(CaptureOut { slot: slot.clone() }),
        songs_root: dir.path().to_owned(),
    };
    let studio = tune_studio::Studio::spawn(config, Some(path.clone())).expect("spawns");
    let events = studio.subscribe();
    // Let FSEvents arm before the test touches the file.
    sleep(Duration::from_millis(300));
    Fixture {
        _dir: dir,
        path,
        slot,
        studio,
        events,
    }
}

impl Fixture {
    fn call(&self, cmd: Command) -> Value {
        self.studio.call(cmd).expect("command succeeds")
    }

    /// Pulls `frames` stereo frames from the playing processor.
    fn pull(&self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0_f32; 2 * frames];
        let mut slot = lock(&self.slot);
        let processor = slot.as_mut().expect("audio is playing");
        for block in out.chunks_mut(2 * BLOCK) {
            processor.process(block);
        }
        out
    }

    /// The next event matching `pred`, skipping others.
    fn wait(&self, what: &str, mut pred: impl FnMut(&Event) -> bool) -> Event {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(e) if pred(&e) => return e,
                Ok(_) => {}
                Err(e) => panic!("no {what} within {WAIT:?}: {e}"),
            }
        }
    }

    /// The next state matching `pred`.
    fn state(&self, what: &str, mut pred: impl FnMut(&StudioState) -> bool) -> StudioState {
        match self.wait(what, |e| matches!(e, Event::State(s) if pred(s))) {
            Event::State(s) => s,
            _ => unreachable!(),
        }
    }

    /// Fails if an event matching `pred` arrives within `window`.
    fn quiet(&self, window: Duration, what: &str, mut pred: impl FnMut(&Event) -> bool) {
        let deadline = Instant::now() + window;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            if let Ok(e) = self.events.recv_timeout(left) {
                assert!(!pred(&e), "unexpected {what}: {e:?}");
            }
        }
    }

    fn disk(&self) -> String {
        fs::read_to_string(&self.path).expect("read song")
    }

    /// Waits until the file on disk satisfies `pred`.
    fn disk_until(&self, within: Duration, what: &str, pred: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + within;
        loop {
            let text = self.disk();
            if pred(&text) {
                return text;
            }
            assert!(Instant::now() < deadline, "file never {what}:\n{text}");
            sleep(Duration::from_millis(10));
        }
    }

    fn human_log_lines(&self) -> usize {
        EditLog::for_song(&self.path)
            .read_since(None)
            .expect("read log")
            .iter()
            .filter(|e| e.source == Source::Human)
            .count()
    }
}

fn play(from_bar: Option<u32>) -> Command {
    Command::Action(Action::Play { from_bar })
}

fn knob(param: &str, value: f64, commit: bool) -> Command {
    Command::Action(Action::SetParam {
        track: "acid".into(),
        param: param.into(),
        value,
        commit,
    })
}

fn acid_param(param: &str) -> SongPath {
    SongPath::TrackParam {
        track: "acid".into(),
        param: param.into(),
    }
}

/// The offline render of bars `start..end` of `text`, interleaved stereo.
fn render_bars(text: &str, start: u32, end: u32) -> Vec<f32> {
    let song = parse_song(text).expect("valid song");
    let opts = RenderOptions {
        start_bar: start,
        end_bar: end,
        stems: false,
    };
    render(Arc::new(CompiledSong::new(&song, RATE)), &opts)
        .expect("renders")
        .mix
}

/// The param value the file sets on the acid track.
fn acid_value(text: &str, param: &str) -> f64 {
    let local = param.strip_prefix("acid303.").expect("an acid303 param");
    parse_song(text).expect("valid song").tracks["acid"]
        .instrument
        .params[local]
}

/// `mini`, with the acid also playing in section `a` (bars 0..2), where no
/// lane drives the cutoff: a knob there is both audible and writable.
fn acid_in_a() -> String {
    let from = "play = [{ at = 3, clip = \"a1\", bars = 2 }]";
    assert!(MINI.contains(from));
    MINI.replacen(
        from,
        "play = [\n  { at = 1, clip = \"a1\", bars = 2 },\n  { at = 3, clip = \"a1\", bars = 2 },\n]",
        1,
    )
}

fn with_bpm_121(text: &str) -> String {
    assert!(text.contains("bpm = 120"));
    text.replacen("bpm = 120", "bpm = 121", 1)
}

#[test]
fn subscribe_starts_with_state() {
    let f = fixture(MINI);
    match f.events.recv_timeout(WAIT).expect("first event") {
        Event::State(s) => {
            assert_eq!(s.song.expect("a song").name, "mini");
            assert!(!s.playing);
            assert_eq!(s.path.as_deref(), f.path.to_str());
        }
        other => panic!("expected State, got {other:?}"),
    }
    let second = f.studio.subscribe();
    assert!(matches!(second.recv_timeout(WAIT), Ok(Event::State(_))));
}

#[test]
fn state_reports_migrated_from_after_opening_v1() {
    let v1 = include_str!("fixtures/commented-v1.toml");
    let f = fixture(v1);
    match f.events.recv_timeout(WAIT).expect("first event") {
        Event::State(s) => {
            assert_eq!(s.migrated_from, Some(1));
            assert!(s.can_undo, "the migration is an undo step");
        }
        other => panic!("expected State, got {other:?}"),
    }
    assert_eq!(f.disk(), include_str!("fixtures/commented.toml"));
    // A v2 song has none.
    let f = fixture(MINI);
    match f.events.recv_timeout(WAIT).expect("first event") {
        Event::State(s) => assert_eq!(s.migrated_from, None),
        other => panic!("expected State, got {other:?}"),
    }
}

/// A v1 song whose folder cannot be written opens migrated in memory and
/// says why it was not saved: to a listener on subscribing, and again
/// after `get_state`, for a transport that drops the first events (Tauri
/// does) and asks for the state instead.
#[cfg(unix)]
#[test]
fn get_state_repeats_a_migration_that_could_not_be_saved() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("song.toml");
    fs::write(&path, include_str!("fixtures/commented-v1.toml")).expect("write song");
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let config = StudioConfig {
        audio: Box::new(CaptureOut {
            slot: Slot::default(),
        }),
        songs_root: dir.path().to_owned(),
    };
    let studio = tune_studio::Studio::spawn(config, Some(path.clone()));
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let studio = studio.expect("opens");
    let events = studio.subscribe();
    let next = || events.recv_timeout(WAIT).expect("an event");
    assert!(matches!(next(), Event::State(s) if s.migrated_from.is_none()));
    let unsaved = |e: Event| match e {
        Event::Error { message } => assert!(
            message.starts_with(
                "the song was upgraded to the new file format in memory but not saved: "
            ),
            "{message}"
        ),
        other => panic!("expected the migration error, got {other:?}"),
    };
    unsaved(next());
    studio
        .call(Command::Studio(StudioCommand::GetState))
        .expect("get_state");
    unsaved(next());
}

#[test]
fn get_state_returns_current_state_and_changes_nothing() {
    let f = fixture(MINI);
    f.call(Command::Action(Action::SeekBar { bar: 1 }));
    f.call(Command::Action(Action::SetLoop { bars: Some([0, 2]) }));
    let shown = f.state("loop state", |s| s.loop_bars == Some([0, 2]));
    let before = f.disk();
    let got = f.call(Command::Studio(StudioCommand::GetState));
    assert_eq!(got, serde_json::to_value(&shown).expect("state to JSON"));
    assert_eq!(got["cursor_bar"], json!(1));
    assert_eq!(got["song"]["name"], json!("mini"));
    // Nothing goes out and nothing is written.
    f.quiet(Duration::from_millis(300), "event", |_| true);
    assert_eq!(f.disk(), before);
    assert!(
        !f.studio
            .call(Command::Studio(StudioCommand::GetState))
            .unwrap()["can_undo"]
            .as_bool()
            .unwrap()
    );
}

/// A song file that does not parse at launch still starts the studio: the
/// state names the file and its errors (the app shows them), and fixing the
/// file opens it.
#[test]
fn broken_song_at_launch_opens_once_fixed() {
    let broken = MINI.replacen("bpm = 120", "bpm = ", 1);
    assert_ne!(broken, MINI);
    let f = fixture(&broken);
    let s = f.state("first state", |_| true);
    assert_eq!(s.path.as_deref(), f.path.to_str());
    assert!(s.song.is_none());
    assert!(!s.file_errors.is_empty());
    assert!(s.file_errors[0].line.is_some(), "{:?}", s.file_errors);
    assert!(matches!(
        f.studio.call(play(None)),
        Err(StudioError::NoSong)
    ));

    fs::write(&f.path, MINI).unwrap();
    let s = f.state("opened state", |s| s.song.is_some());
    assert_eq!(s.song.expect("a song").name, "mini");
    assert!(s.file_errors.is_empty());
    assert_eq!(s.path.as_deref(), f.path.to_str());
    assert_eq!(f.call(play(Some(0))), Value::Null);
}

#[test]
fn play_emits_playhead() {
    let f = fixture(MINI);
    assert_eq!(f.call(play(Some(0))), Value::Null);
    f.state("playing state", |s| s.playing);
    f.pull(48_000);
    f.wait("playhead at step 8", |e| {
        matches!(
            e,
            Event::Playhead {
                step: 8,
                bar: 0,
                beat: 2,
                playing: true
            }
        )
    });
}

#[test]
fn knob_drag_is_live_only() {
    let f = fixture(MINI);
    // Section `b` plays the acid.
    f.call(play(Some(2)));
    let reference = render_bars(MINI, 2, 3);
    let before = f.pull(4800);
    assert!(before == reference[..9600], "capture matches render");

    assert_eq!(f.call(knob("acid303.cutoff", 2394.0, false)), Value::Null);
    sleep(Duration::from_millis(300));
    assert_eq!(f.disk(), MINI, "a drag writes nothing");
    assert_eq!(f.human_log_lines(), 0);

    let after = f.pull(4800);
    assert!(after != reference[9600..19_200], "the drag is heard");
}

#[test]
fn release_commits() {
    let f = fixture(MINI);
    let start = Instant::now();
    assert_eq!(f.call(knob("acid303.cutoff", 823.456, true)), Value::Null);
    let text = f.disk_until(Duration::from_millis(500), "held 823.5", |t| {
        parse_song(t).is_ok_and(|s| s.tracks["acid"].instrument.params["cutoff"] == 823.5)
    });
    assert!(start.elapsed() <= Duration::from_millis(500));
    assert_eq!(acid_value(&text, "acid303.resonance"), 70.0);
    match f.wait("applied", |e| matches!(e, Event::Applied { .. })) {
        Event::Applied {
            source,
            changes,
            run,
        } => {
            assert_eq!(source, Source::Human);
            assert_eq!(run, None);
            assert_eq!(changes.len(), 1);
            assert_eq!(changes[0].path, acid_param("acid303.cutoff"));
            assert_eq!(changes[0].new, json!(823.5));
        }
        _ => unreachable!(),
    }
    // The log line follows the write.
    let deadline = Instant::now() + WAIT;
    while f.human_log_lines() == 0 && Instant::now() < deadline {
        sleep(Duration::from_millis(10));
    }
    assert_eq!(f.human_log_lines(), 1);
}

/// "Undo this run" from the UI is written before the call returns, as the
/// producer's one undo step, and its result is the run undo.
#[test]
fn undo_run_from_the_ui_writes_at_once() {
    let f = fixture(MINI);
    let run = RunTag::new("r1", None).unwrap();
    let bpm = Action::SetValue {
        path: SongPath::Bpm,
        value: json!(130),
    };
    f.studio
        .run_actions(vec![bpm], Origin::claude(Some(run)))
        .unwrap();
    let out = f.call(Command::Action(Action::UndoRun { run: "r1".into() }));
    assert_eq!(
        out["reverted"],
        json!([{ "target": "song.bpm", "item": null }])
    );
    assert_eq!(f.disk(), MINI);
    match f.wait("applied", |e| {
        matches!(
            e,
            Event::Applied {
                source: Source::Human,
                ..
            }
        )
    }) {
        Event::Applied { changes, run, .. } => {
            assert_eq!(run, None);
            assert_eq!(changes[0].path, SongPath::Bpm);
        }
        _ => unreachable!(),
    }
    assert_eq!(f.human_log_lines(), 1);
}

#[test]
fn external_edit_hot_reloads() {
    let f = fixture(MINI);
    f.call(play(Some(0)));
    fs::write(&f.path, with_bpm_121(MINI)).unwrap();
    match f.wait("external change", |e| matches!(e, Event::ExternalChange(_))) {
        Event::ExternalChange(ext) => {
            assert_eq!(ext.source, Source::External);
            assert_eq!(ext.changes.len(), 1);
            assert_eq!(ext.changes[0].path, SongPath::Bpm);
        }
        _ => unreachable!(),
    }
    let s = f.state("state at 121 BPM", |s| {
        s.song.as_ref().is_some_and(|v| v.bpm == 121.0)
    });
    assert!(s.playing, "a reload keeps playing");
    // The swap lands at the next step and playback runs on.
    f.pull(12_000);
}

#[test]
fn own_write_is_quiet() {
    let f = fixture(MINI);
    f.call(knob("acid303.cutoff", 820.0, true));
    f.disk_until(WAIT, "held 820", |t| t.contains("\"acid303.cutoff\" = 820"));
    f.quiet(Duration::from_secs(1), "external change", |e| {
        matches!(e, Event::ExternalChange(_))
    });
}

#[test]
fn automated_release_writes_nothing() {
    let f = fixture(MINI);
    // Section `b` has a cutoff lane.
    f.call(Command::Action(Action::SeekBar { bar: 2 }));
    assert_eq!(
        f.call(knob("acid303.cutoff", 900.0, true)),
        json!({ "automated": true })
    );
    sleep(Duration::from_millis(300));
    assert_eq!(f.disk(), MINI);
    assert_eq!(f.human_log_lines(), 0);
}

/// Review Focus 1: Claude saves while the producer holds a knob. The drag
/// stays audible through the reload and the release lands on the new file.
#[test]
fn external_edit_during_drag_keeps_gesture() {
    let song = acid_in_a();
    let f = fixture(&song);
    f.call(play(Some(0)));
    let reference = render_bars(&song, 0, 1);
    assert!(f.pull(4800) == reference[..9600], "capture matches render");

    f.call(knob("acid303.cutoff", 2000.0, false));
    assert!(f.pull(4800) != reference[9600..19_200], "the drag is heard");

    set_paths(
        &f.path,
        &[(acid_param("acid303.resonance"), json!(90))],
        Source::Claude,
    )
    .unwrap();
    match f.wait("Claude's change", |e| matches!(e, Event::ExternalChange(_))) {
        Event::ExternalChange(ext) => {
            assert_eq!(ext.source, Source::Claude);
            assert_eq!(ext.changes[0].path, acid_param("acid303.resonance"));
        }
        _ => unreachable!(),
    }
    let claude_text = f.disk();
    assert_eq!(acid_value(&claude_text, "acid303.cutoff"), 500.0);

    // Let the reload land, then restart the bar so instrument state is
    // fresh on both sides: only the knob can make them differ.
    f.pull(12_000);
    sleep(Duration::from_millis(100));
    f.call(Command::Action(Action::SeekBar { bar: 0 }));
    let at_file_value = render_bars(&claude_text, 0, 1);
    assert!(
        f.pull(9600) != at_file_value[..19_200],
        "the drag is still heard after the reload"
    );

    // Release at bar 0, where nothing automates the cutoff.
    assert_eq!(f.call(knob("acid303.cutoff", 2000.0, true)), Value::Null);
    let text = f.disk_until(WAIT, "held both edits", |t| {
        parse_song(t).is_ok_and(|s| s.tracks["acid"].instrument.params["cutoff"] == 2000.0)
    });
    assert_eq!(acid_value(&text, "acid303.resonance"), 90.0);

    // Control: with the knob in the file and the override cleared, the
    // capture is the render of the file again, so the checks above compare
    // like with like.
    f.call(Command::Action(Action::SeekBar { bar: 0 }));
    assert!(
        f.pull(9600) == render_bars(&text, 0, 1)[..19_200],
        "after the release the file is what plays"
    );
}

/// Tauri runs calls concurrently, so a throttled live value can land after
/// the release. It must not re-arm the drag: the override would stick.
#[test]
fn live_value_after_release_is_dropped() {
    let song = acid_in_a();
    let f = fixture(&song);
    f.call(play(Some(0)));
    f.pull(4800);

    f.call(knob("acid303.cutoff", 2000.0, false));
    f.call(knob("acid303.cutoff", 2000.0, true));
    // The stale live value from the same gesture, arriving late.
    assert_eq!(f.call(knob("acid303.cutoff", 300.0, false)), Value::Null);
    let text = f.disk_until(WAIT, "held 2000", |t| {
        parse_song(t).is_ok_and(|s| s.tracks["acid"].instrument.params["cutoff"] == 2000.0)
    });

    // Let the release's swap land, then compare a fresh bar with the file.
    f.pull(12_000);
    sleep(Duration::from_millis(150));
    f.call(Command::Action(Action::SeekBar { bar: 0 }));
    assert!(
        f.pull(9600) == render_bars(&text, 0, 1)[..19_200],
        "the file plays, not the stale live value"
    );

    // A new gesture after the guard is heard again.
    sleep(Duration::from_millis(250));
    f.call(knob("acid303.cutoff", 300.0, false));
    f.pull(4800);
    sleep(Duration::from_millis(100));
    f.call(Command::Action(Action::SeekBar { bar: 0 }));
    assert!(
        f.pull(9600) != render_bars(&text, 0, 1)[..19_200],
        "a later drag is heard"
    );
}

/// A drag sent while a reload is on its way names the old song's tracks, so
/// the engine drops it; the studio sends it again once the reload lands.
#[test]
fn drag_during_reload_is_resent() {
    let song = acid_in_a();
    let f = fixture(&song);
    f.call(play(Some(0)));
    f.pull(4800);

    let reloaded = song.replacen(
        "\"acid303.resonance\" = 70",
        "\"acid303.resonance\" = 90",
        1,
    );
    assert_ne!(reloaded, song);
    fs::write(&f.path, &reloaded).unwrap();
    // The swap is sent but has not landed: no frames pulled since.
    f.wait("external change", |e| matches!(e, Event::ExternalChange(_)));
    f.call(knob("acid303.cutoff", 2000.0, false));

    // The swap lands at the next step; the drag sent before it was dropped.
    f.pull(12_000);
    sleep(Duration::from_millis(150));
    f.call(Command::Action(Action::SeekBar { bar: 0 }));
    assert!(
        f.pull(9600) != render_bars(&reloaded, 0, 1)[..19_200],
        "the drag is heard after the reload"
    );
}

/// Ruling 1: an external change that a flush (here, inside undo) merges
/// before the watcher reports it is still announced.
#[test]
fn unreported_external_change_is_emitted() {
    let f = fixture(MINI);
    f.call(knob("acid303.cutoff", 820.0, true));
    fs::write(&f.path, with_bpm_121(MINI)).unwrap();
    // Undo flushes at once, long before the watcher's debounce ends.
    f.call(Command::Action(Action::Undo));
    match f.wait("external change", |e| matches!(e, Event::ExternalChange(_))) {
        Event::ExternalChange(ext) => {
            assert_eq!(ext.source, Source::External);
            assert_eq!(ext.changes[0].path, SongPath::Bpm);
        }
        _ => unreachable!(),
    }
    let s = f.state("state at 121 BPM", |s| {
        s.song.as_ref().is_some_and(|v| v.bpm == 121.0)
    });
    assert!(s.can_undo);
    // The watcher then sees only the studio's own writes.
    f.quiet(Duration::from_secs(1), "second external change", |e| {
        matches!(e, Event::ExternalChange(_))
    });
}

/// Review Focus 3: deleting the section under the playhead and the loop.
#[test]
fn delete_section_under_playhead() {
    let f = fixture(MINI);
    f.call(Command::Action(Action::SetLoop { bars: Some([2, 4]) }));
    f.call(play(Some(3)));
    f.pull(4800);
    f.call(Command::Action(Action::DeleteSection { index: 1 }));
    let s = f.state("state of the 2-bar song", |s| {
        s.song.as_ref().is_some_and(|v| v.total_bars == 2)
    });
    assert_eq!(s.loop_bars, None);
    assert!(s.cursor_bar < 2, "cursor {}", s.cursor_bar);
    f.pull(200_000);
    // Past the new end, playback finishes and stops cleanly.
    let s = f.state("stopped state", |s| !s.playing);
    assert!(s.cursor_bar < 2, "cursor {}", s.cursor_bar);
}

#[test]
fn stop_keeps_cursor() {
    let f = fixture(MINI);
    f.call(play(Some(0)));
    f.state("playing state", |s| s.playing);
    f.pull(96_000);
    f.call(Command::Action(Action::Stop));
    let s = f.state("stopped state", |s| !s.playing);
    assert_eq!(s.cursor_bar, 1);
    f.call(play(None));
    match f.wait("playhead", |e| matches!(e, Event::Playhead { .. })) {
        Event::Playhead {
            bar, step, playing, ..
        } => {
            assert_eq!(bar, 1);
            assert_eq!(step, 16);
            assert!(playing);
        }
        _ => unreachable!(),
    }
}

#[test]
fn list_and_open() {
    let root = tempfile::tempdir().unwrap();
    for name in ["b", "a"] {
        let dir = root.path().join("songs").join(name);
        fs::create_dir_all(&dir).unwrap();
        let text = MINI.replacen("name = \"mini\"", &format!("name = \"{name}\""), 1);
        fs::write(dir.join("song.toml"), text).unwrap();
    }
    // Neither is a song.
    fs::create_dir_all(root.path().join("songs/notes")).unwrap();
    fs::write(root.path().join("songs/readme.txt"), "hi").unwrap();

    let config = StudioConfig {
        audio: Box::new(CaptureOut {
            slot: Slot::default(),
        }),
        songs_root: root.path().to_owned(),
    };
    let studio = tune_studio::Studio::spawn(config, None).unwrap();
    let events = studio.subscribe();
    match events.recv_timeout(WAIT) {
        Ok(Event::State(s)) => assert!(s.path.is_none() && s.song.is_none()),
        other => panic!("expected State, got {other:?}"),
    }
    assert!(matches!(studio.call(play(None)), Err(StudioError::NoSong)));
    assert_eq!(
        studio
            .call(Command::Studio(StudioCommand::ListSongs))
            .unwrap(),
        json!(["songs/a/song.toml", "songs/b/song.toml"])
    );
    let open = Command::Studio(StudioCommand::OpenSong {
        path: "songs/b/song.toml".into(),
    });
    assert_eq!(studio.call(open).unwrap(), Value::Null);
    let deadline = Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left).expect("state for song b") {
            Event::State(s) if s.path.is_some() => {
                let path = s.path.unwrap();
                assert!(Path::new(&path).ends_with("songs/b/song.toml"), "{path}");
                assert_eq!(s.song.unwrap().name, "b");
                assert!(!s.playing);
                break;
            }
            _ => {}
        }
    }
}

#[test]
fn shutdown_writes_pending_edits() {
    let f = fixture(MINI);
    f.call(knob("acid303.cutoff", 820.0, true));
    f.studio.shutdown();
    assert!(f.disk().contains("\"acid303.cutoff\" = 820"));
    assert!(matches!(
        f.studio.call(Command::Action(Action::Stop)),
        Err(StudioError::Closed)
    ));
}

/// Starts the processor once, then refuses: a device that went away
/// between two plays.
struct OnceOut {
    slot: Slot,
    started: bool,
}

impl AudioOut for OnceOut {
    fn sample_rate(&mut self, wanted: u32) -> Result<u32, String> {
        Ok(wanted)
    }

    fn start(&mut self, processor: LiveProcessor) -> Result<Box<dyn Send>, String> {
        if self.started {
            return Err("the device is gone".into());
        }
        self.started = true;
        *lock(&self.slot) = Some(processor);
        Ok(Box::new(Parked(self.slot.clone())))
    }
}

/// A failed play stops the old playback, and every client hears of it.
#[test]
fn failed_play_publishes_stopped_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.toml");
    fs::write(&path, MINI).unwrap();
    let config = StudioConfig {
        audio: Box::new(OnceOut {
            slot: Slot::default(),
            started: false,
        }),
        songs_root: dir.path().to_owned(),
    };
    let studio = tune_studio::Studio::spawn(config, Some(path)).unwrap();
    let events = studio.subscribe();
    studio.call(play(Some(0))).unwrap();
    assert!(matches!(
        studio.call(play(Some(1))),
        Err(StudioError::Audio(_))
    ));
    let mut playing = None;
    while let Ok(e) = events.recv_timeout(Duration::from_millis(200)) {
        if let Event::State(s) = e {
            playing = Some(s.playing);
        }
    }
    assert_eq!(playing, Some(false), "the last state says stopped");
}

/// An undo that changes no value still moves the history (here: a save
/// that only added a comment), so the state goes out.
#[test]
fn comment_only_undo_publishes_state() {
    let f = fixture(MINI);
    let commented = format!("# a note\n{MINI}");
    fs::write(&f.path, &commented).unwrap();
    match f.wait("external change", |e| matches!(e, Event::ExternalChange(_))) {
        Event::ExternalChange(ext) => assert!(ext.changes.is_empty()),
        _ => unreachable!(),
    }
    f.state("undoable state", |s| s.can_undo && !s.can_redo);
    assert_eq!(f.call(Command::Action(Action::Undo)), json!([]));
    f.state("redoable state", |s| s.can_redo);
    assert_eq!(f.disk(), MINI);
}

/// Fills the engine's command queue (capacity 16) while no frames are
/// pulled, so the next swap or loop cannot be sent at once.
fn flood_queue(f: &Fixture) {
    for i in 0..20 {
        f.call(knob("acid303.cutoff", 1000.0 + f64::from(i), false));
    }
}

/// A loop and a song change refused by a full queue are sent once the
/// queue drains, the loop after the swap.
#[test]
fn loop_survives_a_full_queue() {
    let f = fixture(MINI);
    f.call(play(Some(0)));
    flood_queue(&f);
    f.call(Command::Action(Action::SetTrackMix {
        track: "hat".into(),
        gain_db: Some(-12.0),
        pan: None,
        mute: None,
        solo: None,
    }));
    assert_eq!(
        f.call(Command::Action(Action::SetLoop { bars: Some([0, 1]) })),
        Value::Null
    );
    f.pull(BLOCK);
    // Ticks send the swap, then the loop.
    sleep(Duration::from_millis(150));
    // Past the end of bar 0: the loop brings the playhead back.
    f.pull(96_000 + 54_000);
    match f.wait(
        "playhead past step 8",
        |e| matches!(e, Event::Playhead { step, .. } if *step >= 8),
    ) {
        Event::Playhead { bar, step, .. } => assert_eq!(bar, 0, "step {step}"),
        _ => unreachable!(),
    }
}

/// Clients name songs under the songs root only: a path that leaves it is
/// refused even when a song file is there.
#[test]
fn open_song_stays_under_root() {
    let outer = tempfile::tempdir().unwrap();
    let root = outer.path().join("root");
    fs::create_dir_all(root.join("songs/a")).unwrap();
    fs::write(root.join("songs/a/song.toml"), MINI).unwrap();
    fs::create_dir_all(outer.path().join("x")).unwrap();
    let outside = outer.path().join("x/song.toml");
    fs::write(&outside, MINI).unwrap();

    let config = StudioConfig {
        audio: Box::new(CaptureOut {
            slot: Slot::default(),
        }),
        songs_root: root.clone(),
    };
    let studio = tune_studio::Studio::spawn(config, None).unwrap();
    for path in [
        "../x/song.toml".to_owned(),
        "songs/../../x/song.toml".to_owned(),
        // Missing, and outside: still "outside", so nothing can be probed.
        "../nope/song.toml".to_owned(),
        outside.to_str().unwrap().to_owned(),
    ] {
        match studio.call(Command::Studio(StudioCommand::OpenSong {
            path: path.clone(),
        })) {
            Err(StudioError::OutsideRoot(p)) => assert_eq!(p, path),
            other => panic!("{path}: expected OutsideRoot, got {other:?}"),
        }
    }
    assert!(matches!(
        studio.call(Command::Studio(StudioCommand::OpenSong {
            path: "songs/nope/song.toml".into()
        })),
        Err(StudioError::Io(_))
    ));
    // Inside the root, relative or absolute, is fine.
    let inside = root.join("songs/a/song.toml");
    for path in [
        "songs/a/song.toml",
        "songs/../songs/a/song.toml",
        inside.to_str().unwrap(),
    ] {
        let open = Command::Studio(StudioCommand::OpenSong { path: path.into() });
        assert_eq!(studio.call(open).unwrap(), Value::Null, "{path}");
    }
}

/// Every `OpenSong` starts a file watcher, and on macOS registering its
/// FSEvents stream takes time in proportion to the number of entries in
/// the directory of the running executable (measured: about 0.3 s per
/// stream with 200k entries on an idle machine, seconds under load). Test
/// binaries run from `target/debug/deps`, where the default unpacked split
/// debuginfo leaves every codegen unit's `.rcgu.o` behind and incremental
/// rebuilds never remove them: the directory grew past 200k entries and
/// `OpenSong` took seconds, past the call timeout under load. The workspace builds with packed debuginfo so
/// the objects are deleted after linking; this keeps the directory small.
#[cfg(target_os = "macos")]
#[test]
fn test_binary_directory_stays_small() {
    // Stale objects pile up by the hundreds per rebuild; a concurrent
    // build's transient files stay far below this.
    const MAX_ENTRIES: usize = 20_000;
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap();
    let entries = fs::read_dir(dir).unwrap().count();
    assert!(
        entries <= MAX_ENTRIES,
        "{} holds {entries} entries (at most {MAX_ENTRIES}): every watcher \
         start pays for them. Check `split-debuginfo = \"packed\"` in the \
         workspace Cargo.toml and delete the stale `*.rcgu.o` files there.",
        dir.display()
    );
}

fn mix_knob(param: &str, value: f64, commit: bool) -> Command {
    Command::Action(Action::SetParam {
        track: "mix".into(),
        param: param.into(),
        value,
        commit,
    })
}

/// A mix knob is a live override like a track knob: heard while held,
/// written on release (creating `[mix]`), and the override ends there.
#[test]
fn drag_mix_knob_overrides_live() {
    let f = fixture(MINI);
    f.call(play(Some(0)));
    let reference = render_bars(MINI, 0, 1);
    assert!(f.pull(4800) == reference[..9600], "capture matches render");

    assert_eq!(f.call(mix_knob("master.gain", -12.0, false)), Value::Null);
    assert!(f.pull(4800) != reference[9600..19_200], "the drag is heard");
    sleep(Duration::from_millis(300));
    assert_eq!(f.disk(), MINI, "a drag writes nothing");
    assert_eq!(f.human_log_lines(), 0);

    // Released at another value than the drag's, so the capture can tell
    // a lingering override from the file.
    assert_eq!(f.call(mix_knob("master.gain", -6.0, true)), Value::Null);
    let text = f.disk_until(WAIT, "held master.gain", |t| {
        parse_song(t).is_ok_and(|s| s.mix.params.get("master.gain") == Some(&-6.0))
    });
    assert!(
        text.contains("[mix]\nparams = { \"master.gain\" = -6 }\n"),
        "{text}"
    );
    match f.wait("applied", |e| matches!(e, Event::Applied { .. })) {
        Event::Applied {
            source,
            changes,
            run,
        } => {
            assert_eq!(source, Source::Human);
            assert_eq!(run, None);
            assert_eq!(changes[0].path, SongPath::param("mix", "master.gain"));
            assert_eq!((&changes[0].old, &changes[0].new), (&json!(0), &json!(-6)));
        }
        _ => unreachable!(),
    }

    // Let the release's swap land, then compare a fresh bar with the file.
    f.pull(12_000);
    sleep(Duration::from_millis(150));
    f.call(Command::Action(Action::SeekBar { bar: 0 }));
    assert!(
        f.pull(9600) == render_bars(&text, 0, 1)[..19_200],
        "the file plays, not the dragged value"
    );
}

#[test]
fn a_new_track_plays_live_and_is_one_undo_step() {
    let f = fixture(MINI);
    f.call(play(None));
    f.pull(4 * BLOCK);
    let add = Command::Action(Action::AddTrack {
        track: "bass".into(),
        instrument: "acid303".into(),
    });
    assert_eq!(f.call(add), Value::Null);
    let text = f.disk_until(WAIT, "gained the track", |t| t.contains("[tracks.bass]"));
    assert!(parse_song(&text).is_ok_and(|s| s.tracks.contains_key("bass")));
    // The swap reached the player, which keeps producing frames.
    f.pull(4 * BLOCK);
    let undone = f.call(Command::Action(Action::Undo));
    assert_eq!(undone[0]["path"], json!("tracks.bass"));
    f.disk_until(WAIT, "lost the track", |t| !t.contains("bass"));
    assert_eq!(f.disk(), MINI);
}

/// A scene move changes no value the `Song` comparison can see, yet the
/// engine's scene order must follow it: the studio swaps on the write.
#[test]
fn a_scene_move_reaches_the_playing_engine() {
    let f = fixture(SESSION);
    f.call(play(Some(0)));
    f.pull(2_000);
    let swaps = || {
        lock(&f.slot)
            .as_ref()
            .expect("audio is playing")
            .swaps_applied()
    };
    let before = swaps();
    let move_break = Action::MoveScene {
        scene: "break".into(),
        index: 0,
    };
    f.studio
        .run_actions(vec![move_break], Origin::claude(None))
        .expect("moves");
    // The swap lands at the next step.
    f.pull(12_000);
    assert_eq!(swaps(), before + 1, "the move was swapped in");
    let moved = parse_song(&f.disk()).expect("parses");
    assert_eq!(moved.scenes.keys().collect::<Vec<_>>(), ["break", "groove"]);
}
