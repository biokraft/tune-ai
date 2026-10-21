//! Shared input handling for the analysis commands: a song rendered in
//! memory, or an audio file optionally mapped onto a song's bars.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use tune_analysis::{Analysis, Audio, BarMap, SectionSpan, decode_file};
use tune_engine::{CompiledSong, RenderOptions, render};

use crate::cmd::{load, sample_rate};
use crate::refs::find_refs;
use crate::report::{Failure, parse_bar_range};

/// What an analysis command measures.
pub struct Input {
    pub audio: Audio,
    pub map: Option<BarMap>,
    /// Song name, or the file stem.
    pub stem: String,
    /// `stem`, plus `-bars-A-B` when a bar range was asked for.
    pub png_name: String,
    /// `{kind: "song"|"file", path, name, bars: [a, b] | null, sample_rate, channels, seconds}`.
    pub source: Value,
    /// Things the reader should know about the input, e.g. audio past the
    /// song's end that was left out.
    pub notes: Vec<String>,
}

fn fail(message: impl Into<String>) -> Vec<Failure> {
    vec![Failure::msg(message)]
}

/// A `.toml` input is a song; anything else is audio.
pub fn is_song(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
}

/// Frames of each bar start in `range` (relative to its start), plus the end.
/// Bars are 0-based; the sections are the whole song's.
pub fn bar_map(song: &CompiledSong, range: Range<u32>) -> BarMap {
    let base = song.clock.bar_start(range.start);
    BarMap {
        first_bar: range.start,
        bar_frames: (range.start..=range.end)
            .map(|b| (song.clock.bar_start(b) - base) as usize)
            .collect(),
        partial_end: false,
        sections: song
            .sections
            .iter()
            .map(|s| SectionSpan {
                name: s.name.clone(),
                start_bar: s.start_bar,
                bars: s.bars,
            })
            .collect(),
    }
}

fn source_json(
    kind: &str,
    path: &Path,
    name: &str,
    bars: Option<(u32, u32)>,
    audio: &Audio,
) -> Value {
    json!({
        "kind": kind,
        "path": path.display().to_string(),
        "name": name,
        "bars": bars.map(|(a, b)| json!([a, b])),
        "sample_rate": audio.sample_rate,
        "channels": audio.channels.len(),
        "seconds": audio.seconds(),
    })
}

/// `.toml` loads, compiles and renders the range in memory (stems off).
/// Anything else is decoded, mapped onto `song` from `from_bar` (1-based)
/// when given, then trimmed to `bars`.
pub fn load_input(
    path: &Path,
    bars: Option<&str>,
    song: Option<&Path>,
    from_bar: Option<u32>,
) -> Result<Input, Vec<Failure>> {
    if is_song(path) {
        if song.is_some() {
            return Err(fail(
                "--song maps a WAV onto a song; the input is already a song",
            ));
        }
        if from_bar.is_some() {
            return Err(fail(
                "--from-bar maps a WAV onto a song; use --bars for a song",
            ));
        }
        load_song_input(path, bars)
    } else {
        load_file_input(path, bars, song, from_bar)
    }
}

fn load_song_input(path: &Path, bars: Option<&str>) -> Result<Input, Vec<Failure>> {
    let song = load(path)?;
    let total = song.total_bars();
    let range = match bars {
        Some(s) => parse_bar_range(s, total).map_err(fail)?,
        None => 0..total,
    };
    let compiled = Arc::new(CompiledSong::new(&song, sample_rate(&song)));
    let opts = RenderOptions {
        start_bar: range.start,
        end_bar: range.end,
        stems: false,
    };
    let rendered = render(Arc::clone(&compiled), &opts).map_err(|e| fail(e.to_string()))?;
    let audio = Audio::from_interleaved(rendered.sample_rate, 2, &rendered.mix)
        .map_err(|e| fail(e.to_string()))?;
    let map = bar_map(&compiled, range.clone());
    let stem = song.name.clone();
    let png_name = match bars {
        Some(_) => format!("{stem}-bars-{}-{}", range.start + 1, range.end),
        None => stem.clone(),
    };
    let source = source_json(
        "song",
        path,
        &stem,
        Some((range.start + 1, range.end)),
        &audio,
    );
    Ok(Input {
        audio,
        map: Some(map),
        stem,
        png_name,
        source,
        notes: Vec::new(),
    })
}

