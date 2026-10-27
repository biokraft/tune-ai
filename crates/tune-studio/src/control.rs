//! The control endpoint: how `tune do` and `tune set` reach a running
//! studio, so Claude's edits show up at once and are one undo step each.
//!
//! A studio listens on `127.0.0.1` (never another address) on a free port
//! and names it in `<dir>/studio-<pid>-<port>.json`, with a secret of 64
//! hex digits. The directory is made `0700` and the file is created `0600`
//! (never readable by others, not even briefly), so only the user can read
//! the secret. The protocol is one JSON
//! object per line each way and one request per connection; a line is at
//! most 4 MiB and the whole request must arrive within 5 s:
//!
//! - `{"secret": "…", "op": "song"}` → `{"ok": true, "song": "<canonical path>" | null}`
//! - `{"secret": "…", "op": "do", "song": "<canonical path>", "actions": [Action…], "source": "claude", "run": RunTag | null}`
//!   (`source` is always `claude`; any other is a bad request)
//!   → `{"ok": true, "outcome": ActionsOutcome}` or `{"ok": false, "error": "<message>"}`;
//!   the studio checks `song` against its open song as it runs the actions,
//!   so a song switch since `op: song` refuses the request instead of
//!   editing the other song.
//!
//! A first line that is not JSON, lacks `secret`, or carries another one
//! (compared in constant time) gets `{"ok": false, "error": "refused"}` and
//! the connection closes; nothing reaches the studio. A browser cannot pass:
//! its request starts with an HTTP request line, never with JSON. The secret
//! is never printed: neither handle's `Debug` shows it and no error names it.

use std::fmt;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::action::{Action, ActionsOutcome, Origin, RunTag};
use crate::error::StudioError;
use crate::log::Source;
use crate::studio::StudioHandle;

/// The environment variable that moves the endpoint directory (tests point
/// it at a temp dir).
pub const STUDIO_DIR_ENV: &str = "TUNE_STUDIO_DIR";
/// The longest line either side reads.
const MAX_LINE: usize = 4 * 1024 * 1024;
/// How long a whole request (or reply) may take to arrive.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// How long `find_studio` tries to connect before calling a file stale.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(200);
/// Connections served at once; more are closed at once.
const MAX_CONNECTIONS: usize = 16;
/// What a request without the right secret gets.
const REFUSED: &str = "refused";

/// The endpoint directory: `$TUNE_STUDIO_DIR`, else `$HOME/.tune/studios`.
pub fn studio_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(STUDIO_DIR_ENV).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map_or_else(PathBuf::new, PathBuf::from);
    home.join(".tune").join("studios")
}

/// The secret: 64 lowercase hex digits. Its `Debug` hides it.
#[derive(Clone)]
struct Secret(String);

impl Secret {
    /// 32 bytes from the operating system's random source.
    fn generate() -> io::Result<Self> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|e| io::Error::other(format!("no random source: {e}")))?;
        Ok(Self(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// Whether `given` is this secret, in time that depends only on the
    /// secret's length.
    fn matches(&self, given: &str) -> bool {
        let (want, given) = (self.0.as_bytes(), given.as_bytes());
        let mut diff = want.len() ^ given.len();
        for (i, w) in want.iter().enumerate() {
            diff |= usize::from(w ^ given.get(i).copied().unwrap_or(0));
        }
        std::hint::black_box(diff) == 0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// The endpoint file's contents.
#[derive(Serialize, Deserialize)]
struct EndpointFile {
    port: u16,
    secret: String,
    pid: u32,
}

/// A studio's control endpoint. Dropping it stops the listener and removes
/// the endpoint file.
pub struct ControlServer {
    port: u16,
    file: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for ControlServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ControlServer")
            .field("port", &self.port)
            .field("file", &self.file)
            .finish_non_exhaustive()
    }
}

impl ControlServer {
    /// Listens on `127.0.0.1` for the studio behind `handle` and writes the
    /// endpoint file into `dir`, which is created (or set) `0700`.
    pub fn start(handle: StudioHandle, dir: &Path) -> io::Result<Self> {
        DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        // An existing directory is made private too (failing, say, on a
        // directory of another user's).
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let secret = Secret::generate()?;
        let file = dir.join(format!("studio-{}-{port}.json", std::process::id()));
        write_endpoint_file(&file, port, &secret)?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("studio-control".into())
                .spawn(move || serve(&listener, &handle, &secret, &stop))
        };
        let thread = match thread {
            Ok(t) => t,
            Err(e) => {
                let _ = fs::remove_file(&file);
                return Err(e);
            }
        };
        Ok(Self {
            port,
            file,
            stop,
            thread: Some(thread),
        })
    }

    /// The port the endpoint listens on.
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.file);
        self.stop.store(true, Ordering::SeqCst);
        // Wake the listener so it sees the flag. Should that fail, leave the
        // thread rather than wait for an accept that may never come.
        let woken = TcpStream::connect_timeout(&local(self.port), Duration::from_secs(1)).is_ok();
        if let Some(thread) = self.thread.take()
            && woken
        {
            let _ = thread.join();
        }
    }
}

