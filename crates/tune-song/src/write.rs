//! The canonical serializer. Hand-written so the layout is fixed: the same
//! song always produces the same text, and diffs between saves stay small.
//! Comments are not preserved.

use std::fmt::Write as _;

use indexmap::IndexMap;

use crate::model::{Clip, Device, Song};

impl Song {
    /// The track a v1 file names as the owner of clip `id`: the track of
    /// the earliest placement of it (the first in track order on a tie),
    /// else the first track whose instrument plays its kind.
    fn v1_owner(&self, id: &str, clip: &Clip) -> &str {
        let used = self
            .tracks
            .iter()
            .flat_map(|(t, track)| track.play.iter().map(move |p| (t, p)))
            .filter(|(_, p)| p.clip == id)
            .min_by_key(|(_, p)| p.start_bar)
            .map(|(t, _)| t.as_str());
        used.or_else(|| {
            self.tracks
                .iter()
                .find(|(_, t)| t.instrument.module.plays(clip.kind()))
                .map(|(t, _)| t.as_str())
        })
        .or_else(|| self.tracks.keys().next().map(String::as_str))
        .unwrap_or("")
    }

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
            // v1 names the instrument by module; its device id is the module's prefix.
            let _ = writeln!(out, "instrument = {}", string(track.instrument.module.name));
            let _ = writeln!(out, "gain_db = {}", num(track.gain_db));
            let _ = writeln!(out, "pan = {}", num(track.pan));
            let _ = writeln!(out, "mute = {}", track.mute);
            let _ = writeln!(out, "solo = {}", track.solo);
            // Instrument params first, then the strip's, in one table.
            let mut params = device_params(&track.instrument);
            params.extend(track.strip.iter().map(|(k, v)| (k.clone(), *v)));
            if !params.is_empty() {
                let _ = writeln!(out, "params = {}", num_table(&params));
            }
            if !track.fx.is_empty() {
                let fx: Vec<String> = track.fx.iter().map(fx_inline).collect();
                let _ = writeln!(out, "fx = [{}]", fx.join(", "));
            }
        }

        // v1 binds a pattern to a track; the model keeps no binding, so write
        // the first track that plays it, else the first that could.
        for (id, clip) in &self.clips {
            let _ = write!(out, "\n[patterns.{}]\n", key(id));
            let _ = writeln!(out, "track = {}", string(self.v1_owner(id, clip)));
            let _ = writeln!(out, "steps = {}", string(&clip.steps_text()));
        }

        // v1 keeps placements and lanes in sections, so each must cover
        // exactly one section (and start at the clip's first bar).
        let starts = self.section_start_bars();
        let aligned = |start_bar: u32, bars: u32| {
            starts
                .iter()
                .zip(&self.sections)
                .any(|(&s, section)| s == start_bar && section.bars == bars)
        };
        let all_aligned = self
            .tracks
            .values()
            .flat_map(|t| &t.play)
            .all(|p| p.offset_bars == 0 && aligned(p.start_bar, p.bars))
            && self.automation.iter().all(|l| aligned(l.start_bar, l.bars));
        assert!(
            all_aligned,
            "the v1 writer can only write placements and lanes aligned to sections"
        );
        for (section, start) in self.sections.iter().zip(starts.iter().copied()) {
            out.push_str("\n[[sections]]\n");
            let _ = writeln!(out, "name = {}", string(&section.name));
            let _ = writeln!(out, "bars = {}", section.bars);
            let covers = |start_bar: u32, bars: u32| start_bar == start && bars == section.bars;
            let play: Vec<String> = self
                .tracks
                .iter()
                .filter_map(|(t, track)| {
                    let p = track.play.iter().find(|p| covers(p.start_bar, p.bars))?;
                    Some(format!("{} = {}", key(t), string(&p.clip)))
                })
                .collect();
            let _ = writeln!(out, "play = {}", inline(&play));
            for lane in self
                .automation
                .iter()
                .filter(|l| covers(l.start_bar, l.bars))
            {
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

/// `{ type = "<module>", <setting> = "<value>", …, params = { … } }`: every
/// setting, then the explicit params.
fn fx_inline(fx: &Device) -> String {
    let mut parts = vec![format!("type = {}", string(fx.module.name))];
    for (spec, value) in fx.module.settings.iter().zip(&fx.settings) {
        parts.push(format!("{} = {}", key(spec.name), string(value)));
    }
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
fn num(v: f64) -> String {
    let plain = v.to_string();
    if v.abs() >= 9.0e15 && !plain.contains('.') {
        format!("{v:e}")
    } else {
        plain
    }
}
