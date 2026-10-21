//! The studio actor: one thread that owns the open song, the live engine
//! and the audio device, serving any number of clients.
//!
//! Clients send typed [`Command`]s through a [`StudioHandle`] and each gets
//! the [`Event`] stream from [`StudioHandle::subscribe`]. The actor watches
//! the song file and hot-reloads every change while playing, keeping the
//! playhead: an edit from a client, an undo or redo, and a text adopted from
//! disk all compile the song again and swap it into the running engine. A
//! knob drag is heard at once as a live override and written only on
//! release.
//!
//! Commands, file changes and shutdown arrive as one message queue, so they
//! are handled strictly in order. Between messages the actor ticks every
//! 33 ms: playhead events at 30 Hz, garbage from the audio thread, the
//! debounced write of pending edits, the end of the song.

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use tune_engine::{AudioOut, CompiledSong, LiveController, LiveError, ParamTarget, live};
use tune_song::{RawReload, SongWatcher};

use crate::command::Command;
use crate::core::{ExternalChange, StudioCore};
use crate::diff::Change;
use crate::doc::{EditError, effective_value};
use crate::error::{ErrorView, StudioError};
use crate::log::Source;
use crate::path::SongPath;
use crate::view::StudioState;

/// The actor's tick: playhead events go out at this rate while playing.
pub const PLAYHEAD_INTERVAL: Duration = Duration::from_millis(33);
/// How long [`StudioHandle::call`] waits for the actor's answer.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// The rate asked of the device when the song names none.
const DEFAULT_RATE: u32 = 48_000;
/// After a failed write, how long the tick waits before trying again.
const FLUSH_RETRY: Duration = Duration::from_secs(1);
/// After a knob's release, how long a live value for the same param counts
/// as a straggler from that gesture (calls can arrive out of order) and is
/// dropped instead of starting a new drag.
const RELEASE_GUARD: Duration = Duration::from_millis(200);

/// What the studio tells every subscriber.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// The whole studio state, after anything in it changed.
    State(StudioState),
    /// Where playback is, 30 times a second while playing. All 0-based:
    /// `step` counts 16ths from the song start, `bar = step / 16` and
    /// `beat = step % 16 / 4`.
    Playhead {
        step: u64,
        bar: u32,
        beat: u32,
        playing: bool,
    },
    /// A client's edit (or undo, redo, re-apply) changed the song.
    Applied {
        source: Source,
        changes: Vec<Change>,
    },
    /// The file changed on disk (Claude, an editor, git).
    ExternalChange(ExternalChange),
    /// Something went wrong outside any command: a failed write, a lost
    /// device, an unreadable file.
    Error { message: String },
}

/// What the studio runs with.
pub struct StudioConfig {
    /// The device playback goes to.
    pub audio: Box<dyn AudioOut>,
    /// The directory holding `songs/<name>/song.toml`; relative song paths
    /// in [`Command::OpenSong`] resolve against it.
    pub songs_root: PathBuf,
}

/// Starts the studio actor.
pub struct Studio;

impl Studio {
    /// Starts the actor thread, with `song` open if given. A song file that
    /// does not parse still starts the studio: the state names the file and
    /// its errors, and the file opens once it is fixed. Fails if the song
    /// cannot be read or watched.
    pub fn spawn(config: StudioConfig, song: Option<PathBuf>) -> Result<StudioHandle, StudioError> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let actor_tx = tx.clone();
        let thread = std::thread::Builder::new()
            .name("studio".into())
            .spawn(move || {
                let mut actor = Actor::new(config, actor_tx);
                if let Some(path) = song
                    && let Err(e) = actor.open_or_wait(&path)
                {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
                actor.last_state = Some(actor.state());
                let _ = ready_tx.send(Ok(()));
                actor.run(&rx);
            })
            .map_err(|e| StudioError::Io(format!("cannot start the studio thread: {e}")))?;
        let started = ready_rx.recv().unwrap_or(Err(StudioError::Closed));
        if let Err(e) = started {
            let _ = thread.join();
            return Err(e);
        }
        Ok(StudioHandle {
            inner: Arc::new(Inner {
                tx,
                thread: Mutex::new(Some(thread)),
            }),
        })
    }
}

