//! The control endpoint: `tune do` reaching a running studio over a local
//! socket. Studios run on temp copies of the commented fixture with a
//! capture device (no audio plays); every endpoint directory is a temp dir.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tune_engine::{AudioOut, CompiledSong, LiveProcessor, RenderOptions, render};
use tune_song::parse_song;
use tune_studio::{
    Action, Command, ControlServer, EditLog, Event, Origin, RunTag, SongPath, Source,
    StudioCommand, StudioConfig, StudioError, StudioHandle, StudioLink, Via, find_studio,
};

const COMMENTED: &str = include_str!("fixtures/commented.toml");
/// The longest any test waits for one thing.
const WAIT: Duration = Duration::from_secs(10);
const RATE: u32 = 48_000;
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

struct Parked(Slot);

impl Drop for Parked {
    fn drop(&mut self) {
        lock(&self.0).take();
    }
}

fn lock(slot: &Slot) -> std::sync::MutexGuard<'_, Option<LiveProcessor>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A studio on a temp copy of a song, with its control endpoint in a temp
/// directory, subscribed.
struct Fixture {
    path: PathBuf,
    endpoints: tempfile::TempDir,
    slot: Slot,
    studio: StudioHandle,
    server: Option<ControlServer>,
    events: Receiver<Event>,
    _dir: tempfile::TempDir,
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
    let endpoints = tempfile::tempdir().expect("tempdir");
    let server = ControlServer::start(studio.clone(), endpoints.path()).expect("starts");
    // Let FSEvents arm before the test touches the file.
    sleep(Duration::from_millis(300));
    Fixture {
        path,
        endpoints,
        slot,
        studio,
        server: Some(server),
        events,
        _dir: dir,
    }
}

impl Fixture {
    fn call(&self, cmd: Command) -> Value {
        self.studio.call(cmd).expect("command succeeds")
    }

    fn link(&self) -> StudioLink {
        find_studio(self.endpoints.path(), &self.path).expect("finds the studio")
    }

    /// Runs `actions` through the endpoint as Claude in `run`.
    fn agent(
        &self,
        actions: &[Action],
        run: Option<&str>,
    ) -> Result<tune_studio::ActionsOutcome, StudioError> {
        let run = run.map(|id| RunTag::new(id, None).expect("valid run"));
        self.link().run(actions, &Origin::claude(run))
    }

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

    /// The studio state, as the wire carries it.
    fn state(&self) -> Value {
        self.call(Command::Studio(StudioCommand::GetState))
    }

    fn disk(&self) -> String {
        fs::read_to_string(&self.path).expect("read song")
    }

    fn disk_until(&self, what: &str, pred: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + WAIT;
        loop {
            let text = self.disk();
            if pred(&text) {
                return text;
            }
            assert!(Instant::now() < deadline, "file never {what}:\n{text}");
            sleep(Duration::from_millis(10));
        }
    }

    fn log(&self) -> Vec<tune_studio::EditEntry> {
        EditLog::for_song(&self.path)
            .read_since(None)
            .expect("read log")
    }

    fn endpoint_files(&self) -> Vec<PathBuf> {
        endpoint_files(self.endpoints.path())
    }
}

fn endpoint_files(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .expect("read endpoint dir")
        .map(|e| e.expect("entry").path())
        .collect()
}

fn set(path: &str, value: Value) -> Action {
    Action::SetValue {
        path: path.parse().expect("a path"),
        value,
    }
}

fn knob(param: &str, value: f64, commit: bool) -> Action {
    Action::SetParam {
        track: "acid".into(),
        param: param.into(),
        value,
        commit,
    }
}

/// The plain value at `path` in the song file.
fn value_at(text: &str, path: &str) -> Value {
    let song = parse_song(text).expect("valid song");
    tune_studio::effective_value(&song, &path.parse::<SongPath>().unwrap()).expect("a value")
}

/// `commented`, with the acid also playing in section `a` (bars 0..2),
/// where no lane drives the cutoff.
fn acid_in_a() -> String {
    let from = "play = [{ at = 3, clip = \"a1\", bars = 2 }]";
    assert!(COMMENTED.contains(from));
    COMMENTED.replacen(
        from,
        "play = [\n  { at = 1, clip = \"a1\", bars = 2 },\n  { at = 3, clip = \"a1\", bars = 2 },\n]",
        1,
    )
}

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

/// Sends `request` over a raw connection and reads until the server closes.
fn raw(port: u16, request: &[u8]) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connects");
    s.set_read_timeout(Some(WAIT)).unwrap();
    s.write_all(request).expect("sends");
    let mut reply = String::new();
    s.read_to_string(&mut reply)
        .expect("the server closes the connection");
    reply
}