/// Writes the endpoint file `0600` from the start: into a temp file created
/// with that mode, renamed into place, so no reader sees it half written.
fn write_endpoint_file(file: &Path, port: u16, secret: &Secret) -> io::Result<()> {
    let body = serde_json::to_string(&EndpointFile {
        port,
        secret: secret.0.clone(),
        pid: std::process::id(),
    })
    .map_err(io::Error::other)?;
    let tmp = file.with_extension("json.tmp");
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(body.as_bytes()))
        .and_then(|()| fs::rename(&tmp, file));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

fn local(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

/// Accepts connections until `stop`, each on its own short-lived thread.
fn serve(listener: &TcpListener, handle: &StudioHandle, secret: &Secret, stop: &AtomicBool) {
    let open = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else {
            // Out of file descriptors, say: let some connections end.
            std::thread::sleep(Duration::from_millis(10));
            continue;
        };
        let (slot, open_before) = ConnectionSlot::take(&open);
        if open_before >= MAX_CONNECTIONS {
            continue;
        }
        let (handle, secret) = (handle.clone(), secret.clone());
        // Should the spawn fail, the closure and so the slot are dropped.
        let _ = std::thread::Builder::new()
            .name("studio-control-conn".into())
            .spawn(move || {
                let _slot = slot;
                connection(stream, &handle, &secret);
            });
    }
}

/// One of the [`MAX_CONNECTIONS`] slots, given back when dropped: when the
/// connection ends, also by a panic.
struct ConnectionSlot(Arc<AtomicUsize>);

impl ConnectionSlot {
    /// Takes a slot and says how many were taken before it.
    fn take(open: &Arc<AtomicUsize>) -> (Self, usize) {
        let before = open.fetch_add(1, Ordering::SeqCst);
        (Self(open.clone()), before)
    }
}

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A request after the secret.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Song,
    Do {
        /// The song the client found the studio for (canonical).
        song: PathBuf,
        actions: Vec<Action>,
        source: Source,
        #[serde(default)]
        run: Option<RunTag>,
    },
}

/// One connection: one request line, one reply line.
fn connection(mut stream: TcpStream, handle: &StudioHandle, secret: &Secret) {
    let _ = stream.set_write_timeout(Some(READ_TIMEOUT));
    let reply = match read_line(&mut stream) {
        Ok(line) => answer(&line, handle, secret),
        Err(_) => refused(),
    };
    let _ = stream.write_all(format!("{reply}\n").as_bytes());
}

fn refused() -> Value {
    json!({ "ok": false, "error": REFUSED })
}

fn failed(message: impl fmt::Display) -> Value {
    json!({ "ok": false, "error": message.to_string() })
}

/// The studio's error as a reply. A studio busy past the call's 5 s did
/// not answer, which the client must hear as such, not as `timed out`.
fn studio_failed(e: StudioError) -> Value {
    match e {
        StudioError::Timeout => failed(StudioError::NoAnswer),
        other => failed(other),
    }
}

/// Checks the secret before anything else, then runs the request.
fn answer(line: &[u8], handle: &StudioHandle, secret: &Secret) -> Value {
    let Ok(mut request) = serde_json::from_slice::<Value>(line) else {
        return refused();
    };
    let given = request
        .get("secret")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !secret.matches(given) {
        return refused();
    }
    if let Some(fields) = request.as_object_mut() {
        fields.remove("secret");
    }
    let request: Request = match serde_json::from_value(request) {
        Ok(r) => r,
        Err(e) => return failed(format!("bad request: {e}")),
    };
    match request {
        Request::Song => match handle.song_path() {
            Ok(path) => {
                let song = path.and_then(|p| fs::canonicalize(p).ok());
                json!({ "ok": true, "song": song })
            }
            Err(e) => studio_failed(e),
        },
        Request::Do {
            song,
            actions,
            source,
            run,
        } => {
            // Only an agent acts here: never the producer, and not as
            // `external`, which the log keeps for changes on disk.
            if source != Source::Claude {
                return failed("bad request: the control endpoint acts only as `claude`");
            }
            // The wire skips `RunTag::new`'s checks; run them here.
            let run = match run
                .map(|r| RunTag::new(&r.id, r.title.as_deref()))
                .transpose()
            {
                Ok(run) => run,
                Err(e) => return failed(e),
            };
            match handle.run_actions_on(Some(song), actions, Origin { source, run }) {
                Ok(outcome) => json!({ "ok": true, "outcome": outcome }),
                Err(e) => studio_failed(e),
            }
        }
    }
}

