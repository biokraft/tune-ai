//! The canonical serializer. Hand-written so the layout is fixed: the same
//! song always produces the same text, and diffs between saves stay small.
//! Comments are not preserved.

use std::fmt::Write as _;

use indexmap::IndexMap;

use crate::model::{FxSpec, PatternSteps, Song};

impl Song {
    /// The song in canonical TOML form. Parsing the result gives back an equal
    /// song.
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        out.push_str("[song]\n");
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
            let _ = writeln!(out, "instrument = \"{}\"", track.instrument.name());
            let _ = writeln!(out, "gain_db = {}", num(track.gain_db));
            let _ = writeln!(out, "pan = {}", num(track.pan));
            let _ = writeln!(out, "mute = {}", track.mute);
            let _ = writeln!(out, "solo = {}", track.solo);
            if !track.params.is_empty() {
                let _ = writeln!(out, "params = {}", num_table(&track.params));
            }
            if !track.fx.is_empty() {
                let fx: Vec<String> = track.fx.iter().map(fx_inline).collect();
                let _ = writeln!(out, "fx = [{}]", fx.join(", "));
            }
        }

        for (id, pattern) in &self.patterns {
            let _ = write!(out, "\n[patterns.{}]\n", key(id));
            let _ = writeln!(out, "track = {}", string(&pattern.track));
            let steps = match &pattern.steps {
                PatternSteps::Acid(p) => p.to_string(),
                PatternSteps::Drum(p) => p.to_string(),
            };
            let _ = writeln!(out, "steps = {}", string(&steps));
        }

        for section in &self.sections {
            out.push_str("\n[[sections]]\n");
            let _ = writeln!(out, "name = {}", string(&section.name));
            let _ = writeln!(out, "bars = {}", section.bars);
            let play: Vec<String> = section
                .play
                .iter()
                .map(|(t, p)| format!("{} = {}", key(t), string(p)))
                .collect();
            let _ = writeln!(out, "play = {}", inline(&play));
            for lane in &section.automation {
                out.push_str("\n[[sections.automation]]\n");
                let _ = writeln!(
                    out,
                    "param = {}",
                    string(&format!("{}.{}", lane.track, lane.param))
                );
                let points: Vec<String> = lane
                    .points
                    .iter()
                    .map(|(bar, value)| format!("[{}, {}]", num(*bar), num(*value)))
                    .collect();
                let _ = writeln!(out, "points = [{}]", points.join(", "));
            }
        }
        out
    }
}

fn fx_inline(fx: &FxSpec) -> String {
    match fx {
        FxSpec::Distortion { mode, params } => {
            let mut parts = vec![
                format!("type = {}", string(fx.type_name())),
                format!("mode = {}", string(mode.name())),
            ];
            if !params.is_empty() {
                parts.push(format!("params = {}", num_table(params)));
            }
            inline(&parts)
        }
        FxSpec::Eq { params } => {
            let mut parts = vec![format!("type = {}", string(fx.type_name()))];
            if !params.is_empty() {
                parts.push(format!("params = {}", num_table(params)));
            }
            inline(&parts)
        }
    }
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
fn num(v: f64) -> String {
    let plain = v.to_string();
    if v.abs() >= 9.0e15 && !plain.contains('.') {
        format!("{v:e}")
    } else {
        plain
    }
}