fn load_file_input(
    path: &Path,
    bars: Option<&str>,
    song_path: Option<&Path>,
    from_bar: Option<u32>,
) -> Result<Input, Vec<Failure>> {
    const NEEDS_MAP: &str = "needs a bar map: analyse the song, or pass --song with the WAV";
    if song_path.is_none() {
        if from_bar.is_some() {
            return Err(fail(
                "--from-bar needs --song: the song the WAV was rendered from",
            ));
        }
        if bars.is_some() {
            return Err(fail(format!("--bars {NEEDS_MAP}")));
        }
    }
    let audio = decode_file(path).map_err(|e| fail(e.to_string()))?;
    let stem = path
        .file_stem()
        .map_or_else(|| "audio".to_string(), |s| s.to_string_lossy().into_owned());
    let Some(song_path) = song_path else {
        let source = source_json("file", path, &stem, None, &audio);
        return Ok(Input {
            audio,
            map: None,
            png_name: stem.clone(),
            stem,
            source,
            notes: Vec::new(),
        });
    };

    let song = load(song_path)?;
    let sr = sample_rate(&song);
    if audio.sample_rate != sr {
        return Err(fail(format!(
            "file is {} Hz but the song renders at {sr} Hz",
            audio.sample_rate
        )));
    }
    let compiled = CompiledSong::new(&song, sr);
    let total = compiled.total_bars;
    let from = from_bar.unwrap_or(1);
    if from < 1 || from > total {
        return Err(fail(format!(
            "bad --from-bar {from}: the song has bars 1..{total}"
        )));
    }
    let first = from - 1;
    // Bars the file reaches into, the last one possibly partial.
    let base = compiled.clock.bar_start(first);
    let frames = audio.frames();
    let mut last = first;
    while last < total && ((compiled.clock.bar_start(last) - base) as usize) < frames {
        last += 1;
    }
    let held = first..last;
    // Audio past the song's last bar has no bar to map onto. A few frames of
    // codec padding are normal; a tenth of a second or more is worth a note.
    let song_end = (compiled.clock.bar_start(total) - base) as usize;
    let mut notes = Vec::new();
    let past = frames.saturating_sub(song_end) as f64 / f64::from(sr);
    if past >= 0.1 {
        notes.push(format!(
            "the file runs {past:.1} s past the song's last bar; that audio is ignored (check --from-bar)"
        ));
    }
    let range = match bars {
        Some(s) => {
            let r = parse_bar_range(s, u32::MAX).map_err(fail)?;
            if r.start < held.start || r.end > held.end {
                return Err(fail(format!(
                    "bars {}..{} are not inside the file (it holds bars {}..{})",
                    r.start + 1,
                    r.end,
                    held.start + 1,
                    held.end
                )));
            }
            r
        }
        None => held.clone(),
    };
    let lo = (compiled.clock.bar_start(range.start) - base) as usize;
    let hi = (compiled.clock.bar_start(range.end) - base) as usize;
    let audio = audio.slice(lo..hi);
    let mut map = bar_map(&compiled, range.clone());
    let end = audio.frames();
    map.partial_end = map.bar_frames.last().is_some_and(|&f| f > end);
    for f in &mut map.bar_frames {
        *f = (*f).min(end);
    }
    let png_name = match bars {
        Some(_) => format!("{stem}-bars-{}-{}", range.start + 1, range.end),
        None => stem.clone(),
    };
    let source = source_json(
        "file",
        path,
        &stem,
        Some((range.start + 1, range.end)),
        &audio,
    );
    Ok(Input {
        audio,
        map: Some(map),
        stem,
        png_name,
        source,
        notes,
    })
}

/// Loads the reference called `name` (from `tune refs`). Its source `path`
/// and any error show the configured path (`~/...`, `refs/<file>`), never the
/// expanded one, so a home directory stays out of output that gets quoted.
pub fn load_named_ref(name: &str) -> Result<Input, Vec<Failure>> {
    let cwd = std::env::current_dir().map_err(|e| fail(e.to_string()))?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let refs = find_refs(&cwd, home.as_deref()).map_err(fail)?;
    let entry = refs.resolve(name).map_err(fail)?;
    let resolved = entry.path.display().to_string();
    let hide = |s: &str| s.replace(&resolved, &entry.configured);
    let mut input = load_input(&entry.path, None, None, None).map_err(|failures| {
        failures
            .into_iter()
            .map(|mut f| {
                f.message = hide(&f.message);
                f.file = f.file.as_deref().map(hide);
                f
            })
            .collect::<Vec<_>>()
    })?;
    input.source["path"] = Value::String(entry.configured.clone());
    input.stem = name.to_string();
    input.png_name = name.to_string();
    Ok(input)
}

/// `--png` with no value (or `""`) writes `scratch/<default_name>.png`.
pub fn png_path(arg: Option<&str>, default_name: &str) -> PathBuf {
    match arg {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => Path::new("scratch").join(format!("{default_name}.png")),
    }
}

/// Writes through `<path>.tmp` and a rename, creating the parent directory.
pub fn write_png(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let err = |e: std::io::Error| Failure::msg(format!("cannot write `{}`: {e}", path.display()));
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(err)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).map_err(err)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        err(e)
    })
}

/// `<stem> bars 33-64 | -11.2 LUFS | TP -0.8 dBTP`; no bars part when unmapped.
pub fn title(input: &Input, a: &Analysis) -> String {
    let mut t = input.stem.clone();
    if input.map.is_some()
        && let Some(b) = input.source["bars"].as_array()
        && let (Some(x), Some(y)) = (b[0].as_u64(), b[1].as_u64())
    {
        t.push_str(&format!(" bars {x}-{y}"));
    }
    match a.loudness.integrated_lufs {
        Some(l) => t.push_str(&format!(" | {l:.1} LUFS")),
        None => t.push_str(" | silent"),
    }
    t.push_str(&format!(" | TP {:.1} dBTP", a.loudness.true_peak_dbtp));
    t
}