/// Reads one line (without its `\n`) of at most [`MAX_LINE`] bytes, all of
/// it within [`READ_TIMEOUT`]. A line cut short by the peer closing counts.
fn read_line(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let deadline = Instant::now() + READ_TIMEOUT;
    let mut line = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(ErrorKind::TimedOut.into());
        }
        stream.set_read_timeout(Some(left))?;
        let n = match stream.read(&mut chunk) {
            Ok(0) if line.is_empty() => return Err(ErrorKind::UnexpectedEof.into()),
            Ok(0) => return Ok(line),
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let got = &chunk[..n];
        let end = got.iter().position(|&b| b == b'\n');
        line.extend_from_slice(&got[..end.unwrap_or(n)]);
        if line.len() > MAX_LINE {
            return Err(io::Error::new(ErrorKind::InvalidData, "line too long"));
        }
        if end.is_some() {
            return Ok(line);
        }
    }
}

/// A running studio's endpoint, found by [`find_studio`].
pub struct StudioLink {
    port: u16,
    secret: Secret,
    /// The song it was found for (canonical); every `do` names it.
    song: PathBuf,
}

impl fmt::Debug for StudioLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StudioLink")
            .field("port", &self.port)
            .field("song", &self.song)
            .finish_non_exhaustive()
    }
}

/// The studio that has `song` open, if one runs: reads every
/// `studio-*.json` in `dir`, newest first, skips those it cannot connect to
/// within 200 ms (deleting them when the connection is refused: nothing
/// listens there any more), and returns the first whose song is `song` (both
/// canonical).
pub fn find_studio(dir: &Path, song: &Path) -> Option<StudioLink> {
    let song = fs::canonicalize(song).ok()?;
    for file in endpoint_files(dir) {
        let Some(link) = fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<EndpointFile>(&text).ok())
            .map(|e| StudioLink {
                port: e.port,
                secret: Secret(e.secret),
                song: song.clone(),
            })
        else {
            continue;
        };
        let stream = match TcpStream::connect_timeout(&local(link.port), CONNECT_TIMEOUT) {
            Ok(s) => s,
            Err(e) => {
                if left_behind(&e) {
                    let _ = fs::remove_file(&file);
                }
                continue;
            }
        };
        let reply = link.exchange(stream, &json!({ "op": "song" }));
        let open = reply
            .ok()
            .filter(|r| r["ok"] == true)
            .and_then(|r| r["song"].as_str().map(PathBuf::from));
        if open.as_deref() == Some(song.as_path()) {
            return Some(link);
        }
    }
    None
}

/// Whether a failed connect says nothing listens on the port: the studio
/// that wrote the file is gone. A timeout or any other error may be a busy
/// or slow studio, whose file stays.
fn left_behind(e: &io::Error) -> bool {
    e.kind() == ErrorKind::ConnectionRefused
}

/// The `studio-*.json` files in `dir`, newest first.
fn endpoint_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("studio-") && n.ends_with(".json"))
        })
        .map(|e| {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (modified, e.path())
        })
        .collect();
    files.sort_by(|a, b| b.cmp(a));
    files.into_iter().map(|(_, p)| p).collect()
}

impl StudioLink {
    /// Runs `actions` in the studio as `origin`, if it still has the song
    /// open that it was found for (else the studio refuses with
    /// [`StudioError::SongChanged`]'s message). Once the request is sent,
    /// no answer within 5 s is [`StudioError::NoAnswer`], and the studio's
    /// refusal is its message; neither may be retried against the file.
    pub fn run(&self, actions: &[Action], origin: &Origin) -> Result<ActionsOutcome, StudioError> {
        let stream = TcpStream::connect_timeout(&local(self.port), READ_TIMEOUT)
            .map_err(|e| StudioError::Io(format!("cannot reach the studio: {e}")))?;
        let request = json!({
            "op": "do",
            "song": self.song,
            "actions": actions,
            "source": origin.source,
            "run": origin.run,
        });
        let reply = self.exchange(stream, &request)?;
        if reply["ok"] == true {
            serde_json::from_value(reply["outcome"].clone()).map_err(|_| StudioError::NoAnswer)
        } else {
            // The studio's own message (`StudioError`'s text there).
            let message = reply["error"].as_str().unwrap_or("the studio refused");
            Err(StudioError::Io(message.to_owned()))
        }
    }