#[test]
fn endpoint_file_names_a_secret_and_goes_with_the_server() {
    let mut f = fixture(COMMENTED);
    let port = f.server.as_ref().unwrap().port();
    let files = f.endpoint_files();
    assert_eq!(files.len(), 1, "{files:?}");
    let name = files[0].file_name().unwrap().to_str().unwrap().to_owned();
    assert_eq!(name, format!("studio-{}-{port}.json", std::process::id()));
    let v: Value = serde_json::from_str(&fs::read_to_string(&files[0]).unwrap()).unwrap();
    assert_eq!(v["port"], port);
    assert_eq!(v["pid"], std::process::id());
    let secret = v["secret"].as_str().unwrap();
    assert_eq!(secret.len(), 64);
    assert!(
        secret
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "64 lowercase hex digits"
    );
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&files[0]), 0o600);
    assert_eq!(mode(f.endpoints.path()), 0o700);
    // Neither handle shows the secret.
    let shown = format!("{:?} {:?}", f.link(), f.server.as_ref().unwrap());
    assert!(!shown.contains(secret), "{shown}");

    f.server = None;
    assert!(
        f.endpoint_files().is_empty(),
        "the file goes with the server"
    );
}

#[test]
fn the_endpoint_directory_is_created_private() {
    let f = fixture(COMMENTED);
    let parent = tempfile::tempdir().unwrap();
    let dir = parent.path().join("tune").join("studios");
    let server = ControlServer::start(f.studio.clone(), &dir).expect("starts");
    for d in [parent.path().join("tune"), dir.clone()] {
        assert_eq!(
            fs::metadata(&d).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    drop(server);
    assert!(endpoint_files(&dir).is_empty());
}

#[test]
fn find_studio_matches_the_open_song() {
    let f = fixture(COMMENTED);
    assert!(find_studio(f.endpoints.path(), &f.path).is_some());
    let dir = f.path.parent().unwrap();
    let roundabout = dir
        .join("..")
        .join(dir.file_name().unwrap())
        .join("song.toml");
    assert!(find_studio(f.endpoints.path(), &roundabout).is_some());

    let other = tempfile::tempdir().unwrap();
    let other_song = other.path().join("song.toml");
    fs::write(&other_song, COMMENTED).unwrap();
    assert!(find_studio(f.endpoints.path(), &other_song).is_none());
    assert_eq!(f.endpoint_files().len(), 1, "a live studio's file stays");
}

#[test]
fn a_wrong_secret_or_http_is_refused() {
    let f = fixture(COMMENTED);
    let port = f.server.as_ref().unwrap().port();
    let refused = "{\"ok\":false,\"error\":\"refused\"}\n";
    let http = b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: http://evil.example\r\n\r\n";
    assert_eq!(raw(port, http), refused);
    let wrong = json!({
        "secret": "00",
        "op": "do",
        "actions": [set("song.bpm", json!(130))],
        "source": "claude",
        "run": null,
    });
    assert_eq!(raw(port, format!("{wrong}\n").as_bytes()), refused);
    let missing = json!({ "op": "song" });
    assert_eq!(raw(port, format!("{missing}\n").as_bytes()), refused);
    sleep(Duration::from_millis(300));
    assert_eq!(f.disk(), COMMENTED);
    assert!(f.log().is_empty());
}

/// With the right secret, only `claude` may act: `external` and `human`
/// are bad requests and change nothing.
#[test]
fn only_claude_acts_through_the_endpoint() {
    let f = fixture(COMMENTED);
    let port = f.server.as_ref().unwrap().port();
    let file: Value =
        serde_json::from_str(&fs::read_to_string(&f.endpoint_files()[0]).unwrap()).unwrap();
    let song = fs::canonicalize(&f.path).unwrap();
    for source in ["external", "human"] {
        let request = json!({
            "secret": file["secret"],
            "op": "do",
            "song": song,
            "actions": [set("song.bpm", json!(130))],
            "source": source,
            "run": null,
        });
        let reply: Value =
            serde_json::from_str(&raw(port, format!("{request}\n").as_bytes())).unwrap();
        assert_eq!(reply["ok"], false);
        assert_eq!(
            reply["error"], "bad request: the control endpoint acts only as `claude`",
            "{source}"
        );
    }
    sleep(Duration::from_millis(300));
    assert_eq!(f.disk(), COMMENTED);
    assert!(f.log().is_empty());
}

#[test]
fn stale_endpoint_files_are_removed() {
    let endpoints = tempfile::tempdir().unwrap();
    let song_dir = tempfile::tempdir().unwrap();
    let song = song_dir.path().join("song.toml");
    fs::write(&song, COMMENTED).unwrap();
    let stale = endpoints.path().join("studio-1-9.json");
    let body = json!({ "port": 9, "secret": "0".repeat(64), "pid": 1 });
    fs::write(&stale, body.to_string()).unwrap();
    assert!(find_studio(endpoints.path(), &song).is_none());
    assert!(!stale.exists(), "the stale file is gone");
}

/// A listener that answers `song` with `song` but reads a `do` and never
/// replies.
fn silent_studio(endpoints: &Path, song: &Path) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let secret = "ab".repeat(32);
    let body = json!({ "port": port, "secret": secret, "pid": std::process::id() });
    fs::write(
        endpoints.join(format!("studio-{}-{port}.json", std::process::id())),
        body.to_string(),
    )
    .unwrap();
    let song = fs::canonicalize(song).unwrap();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let req: Value = serde_json::from_str(&line).unwrap();
            if req["op"] == "song" {
                let reply = json!({ "ok": true, "song": song });
                stream.write_all(format!("{reply}\n").as_bytes()).unwrap();
            } else {
                held.push(stream);
            }
        }
    });
}

