#![forbid(unsafe_code)]
//! The tune studio in a native window.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use tauri::{Emitter, Manager, State};
use tune_audio::CpalOut;
use tune_studio::{Command, ControlServer, Studio, StudioConfig, StudioHandle};

/// The studio's control endpoint, so `tune do` and `tune set` on the open
/// song go through the app. Taken at exit: Tauri does not drop managed
/// state, and dropping the server removes its endpoint file.
struct Control(Mutex<Option<ControlServer>>);

/// The song to open at start: `--song <path>` first, then the `TUNE_SONG`
/// value. An empty value counts as unset.
pub fn song_arg(args: &[String], env: Option<String>) -> Option<PathBuf> {
    let flag = args
        .iter()
        .position(|a| a == "--song")
        .and_then(|i| args.get(i + 1))
        .filter(|v| !v.is_empty());
    flag.cloned()
        .or(env.filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}

/// The one command the frontend calls: runs a studio `Command`.
#[tauri::command]
async fn studio(
    command: Command,
    state: State<'_, StudioHandle>,
) -> Result<serde_json::Value, String> {
    let handle = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle.call(command))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Writes pending edits and stops playback and the file watcher. Runs once,
/// when the app exits: Tauri ends the process without dropping managed state.
pub fn on_exit(handle: &StudioHandle) {
    handle.shutdown();
}

/// Stops the control endpoint and removes its file, before the studio stops.
fn stop_control(control: &Control) {
    let server = control
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    drop(server);
}

/// Opens the window with the studio behind it.
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let songs_root = match std::env::var_os("TUNE_ROOT") {
                Some(root) => PathBuf::from(root),
                None => std::env::current_dir()?,
            };
            let args: Vec<String> = std::env::args().collect();
            let song = song_arg(&args, std::env::var("TUNE_SONG").ok());
            let handle = Studio::spawn(
                StudioConfig {
                    audio: Box::new(CpalOut::new()),
                    songs_root,
                },
                song,
            )?;
            let events = handle.subscribe();
            let emitter = app.handle().clone();
            std::thread::Builder::new()
                .name("studio-events".into())
                .spawn(move || {
                    for event in events {
                        if emitter.emit("studio-event", &event).is_err() {
                            break;
                        }
                    }
                })?;
            // Without it the CLI edits the file instead: log it, go on.
            let control = ControlServer::start_or_warn(handle.clone())
                .map_err(|warning| eprintln!("{warning}"))
                .ok();
            app.manage(Control(Mutex::new(control)));
            app.manage(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![studio])
        .build(tauri::generate_context!())
        .expect("error while building tune");
    app.run(|app, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(control) = app.try_state::<Control>() {
                stop_control(&control);
            }
            if let Some(handle) = app.try_state::<StudioHandle>() {
                on_exit(&handle);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tune_studio::Action;

    const MINI: &str = include_str!("../../../../crates/tune-song/tests/fixtures/mini.toml");

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn on_exit_flushes_pending_edit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("song.toml");
        std::fs::write(&path, MINI).expect("write song");
        let handle = Studio::spawn(
            StudioConfig {
                audio: Box::new(tune_audio::NullOut::new()),
                songs_root: dir.path().to_owned(),
            },
            Some(path.clone()),
        )
        .expect("spawns");
        handle
            .call(Command::Action(Action::SetParam {
                track: "acid".into(),
                param: "acid303.cutoff".into(),
                value: 823.456,
                commit: true,
            }))
            .expect("commit");
        // The 150 ms debounce has not fired yet.
        assert!(!std::fs::read_to_string(&path).unwrap().contains("823.5"));
        on_exit(&handle);
        assert!(std::fs::read_to_string(&path).unwrap().contains("823.5"));
    }

    #[test]
    fn the_control_endpoint_comes_and_goes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("song.toml");
        std::fs::write(&path, MINI).expect("write song");
        let handle = Studio::spawn(
            StudioConfig {
                audio: Box::new(tune_audio::NullOut::new()),
                songs_root: dir.path().to_owned(),
            },
            Some(path.clone()),
        )
        .expect("spawns");
        let studios = dir.path().join("studios");
        let control = Control(Mutex::new(Some(
            ControlServer::start(handle.clone(), &studios).expect("starts"),
        )));
        assert!(tune_studio::find_studio(&studios, &path).is_some());
        stop_control(&control);
        assert_eq!(std::fs::read_dir(&studios).unwrap().count(), 0);
        on_exit(&handle);
    }

    #[test]
    fn song_arg_prefers_flag() {
        let got = song_arg(
            &args(&["tune-desktop", "--song", "a.toml"]),
            Some("b.toml".into()),
        );
        assert_eq!(got, Some(PathBuf::from("a.toml")));
    }

    #[test]
    fn song_arg_env_fallback() {
        let got = song_arg(&args(&["tune-desktop"]), Some("b.toml".into()));
        assert_eq!(got, Some(PathBuf::from("b.toml")));
    }

    #[test]
    fn song_arg_none() {
        assert_eq!(song_arg(&args(&["tune-desktop"]), None), None);
        assert_eq!(song_arg(&args(&["tune-desktop", "--song"]), None), None);
    }
}