    /// Sends `request` with the secret as one line and reads the one-line
    /// reply. Anything but a JSON reply in time is
    /// [`StudioError::NoAnswer`].
    fn exchange(&self, mut stream: TcpStream, request: &Value) -> Result<Value, StudioError> {
        let mut request = request.clone();
        request["secret"] = Value::from(self.secret.0.as_str());
        let mut line = serde_json::to_vec(&request)
            .map_err(|e| StudioError::Io(format!("cannot encode the request: {e}")))?;
        line.push(b'\n');
        stream
            .set_write_timeout(Some(READ_TIMEOUT))
            .and_then(|()| stream.write_all(&line))
            .map_err(|e| StudioError::Io(format!("cannot reach the studio: {e}")))?;
        let reply = read_line(&mut stream).map_err(|_| StudioError::NoAnswer)?;
        serde_json::from_slice(&reply).map_err(|_| StudioError::NoAnswer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connection_slot_comes_back_after_a_panic() {
        let open = Arc::new(AtomicUsize::new(0));
        let (slot, before) = ConnectionSlot::take(&open);
        assert_eq!((before, open.load(Ordering::SeqCst)), (0, 1));
        let panicked = std::thread::spawn(move || {
            let _slot = slot;
            // Unwinds as a panic does, without printing one.
            std::panic::resume_unwind(Box::new("a connection fails"));
        })
        .join();
        assert!(panicked.is_err());
        assert_eq!(open.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn only_a_refused_connection_marks_a_file_stale() {
        assert!(left_behind(&ErrorKind::ConnectionRefused.into()));
        for kind in [
            ErrorKind::TimedOut,
            ErrorKind::WouldBlock,
            ErrorKind::PermissionDenied,
        ] {
            assert!(!left_behind(&kind.into()), "{kind:?}");
        }
    }

    #[test]
    fn a_studio_timeout_is_no_answer() {
        assert_eq!(
            studio_failed(StudioError::Timeout),
            json!({ "ok": false, "error": "the studio did not answer; check it before retrying" })
        );
        assert_eq!(
            studio_failed(StudioError::NoSong),
            json!({ "ok": false, "error": "no song is open" })
        );
    }

    #[test]
    fn secrets_compare_whole() {
        let s = Secret("ab".repeat(32));
        assert!(s.matches(&"ab".repeat(32)));
        assert!(!s.matches(&"ab".repeat(31)));
        assert!(!s.matches(&format!("{}0", "ab".repeat(32))));
        assert!(!s.matches(""));
        assert!(!s.matches(&format!("ac{}", "ab".repeat(31))));
    }

    #[test]
    fn secrets_are_hex_and_hidden() {
        let s = Secret::generate().unwrap();
        assert_eq!(s.0.len(), 64);
        assert!(
            s.0.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        assert_ne!(s.0, Secret::generate().unwrap().0);
        assert_eq!(format!("{s:?}"), "Secret(..)");
    }

    /// A connected pair: the client end and the server's.
    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        (client, listener.accept().unwrap().0)
    }

    #[test]
    fn a_line_past_4_mib_is_cut_off() {
        let (mut client, mut server) = pair();
        let writer = std::thread::spawn(move || {
            // The reader stops early, so the write may fail; that is fine.
            let _ = client.write_all(&vec![b'x'; MAX_LINE + 10]);
            client
        });
        let err = read_line(&mut server).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData);
        drop(server);
        drop(writer.join());
    }

    #[test]
    fn a_line_must_arrive_within_5_s() {
        let (mut client, mut server) = pair();
        let start = Instant::now();
        let writer = std::thread::spawn(move || {
            // A byte now and then never finishes the line in time.
            for _ in 0..7 {
                if client.write_all(b"{").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        let err = read_line(&mut server).unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock),
            "{err}"
        );
        let took = start.elapsed();
        assert!(
            (READ_TIMEOUT..READ_TIMEOUT + Duration::from_secs(1)).contains(&took),
            "{took:?}"
        );
        drop(server);
        writer.join().unwrap();
    }

    #[test]
    fn studio_dir_follows_the_variable() {
        // Read only: the variable is set by the test runner or not at all.
        let dir = studio_dir();
        match std::env::var_os(STUDIO_DIR_ENV).filter(|d| !d.is_empty()) {
            Some(d) => assert_eq!(dir, PathBuf::from(d)),
            None => assert!(dir.ends_with(".tune/studios")),
        }
    }
}
