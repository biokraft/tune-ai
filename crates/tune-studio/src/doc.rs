//! The song file as an editable document.
//!
//! Claude writes commented song files for the producer to learn from, so an
//! edit must change only the value it names. [`SongDoc`] keeps the file as a
//! `toml_edit` document next to its validated [`Song`]. Every mutation runs
//! on a copy, re-parses the copy's text with [`parse`] and is committed
//! only when that succeeds, so the two always agree and the text is always a
//! valid song.

use serde_json::{Value, json};
use toml_edit::{DocumentMut, Item, Table};
use tune_dsp::{ParamSpec, STRIP_PARAMS, find_spec};
use tune_song::toml_doc::{
    Slot, element_mut, inline_element, normalize_positions, reshape, root_child, walk, walk_mut,
};
use tune_song::{
    Device, MIX_TRACK_ID, Placement, Registry, SectionPlay, SettingSpec, Song, SongError, Track,
    find_mix_param, parse,
};

use crate::path::{SongPath, TrackField};

/// Why an edit was refused. The document is unchanged after any of these.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error(
        "`{param}` is not a parameter of track `{track}` ({instrument}); see `tune params --json`"
    )]
    UnknownParam {
        track: String,
        instrument: &'static str,
        param: String,
    },
    #[error("`{0}` cannot be set directly")]
    ReadOnly(String),
    #[error("`{0}` does not exist in this song")]
    NoSuchTarget(String),
    #[error("bad value for `{path}`: {message}")]
    BadValue { path: String, message: String },
    #[error("index {index} is out of bounds (there are {len})")]
    OutOfBounds { index: usize, len: usize },
    #[error("a song needs at least one section")]
    LastSection,
    #[error("the edit would make the song invalid:\n{}", summary(.0))]
    Invalid(Vec<SongError>),
    /// Until the studio edits version 2 text, an edit of the arrangement
    /// (sections, what plays, patterns) is refused on a v2 file rather than
    /// leaving its placements and lanes behind.
    #[error("`{0}` is not yet supported on v2 files")]
    NotYetOnV2(String),
}

