//! `tune serve` end to end: a child process on a free port with the null
//! device (no audio plays), driven over HTTP and WebSocket.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const COMMENTED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tune-studio/tests/fixtures/commented.toml"
);

struct Server {
    child: Child,
    url: String,
    port: u16,
    song: PathBuf,
    _dir: tempfile::TempDir,
}

impl Server {
    fn spawn() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let song = dir.path().join("song.toml");
        std::fs::copy(COMMENTED, &song).unwrap();
        let ui = dir.path().join("ui");
        std::fs::create_dir(&ui).unwrap();
        std::fs::write(ui.join("index.html"), "<html>hello</html>").unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_tune"))
            .arg("serve")
            .arg(&song)
            .args(["--port", "0", "--device", "null", "--json", "--ui"])
            .arg(&ui)
            .current_dir(dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn tune serve");
        let mut first = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut first)
            .unwrap();
        let v: Value = serde_json::from_str(&first).unwrap_or_else(|_| panic!("not JSON: {first}"));
        assert_eq!(v["event"], "serving");
        let url = v["url"].as_str().unwrap().to_owned();
        assert!(url.starts_with("http://127.0.0.1:"), "url: {url}");
        let port = url
            .trim_end_matches('/')
            .rsplit(':')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        Self {
            child,
            url,
            port,
            song,
            _dir: dir,
        }
    }

    fn ws_request(&self, origin: Option<&str>) -> impl IntoClientRequest {
        let mut req = format!("ws://127.0.0.1:{}/ws", self.port)
            .into_client_request()
            .unwrap();
        if let Some(o) = origin {
            req.headers_mut().insert("Origin", o.parse().unwrap());
        }
        req
    }

    async fn connect(&self) -> Ws {
        let origin = format!("http://127.0.0.1:{}", self.port);
        let (ws, _) = tokio_tungstenite::connect_async(self.ws_request(Some(&origin)))
            .await
            .expect("websocket connects");
        Ws(ws)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Ws(Socket);

impl Ws {
    async fn next_json(&mut self) -> Value {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(5), self.0.next())
                .await
                .expect("a frame within 5 s")
                .expect("socket open")
                .expect("frame ok");
            if let Message::Text(t) = msg {
                return serde_json::from_str(t.as_str()).unwrap();
            }
        }
    }

    /// The next frame satisfying `pred`, skipping the rest (playhead ticks).
    async fn until(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..200 {
            let v = self.next_json().await;
            if pred(&v) {
                return v;
            }
        }
        panic!("no matching frame");
    }

    async fn send(&mut self, v: Value) {
        self.0.send(Message::text(v.to_string())).await.unwrap();
    }
}

#[test]
fn serves_index() {
    let s = Server::spawn();
    let mut res = ureq::get(&s.url).call().unwrap();
    assert_eq!(res.status(), 200);
    assert!(res.body_mut().read_to_string().unwrap().contains("hello"));
}

#[tokio::test]
async fn ws_first_message_is_state() {
    let s = Server::spawn();
    let mut ws = s.connect().await;
    let first = ws.next_json().await;
    assert_eq!(first["event"], "state");
    assert_eq!(first["song"]["name"], "mini");
}

