//! Spec §11 exits 1, 3 and 4, end to end: a song built only with `tune do`, the
//! running studio hot-reloading a CLI edit without a click, and an agent run
//! undone while a later human edit stays. The studio runs in the test process
//! on a capture device (no audio is ever played) with its control endpoint on
//! a temp `TUNE_STUDIO_DIR`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::TempDir;
use tune_engine::{AudioOut, CompiledSong, LiveProcessor, RenderOptions, render};
use tune_song::parse_song;
use tune_studio::{
    Action, Command as StudioCall, ControlServer, Event, Studio, StudioConfig, StudioHandle,
};

const START: &str = "[song]\nversion = 2\nname = \"built\"\nbpm = 120\n\n\
                     [[sections]]\nname = \"main\"\nbars = 4\n";
const RATE: usize = 48_000;
/// Frames per `process` call when pulling, like a small device buffer.
const BLOCK: usize = 512;
const WAIT: Duration = Duration::from_secs(10);

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

struct Env {
    dir: TempDir,
    studio: TempDir,
    song: PathBuf,
}

fn env_with(text: &str) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.toml");
    fs::write(&song, text).unwrap();
    Env {
        dir,
        studio: tempfile::tempdir().unwrap(),
        song,
    }
}

impl Env {
    fn tune(&self) -> Command {
        let mut c = Command::cargo_bin("tune").unwrap();
        c.env("TUNE_STUDIO_DIR", self.studio.path())
            .env_remove("TUNE_RUN")
            .env_remove("TUNE_RUN_TITLE");
        c
    }

    /// `tune do <song> <args…>` with `--json`; asserts success.
    fn do_json(&self, args: &[&str]) -> Value {
        let out = self
            .tune()
            .args(["do", self.song.to_str().unwrap()])
            .args(args)
            .arg("--json")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "tune do {args:?}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn text(&self) -> String {
        fs::read_to_string(&self.song).unwrap()
    }
}

fn json_out(cmd: &mut Command) -> Value {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// A studio on the env's song with a capture device and a control endpoint.
struct Running {
    handle: StudioHandle,
    events: Receiver<Event>,
    slot: Slot,
    _control: ControlServer,
}

fn start_studio(e: &Env) -> Running {
    let slot = Slot::default();
    let handle = Studio::spawn(
        StudioConfig {
            audio: Box::new(CaptureOut { slot: slot.clone() }),
            songs_root: e.dir.path().to_owned(),
        },
        Some(e.song.clone()),
    )
    .unwrap();
    let events = handle.subscribe();
    let control = ControlServer::start(handle.clone(), e.studio.path()).unwrap();
    // Let the file watcher arm before anything touches the file.
    thread::sleep(Duration::from_millis(300));
    Running {
        handle,
        events,
        slot,
        _control: control,
    }
}

impl Running {
    /// Pulls `frames` stereo frames from the playing processor, interleaved.
    fn pull(&self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0_f32; 2 * frames];
        let mut slot = lock(&self.slot);
        let processor = slot.as_mut().expect("audio is playing");
        for block in out.chunks_mut(2 * BLOCK) {
            processor.process(block);
        }
        out
    }

    /// Like [`Running::pull`], keeping the left channel.
    fn pull_left(&self, frames: usize) -> Vec<f32> {
        self.pull(frames).chunks(2).map(|f| f[0]).collect()
    }
}

/// The largest sample-to-sample step of a signal.
fn max_jump(x: &[f32]) -> f32 {
    x.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

fn peak_dbfs(wav: &Path) -> f32 {
    let mut reader = hound::WavReader::open(wav).unwrap();
    let peak = match reader.spec().sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.unwrap().abs())
            .fold(0.0, f32::max),
        hound::SampleFormat::Int => {
            let scale = (1_i64 << (reader.spec().bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap().abs() as f32 / scale)
                .fold(0.0, f32::max)
        }
    };
    20.0 * peak.log10()
}

