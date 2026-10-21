//! `tune info`.

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};
use tune_engine::Clock;

use super::{load, sample_rate};
use crate::report::{Success, finish, fmt_time};

pub fn run(path: &Path, json: bool) -> ExitCode {
    finish(
        json,
        load(path).map(|song| {
            let sr = sample_rate(&song);
            let clock = Clock::new(song.bpm, sr);
            let bars = song.total_bars();
            let seconds = clock.seconds(clock.bar_start(bars));
            let mut text = format!(
                "{}: {} BPM, {sr} Hz, {bars} bars, {}\nsections:",
                song.name,
                song.bpm,
                fmt_time(seconds)
            );
            let mut sections = Vec::new();
            for (s, start) in song.sections.iter().zip(song.section_start_bars()) {
                let start_seconds = clock.seconds(clock.bar_start(start));
                let play: Vec<String> = s.play.iter().map(|(t, p)| format!("{t}={p}")).collect();
                let automation: Vec<String> = s
                    .automation
                    .iter()
                    .map(|l| format!("{}.{}", l.track, l.param))
                    .collect();
                let _ = write!(
                    text,
                    "\n  {:<10} bars {}-{}  {}  play: {}  automation: {}",
                    s.name,
                    start + 1,
                    start + s.bars,
                    fmt_time(start_seconds),
                    if play.is_empty() {
                        "-".into()
                    } else {
                        play.join(" ")
                    },
                    if automation.is_empty() {
                        "-".into()
                    } else {
                        automation.join(" ")
                    },
                );
                sections.push(json!({
                    "name": s.name,
                    "bars": s.bars,
                    "start_bar": start + 1,
                    "end_bar": start + s.bars,
                    "start_seconds": start_seconds,
                    "play": Value::Object(
                        s.play.iter().map(|(t, p)| (t.clone(), Value::String(p.clone()))).collect()
                    ),
                    "automation": automation,
                }));
            }
            text.push_str("\ntracks:");
            let mut tracks = Vec::new();
            for (id, t) in &song.tracks {
                let fx: Vec<&str> = t.fx.iter().map(|f| f.type_name()).collect();
                let mut flags = String::new();
                if t.mute {
                    flags.push_str("  muted");
                }
                if t.solo {
                    flags.push_str("  solo");
                }
                let _ = write!(
                    text,
                    "\n  {:<10} {}  gain {} dB  pan {}  fx: {}{flags}",
                    id,
                    t.instrument.name(),
                    t.gain_db,
                    t.pan,
                    if fx.is_empty() {
                        "-".into()
                    } else {
                        fx.join(" ")
                    },
                );
                tracks.push(json!({
                    "id": id,
                    "instrument": t.instrument.name(),
                    "gain_db": t.gain_db,
                    "pan": t.pan,
                    "mute": t.mute,
                    "solo": t.solo,
                    "fx": fx,
                }));
            }
            Success {
                text,
                json: json!({
                    "name": song.name,
                    "bpm": song.bpm,
                    "sample_rate": sr,
                    "bars": bars,
                    "seconds": seconds,
                    "sections": Value::Array(sections),
                    "tracks": Value::Array(tracks),
                }),
            }
        }),
    )
}
