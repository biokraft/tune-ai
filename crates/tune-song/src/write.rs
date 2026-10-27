//! The canonical serializer, format version 2. Hand-written so the layout
//! is fixed: the same song always produces the same text, and diffs between
//! saves stay small. Comments are not preserved.

use std::fmt::Write as _;

use indexmap::IndexMap;

use crate::FORMAT_VERSION;
use crate::model::{
    Clip, DEFAULT_VELOCITY, Device, Note, Placement, Song, TICKS_PER_STEP, Track, pitch_name,
};

impl Song {
    /// The song in canonical TOML form, format version 2. Parsing the result
    /// gives back an equal song.
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        out.push_str("[song]\n");
        let _ = writeln!(out, "version = {FORMAT_VERSION}");
        let _ = writeln!(out, "name = {}", string(&self.name));
        let _ = writeln!(out, "bpm = {}", num(self.bpm));
        let _ = writeln!(out, "seed = {}", self.seed);
        if let Some(rate) = self.sample_rate {
            let _ = writeln!(out, "sample_rate = {rate}");
        }

        if !self.mix.is_default() {
            out.push_str("\n[mix]\n");
            let _ = writeln!(out, "master = {}", string(self.mix.master.name()));
            if let Some(source) = &self.mix.duck_source {
                let _ = writeln!(out, "duck_source = {}", string(source));
            }
            if !self.mix.params.is_empty() {
                let _ = writeln!(out, "params = {}", num_table(&self.mix.params));
            }
        }

        for (id, track) in &self.tracks {
            let _ = write!(out, "\n[tracks.{}]\n", key(id));
            let _ = writeln!(out, "instrument = {}", instrument(&track.instrument));
            let _ = writeln!(out, "gain_db = {}", num(track.gain_db));
            let _ = writeln!(out, "pan = {}", num(track.pan));
            let _ = writeln!(out, "mute = {}", track.mute);
            let _ = writeln!(out, "solo = {}", track.solo);
            // The model keeps the instrument's params and the strip's apart:
            // the instrument's come first, each in model order.
            let mut params = device_params(&track.instrument);
            params.extend(track.strip.iter().map(|(k, v)| (k.clone(), *v)));
            if !params.is_empty() {
                let _ = writeln!(out, "params = {}", num_table(&params));
            }
            if !track.fx.is_empty() {
                let fx: Vec<String> = track.fx.iter().map(fx_entry_toml).collect();
                let _ = writeln!(out, "fx = [{}]", fx.join(", "));
            }
            if !track.play.is_empty() {
                let _ = writeln!(out, "play = {}", placements_toml(&track.play));
            }
        }

        for (id, clip) in &self.clips {
            let _ = write!(out, "\n[clips.{}]\n", key(id));
            let _ = writeln!(out, "kind = {}", string(clip.kind().name()));
            match clip {
                Clip::Notes(n) => {
                    let _ = writeln!(out, "bars = {}", n.bars);
                    if !n.notes.is_empty() {
                        let _ = writeln!(out, "notes = {}", notes_toml(&n.notes));
                    }
                }
                steps => {
                    let text = steps.steps_text().expect("a steps clip has step notation");
                    let _ = writeln!(out, "steps = {}", string(&text));
                }
            }
        }

        for section in &self.sections {
            out.push_str("\n[[sections]]\n");
            let _ = writeln!(out, "name = {}", string(&section.name));
            let _ = writeln!(out, "bars = {}", section.bars);
        }

        for lane in &self.automation {
            out.push_str("\n[[automation]]\n");
            let _ = writeln!(
                out,
                "param = {}",
                string(&format!("{}.{}", lane.track, lane.param))
            );
            let _ = writeln!(out, "at = {}", lane.start_bar + 1);
            let _ = writeln!(out, "bars = {}", lane.bars);
            let points: Vec<String> = lane
                .points
                .iter()
                .map(|(bar, value)| format!("[{}, {}]", num(*bar), num(*value)))
                .collect();
            let _ = writeln!(out, "points = [{}]", points.join(", "));
        }
        out
    }
}

/// The table of track `id`, `[tracks.<id>]` and its keys: the instrument,
/// then only the fields that are not at their default (`gain_db`, `pan`,
/// `mute`, `solo`), the `params` (the instrument's, then the strip's), the
/// `fx` and the `play` list. Ends with a newline.
pub fn track_table_toml(id: &str, track: &Track) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[tracks.{}]", key(id));
    let _ = writeln!(out, "instrument = {}", instrument(&track.instrument));
    if track.gain_db != 0.0 {
        let _ = writeln!(out, "gain_db = {}", num(track.gain_db));
    }
    if track.pan != 0.0 {
        let _ = writeln!(out, "pan = {}", num(track.pan));
    }
    if track.mute {
        out.push_str("mute = true\n");
    }
    if track.solo {
        out.push_str("solo = true\n");
    }
    let mut params = device_params(&track.instrument);
    params.extend(track.strip.iter().map(|(k, v)| (k.clone(), *v)));
    if !params.is_empty() {
        let _ = writeln!(out, "params = {}", num_table(&params));
    }
    if !track.fx.is_empty() {
        let fx: Vec<String> = track.fx.iter().map(fx_entry_toml).collect();
        let _ = writeln!(out, "fx = [{}]", fx.join(", "));
    }
    if !track.play.is_empty() {
        let _ = writeln!(out, "play = {}", placements_toml(&track.play));
    }
    out
}