/// A client's connection to the studio. Clones share one actor, which stops
/// when the last clone is dropped (or at [`shutdown`](Self::shutdown)).
#[derive(Clone)]
pub struct StudioHandle {
    inner: Arc<Inner>,
}

struct Inner {
    tx: Sender<Msg>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Inner {
    fn shutdown(&self) {
        let _ = self.tx.send(Msg::Shutdown);
        let thread = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(thread) = thread
            && thread.thread().id() != std::thread::current().id()
        {
            let _ = thread.join();
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl StudioHandle {
    /// Runs `cmd` and returns its result: `null` for most commands, the
    /// song list for `list_songs`, the state for `get_state`, the changes for `undo`/`redo`, and
    /// `{"automated": true}` for a knob released on an automated param.
    /// Blocks for at most 5 s.
    pub fn call(&self, cmd: Command) -> Result<Value, StudioError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.inner
            .tx
            .send(Msg::Call(cmd, reply_tx))
            .map_err(|_| StudioError::Closed)?;
        match reply_rx.recv_timeout(CALL_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(StudioError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(StudioError::Closed),
        }
    }

    /// A new event stream. Its first event is the current
    /// [`Event::State`]. Dropping the receiver unsubscribes.
    pub fn subscribe(&self) -> Receiver<Event> {
        let (tx, rx) = mpsc::channel();
        // A stopped studio leaves the receiver disconnected.
        let _ = self.inner.tx.send(Msg::Subscribe(tx));
        rx
    }

    /// Writes pending edits, stops playback and ends the actor; waits for
    /// it. Later calls give [`StudioError::Closed`].
    pub fn shutdown(&self) {
        self.inner.shutdown();
    }
}

enum Msg {
    Call(Command, Sender<Result<Value, StudioError>>),
    Subscribe(Sender<Event>),
    /// A change of the song file; `epoch` names the watcher that saw it,
    /// so reports for a song no longer open are dropped.
    Disk {
        epoch: u64,
        reload: RawReload,
    },
    Shutdown,
}

/// The open song and its watcher.
struct OpenSong {
    core: StudioCore,
    epoch: u64,
    _watcher: SongWatcher,
}

/// A song asked for at start whose file does not parse yet, watched so it
/// opens once fixed.
struct Unopened {
    path: PathBuf,
    errors: Vec<ErrorView>,
    epoch: u64,
    _watcher: SongWatcher,
}

/// Live playback.
struct Player {
    /// Dropping it stops the device (declared first, so it goes first).
    _stream: Box<dyn Send>,
    controller: LiveController,
    /// The rate the device runs at; every swap compiles at it.
    rate: u32,
    /// The songs sent to the engine, by generation, oldest first: the one
    /// playing and any sent after it. The last is the newest.
    songs: VecDeque<(u64, Arc<CompiledSong>)>,
    /// A song change not sent yet (the command queue was full), and
    /// whether it clears the knob overrides; retried every tick.
    retry: Option<bool>,
    /// The loop changed and has not reached the engine yet; it goes after
    /// any waiting swap, so the engine clamps it against the new song.
    loop_unsent: bool,
}

impl Player {
    /// The newest song sent; overrides are stamped with its generation.
    fn latest(&self) -> &Arc<CompiledSong> {
        &self.songs.back().expect("a song is always sent").1
    }

    fn sent_generation(&self) -> u64 {
        self.songs.back().expect("a song is always sent").0
    }

    /// The song the engine plays, as of its last block.
    fn playing(&mut self) -> &Arc<CompiledSong> {
        let generation = self.controller.playing_generation();
        while self.songs.len() > 1 && self.songs[0].0 < generation {
            self.songs.pop_front();
        }
        &self.songs[0].1
    }

    /// Compiles `song` and sends it to the engine.
    fn swap(&mut self, song: &tune_song::Song, clear_overrides: bool) -> Result<(), LiveError> {
        let compiled = Arc::new(CompiledSong::new(song, self.rate));
        if clear_overrides {
            self.controller.swap_clearing_overrides(compiled.clone())?;
        } else {
            self.controller.swap(compiled.clone())?;
        }
        let generation = self.sent_generation() + 1;
        self.songs.push_back((generation, compiled));
        Ok(())
    }

    /// The current bar, below `total` (0 once finished).
    fn bar(&self, total: u32) -> u32 {
        if self.controller.finished() {
            0
        } else {
            self.controller.bar().min(total.saturating_sub(1))
        }
    }
}

/// A knob held down: its last value, live only.
struct Drag {
    track: String,
    param: String,
    value: f64,
    /// The engine position when the override was last sent; `None` until
    /// it was sent at all.
    sent_at: Option<u64>,
}

struct Actor {
    /// For the watchers' forwarding threads.
    tx: Sender<Msg>,
    audio: Box<dyn AudioOut>,
    songs_root: PathBuf,
    subscribers: Vec<Sender<Event>>,
    song: Option<OpenSong>,
    /// Only while no song is open.
    unopened: Option<Unopened>,
    player: Option<Player>,
    /// Where playback starts; 0-based, below the song's bar count.
    cursor_bar: u32,
    /// Half-open, clamped to the song.
    loop_bars: Option<[u32; 2]>,
    drag: Option<Drag>,
    /// The last knob released: track, param and when.
    released: Option<(String, String, Instant)>,
    /// The last watcher's epoch.
    epoch: u64,
    /// When the last write of pending edits failed, if it did.
    flush_failed_at: Option<Instant>,
    /// The state last broadcast, to catch a change that went out unshown.
    last_state: Option<StudioState>,
}

impl Actor {
    fn new(config: StudioConfig, tx: Sender<Msg>) -> Self {
        Self {
            tx,
            audio: config.audio,
            songs_root: config.songs_root,
            subscribers: Vec::new(),
            song: None,
            unopened: None,
            player: None,
            cursor_bar: 0,
            loop_bars: None,
            drag: None,
            released: None,
            epoch: 0,
            flush_failed_at: None,
            last_state: None,
        }
    }

    fn run(&mut self, rx: &Receiver<Msg>) {
        let mut next_tick = Instant::now() + PLAYHEAD_INTERVAL;
        loop {
            let wait = next_tick.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(Msg::Call(cmd, reply)) => {
                    // A drag changes nothing shown; skip building the state.
                    let shown = !matches!(
                        cmd,
                        Command::SetParam { commit: false, .. }
                            | Command::ListSongs
                            | Command::GetState
                    );
                    let result = self.handle(cmd);
                    if shown {
                        self.publish_if_stale();
                    }
                    let _ = reply.send(result);
                }
                Ok(Msg::Subscribe(tx)) => {
                    if tx.send(Event::State(self.state())).is_ok() {
                        self.subscribers.push(tx);
                    }
                }
                Ok(Msg::Disk { epoch, reload }) => {
                    self.on_disk(epoch, reload);
                    self.publish_if_stale();
                }
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            let now = Instant::now();
            if now >= next_tick {
                self.tick(now);
                next_tick = now + PLAYHEAD_INTERVAL;
            }
        }
        self.flush_now();
        self.player = None;
    }

    // ----- commands -----

    fn handle(&mut self, cmd: Command) -> Result<Value, StudioError> {
        match cmd {
            Command::ListSongs => self.list_songs(),
            Command::GetState => Ok(serde_json::to_value(self.state()).unwrap_or(Value::Null)),
            Command::OpenSong { path } => {
                let path = self.under_root(&path)?;
                self.open(&path)?;
                self.emit_state();
                Ok(Value::Null)
            }
            Command::Play { from_bar } => self.play(from_bar),
            Command::Stop => {
                self.stop();
                self.emit_state();
                Ok(Value::Null)
            }
            Command::SeekBar { bar } => self.seek(bar),
            Command::SetLoop { bars } => self.set_loop(bars),
            Command::SetParam {
                track,
                param,
                value,
                commit: false,
            } => self.drag(track, param, value),
            cmd @ Command::SetParam { commit: true, .. } => self.release(&cmd),
            Command::Undo => self.undo_redo(true),
            Command::Redo => self.undo_redo(false),
            Command::ResolveOffered { reapply } => self.resolve_offered(reapply),
            edit => self.edit(&edit),
        }
    }

    /// The sorted `songs/*/song.toml` paths under the songs root, relative
    /// to it.
    fn list_songs(&self) -> Result<Value, StudioError> {
        let dir = self.songs_root.join("songs");
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(json!([])),
            Err(e) => {
                return Err(StudioError::Io(format!(
                    "cannot list {}: {e}",
                    dir.display()
                )));
            }
        };
        let mut songs: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().join("song.toml").is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .map(|name| format!("songs/{name}/song.toml"))
            .collect();
        songs.sort();
        Ok(json!(songs))
    }

    /// Resolves a client's song path against the songs root and refuses
    /// one that leads outside it (through `..`, an absolute path or a
    /// symlink). Gives the canonical path.
    fn under_root(&self, path: &str) -> Result<PathBuf, StudioError> {
        let root = fs::canonicalize(&self.songs_root).map_err(|e| {
            StudioError::Io(format!(
                "cannot read the songs root {}: {e}",
                self.songs_root.display()
            ))
        })?;
        let joined = self.songs_root.join(path);
        match fs::canonicalize(&joined) {
            Ok(full) if full.starts_with(&root) => Ok(full),
            Ok(_) => Err(StudioError::OutsideRoot(path.to_owned())),
            // Say "outside" for a missing file outside the root too, so a
            // client cannot probe what exists there.
            Err(e) => {
                let normal = normalize(&joined);
                if normal.starts_with(normalize(&self.songs_root)) || normal.starts_with(&root) {
                    Err(StudioError::Io(format!("cannot open {path}: {e}")))
                } else {
                    Err(StudioError::OutsideRoot(path.to_owned()))
                }
            }
        }
    }

    /// Opens `path` in place of the current song (whose pending edits are
    /// written first) and stops playback. On failure the current song
    /// stays open.
    fn open(&mut self, path: &Path) -> Result<(), StudioError> {
        self.flush_now();
        let core = StudioCore::open(path)?;
        let epoch = self.epoch + 1;
        let watcher = watch(path, self.tx.clone(), epoch)?;
        self.epoch = epoch;
        self.stop();
        if let Some(old) = &self.song
            && !old.core.pending().is_empty()
        {
            self.emit(Event::Error {
                message: format!(
                    "edits to {} could not be written",
                    old.core.path().display()
                ),
            });
        }
        self.song = Some(OpenSong {
            core,
            epoch,
            _watcher: watcher,
        });
        self.unopened = None;
        self.cursor_bar = 0;
        self.loop_bars = None;
        self.drag = None;
        self.released = None;
        self.flush_failed_at = None;
        Ok(())
    }

    /// Opens `path`; if it does not parse, watches it and waits for a fix.
    fn open_or_wait(&mut self, path: &Path) -> Result<(), StudioError> {
        match self.open(path) {
            Err(StudioError::Load(errors)) => {
                let epoch = self.epoch + 1;
                let watcher = watch(path, self.tx.clone(), epoch)?;
                self.epoch = epoch;
                self.unopened = Some(Unopened {
                    path: path.to_owned(),
                    errors,
                    epoch,
                    _watcher: watcher,
                });
                Ok(())
            }
            other => other,
        }
    }

    /// The unopened song's file changed: open it if it parses now.
    fn retry_unopened(&mut self, path: &Path) {
        match self.open(path) {
            Ok(()) => self.emit_state(),
            Err(StudioError::Load(errors)) => {
                if let Some(u) = &mut self.unopened {
                    u.errors = errors;
                }
            }
            Err(e) => self.emit(Event::Error {
                message: e.to_string(),
            }),
        }
    }

    fn play(&mut self, from_bar: Option<u32>) -> Result<Value, StudioError> {
        let song = self.core()?.song().clone();
        let bar = from_bar
            .unwrap_or(self.cursor_bar)
            .min(song.total_bars().saturating_sub(1));
        self.stop();
        let wanted = song.sample_rate.unwrap_or(DEFAULT_RATE);
        let rate = self.audio.sample_rate(wanted).map_err(StudioError::Audio)?;
        let compiled = Arc::new(CompiledSong::new(&song, rate));
        let (controller, processor) = live(compiled.clone(), bar, self.loop_range());
        let stream = self.audio.start(processor).map_err(StudioError::Audio)?;
        self.player = Some(Player {
            _stream: stream,
            controller,
            rate,
            songs: VecDeque::from([(0, compiled)]),
            retry: None,
            loop_unsent: false,
        });
        self.cursor_bar = bar;
        self.send_drag();
        self.emit_state();
        Ok(Value::Null)
    }

    /// Stops playback, leaving the cursor at the bar it reached.
    fn stop(&mut self) {
        if let Some(player) = self.player.take() {
            let total = self.total_bars();
            self.cursor_bar = player.bar(total);
        }
    }

    fn seek(&mut self, bar: u32) -> Result<Value, StudioError> {
        let bar = bar.min(self.core()?.song().total_bars().saturating_sub(1));
        self.cursor_bar = bar;
        if let Some(player) = &mut self.player {
            player.controller.seek(bar).map_err(audio_error)?;
        }
        self.emit_state();
        Ok(Value::Null)
    }

    fn set_loop(&mut self, bars: Option<[u32; 2]>) -> Result<Value, StudioError> {
        let total = self.core()?.song().total_bars();
        self.loop_bars = clamp_loop(bars, total);
        if let Some(player) = &mut self.player {
            player.loop_unsent = true;
        }
        self.send_loop();
        self.emit_state();
        Ok(Value::Null)
    }

    /// A knob moving: heard at once, written never (until the release).
    fn drag(&mut self, track: String, param: String, value: f64) -> Result<Value, StudioError> {
        let path = SongPath::TrackParam {
            track: track.clone(),
            param: param.clone(),
        };
        if effective_value(self.core()?.song(), &path).is_none() {
            return Err(EditError::NoSuchTarget(path.to_string()).into());
        }
        if self
            .released
            .as_ref()
            .is_some_and(|(t, p, at)| *t == track && *p == param && at.elapsed() < RELEASE_GUARD)
        {
            return Ok(Value::Null);
        }
        self.drag = Some(Drag {
            track,
            param,
            value,
            sent_at: None,
        });
        self.send_drag();
        Ok(Value::Null)
    }

    /// A knob let go: the value is written, unless a lane drives the param
    /// at the current bar; either way the live override ends.
    fn release(&mut self, cmd: &Command) -> Result<Value, StudioError> {
        let Command::SetParam { track, param, .. } = cmd else {
            unreachable!()
        };
        if self
            .drag
            .as_ref()
            .is_some_and(|d| d.track == *track && d.param == *param)
        {
            self.drag = None;
        }
        self.released = Some((track.clone(), param.clone(), Instant::now()));
        let bar = self.current_bar();
        let core = &mut self.song.as_mut().ok_or(StudioError::NoSong)?.core;
        if core.is_automated(track, param, bar) {
            self.clear_overrides();
            return Ok(json!({ "automated": true }));
        }
        let result = core.apply_edit(cmd, Instant::now());
        // The override goes even when the edit fails or changes nothing.
        self.song_changed(true);
        let changes = result?;
        self.announce_applied(changes);
        Ok(Value::Null)
    }

    fn edit(&mut self, cmd: &Command) -> Result<Value, StudioError> {
        let changes = self.core_mut()?.apply_edit(cmd, Instant::now())?;
        if !changes.is_empty() {
            self.song_changed(false);
            self.announce_applied(changes);
        }
        Ok(Value::Null)
    }

    fn undo_redo(&mut self, undo: bool) -> Result<Value, StudioError> {
        let core = self.core_mut()?;
        let result = if undo { core.undo() } else { core.redo() };
        // Undo and redo flush first, which may take in an unseen change.
        self.report_unreported();
        let changes = result?;
        let value = serde_json::to_value(&changes).unwrap_or(Value::Null);
        if !changes.is_empty() {
            self.song_changed(false);
            self.announce_applied(changes);
        }
        Ok(value)
    }

    fn resolve_offered(&mut self, reapply: bool) -> Result<Value, StudioError> {
        let result = self.core_mut()?.resolve_offered(reapply);
        self.report_unreported();
        let changes = result?;
        if changes.is_empty() {
            self.emit_state();
        } else {
            self.song_changed(false);
            self.announce_applied(changes);
        }
        Ok(Value::Null)
    }

    // ----- the file -----

    fn on_disk(&mut self, epoch: u64, reload: RawReload) {
        if let Some(u) = &self.unopened
            && u.epoch == epoch
        {
            let path = u.path.clone();
            self.retry_unopened(&path);
            return;
        }
        let Some(song) = &mut self.song else {
            return;
        };
        if song.epoch != epoch {
            return;
        }
        match reload {
            RawReload::Changed(text) => {
                // A report can trail the file: the studio may have written
                // since the watcher read it. Take the file as it is now.
                let text = match fs::read_to_string(song.core.path()) {
                    Ok(now) => now,
                    Err(_) => text,
                };
                if let Some(ext) = song.core.on_disk_text(text) {
                    self.report_external(ext);
                }
            }
            RawReload::Io(message) => self.emit(Event::Error {
                message: format!("cannot read the song file: {message}"),
            }),
        }
    }

    /// Announces a change [`StudioCore::flush`] took in from disk before the
    /// watcher reported it.
    fn report_unreported(&mut self) {
        let ext = self
            .song
            .as_mut()
            .and_then(|s| s.core.take_unreported_external());
        if let Some(ext) = ext {
            self.report_external(ext);
        }
    }

    fn report_external(&mut self, ext: ExternalChange) {
        if !ext.changes.is_empty() || !ext.merged.is_empty() {
            self.song_changed(false);
        }
        self.emit(Event::ExternalChange(ext));
        self.emit_state();
    }

    /// Writes the pending edits now and announces what that took in.
    fn flush_now(&mut self) {
        let Some(song) = &mut self.song else {
            return;
        };
        match song.core.flush() {
            Ok(_) => self.flush_failed_at = None,
            Err(e) => {
                self.flush_failed_at = Some(Instant::now());
                self.emit(Event::Error {
                    message: e.to_string(),
                });
            }
        }
        self.report_unreported();
    }

    // ----- playback -----

    /// After the song changed in memory: clamp the cursor and the loop to
    /// it, and while playing swap it in (clearing knob overrides when
    /// `clear_overrides`) and send the loop again.
    fn song_changed(&mut self, clear_overrides: bool) {
        let total = self.total_bars();
        self.cursor_bar = self.cursor_bar.min(total.saturating_sub(1));
        self.loop_bars = clamp_loop(self.loop_bars, total);
        let Some(player) = &mut self.player else {
            return;
        };
        player.retry = Some(player.retry.unwrap_or(false) || clear_overrides);
        player.loop_unsent = true;
        self.send_swap();
        self.send_loop();
    }

    /// Sends the stored loop if it has not reached the engine, once no swap
    /// waits; a full queue leaves it for the next tick.
    fn send_loop(&mut self) {
        let range = self.loop_range();
        let Some(player) = &mut self.player else {
            return;
        };
        if player.loop_unsent && player.retry.is_none() && player.controller.set_loop(range).is_ok()
        {
            player.loop_unsent = false;
        }
    }

    /// Sends the song change waiting in [`Player::retry`]; a full queue
    /// leaves it for the next tick.
    fn send_swap(&mut self) {
        let (Some(player), Some(song)) = (&mut self.player, &self.song) else {
            return;
        };
        let Some(clear) = player.retry else {
            return;
        };
        match player.swap(song.core.song(), clear) {
            Ok(()) => player.retry = None,
            Err(LiveError::QueueFull) => {}
            Err(e) => {
                player.retry = None;
                self.emit(Event::Error {
                    message: format!("cannot reload the song: {e}"),
                });
            }
        }
    }

    /// Ends every knob override by swapping the newest song in again.
    fn clear_overrides(&mut self) {
        if let Some(player) = &mut self.player {
            player.retry = Some(true);
        }
        self.send_swap();
    }

    /// Sends the held knob's value as an override, resolved against the
    /// newest song sent (track ids, not indices, survive a reload).
    fn send_drag(&mut self) {
        let (Some(player), Some(drag)) = (&mut self.player, &mut self.drag) else {
            return;
        };
        let song = player.latest();
        let Some((track, target)) = param_target(song, &drag.track, &drag.param) else {
            return;
        };
        drag.sent_at = None;
        if player
            .controller
            .set_override(track, target, Some(drag.value))
            .is_ok()
        {
            drag.sent_at = Some(player.controller.position());
        }
    }

    /// Sends the held knob again when the engine dropped it: an override
    /// sent between a swap going out and landing names the old song, so
    /// the engine ignores it. Once the newest swap plays, the engine has
    /// run since the last send and still holds no override, it goes again.
    fn refresh_drag(&mut self) {
        let (Some(player), Some(drag)) = (&self.player, &self.drag) else {
            return;
        };
        let c = &player.controller;
        let landed = c.playing_generation() == player.sent_generation();
        let ran = drag.sent_at != Some(c.position());
        if landed && ran && c.overrides_active() == 0 {
            self.send_drag();
        }
    }

    fn tick(&mut self, now: Instant) {
        if self.player.is_some() && self.audio.device_lost() {
            self.stop();
            self.emit(Event::Error {
                message: "audio device lost".into(),
            });
            self.emit_state();
        }
        if let Some(player) = &mut self.player {
            player.controller.collect_garbage();
            if player.controller.finished() {
                self.stop();
                self.emit_state();
            } else {
                let position = player.controller.position();
                let step = player.playing().clock.step_at(position);
                self.emit(Event::Playhead {
                    step,
                    bar: (step / 16) as u32,
                    beat: (step % 16 / 4) as u32,
                    playing: true,
                });
                self.send_swap();
                self.send_loop();
                self.refresh_drag();
            }
        }
        // A failed write stays due; retry it now and then, not every tick.
        let retry_ok = self
            .flush_failed_at
            .is_none_or(|t| now.saturating_duration_since(t) >= FLUSH_RETRY);
        if retry_ok && self.song.as_ref().is_some_and(|s| s.core.flush_due(now)) {
            self.flush_now();
        }
    }

    // ----- state -----

    fn core(&self) -> Result<&StudioCore, StudioError> {
        self.song
            .as_ref()
            .map(|s| &s.core)
            .ok_or(StudioError::NoSong)
    }

    fn core_mut(&mut self) -> Result<&mut StudioCore, StudioError> {
        self.song
            .as_mut()
            .map(|s| &mut s.core)
            .ok_or(StudioError::NoSong)
    }

    fn total_bars(&self) -> u32 {
        self.song.as_ref().map_or(0, |s| s.core.song().total_bars())
    }

    /// The bar a knob release is judged at: the playing bar, else the
    /// cursor.
    fn current_bar(&self) -> u32 {
        match &self.player {
            Some(player) => player.bar(self.total_bars()),
            None => self.cursor_bar,
        }
    }

    fn loop_range(&self) -> Option<Range<u32>> {
        self.loop_bars.map(|[start, end]| start..end)
    }

    fn state(&self) -> StudioState {
        let core = self.song.as_ref().map(|s| &s.core);
        let unopened = self.unopened.as_ref();
        StudioState {
            path: core
                .map(StudioCore::path)
                .or(unopened.map(|u| u.path.as_path()))
                .map(|p| p.to_string_lossy().into_owned()),
            song: core.map(StudioCore::view),
            playing: self.player.is_some(),
            cursor_bar: self.cursor_bar,
            loop_bars: self.loop_bars,
            can_undo: core.is_some_and(StudioCore::can_undo),
            can_redo: core.is_some_and(StudioCore::can_redo),
            offered: core.map(|c| c.offered().to_vec()).unwrap_or_default(),
            file_errors: core
                .map(|c| c.file_errors().to_vec())
                .or(unopened.map(|u| u.errors.clone()))
                .unwrap_or_default(),
        }
    }

    fn announce_applied(&mut self, changes: Vec<Change>) {
        if !changes.is_empty() {
            self.emit(Event::Applied {
                source: Source::Human,
                changes,
            });
        }
        self.emit_state();
    }

    fn emit_state(&mut self) {
        let state = self.state();
        self.last_state = Some(state.clone());
        self.emit(Event::State(state));
    }

    /// Broadcasts the state if it differs from the last one sent. Runs after
    /// every command and file change, so a path that changed something and
    /// then failed (a play the device refused after the old playback
    /// stopped, a seek into a full queue) or changed only the history (an
    /// undo of a comment-only save) still reaches every client.
    fn publish_if_stale(&mut self) {
        if self.last_state.as_ref() != Some(&self.state()) {
            self.emit_state();
        }
    }

    /// Sends `event` to every subscriber, dropping those that went away.
    fn emit(&mut self, event: Event) {
        self.subscribers.retain(|s| s.send(event.clone()).is_ok());
    }
}

/// Watches `path` and forwards its changes into the actor's queue, tagged
/// with `epoch`. The forwarding thread ends with the watcher.
fn watch(path: &Path, tx: Sender<Msg>, epoch: u64) -> Result<SongWatcher, StudioError> {
    let (raw_tx, raw_rx) = mpsc::channel();
    let watcher = SongWatcher::start_raw(path, raw_tx)
        .map_err(|e| StudioError::Io(format!("cannot watch {}: {e}", path.display())))?;
    std::thread::Builder::new()
        .name("studio-watch".into())
        .spawn(move || {
            for reload in raw_rx {
                if tx.send(Msg::Disk { epoch, reload }).is_err() {
                    return;
                }
            }
        })
        .map_err(|e| StudioError::Io(format!("cannot start the watch thread: {e}")))?;
    Ok(watcher)
}

/// The track index and param target of `track`'s `param` in `song`.
fn param_target(song: &CompiledSong, track: &str, param: &str) -> Option<(usize, ParamTarget)> {
    let index = song.track_index(track)?;
    let (target, _) = song.param_target(index, param)?;
    Some((index, target))
}

/// `path` with `.` and `..` resolved by name alone (no filesystem access).
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `bars` clamped to a song of `total` bars; `None` once empty.
fn clamp_loop(bars: Option<[u32; 2]>, total: u32) -> Option<[u32; 2]> {
    let [start, end] = bars?;
    let end = end.min(total);
    (start < end).then_some([start, end])
}

fn audio_error(e: LiveError) -> StudioError {
    StudioError::Audio(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loops_clamp_to_the_song() {
        assert_eq!(clamp_loop(Some([2, 4]), 4), Some([2, 4]));
        assert_eq!(clamp_loop(Some([2, 6]), 4), Some([2, 4]));
        assert_eq!(clamp_loop(Some([2, 4]), 2), None);
        assert_eq!(clamp_loop(Some([3, 3]), 4), None);
        assert_eq!(clamp_loop(Some([3, 1]), 4), None);
        assert_eq!(clamp_loop(None, 4), None);
    }

    #[test]
    fn normalize_resolves_dots_by_name() {
        assert_eq!(
            normalize(Path::new("/r/songs/../../x/./song.toml")),
            Path::new("/x/song.toml")
        );
        assert_eq!(normalize(Path::new("a/./b/../c")), Path::new("a/c"));
    }

    #[test]
    fn handle_is_send_and_sync() {
        fn both<T: Send + Sync>() {}
        both::<StudioHandle>();
    }
}