fn summary(errors: &[SongError]) -> String {
    errors
        .iter()
        .map(SongError::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A song file kept as text with its comments and layout, plus the validated
/// song it describes. The two are always in agreement.
#[derive(Clone, Debug)]
pub struct SongDoc {
    doc: DocumentMut,
    song: Song,
    /// The format version of the text.
    version: u32,
}

impl SongDoc {
    /// Parses and validates a song file, keeping its formatting for edits.
    pub fn parse(text: &str) -> Result<Self, Vec<SongError>> {
        let parsed = parse(text)?;
        let doc = text.parse::<DocumentMut>().map_err(|e| {
            vec![SongError {
                line: None,
                column: None,
                message: e.message().trim_end().to_owned(),
            }]
        })?;
        Ok(Self {
            doc,
            song: parsed.song,
            version: parsed.version,
        })
    }

    /// The file's text: the original bytes plus the edits made since.
    pub fn text(&self) -> String {
        self.doc.to_string()
    }

    /// The validated song the text describes.
    pub fn song(&self) -> &Song {
        &self.song
    }

    /// The format version the text is written in.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Refuses `what` (an arrangement edit) on a v2 file.
    fn v1_only(&self, what: impl FnOnce() -> String) -> Result<(), EditError> {
        if self.version == 1 {
            Ok(())
        } else {
            Err(EditError::NotYetOnV2(what()))
        }
    }

    /// The effective value at `path` (defaults filled in), or `None` when the
    /// target does not exist in this song. See [`effective_value`].
    pub fn get(&self, path: &SongPath) -> Option<Value> {
        effective_value(&self.song, path)
    }

    /// Writes `value` at `path`, changing only that value's text.
    ///
    /// Numbers that are exact integers are written as integers. A param that
    /// the file leaves at its default is inserted (creating the `params`
    /// table when absent). A `mix.*` value in a file without `[mix]` creates
    /// the table right after `[song]`. `sections[<i>].play.<t> = null` and
    /// `mix.duck_source = null` remove the key.
    pub fn set(&mut self, path: &SongPath, value: &Value) -> Result<(), EditError> {
        if path.is_read_only() {
            return Err(EditError::ReadOnly(path.to_string()));
        }
        if matches!(
            path,
            SongPath::SectionBars(_) | SongPath::SectionPlay { .. } | SongPath::Pattern { .. }
        ) {
            self.v1_only(|| path.to_string())?;
        }
        let mut doc = self.doc.clone();
        apply(&mut doc, &self.song, path, value)?;
        self.commit(doc)
    }

    /// Moves section `from` to index `to` (as `Vec::remove` then `insert`).
    /// The section's leading comments and its `[[sections.automation]]`
    /// tables move with it.
    pub fn move_section(&mut self, from: usize, to: usize) -> Result<(), EditError> {
        self.v1_only(|| "moving a section".to_owned())?;
        let len = self.song.sections.len();
        for index in [from, to] {
            if index >= len {
                return Err(EditError::OutOfBounds { index, len });
            }
        }
        if from == to {
            return Ok(());
        }
        let mut doc = self.doc.clone();
        normalize_positions(&mut doc);
        match sections_item(&mut doc)? {
            Item::ArrayOfTables(aot) => {
                // The document slots the sections occupy stay the same; the
                // tables are dealt into them in their new order.
                let mut slots = Vec::new();
                for t in aot.iter() {
                    walk(t, &mut |t| slots.extend(t.position()));
                }
                slots.sort_unstable();
                let moved = aot.remove(from);
                aot.insert(to, moved);
                let mut slots = slots.into_iter();
                for t in aot.iter_mut() {
                    walk_mut(t, &mut |t| t.set_position(slots.next()));
                }
            }
            Item::Value(toml_edit::Value::Array(array)) => reshape(array, |values| {
                let moved = values.remove(from);
                values.insert(to, moved);
            }),
            _ => return Err(EditError::NoSuchTarget(SongPath::Sections.to_string())),
        }
        self.commit(doc)
    }

    /// Inserts a copy of section `index` (comments and lanes included) right
    /// after it.
    pub fn duplicate_section(&mut self, index: usize) -> Result<(), EditError> {
        self.v1_only(|| "duplicating a section".to_owned())?;
        let len = self.song.sections.len();
        if index >= len {
            return Err(EditError::OutOfBounds { index, len });
        }
        let mut doc = self.doc.clone();
        normalize_positions(&mut doc);
        match sections_item(&mut doc)? {
            Item::ArrayOfTables(aot) => {
                let mut copy = aot.get(index).expect("index checked").clone();
                // Open a gap right after the original's last table and give
                // the copy's tables those positions, in order.
                let mut last = isize::MIN;
                let mut count = 0;
                walk(&copy, &mut |t| {
                    last = last.max(t.position().unwrap_or(last));
                    count += 1;
                });
                walk_mut(doc.as_table_mut(), &mut |t| {
                    if let Some(p) = t.position().filter(|p| *p > last) {
                        t.set_position(Some(p + count));
                    }
                });
                let mut next = last;
                walk_mut(&mut copy, &mut |t| {
                    next += 1;
                    t.set_position(Some(next));
                });
                match sections_item(&mut doc)? {
                    Item::ArrayOfTables(aot) => aot.insert(index + 1, copy),
                    _ => unreachable!("sections was an array of tables a moment ago"),
                }
            }
            Item::Value(toml_edit::Value::Array(array)) => reshape(array, |values| {
                let copy = values[index].clone();
                values.insert(index + 1, copy);
            }),
            _ => return Err(EditError::NoSuchTarget(SongPath::Sections.to_string())),
        }
        self.commit(doc)
    }

    /// Removes section `index` with its comments and lanes. The last section
    /// cannot be removed.
    pub fn delete_section(&mut self, index: usize) -> Result<(), EditError> {
        self.v1_only(|| "deleting a section".to_owned())?;
        let len = self.song.sections.len();
        if index >= len {
            return Err(EditError::OutOfBounds { index, len });
        }
        if len == 1 {
            return Err(EditError::LastSection);
        }
        let mut doc = self.doc.clone();
        match sections_item(&mut doc)? {
            Item::ArrayOfTables(aot) => {
                aot.remove(index);
            }
            Item::Value(toml_edit::Value::Array(array)) => reshape(array, |values| {
                values.remove(index);
            }),
            _ => return Err(EditError::NoSuchTarget(SongPath::Sections.to_string())),
        }
        self.commit(doc)
    }

    /// Validates the edited copy and keeps it only when it is a valid song.
    fn commit(&mut self, doc: DocumentMut) -> Result<(), EditError> {
        let parsed = parse(&doc.to_string()).map_err(EditError::Invalid)?;
        self.doc = doc;
        self.song = parsed.song;
        self.version = parsed.version;
        Ok(())
    }
}

/// The effective value at `path` in `song`, as JSON: numbers (exact integers
/// as integers), bools and strings for leaf values; `null` for a track that
/// is silent in a section; `None` when the target does not exist.
///
/// The read-only aggregates are valued as follows: `tracks.<t>` is
/// `{"instrument": "<module>", "fx": [{"id": "<device id>", "type":
/// "<module>"}, …]}`, `sections[<i>].lanes` is `[{track, param, at, bars,
/// points}]` for every lane stretch overlapping the section (`at` 1-based)
/// and `sections` is `["<name>:<bars>", …]`. `sections[<i>].play.<t>` is
/// `null` when the track is silent in the section, the clip id when one
/// placement covers all of it, and otherwise `[{at, clip, bars, offset}]`
/// for the placements overlapping it (`at` 1-based).
pub fn effective_value(song: &Song, path: &SongPath) -> Option<Value> {
    Some(match path {
        SongPath::Bpm => num(song.bpm),
        SongPath::TrackField { track, field } => {
            let t = song.tracks.get(track)?;
            match field {
                TrackField::GainDb => num(t.gain_db),
                TrackField::Pan => num(t.pan),
                TrackField::Mute => Value::Bool(t.mute),
                TrackField::Solo => Value::Bool(t.solo),
            }
        }
        SongPath::TrackParam { track, param } => {
            let t = song.tracks.get(track)?;
            match locate(t, param)? {
                Target::Param(home, local, spec) => num(home
                    .device(t)
                    .params
                    .get(local)
                    .copied()
                    .unwrap_or(spec.default)),
                Target::Strip(spec) => num(t.strip.get(param).copied().unwrap_or(spec.default)),
                Target::Setting(home, spec) => Value::from(home.device(t).setting(spec.name)?),
            }
        }
        SongPath::MixParam { param } => {
            let (_, _, spec) = find_mix_param(param)?;
            num(song.mix.params.get(param).copied().unwrap_or(spec.default))
        }
        SongPath::MixMaster => Value::from(song.mix.master.name()),
        SongPath::MixDuckSource => song.duck_source().map_or(Value::Null, Value::from),
        SongPath::Track { track } => {
            let t = song.tracks.get(track)?;
            let fx: Vec<Value> =
                t.fx.iter()
                    .map(|d| json!({ "id": d.id, "type": d.module.name }))
                    .collect();
            json!({ "instrument": t.instrument.module.name, "fx": fx })
        }
        SongPath::Pattern { pattern } => Value::from(song.clips.get(pattern)?.steps_text()),
        SongPath::SectionBars(i) => Value::from(song.sections.get(*i)?.bars),
        SongPath::SectionName(i) => Value::from(song.sections.get(*i)?.name.clone()),
        SongPath::SectionPlay { section, track } => {
            song.sections.get(*section)?;
            song.tracks.get(track)?;
            match song.section_play(*section, track) {
                SectionPlay::Silent => Value::Null,
                SectionPlay::Clip(clip) => Value::from(clip),
                SectionPlay::Other(placements) => {
                    Value::Array(placements.into_iter().map(placement_json).collect())
                }
            }
        }
        SongPath::SectionLanes(i) => {
            song.sections.get(*i)?;
            Value::Array(
                song.lanes_in(*i)
                    .into_iter()
                    .map(|lane| {
                        let points: Vec<Value> = lane
                            .points
                            .iter()
                            .map(|(bar, value)| json!([num(*bar), num(*value)]))
                            .collect();
                        json!({
                            "track": lane.track,
                            "param": lane.param,
                            "at": lane.start_bar + 1,
                            "bars": lane.bars,
                            "points": points,
                        })
                    })
                    .collect(),
            )
        }
        SongPath::Sections => Value::Array(
            song.sections
                .iter()
                .map(|s| Value::from(format!("{}:{}", s.name, s.bars)))
                .collect(),
        ),
    })
}

/// A placement as JSON: `{"at", "clip", "bars", "offset"}`, `at` 1-based.
pub fn placement_json(p: &Placement) -> Value {
    json!({
        "at": p.start_bar + 1,
        "clip": p.clip,
        "bars": p.bars,
        "offset": p.offset_bars,
    })
}

/// A number as JSON, exact integers as integers (`-6`, not `-6.0`).
fn num(v: f64) -> Value {
    match as_integer(v) {
        Some(i) => Value::from(i),
        None => serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number),
    }
}

/// `v` as an `i64` when it is an exact integer that `f64` represents
/// without rounding (|v| <= 2^53).
fn as_integer(v: f64) -> Option<i64> {
    const EXACT: f64 = 9_007_199_254_740_992.0;
    (v.fract() == 0.0 && v.abs() <= EXACT).then_some(v as i64)
}

/// Which device of a track.
#[derive(Clone, Copy)]
enum Home {
    Instrument,
    /// Index into the track's fx chain.
    Fx(usize),
}

impl Home {
    fn device(self, track: &Track) -> &Device {
        match self {
            Self::Instrument => &track.instrument,
            Self::Fx(i) => &track.fx[i],
        }
    }
}

/// What a track-relative id names.
enum Target<'p> {
    /// A device's param, with its local name (`cutoff`).
    Param(Home, &'p str, &'static ParamSpec),
    /// A strip param (`send.delay`), kept in the track's `params` table.
    Strip(&'static ParamSpec),
    /// A device's setting (`mode`), a key of its table.
    Setting(Home, &'static SettingSpec),
}

/// Resolves `<device id>.<name>` like the engine does: the instrument, the
/// strip, then the fx device with that id. On a device, `name` is a param
/// first, then a setting.
fn locate<'p>(track: &Track, id: &'p str) -> Option<Target<'p>> {
    let (device_id, name) = id.split_once('.')?;
    let on = |home: Home| {
        let module = home.device(track).module;
        match module.param(name) {
            Some((_, spec)) => Some(Target::Param(home, name, spec)),
            None => module
                .setting(name)
                .map(|(_, spec)| Target::Setting(home, spec)),
        }
    };
    if device_id == track.instrument.id {
        return on(Home::Instrument);
    }
    if let Ok((_, spec)) = find_spec(STRIP_PARAMS, id) {
        return Some(Target::Strip(spec));
    }
    let slot = track.fx.iter().position(|d| d.id == device_id)?;
    on(Home::Fx(slot))
}