#[test]
fn a_studio_that_does_not_answer_is_an_error_not_a_fallback() {
    let endpoints = tempfile::tempdir().unwrap();
    let song_dir = tempfile::tempdir().unwrap();
    let song = song_dir.path().join("song.toml");
    fs::write(&song, COMMENTED).unwrap();
    silent_studio(endpoints.path(), &song);
    let link = find_studio(endpoints.path(), &song).expect("finds the silent studio");
    let start = Instant::now();
    let result = link.run(&[set("song.bpm", json!(130))], &Origin::claude(None));
    let took = start.elapsed();
    assert_eq!(result, Err(StudioError::NoAnswer));
    assert!(took >= Duration::from_secs(5), "gave up after {took:?}");
    assert!(took < Duration::from_secs(8), "waited {took:?}");
    assert_eq!(fs::read_to_string(&song).unwrap(), COMMENTED, "no fallback");
}

/// A link names the song it was found for: once the studio has switched
/// to another song, its `do` is refused and neither file changes.
#[test]
fn a_studio_that_switched_songs_refuses_the_link() {
    let f = fixture(COMMENTED);
    let other = f.path.parent().unwrap().join("other").join("song.toml");
    fs::create_dir(other.parent().unwrap()).unwrap();
    fs::write(&other, COMMENTED).unwrap();
    let link = f.link();
    f.call(Command::Studio(StudioCommand::OpenSong {
        path: "other/song.toml".into(),
    }));
    let err = link
        .run(&[set("song.bpm", json!(130))], &Origin::claude(None))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "the studio has another song open now; nothing was changed"
    );
    sleep(Duration::from_millis(300));
    assert_eq!(f.disk(), COMMENTED);
    assert_eq!(fs::read_to_string(&other).unwrap(), COMMENTED);
}

#[test]
fn actions_through_the_link_are_one_undo_step_each() {
    let f = fixture(COMMENTED);
    let out = f
        .agent(&[set("tracks.acid.acid303.cutoff", json!(820))], Some("r1"))
        .unwrap();
    assert_eq!(out.via, Via::Studio);
    assert_eq!(out.changes.len(), 1);
    assert!(out.file_hash.is_some());
    // Written before the answer: no debounce.
    assert_eq!(
        value_at(&f.disk(), "tracks.acid.acid303.cutoff"),
        json!(820)
    );
    match f.wait("Claude's edit", |e| matches!(e, Event::Applied { .. })) {
        Event::Applied {
            source,
            changes,
            run,
        } => {
            assert_eq!(source, Source::Claude);
            assert_eq!(changes[0].path.to_string(), "tracks.acid.acid303.cutoff");
            assert_eq!(run.expect("a run").id, "r1");
        }
        _ => unreachable!(),
    }
    let last = f.log().pop().unwrap();
    assert_eq!(last.source, Source::Claude);
    assert_eq!(last.action.as_deref(), Some("set_value"));
    assert_eq!(last.run.as_deref(), Some("r1"));

    f.agent(
        &[
            set("song.bpm", json!(130)),
            set("tracks.acid.gain_db", json!(-6)),
        ],
        Some("r1"),
    )
    .unwrap();
    let text = f.disk();
    assert_eq!(value_at(&text, "song.bpm"), json!(130));
    assert_eq!(value_at(&text, "tracks.acid.gain_db"), json!(-6));
    for c in ["# the body", "# the squelch", "# acid enters"] {
        assert!(text.contains(c), "lost comment {c}");
    }

    f.call(Command::Action(Action::Undo));
    let text = f.disk();
    assert_eq!(value_at(&text, "song.bpm"), json!(120));
    assert_eq!(value_at(&text, "tracks.acid.gain_db"), json!(0));
    assert_eq!(value_at(&text, "tracks.acid.acid303.cutoff"), json!(820));
    f.call(Command::Action(Action::Undo));
    assert_eq!(f.disk(), COMMENTED, "one action at a time");
}

