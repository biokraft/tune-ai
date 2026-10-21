//! `tune play`: realtime playback with hot reload.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde_json::{Value, json};
use tune_audio::output;
use tune_engine::{CompiledSong, live};
use tune_song::{Reload, SongError, SongWatcher};

use super::{load, sample_rate};
use crate::report::{Failure, finish, parse_bar_range};

pub use tune_audio::Device;

pub struct Args {
    pub song: PathBuf,
    pub from_bar: u32,
    pub looping: Option<String>,
    pub device: Device,
    pub json: bool,
}

/// Prints one event: a JSON line, or the text form.
fn emit(json: bool, text: &str, value: Value) {
    if json {
        println!("{value}");
    } else {
        println!("{text}");
    }
}

fn fail(json: bool, msg: impl Into<String>) -> ExitCode {
    finish(json, Err(vec![Failure::msg(msg)]))
}

fn report_reload_error(json: bool, failures: &[Failure]) {
    if json {
        let list: Vec<Value> = failures.iter().map(Failure::to_json).collect();
        println!("{}", json!({ "event": "reload_error", "errors": list }));
    } else {
        println!("reload failed, still playing the previous version:");
        for f in failures {
            println!("{}", f.to_text());
        }
    }
}

fn song_failures(file: &Path, errors: &[SongError]) -> Vec<Failure> {
    errors.iter().map(|e| Failure::at(file, e)).collect()
}

pub fn run(a: &Args) -> ExitCode {
    let song = match load(&a.song) {
        Ok(s) => s,
        Err(e) => return finish(a.json, Err(e)),
    };
    let total = song.total_bars();
    if a.from_bar < 1 || a.from_bar > total {
        return fail(
            a.json,
            format!("--from-bar {} is past the end ({total} bars)", a.from_bar),
        );
    }
    let looping = match &a.looping {
        Some(s) => match parse_bar_range(s, total) {
            Ok(r) => Some(r),
            Err(m) => return fail(a.json, m.replace("--bars", "--loop")),
        },
        None => None,
    };

    // The device decides the rate; the song compiles at it.
    let mut out = output(a.device);
    let wanted = sample_rate(&song);
    let rate = match out.sample_rate(wanted) {
        Ok(r) => r,
        Err(m) => return fail(a.json, m),
    };
    if rate != wanted {
        eprintln!("note: playing at {rate} Hz (song asks {wanted})");
    }

    let compiled = Arc::new(CompiledSong::new(&song, rate));
    let (mut controller, processor) = live(compiled, a.from_bar - 1, looping);

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        if let Err(e) = ctrlc::set_handler(move || stop.store(true, Ordering::Relaxed)) {
            return fail(a.json, format!("cannot install the Ctrl-C handler: {e}"));
        }
    }

    let (tx, rx) = mpsc::channel();
    let _watcher = match SongWatcher::start(&a.song, tx) {
        Ok(w) => w,
        Err(e) => return fail(a.json, format!("cannot watch `{}`: {e}", a.song.display())),
    };

    let _running = match out.start(processor) {
        Ok(r) => r,
        Err(m) => return fail(a.json, m),
    };

    emit(
        a.json,
        &format!(
            "playing {} from bar {} at {} BPM ({rate} Hz, {}) - Ctrl-C to stop",
            song.name,
            a.from_bar,
            song.bpm,
            a.device.name()
        ),
        json!({
            "event": "playing",
            "song": song.name,
            "bar": a.from_bar,
            "bpm": song.bpm,
            "sample_rate": rate,
            "device": a.device.name(),
        }),
    );

    let mut seen_swaps = controller.swaps_applied();
    loop {
        if out.device_lost() {
            return fail(a.json, "audio device lost");
        }
        if stop.load(Ordering::Relaxed) {
            let bar = controller.bar() + 1;
            emit(
                a.json,
                &format!("stopped at bar {bar}"),
                json!({ "event": "stopped", "bar": bar }),
            );
            return ExitCode::SUCCESS;
        }
        while let Ok(reload) = rx.try_recv() {
            match reload {
                Reload::Ok(new) => {
                    let compiled = Arc::new(CompiledSong::new(&new, rate));
                    if let Err(e) = controller.swap(compiled) {
                        report_reload_error(a.json, &[Failure::msg(e.to_string())]);
                    }
                }
                Reload::Invalid(errors) => {
                    report_reload_error(a.json, &song_failures(&a.song, &errors));
                }
                Reload::Io(m) => report_reload_error(a.json, &[Failure::msg(m)]),
            }
        }
        let swaps = controller.swaps_applied();
        if swaps > seen_swaps {
            seen_swaps = swaps;
            let bar = controller.bar() + 1;
            emit(
                a.json,
                &format!("reloaded at bar {bar}"),
                json!({ "event": "reloaded", "bar": bar }),
            );
        }
        controller.collect_garbage();
        if controller.finished() {
            emit(a.json, "finished", json!({ "event": "finished" }));
            return ExitCode::SUCCESS;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