/// Spec §11 exit 1, studio closed: a whole song, only through `tune do`.
#[test]
fn a_song_built_only_with_tune_do() {
    let e = env_with(START);
    let steps = [
        &["add_track", "acid", "instrument=acid303"][..],
        &["add_effect", "acid", "module=eq"],
        &["add_effect", "acid", "module=eq"],
        &["add_effect", "track=acid", "module=distortion", "index=1"],
        &["set_value", "path=tracks.acid.eq2.low_cut", "value=200"],
        &[
            "add_clip",
            "bass",
            "kind=acid",
            "steps=C2 C2! C3~ C2 . D#2 C2~ G2!",
        ],
        &[
            "add_clip",
            "lead",
            "kind=notes",
            "bars=1",
            "notes=[{ start = 0, pitch = \"C3\", len = 4 }, \
             { start = 8, pitch = \"G3\", len = 4, vel = 120 }]",
        ],
        &["add_track", "kick", "instrument=kick"],
        &["add_clip", "k4", "kind=drum", "steps=x...x...x...X..."],
        &["add_placement", "acid", "clip=bass", "at=1", "bars=2"],
        &["add_placement", "acid", "clip=lead", "at=3", "bars=2"],
        &["add_placement", "kick", "clip=k4", "at=1", "bars=4"],
    ];
    for args in steps {
        let v = e.do_json(args);
        assert_eq!(v["ok"], true, "{args:?}: {v}");
        assert_eq!(v["via"], "file", "{args:?}: {v}");
    }

    let v = json_out(
        e.tune()
            .args(["validate", e.song.to_str().unwrap(), "--json"]),
    );
    assert_eq!(v["ok"], true, "{v}");

    let info = json_out(e.tune().args(["info", e.song.to_str().unwrap(), "--json"]));
    let ids = |key: &str| -> Vec<String> {
        info[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(ids("tracks"), ["acid", "kick"]);
    let mut clips = ids("clips");
    clips.sort_unstable();
    assert_eq!(clips, ["bass", "k4", "lead"]);

    let song = parse_song(&e.text()).unwrap();
    let fx: Vec<&str> = song.tracks["acid"]
        .fx
        .iter()
        .map(|d| d.id.as_str())
        .collect();
    assert_eq!(fx, ["eq", "dist", "eq2"]);

    let stems = e.dir.path().join("stems");
    let v = json_out(e.tune().args([
        "render",
        e.song.to_str().unwrap(),
        "--stems",
        "--json",
        "--out",
        stems.to_str().unwrap(),
    ]));
    let files: Vec<&str> = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    for track in ["acid", "kick"] {
        let file = files
            .iter()
            .find(|f| Path::new(f).file_name().unwrap() == format!("{track}.wav").as_str())
            .unwrap_or_else(|| panic!("no {track} stem in {files:?}"));
        let peak = peak_dbfs(Path::new(file));
        assert!(peak > -60.0, "{track} stem peak {peak} dBFS");
    }
}

/// Spec §11 exit 1, studio open: a CLI edit is heard as a crossfaded swap,
/// with no click and no error.
#[test]
fn the_running_studio_hot_reloads_without_a_click() {
    let e = env_with(START);
    for args in [
        &["add_track", "acid", "instrument=acid303"][..],
        &[
            "add_clip",
            "lead",
            "kind=notes",
            "bars=4",
            "notes=[{ start = 0, pitch = \"C2\", len = 64 }]",
        ],
        &["add_placement", "acid", "clip=lead", "at=1", "bars=4"],
    ] {
        e.do_json(args);
    }
    let r = start_studio(&e);

    let v = json_out(
        e.tune()
            .args(["do", e.song.to_str().unwrap(), "play", "--json"]),
    );
    assert_eq!(v["via"], "studio");

    // One second of playing, then the batch from another thread while the
    // test keeps pulling.
    let mut left = r.pull_left(RATE);
    let call_at = left.len();
    let batch = json!([
        {"cmd": "add_effect", "args": {"track": "acid", "module": "eq", "index": 0}},
        {"cmd": "set_value", "args": {"path": "tracks.acid.eq.high_cut", "value": 2000}},
    ]);
    let mut cmd = e.tune();
    cmd.args(["do", e.song.to_str().unwrap(), "--batch", "--json"])
        .write_stdin(batch.to_string());
    let caller = thread::spawn(move || cmd.output().unwrap());

    let deadline = Instant::now() + WAIT;
    let mut applied = false;
    let mut errors = Vec::new();
    while !applied {
        assert!(Instant::now() < deadline, "no applied event");
        left.extend(r.pull_left(BLOCK));
        thread::sleep(Duration::from_millis(5));
        while let Ok(event) = r.events.try_recv() {
            match event {
                Event::Applied { .. } => applied = true,
                Event::Error { .. } => errors.push(format!("{event:?}")),
                _ => {}
            }
        }
    }
    // 200 ms more, covering the whole crossfade.
    left.extend(r.pull_left(RATE / 5));
    while let Ok(event) = r.events.try_recv() {
        if matches!(event, Event::Error { .. }) {
            errors.push(format!("{event:?}"));
        }
    }

    let out = caller.join().unwrap();
    assert!(out.status.success(), "{out:?}");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["via"], "studio", "{v}");
    assert!(errors.is_empty(), "error events: {errors:?}");

    let before = max_jump(&left[call_at - RATE / 10..call_at]);
    let after = max_jump(&left[call_at..]);
    let head = rms(&left[call_at - RATE / 10..call_at]);
    assert!(
        head > 1e-3,
        "the 100 ms before the call are silent (rms {head})"
    );
    assert!(
        after <= 1.5 * before,
        "click: largest jump {after} after the call, {before} in the 100 ms before"
    );
    let tail = rms(&left[left.len() - RATE / 10..]);
    assert!(tail > 1e-3, "the last 100 ms are silent (rms {tail})");
    let song = parse_song(&e.text()).unwrap();
    let fx: Vec<&str> = song.tracks["acid"]
        .fx
        .iter()
        .map(|d| d.id.as_str())
        .collect();
    assert_eq!(fx, ["eq"]);
    r.handle.shutdown();
}

/// Spec §11 exit 4: an agent run is undone, a later human edit is kept, and
/// one undo steps back exactly the run undo.
#[test]
fn an_agent_run_is_undone_keeping_a_later_human_edit() {
    let e = env_with(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tune-studio/tests/fixtures/commented.toml"),
        )
        .unwrap(),
    );
    let r = start_studio(&e);
    for edit in [
        ["path=tracks.acid.acid303.cutoff", "value=820"],
        ["path=tracks.acid.gain_db", "value=-2"],
        ["path=song.bpm", "value=130"],
    ] {
        let v = e.do_json(
            &[
                &["set_value"][..],
                &edit[..],
                &["--run", "r1", "--run-title", "make it hit harder"],
            ]
            .concat(),
        );
        assert_eq!(v["via"], "studio", "{v}");
    }

    // A human releases the cutoff knob after the run.
    r.handle
        .call(StudioCall::Action(Action::SetParam {
            track: "acid".into(),
            param: "acid303.cutoff".into(),
            value: 700.0,
            commit: true,
        }))
        .unwrap();

    let v = e.do_json(&["undo_run", "r1"]);
    assert_eq!(v["via"], "studio", "{v}");
    let kept = &v["results"][0]["kept"];
    assert_eq!(kept.as_array().unwrap().len(), 1, "{v}");
    assert_eq!(kept[0]["target"], "tracks.acid.acid303.cutoff", "{v}");
    assert_eq!(kept[0]["by"], "human", "{v}");

    let deadline = Instant::now() + WAIT;
    let song = loop {
        let song = parse_song(&e.text()).unwrap();
        if song.bpm == 120.0 || Instant::now() > deadline {
            break song;
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(song.tracks["acid"].instrument.params["cutoff"], 700.0);
    assert_eq!(song.tracks["acid"].gain_db, 0.0);
    assert_eq!(song.bpm, 120.0);

    r.handle.call(StudioCall::Action(Action::Undo)).unwrap();
    let deadline = Instant::now() + WAIT;
    let song = loop {
        let song = parse_song(&e.text()).unwrap();
        if song.bpm == 130.0 || Instant::now() > deadline {
            break song;
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(song.tracks["acid"].gain_db, -2.0);
    assert_eq!(song.bpm, 130.0);
    assert_eq!(song.tracks["acid"].instrument.params["cutoff"], 700.0);
    r.handle.shutdown();
}

/// The session fixture, with its scenes and automation to be cut out.
const SESSION: &str = include_str!("../../tune-song/tests/fixtures/session.toml");

/// `text` without the lines from `from` up to (not including) `to`.
fn cut(text: &str, from: &str, to: Option<&str>) -> String {
    let start = text.find(from).unwrap();
    let end = to.map_or(text.len(), |t| start + text[start..].find(t).unwrap());
    format!("{}{}", &text[..start], &text[end..])
}

/// `text` with the one occurrence of `from` replaced; panics if it is absent
/// or repeated, so an edit that stops matching the fixture fails loudly.
fn replace_once(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "fixture lost {from:?}");
    text.replacen(from, to, 1)
}

/// Spec §11 exit 3: a running studio, driven by `tune do`, launches a scene
/// and a clip and goes back to the arrangement, each at a bar line, and the
/// audio equals an arrangement that has those bars written out.
#[test]
fn a_running_studio_launches_and_returns_at_bar_lines() {
    // S: the fixture without its automation, hat's second placement playing
    // the kick pattern; the engine plays it live.
    let s = cut(SESSION, "# the filter opens", None);
    let s = replace_once(
        &s,
        "play = [\n  { at = 1, clip = \"h\", bars = 2 },\n  { at = 3, clip = \"h\", bars = 2 },\n]",
        "play = [\n  { at = 1, clip = \"h\", bars = 2 },\n  { at = 3, clip = \"k4\", bars = 2 },\n]",
    );
    // R: the arrangement the session should sound like, bar by bar. Hat
    // plays the launched `h` at bar 3 and the arrangement's `k4` after the
    // return at bar 4, so a late, early or missing return is audible.
    let r = cut(&s, "# jam rows", Some("[[sections]]"));
    let r = replace_once(
        &r,
        "play = [\n  { at = 1, clip = \"k4\", bars = 2 },\n  { at = 3, clip = \"k4\", bars = 2 },\n]",
        "play = [{ at = 1, clip = \"k4\", bars = 4 }]",
    );
    let r = replace_once(
        &r,
        "play = [{ at = 3, clip = \"a1\", bars = 2 }]",
        "play = [{ at = 2, clip = \"a1\", bars = 3 }]",
    );
    let r = replace_once(
        &r,
        "play = [\n  { at = 1, clip = \"h\", bars = 2 },\n  { at = 3, clip = \"k4\", bars = 2 },\n]",
        "play = [\n  { at = 1, clip = \"h\", bars = 1 },\n  { at = 3, clip = \"h\", bars = 1 },\n  { at = 4, clip = \"k4\", bars = 1 },\n]",
    );
    let reference = {
        let song = parse_song(&r).unwrap();
        let compiled = Arc::new(CompiledSong::new(&song, 48_000));
        let opts = RenderOptions {
            start_bar: 0,
            end_bar: 4,
            stems: false,
        };
        render(compiled, &opts).unwrap().mix
    };
    assert_eq!(reference.len(), 2 * 4 * 96_000);

    let e = env_with(&s);
    let song_before = e.text();
    let log = e.dir.path().join("edits.jsonl");
    let log_before = fs::read_to_string(&log).ok();
    let r = start_studio(&e);

    let v = e.do_json(&["play"]);
    assert_eq!(v["via"], "studio", "{v}");
    // Half a bar in, a scene launched mid-bar lands at bar 1 (frame 96 000).
    let mut frames = r.pull(RATE);
    let v = e.do_json(&["launch_scene", "scene=groove"]);
    assert_eq!(v["via"], "studio", "{v}");
    assert_eq!(v["results"], json!([null]), "{v}");
    frames.extend(r.pull(2 * RATE));
    // Mid bar 1: hat's break clip lands at bar 2 (frame 192 000).
    let v = e.do_json(&["launch_clip", "track=hat", "scene=break"]);
    assert_eq!(v["via"], "studio", "{v}");
    assert_eq!(v["results"], json!([null]), "{v}");
    frames.extend(r.pull(2 * RATE));
    // Mid bar 2: every track goes back to the arrangement at bar 3.
    let v = e.do_json(&["back_to_arrangement"]);
    assert_eq!(v["via"], "studio", "{v}");
    assert_eq!(v["results"], json!([null]), "{v}");
    frames.extend(r.pull(3 * RATE));
    assert_eq!(frames.len(), reference.len());

    let first = frames.iter().zip(&reference).position(|(a, b)| a != b);
    assert!(
        first.is_none(),
        "first differing sample {first:?}, frame {:?} (bar {:?})",
        first.map(|i| i / 2),
        first.map(|i| i / 2 / 96_000)
    );
    assert!(rms(&frames[2 * 96_000..]) > 1e-3, "silent session");
    assert_eq!(e.text(), song_before, "the song file changed");
    assert_eq!(
        fs::read_to_string(&log).ok(),
        log_before,
        "edits.jsonl changed"
    );
    r.handle.shutdown();
}