#[tokio::test]
async fn ws_command_round_trip() {
    let s = Server::spawn();
    let mut ws = s.connect().await;
    ws.next_json().await;
    ws.send(json!({"id": 1, "cmd": "set_param", "args": {
        "track": "acid", "param": "acid303.cutoff", "value": 820.0, "commit": true}}))
        .await;
    let reply = ws.until(|v| v.get("reply").is_some()).await;
    assert_eq!(reply["reply"], 1);
    assert_eq!(reply["ok"], true, "reply: {reply}");

    let log = Path::new(&s.song).with_file_name("edits.jsonl");
    let start = Instant::now();
    loop {
        let text = std::fs::read_to_string(&s.song).unwrap();
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        if text.contains("\"acid303.cutoff\" = 820") && logged.contains("\"human\"") {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "not written:\n{text}\n{logged}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// `kill` (SIGTERM) stops the server like Ctrl-C: pending edits are written
/// before it exits, inside the 150 ms debounce.
#[tokio::test]
async fn sigterm_writes_pending_edits() {
    use std::os::unix::process::ExitStatusExt;
    let mut s = Server::spawn();
    let mut ws = s.connect().await;
    ws.next_json().await;
    ws.send(json!({"id": 1, "cmd": "set_param", "args": {
        "track": "acid", "param": "acid303.cutoff", "value": 823.456, "commit": true}}))
        .await;
    let reply = ws.until(|v| v.get("reply").is_some()).await;
    assert_eq!(reply["ok"], true, "reply: {reply}");

    let killed = Command::new("kill")
        .args(["-TERM", &s.child.id().to_string()])
        .status()
        .expect("run kill");
    assert!(killed.success());
    let start = Instant::now();
    let status = loop {
        if let Some(status) = s.child.try_wait().unwrap() {
            break status;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "still running");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert_eq!(status.signal(), None, "ended by the signal: {status:?}");
    let text = std::fs::read_to_string(&s.song).unwrap();
    assert!(
        text.contains("\"acid303.cutoff\" = 823.5"),
        "not written:\n{text}"
    );
}

#[tokio::test]
async fn ws_errors() {
    let s = Server::spawn();
    let mut ws = s.connect().await;
    ws.next_json().await;
    ws.send(json!({"id": 7, "cmd": "nope"})).await;
    let reply = ws.until(|v| v.get("reply").is_some()).await;
    assert_eq!(reply["reply"], 7);
    assert_eq!(reply["ok"], false);
    assert!(
        reply["error"].as_str().unwrap().contains("unknown variant"),
        "reply: {reply}"
    );

    // Not JSON at all: the reply id is null.
    ws.0.send(Message::text("{oops")).await.unwrap();
    let reply = ws.until(|v| v.get("reply").is_some()).await;
    assert_eq!(reply["reply"], Value::Null);
    assert_eq!(reply["ok"], false);
}

#[tokio::test]
async fn rejects_foreign_origin() {
    use tokio_tungstenite::tungstenite::Error;
    let s = Server::spawn();
    for origin in [
        Some("http://evil.example"),
        Some("http://127.0.0.1:1"),
        None,
    ] {
        match tokio_tungstenite::connect_async(s.ws_request(origin)).await {
            Err(Error::Http(res)) => assert_eq!(res.status(), 403, "origin {origin:?}"),
            other => panic!("origin {origin:?}: expected a 403, got {other:?}"),
        }
    }
    // The vite dev origin is allowed.
    let (_, _) = tokio_tungstenite::connect_async(s.ws_request(Some("http://localhost:1420")))
        .await
        .expect("vite origin allowed");
}

#[tokio::test]
async fn two_clients_share_state() {
    let s = Server::spawn();
    let mut a = s.connect().await;
    let mut b = s.connect().await;
    let (sa, sb) = (a.next_json().await, b.next_json().await);
    assert_eq!(sa["event"], "state");
    assert_eq!(sa["song"], sb["song"]);

    a.send(json!({"id": 1, "cmd": "set_track_mix", "args": {"track": "hat", "mute": true}}))
        .await;
    let seen = b
        .until(|v| {
            v["event"] == "state"
                && v["song"]["tracks"]
                    .as_array()
                    .is_some_and(|t| t.iter().any(|t| t["id"] == "hat" && t["mute"] == true))
        })
        .await;
    assert_eq!(seen["event"], "state");
}

#[test]
fn missing_ui_dir() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_tune"))
        .arg("serve")
        .arg(COMMENTED)
        .args(["--port", "0", "--device", "null", "--ui"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("UI not built"), "stderr: {err}");
}