#[test]
fn pending_human_edits_are_written_before_an_agent_action() {
    let f = fixture(COMMENTED);
    f.call(Command::Action(knob("acid303.cutoff", 820.0, true)));
    f.agent(&[set("song.bpm", json!(130))], None).unwrap();
    let log: Vec<(Source, String)> = f.log().iter().map(|e| (e.source, e.path.clone())).collect();
    assert_eq!(
        log,
        [
            (Source::Human, "tracks.acid.acid303.cutoff".to_owned()),
            (Source::Claude, "song.bpm".to_owned()),
        ]
    );
    f.call(Command::Action(Action::Undo));
    let text = f.disk();
    assert_eq!(value_at(&text, "song.bpm"), json!(120));
    assert_eq!(value_at(&text, "tracks.acid.acid303.cutoff"), json!(820));
    f.call(Command::Action(Action::Undo));
    assert_eq!(f.disk(), COMMENTED);
}

#[test]
fn a_failing_batch_changes_nothing() {
    let f = fixture(COMMENTED);
    // Let the opening events pass.
    f.quiet(Duration::from_millis(200), "nothing", |_| false);
    let before = f.state();
    let err = f
        .agent(
            &[
                set("song.bpm", json!(130)),
                set("tracks.nope.gain_db", json!(1)),
            ],
            None,
        )
        .unwrap_err();
    assert!(err.to_string().contains("tracks.nope.gain_db"), "{err}");
    f.quiet(Duration::from_millis(300), "applied event", |e| {
        matches!(e, Event::Applied { .. })
    });
    assert_eq!(f.disk(), COMMENTED);
    assert!(f.log().is_empty());
    assert_eq!(f.state(), before);
}

#[test]
fn live_actions_through_the_link() {
    let f = fixture(COMMENTED);
    let out = f
        .agent(&[Action::Play { from_bar: Some(1) }], None)
        .unwrap();
    assert_eq!(out.via, Via::Studio);
    let s = f.state();
    assert_eq!(s["playing"], true);
    assert_eq!(s["cursor_bar"], 1);
    f.agent(&[Action::Stop], None).unwrap();
    assert_eq!(f.state()["playing"], false);

    f.agent(&[set("song.bpm", json!(130))], None).unwrap();
    let out = f.agent(&[Action::Undo], None).unwrap();
    assert_eq!(out.changes.len(), 1);
    assert_eq!(f.disk(), COMMENTED);
    let last = f.log().pop().unwrap();
    assert_eq!(last.source, Source::Claude);
    assert_eq!(last.action.as_deref(), Some("undo"));

    let err = f
        .agent(&[Action::Stop, set("song.bpm", json!(130))], None)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "a batch holds song edits only; run `stop` on its own"
    );
}

#[test]
fn agent_set_param_ignores_automation() {
    let f = fixture(COMMENTED);
    // Bar 3 (0-based 2) lies in the cutoff lane.
    f.call(Command::Action(Action::SeekBar { bar: 2 }));
    assert_eq!(
        f.call(Command::Action(knob("acid303.cutoff", 900.0, true))),
        json!({ "automated": true })
    );
    sleep(Duration::from_millis(300));
    assert_eq!(f.disk(), COMMENTED);
    f.agent(&[knob("acid303.cutoff", 820.0, true)], None)
        .unwrap();
    assert_eq!(
        value_at(&f.disk(), "tracks.acid.acid303.cutoff"),
        json!(820)
    );
}

#[test]
fn an_agent_action_during_a_drag_keeps_the_drag() {
    let song = acid_in_a();
    let f = fixture(&song);
    f.call(Command::Action(Action::Play { from_bar: Some(0) }));
    f.pull(4800);
    f.call(Command::Action(knob("acid303.cutoff", 2000.0, false)));
    f.agent(&[set("tracks.hat.gain_db", json!(-12))], None)
        .unwrap();
    let edited = f.disk();
    assert_eq!(value_at(&edited, "tracks.hat.gain_db"), json!(-12));
    assert_eq!(value_at(&edited, "tracks.acid.acid303.cutoff"), json!(500));

    // Let the swap land, then restart the bar so instrument state is fresh
    // on both sides: only the knob can make them differ.
    f.pull(12_000);
    sleep(Duration::from_millis(150));
    f.call(Command::Action(Action::SeekBar { bar: 0 }));
    assert!(
        f.pull(9600) != render_bars(&edited, 0, 1)[..19_200],
        "the drag is heard after the agent's edit"
    );

    assert_eq!(
        f.call(Command::Action(knob("acid303.cutoff", 2000.0, true))),
        Value::Null
    );
    let text = f.disk_until("held the release", |t| {
        value_at(t, "tracks.acid.acid303.cutoff") == json!(2000)
    });
    assert_eq!(value_at(&text, "tracks.hat.gain_db"), json!(-12));
}