/// Whether `<device id>.<name>` names a param or setting of some module
/// under a device id that module's devices get: its prefix, or the prefix
/// and a number as `Track::next_device_id` hands out (`dist.drive`,
/// `eq2.low_cut`).
fn is_known_param(id: &str) -> bool {
    let Some((device, name)) = id.split_once('.') else {
        return false;
    };
    let registry = Registry::builtin();
    registry.instruments().chain(registry.effects()).any(|m| {
        let numbered = device
            .strip_prefix(m.prefix)
            .is_some_and(|n| n.is_empty() || n.parse::<u32>().is_ok_and(|n| n >= 2));
        numbered && (m.param(name).is_some() || m.setting(name).is_some())
    })
}

/// Applies one `set` to `doc`, using `song` (which `doc` describes) to find
/// the target. Validation happens afterwards, in [`SongDoc::commit`].
fn apply(
    doc: &mut DocumentMut,
    song: &Song,
    path: &SongPath,
    value: &Value,
) -> Result<(), EditError> {
    let missing = || EditError::NoSuchTarget(path.to_string());
    let bad = |message: &str| EditError::BadValue {
        path: path.to_string(),
        message: message.to_owned(),
    };
    let number = || value.as_f64().ok_or_else(|| bad("expected a number"));
    let string = || value.as_str().ok_or_else(|| bad("expected a string"));

    match path {
        SongPath::Bpm => {
            let v = toml_number(number()?);
            root_child(doc, "song").ok_or_else(missing)?.put("bpm", v);
        }
        SongPath::TrackField { track, field } => {
            if !song.tracks.contains_key(track) {
                return Err(missing());
            }
            let v = match field {
                TrackField::GainDb | TrackField::Pan => toml_number(number()?),
                TrackField::Mute | TrackField::Solo => toml_edit::Value::from(
                    value
                        .as_bool()
                        .ok_or_else(|| bad("expected true or false"))?,
                ),
            };
            track_table(doc, track)
                .ok_or_else(missing)?
                .put(field.key(), v);
        }
        SongPath::TrackParam { track, param } => {
            let Some(t) = song.tracks.get(track) else {
                return Err(missing());
            };
            let target = match locate(t, param) {
                Some(target) => target,
                None if is_known_param(param) => return Err(missing()),
                None => {
                    return Err(EditError::UnknownParam {
                        track: track.clone(),
                        instrument: t.instrument.module.name,
                        param: param.clone(),
                    });
                }
            };
            match target {
                Target::Setting(Home::Fx(i), spec) => {
                    let v = toml_edit::Value::from(string()?);
                    fx_table(doc, track, i)
                        .ok_or_else(missing)?
                        .put(spec.name, v);
                }
                // A v1 instrument is a bare module name, with no table to
                // hold a setting.
                Target::Setting(Home::Instrument, _) => {
                    return Err(EditError::ReadOnly(path.to_string()));
                }
                // Instrument and strip params share the track's `params`.
                Target::Param(..) | Target::Strip(_) => {
                    let v = toml_number(number()?);
                    let owner = match target {
                        Target::Param(Home::Fx(i), ..) => fx_table(doc, track, i),
                        _ => track_table(doc, track),
                    };
                    owner
                        .and_then(|o| o.child_or_new("params"))
                        .ok_or_else(missing)?
                        .put(param, v);
                }
            }
        }
        SongPath::MixParam { param } => {
            if find_mix_param(param).is_none() {
                return Err(EditError::UnknownParam {
                    track: MIX_TRACK_ID.to_owned(),
                    instrument: MIX_TRACK_ID,
                    param: param.clone(),
                });
            }
            let v = toml_number(number()?);
            mix_table(doc)
                .child_or_new("params")
                .ok_or_else(missing)?
                .put(param, v);
        }
        SongPath::MixMaster => {
            let v = toml_edit::Value::from(string()?);
            mix_table(doc).put("master", v);
        }
        SongPath::MixDuckSource => match value {
            Value::Null => {
                if let Some(mut mix) = root_child(doc, "mix") {
                    mix.remove("duck_source");
                }
            }
            Value::String(id) => {
                let v = toml_edit::Value::from(id.as_str());
                mix_table(doc).put("duck_source", v);
            }
            _ => return Err(bad("expected a track id or null")),
        },
        SongPath::Pattern { pattern } => {
            if !song.clips.contains_key(pattern) {
                return Err(missing());
            }
            let v = toml_edit::Value::from(string()?);
            root_child(doc, "patterns")
                .and_then(|p| p.child(pattern))
                .ok_or_else(missing)?
                .put("steps", v);
        }
        SongPath::SectionBars(i) | SongPath::SectionName(i) => {
            if *i >= song.sections.len() {
                return Err(missing());
            }
            let (key, v) = match path {
                SongPath::SectionBars(_) => {
                    let bars =
                        as_integer(number()?).ok_or_else(|| bad("bars must be a whole number"))?;
                    ("bars", toml_edit::Value::from(bars))
                }
                _ => ("name", toml_edit::Value::from(string()?)),
            };
            section_table(doc, *i).ok_or_else(missing)?.put(key, v);
        }
        SongPath::SectionPlay { section, track } => {
            if *section >= song.sections.len() || !song.tracks.contains_key(track) {
                return Err(missing());
            }
            let s = section_table(doc, *section).ok_or_else(missing)?;
            match value {
                Value::Null => {
                    if let Some(mut play) = s.child("play") {
                        play.remove(track);
                    }
                }
                Value::String(p) => {
                    let v = toml_edit::Value::from(p.as_str());
                    s.child_or_new("play").ok_or_else(missing)?.put(track, v);
                }
                _ => return Err(bad("expected a pattern id or null")),
            }
        }
        SongPath::Track { .. } | SongPath::SectionLanes(_) | SongPath::Sections => {
            return Err(EditError::ReadOnly(path.to_string()));
        }
    }
    Ok(())
}

