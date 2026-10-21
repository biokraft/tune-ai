#![forbid(unsafe_code)]
//! The tune studio in a native window.

use std::path::PathBuf;

use tauri::{Emitter, Manager, State};
use tune_audio::CpalOut;
use tune_studio::{Command, Studio, StudioConfig, StudioHandle};

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

/// Opens the window with the studio behind it.
pub fn run() {
    tauri::Builder::default()
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
            app.manage(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![studio])
        .run(tauri::generate_context!())
        .expect("error while running tune");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_string()).collect()
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
