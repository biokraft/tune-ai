//! `tune validate`.

use std::path::Path;
use std::process::ExitCode;

use serde_json::json;
use tune_engine::Clock;

use super::{load, note_migration, sample_rate};
use crate::report::{Success, finish, fmt_time};

pub fn run(path: &Path, json: bool) -> ExitCode {
    finish(
        json,
        load(path).map(|loaded| {
            let song = loaded.song;
            let clock = Clock::new(song.bpm, sample_rate(&song));
            let bars = song.total_bars();
            let seconds = clock.seconds(clock.bar_start(bars));
            let mut success = Success {
                text: format!(
                    "ok: {}, {} sections, {bars} bars, {} at {} BPM",
                    song.name,
                    song.sections.len(),
                    fmt_time(seconds),
                    song.bpm
                ),
                json: json!({
                    "name": song.name,
                    "bpm": song.bpm,
                    "bars": bars,
                    "seconds": seconds,
                    "sections": song.sections.len(),
                    "tracks": song.tracks.len(),
                }),
            };
            note_migration(path, loaded.migrated_from, json, &mut success);
            success
        }),
    )
}