/// A TOML number, exact integers as integers.
fn toml_number(v: f64) -> toml_edit::Value {
    match as_integer(v) {
        Some(i) => toml_edit::Value::from(i),
        None => toml_edit::Value::from(v),
    }
}

/// The `[mix]` table, created right after `[song]` when the file has none.
fn mix_table(doc: &mut DocumentMut) -> Slot<'_> {
    if !doc.as_table().contains_key("mix") {
        insert_mix_table(doc);
    }
    root_child(doc, "mix").expect("the file has a mix table now")
}

/// Adds an empty `[mix]` table rendered right after the `[song]` table
/// (and anything nested in it), or first among the tables when `[song]`
/// has no header. Later tables move down one place; the text is otherwise
/// unchanged.
fn insert_mix_table(doc: &mut DocumentMut) {
    normalize_positions(doc);
    // The root renders first, at rank 0.
    let mut after = 0;
    if let Some(Item::Table(song)) = doc.as_table().get("song") {
        walk(song, &mut |t| {
            after = after.max(t.position().unwrap_or(after))
        });
    }
    walk_mut(doc.as_table_mut(), &mut |t| {
        if let Some(p) = t.position().filter(|p| *p > after) {
            t.set_position(Some(p + 1));
        }
    });
    let mut mix = Table::new();
    mix.set_position(Some(after + 1));
    mix.decor_mut().set_prefix("\n");
    doc.as_table_mut().insert("mix", Item::Table(mix));
}