/// A track's `play` value: one placement inline (`[{ at = 1, clip = "k4",
/// bars = 2 }]`), two or more one per line with a two-space indent and a
/// trailing comma. `offset` is written only when it is not 0.
pub fn placements_toml(play: &[Placement]) -> String {
    let entries: Vec<String> = play.iter().map(placement_inline).collect();
    list(&entries)
}

/// A notes clip's `notes` value: one note inline, two or more one per line
/// with a two-space indent and a trailing comma, like [`placements_toml`].
/// Positions are ticks over 24 in the shortest round-trip form; `vel` is
/// written only when it is not 100.
pub fn notes_toml(notes: &[Note]) -> String {
    let entries: Vec<String> = notes.iter().map(note_inline).collect();
    list(&entries)
}

/// `{ start = <s>, pitch = "<name>", len = <s>[, vel = <v>] }`.
fn note_inline(n: &Note) -> String {
    let steps = |ticks: u32| num(f64::from(ticks) / f64::from(TICKS_PER_STEP));
    let mut parts = vec![
        format!("start = {}", steps(n.start)),
        format!("pitch = {}", string(&pitch_name(n.pitch))),
        format!("len = {}", steps(n.len)),
    ];
    if n.velocity != DEFAULT_VELOCITY {
        parts.push(format!("vel = {}", n.velocity));
    }
    inline(&parts)
}

/// `[]`, one entry inline (`[{ … }]`), or two or more one per line.
fn list(entries: &[String]) -> String {
    match entries {
        [] => "[]".to_owned(),
        [one] => format!("[{one}]"),
        many => {
            let mut out = String::from("[\n");
            for entry in many {
                let _ = writeln!(out, "  {entry},");
            }
            out.push(']');
            out
        }
    }
}

/// `{ at = <1-based bar>, clip = "<id>", bars = <n>[, offset = <n>] }`.
fn placement_inline(p: &Placement) -> String {
    let mut parts = vec![
        format!("at = {}", p.start_bar + 1),
        format!("clip = {}", string(&p.clip)),
        format!("bars = {}", p.bars),
    ];
    if p.offset_bars != 0 {
        parts.push(format!("offset = {}", p.offset_bars));
    }
    inline(&parts)
}

/// The `instrument` value: the module name when the device has the
/// module's prefix as id and the module has no settings, else a table with
/// `type`, the `id` when it is not the prefix, then every setting.
fn instrument(device: &Device) -> String {
    let module = device.module;
    if device.id == module.prefix && module.settings.is_empty() {
        return string(module.name);
    }
    inline(&device_head(device))
}

/// `type = "<module>"`, `id = "<id>"` when it is not the module's prefix,
/// then every setting.
fn device_head(device: &Device) -> Vec<String> {
    let module = device.module;
    let mut parts = vec![format!("type = {}", string(module.name))];
    if device.id != module.prefix {
        parts.push(format!("id = {}", string(&device.id)));
    }
    for (spec, value) in module.settings.iter().zip(&device.settings) {
        parts.push(format!("{} = {}", key(spec.name), string(value)));
    }
    parts
}

/// An `fx` entry: `{ type = "<module>", [id = "<id>",] <setting> =
/// "<value>", …, params = { … } }`: the head, then the explicit params.
pub fn fx_entry_toml(fx: &Device) -> String {
    let mut parts = device_head(fx);
    let params = device_params(fx);
    if !params.is_empty() {
        parts.push(format!("params = {}", num_table(&params)));
    }
    inline(&parts)
}

/// A device's explicit params keyed by their track-relative ids
/// (`<device id>.<param>`).
fn device_params(device: &Device) -> IndexMap<String, f64> {
    device
        .params
        .iter()
        .map(|(local, v)| (device.param_id(local), *v))
        .collect()
}

/// `{ a = 1, b = 2 }`, or `{}` when empty.
fn inline(parts: &[String]) -> String {
    if parts.is_empty() {
        "{}".to_owned()
    } else {
        format!("{{ {} }}", parts.join(", "))
    }
}

fn num_table(map: &IndexMap<String, f64>) -> String {
    let parts: Vec<String> = map
        .iter()
        .map(|(k, v)| format!("{} = {}", key(k), num(*v)))
        .collect();
    inline(&parts)
}

/// A quoted TOML basic string. Written by hand because the `toml` crate's
/// writer sits behind a feature this crate does not enable.
fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A bare key when it is plain, otherwise a quoted one. Dotted names such as
/// `acid303.cutoff` are quoted so they stay one key.
fn key(k: &str) -> String {
    let bare = !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare { k.to_owned() } else { string(k) }
}

/// Shortest round-trip form. Rust's `Display` never uses an exponent, so a
/// huge value would print as a long integer that overflows TOML's i64; those
/// switch to the exponent form, which is a valid TOML float.
pub(crate) fn num(v: f64) -> String {
    let plain = v.to_string();
    if v.abs() >= 9.0e15 && !plain.contains('.') {
        format!("{v:e}")
    } else {
        plain
    }
}
