//! `tune info`.

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use serde_json::{Map, Value, json};
use tune_engine::Clock;
use tune_song::{Clip, Device, SectionPlay, Song};
use tune_studio::placement_json;

use super::{load, note_migration, sample_rate};
use crate::report::{Success, finish, fmt_time};

pub fn run(path: &Path, json: bool) -> ExitCode {
    finish(
        json,
        load(path).map(|loaded| {
            let song = loaded.song;
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
            for (i, (s, start)) in song
                .sections
                .iter()
                .zip(song.section_start_bars())
                .enumerate()
            {
                let start_seconds = clock.seconds(clock.bar_start(start));
                let (play, play_json) = section_play(&song, i);
                let automation: Vec<String> = song
                    .lanes_in(i)
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
                    "play": Value::Object(play_json),
                    "automation": automation,
                }));
            }
            text.push_str("\ntracks:");
            let mut tracks = Vec::new();
            for (id, t) in &song.tracks {
                let fx: Vec<&str> = t.fx.iter().map(|d| d.module.name).collect();
                let fx_shown: Vec<String> = t.fx.iter().map(shown).collect();
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
                    shown(&t.instrument),
                    t.gain_db,
                    t.pan,
                    if fx.is_empty() {
                        "-".into()
                    } else {
                        fx_shown.join(" ")
                    },
                );
                tracks.push(json!({
                    "id": id,
                    "instrument": t.instrument.module.name,
                    "gain_db": t.gain_db,
                    "pan": t.pan,
                    "mute": t.mute,
                    "solo": t.solo,
                    "fx": fx,
                    "devices": {
                        "instrument": device_json(&t.instrument),
                        "fx": t.fx.iter().map(device_json).collect::<Vec<_>>(),
                    },
                    "placements": t.play.iter().map(placement_json).collect::<Vec<_>>(),
                }));
            }
            let mut clips = Vec::new();
            let mut clip_texts = Vec::new();
            for (id, clip) in &song.clips {
                let steps = clip.steps();
                let kind = clip.kind().name();
                match clip {
                    Clip::Notes(n) => {
                        clip_texts.push(format!(
                            "{id} {kind} {} bars {} notes",
                            n.bars,
                            n.notes.len()
                        ));
                        clips.push(json!({
                            "id": id,
                            "kind": kind,
                            "steps": steps,
                            "bars": n.bars,
                            "notes": n.notes.len(),
                        }));
                    }
                    _ => {
                        clip_texts.push(format!("{id} {kind} {steps} steps"));
                        clips.push(json!({ "id": id, "kind": kind, "steps": steps }));
                    }
                }
            }
            if !clip_texts.is_empty() {
                let _ = write!(text, "\nclips: {}", clip_texts.join(", "));
            }
            let mut success = Success {
                text,
                json: json!({
                    "name": song.name,
                    "bpm": song.bpm,
                    "sample_rate": sr,
                    "bars": bars,
                    "seconds": seconds,
                    "sections": Value::Array(sections),
                    "tracks": Value::Array(tracks),
                    "clips": Value::Array(clips),
                }),
            };
            note_migration(path, loaded.migrated_from, json, &mut success);
            success
        }),
    )
}

/// The module name, with the device id after it in parentheses when the id is
/// not the module's own prefix (`eq(eq2)`).
fn shown(d: &Device) -> String {
    if d.id == d.module.prefix {
        d.module.name.to_owned()
    } else {
        format!("{}({})", d.module.name, d.id)
    }
}

fn device_json(d: &Device) -> Value {
    json!({ "id": d.id, "type": d.module.name })
}

/// What each track plays in section `section`, in track order, as text
/// (`t=clip`, or `t=clip@<bar>+<bars>` per placement for anything but one
/// clip from its first step over the whole section, with `(offset=<bars>)` after a non-zero
/// offset; bars 1-based) and as JSON (the clip id, or `[{at, clip, bars,
/// offset}]`). Silent tracks are left out.
fn section_play(song: &Song, section: usize) -> (Vec<String>, Map<String, Value>) {
    let mut text = Vec::new();
    let mut json = Map::new();
    for t in song.tracks.keys() {
        match song.section_play(section, t) {
            SectionPlay::Silent => {}
            SectionPlay::Clip(clip) => {
                text.push(format!("{t}={clip}"));
                json.insert(t.clone(), Value::from(clip));
            }
            SectionPlay::Other(placements) => {
                let parts: Vec<String> = placements
                    .iter()
                    .map(|p| {
                        let mut part = format!("{}@{}+{}", p.clip, p.start_bar + 1, p.bars);
                        if p.offset_bars != 0 {
                            let _ = write!(part, "(offset={})", p.offset_bars);
                        }
                        part
                    })
                    .collect();
                text.push(format!("{t}={}", parts.join(",")));
                let list = placements.into_iter().map(placement_json).collect();
                json.insert(t.clone(), Value::Array(list));
            }
        }
    }
    (text, json)
}