fn track_table<'a>(doc: &'a mut DocumentMut, track: &str) -> Option<Slot<'a>> {
    root_child(doc, "tracks")?.child(track)
}

fn section_table(doc: &mut DocumentMut, index: usize) -> Option<Slot<'_>> {
    element_mut(doc.as_table_mut().get_mut("sections")?, index)
}

/// The fx entry `index` of a track.
fn fx_table<'a>(doc: &'a mut DocumentMut, track: &str, index: usize) -> Option<Slot<'a>> {
    let fx = match track_table(doc, track)? {
        Slot::Table(t) => t.get_mut("fx")?,
        Slot::Inline(t) => {
            return t
                .get_mut("fx")?
                .as_array_mut()
                .and_then(|a| inline_element(a, index));
        }
    };
    element_mut(fx, index)
}

fn sections_item(doc: &mut DocumentMut) -> Result<&mut Item, EditError> {
    doc.as_table_mut()
        .get_mut("sections")
        .ok_or_else(|| EditError::NoSuchTarget(SongPath::Sections.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_are_exact_only() {
        assert_eq!(as_integer(-6.0), Some(-6));
        assert_eq!(as_integer(-0.0), Some(0));
        assert_eq!(as_integer(0.25), None);
        assert_eq!(as_integer(1e300), None);
        assert_eq!(num(440.0), json!(440));
        assert_eq!(num(0.3), json!(0.3));
    }
}
