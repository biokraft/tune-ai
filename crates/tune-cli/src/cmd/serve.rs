//! `tune serve`: the studio over local HTTP and WebSocket, so the UI runs in
//! a normal browser.
//!
//! `GET /ws` carries the studio protocol. A client sends
//! `{"id": <u64>, "cmd": …, "args": …}` (the [`Command`] serde form plus an
//! `id`) and gets `{"reply": <id>, "ok": true, "result": …}` or
//! `{"reply": <id or null>, "ok": false, "error": "…"}`. Every [`Event`]
//! arrives as its serde form, starting with the current state. Every other
//! `GET` serves a file from the UI directory (`index.html` when nothing
//! matches).
//!
//! The server listens on `127.0.0.1` only and refuses a WebSocket upgrade
//! whose `Origin` is not its own page (or the vite dev server), so a web page
//! on another site cannot drive the studio.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tower_http::services::{ServeDir, ServeFile};
use tune_audio::output;
use tune_studio::{Command, Event, Studio, StudioConfig, StudioHandle};

use crate::report::{Failure, finish};

pub use tune_audio::Device;

/// The vite dev server's page, which also talks to a running `tune serve`.
const VITE_ORIGIN: &str = "http://localhost:1420";
/// How often a forwarding thread checks whether its connection is gone.
const FORWARD_POLL: Duration = Duration::from_millis(500);

pub struct Args {
    pub song: PathBuf,
    pub port: u16,
    pub device: Device,
    pub ui: PathBuf,
    pub json: bool,
}

struct App {
    handle: StudioHandle,
    allowed_origins: Vec<String>,
}

fn fail(json: bool, msg: impl Into<String>) -> ExitCode {
    finish(json, Err(vec![Failure::msg(msg)]))
}

pub fn run(a: &Args) -> ExitCode {
    if !a.ui.join("index.html").is_file() {
        return fail(a.json, "UI not built: run `task ui:build`");
    }
    let songs_root = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => return fail(a.json, format!("cannot find the working directory: {e}")),
    };
    let handle = match Studio::spawn(
        StudioConfig {
            audio: output(a.device),
            songs_root,
        },
        Some(a.song.clone()),
    ) {
        Ok(h) => h,
        Err(e) => return fail(a.json, e.to_string()),
    };
    let result = serve(a, &handle);
    handle.shutdown();
    match result {
        Ok(()) => {
            if a.json {
                println!("{}", json!({ "event": "stopped" }));
            } else {
                println!("stopped");
            }
            ExitCode::SUCCESS
        }
        Err(m) => fail(a.json, m),
    }
}

/// Listens until Ctrl-C.
fn serve(a: &Args, handle: &StudioHandle) -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| format!("cannot start the async runtime: {e}"))?;
    let result = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", a.port))
            .await
            .map_err(|e| format!("cannot listen on 127.0.0.1:{}: {e}", a.port))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("cannot read the listening port: {e}"))?
            .port();
        let url = format!("http://127.0.0.1:{port}/");
        let app = router(handle.clone(), &a.ui, port);
        if a.json {
            println!("{}", json!({ "event": "serving", "url": url }));
        } else {
            println!("serving {url}");
        }
        // Not a graceful shutdown: that would wait for open WebSockets.
        tokio::select! {
            r = axum::serve(listener, app) => r.map_err(|e| format!("server failed: {e}")),
            _ = tokio::signal::ctrl_c() => Ok(()),
        }
    });
    rt.shutdown_timeout(Duration::from_secs(1));
    result
}

fn router(handle: StudioHandle, ui: &std::path::Path, port: u16) -> Router {
    let app = Arc::new(App {
        handle,
        allowed_origins: vec![
            format!("http://127.0.0.1:{port}"),
            format!("http://localhost:{port}"),
            VITE_ORIGIN.to_owned(),
        ],
    });
    // ServeDir resolves request paths inside `ui` only; it refuses `..`.
    let files = ServeDir::new(ui).fallback(ServeFile::new(ui.join("index.html")));
    Router::new()
        .route("/ws", get(ws_route))
        .fallback_service(files)
        .with_state(app)
}

async fn ws_route(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    // Browsers always send an Origin on a WebSocket upgrade; no Origin means
    // a script outside any page, which is refused too.
    let allowed = headers
        .get(header::ORIGIN)
        .and_then(|o| o.to_str().ok())
        .is_some_and(|o| app.allowed_origins.iter().any(|a| a == o));
    if !allowed {
        return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
    }
    upgrade.on_upgrade(move |socket| connection(socket, app.handle.clone()))
}

/// One client: its own event stream out, its commands in, handled in order.
async fn connection(mut socket: WebSocket, handle: StudioHandle) {
    let (out_tx, mut out_rx) = mpsc::channel::<String>(256);
    let events = handle.subscribe();
    std::thread::spawn(move || forward(&events, &out_tx));
    loop {
        tokio::select! {
            out = out_rx.recv() => {
                let Some(text) = out else { break };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(text))) => {
                    let reply = answer(&handle, text.as_str()).await;
                    if socket.send(Message::Text(reply.into())).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
}

/// Moves studio events into the connection's queue until either side ends.
fn forward(events: &std::sync::mpsc::Receiver<Event>, out: &mpsc::Sender<String>) {
    loop {
        match events.recv_timeout(FORWARD_POLL) {
            Ok(event) => {
                let Ok(text) = serde_json::to_string(&event) else {
                    continue;
                };
                if out.blocking_send(text).is_err() {
                    return;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if out.is_closed() {
                    return;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn reply_err(id: Value, error: impl std::fmt::Display) -> String {
    json!({ "reply": id, "ok": false, "error": error.to_string() }).to_string()
}

/// Runs one client frame and returns the reply frame.
async fn answer(handle: &StudioHandle, text: &str) -> String {
    let mut frame: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return reply_err(Value::Null, format!("not JSON: {e}")),
    };
    let id = frame.get("id").and_then(Value::as_u64);
    if let Some(fields) = frame.as_object_mut() {
        fields.remove("id");
    }
    let Some(id) = id else {
        return reply_err(Value::Null, "a frame needs a numeric `id`");
    };
    let cmd: Command = match serde_json::from_value(frame) {
        Ok(c) => c,
        Err(e) => return reply_err(id.into(), e),
    };
    let handle = handle.clone();
    // A call blocks for up to 5 s: keep it off the async workers.
    match tokio::task::spawn_blocking(move || handle.call(cmd)).await {
        Ok(Ok(result)) => json!({ "reply": id, "ok": true, "result": result }).to_string(),
        Ok(Err(e)) => reply_err(id.into(), e),
        Err(e) => reply_err(id.into(), format!("internal error: {e}")),
    }
}
