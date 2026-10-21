//! Watches a song file and reports each reparse, so live playback can
//! hot-reload.
//!
//! Editors save in different ways: some write a temp file and rename it over
//! the song, some truncate and write in chunks. So the watcher observes the
//! parent directory (a rename replaces the inode a file watch would follow),
//! filters events by file name, and reads the file only after 75 ms without
//! further events.
//!
//! [`SongWatcher::start`] parses each new text into a [`Reload`].
//! [`SongWatcher::start_raw`] hands over the text itself as a [`RawReload`],
//! for a caller that parses (and edits) it in its own way.

use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use notify::{EventKind, RecursiveMode, Watcher};

use crate::{Song, SongError, parse_song};

/// Quiet period after the last event before the file is read.
const DEBOUNCE: Duration = Duration::from_millis(75);

/// The outcome of one reload attempt.
#[derive(Debug)]
pub enum Reload {
    /// The file parsed and validated.
    Ok(Song),
    /// The file was read but is not a valid song.
    Invalid(Vec<SongError>),
    /// The file could not be read (message of the IO error).
    Io(String),
}

/// One change of the file's bytes, unparsed (see [`SongWatcher::start_raw`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawReload {
    /// The file's full new text.
    Changed(String),
    /// The file could not be read, or is not UTF-8.
    Io(String),
}

enum Msg {
    /// A filesystem event that may concern the song file.
    Touched,
    Stop,
}

/// Watches one song file. Dropping it stops the watcher and its thread.
pub struct SongWatcher {
    watcher: Option<notify::RecommendedWatcher>,
    stop: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl SongWatcher {
    /// Starts watching `path`. The file's current content is the baseline:
    /// only a later change that alters the bytes sends a [`Reload`].
    pub fn start(path: &Path, tx: Sender<Reload>) -> Result<Self, notify::Error> {
        Self::start_with(path, move |read| {
            let reload = match read {
                Ok(bytes) => reload_from(&bytes),
                Err(message) => Reload::Io(message),
            };
            tx.send(reload).is_ok()
        })
    }

    /// Like [`start`](Self::start), with the same debounce and filtering,
    /// but sends the file's new text without parsing it.
    pub fn start_raw(path: &Path, tx: Sender<RawReload>) -> Result<Self, notify::Error> {
        Self::start_with(path, move |read| {
            let reload = match read.map(String::from_utf8) {
                Ok(Ok(text)) => RawReload::Changed(text),
                Ok(Err(e)) => RawReload::Io(format!("file is not valid UTF-8: {e}")),
                Err(message) => RawReload::Io(message),
            };
            tx.send(reload).is_ok()
        })
    }

    /// Starts the watcher; `report` gets each new content (or the read
    /// error) and returns `false` once nobody listens.
    fn start_with(
        path: &Path,
        report: impl FnMut(Result<Vec<u8>, String>) -> bool + Send + 'static,
    ) -> Result<Self, notify::Error> {
        let name: OsString = path
            .file_name()
            .ok_or_else(|| notify::Error::generic("song path has no file name"))?
            .to_owned();
        let dir = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_owned(),
            _ => Path::new(".").to_owned(),
        };
        let file = dir.join(&name);
        let last = std::fs::read(&file).ok();

        let (msg_tx, msg_rx) = mpsc::channel();
        let event_tx = msg_tx.clone();
        // Matching on the file name alone survives FSEvents reporting
        // `/private/var/...` for a `/var/...` path.
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                let relevant = match res {
                    Ok(ev) => {
                        !matches!(ev.kind, EventKind::Access(_))
                            && (ev.paths.is_empty()
                                || ev
                                    .paths
                                    .iter()
                                    .any(|p| p.file_name() == Some(name.as_os_str())))
                    }
                    // A backend error may hide events: re-read to be safe.
                    Err(_) => true,
                };
                if relevant {
                    let _ = event_tx.send(Msg::Touched);
                }
            })?;
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;

        let thread = std::thread::Builder::new()
            .name("song-watcher".into())
            .spawn(move || run(&file, &msg_rx, report, last))
            .map_err(notify::Error::io)?;

        Ok(Self {
            watcher: Some(watcher),
            stop: msg_tx,
            thread: Some(thread),
        })
    }
}

impl Drop for SongWatcher {
    fn drop(&mut self) {
        // Stop the event source first, then the thread.
        self.watcher.take();
        let _ = self.stop.send(Msg::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    file: &Path,
    rx: &mpsc::Receiver<Msg>,
    mut report: impl FnMut(Result<Vec<u8>, String>) -> bool,
    mut last: Option<Vec<u8>>,
) {
    let mut pending = false;
    loop {
        let msg = if pending {
            rx.recv_timeout(DEBOUNCE)
        } else {
            rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match msg {
            Ok(Msg::Touched) => pending = true,
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                pending = false;
                let read = match std::fs::read(file) {
                    Ok(bytes) => {
                        if last.as_deref() == Some(bytes.as_slice()) {
                            continue;
                        }
                        last = Some(bytes.clone());
                        Ok(bytes)
                    }
                    // Mid-rename: the next event brings the new file.
                    Err(e) if e.kind() == ErrorKind::NotFound => continue,
                    Err(e) => Err(e.to_string()),
                };
                if !report(read) {
                    return;
                }
            }
        }
    }
}

fn reload_from(bytes: &[u8]) -> Reload {
    match std::str::from_utf8(bytes) {
        Ok(src) => match parse_song(src) {
            Ok(song) => Reload::Ok(song),
            Err(errors) => Reload::Invalid(errors),
        },
        Err(e) => Reload::Invalid(vec![SongError {
            line: None,
            column: None,
            message: format!("file is not valid UTF-8: {e}"),
        }]),
    }
}
