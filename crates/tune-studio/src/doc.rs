//! The song file as an editable document.
//!
//! Claude writes commented song files for the producer to learn from, so an
//! edit must change only the value it names. [`SongDoc`] keeps the file as a
//! `toml_edit` document next to its validated [`Song`]. Every mutation runs
//! on a copy, re-parses the copy's text with [`parse_song`] and is committed
//! only when that succeeds, so the two always agree and the text is always a
//! valid song.

use serde_json::{Value, json};
use toml_edit::{DocumentMut, InlineTable, Item, Table};
use tune_dsp::{ParamSpec, STRIP_PARAMS, find_spec};
use tune_song::{
    Device, MIX_TRACK_ID, Registry, SectionPlay, SettingSpec, Song, SongError, Track,
    find_mix_param, parse_song,
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
}

impl SongDoc {
    /// Parses and validates a song file, keeping its formatting for edits.
    pub fn parse(text: &str) -> Result<Self, Vec<SongError>> {
        let song = parse_song(text)?;
        let doc = text.parse::<DocumentMut>().map_err(|e| {
            vec![SongError {
                line: None,
                column: None,
                message: e.message().trim_end().to_owned(),
            }]
        })?;
        Ok(Self { doc, song })
    }

    /// The file's text: the original bytes plus the edits made since.
    pub fn text(&self) -> String {
        self.doc.to_string()
    }

    /// The validated song the text describes.
    pub fn song(&self) -> &Song {
        &self.song
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
        let mut doc = self.doc.clone();
        apply(&mut doc, &self.song, path, value)?;
        self.commit(doc)
    }

    /// Moves section `from` to index `to` (as `Vec::remove` then `insert`).
    /// The section's leading comments and its `[[sections.automation]]`
    /// tables move with it.
    pub fn move_section(&mut self, from: usize, to: usize) -> Result<(), EditError> {
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
        let song = parse_song(&doc.to_string()).map_err(EditError::Invalid)?;
        self.doc = doc;
        self.song = song;
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
                SectionPlay::Other(placements) => Value::Array(
                    placements
                        .iter()
                        .map(|p| {
                            json!({
                                "at": p.start_bar + 1,
                                "clip": p.clip,
                                "bars": p.bars,
                                "offset": p.offset_bars,
                            })
                        })
                        .collect(),
                ),
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

/// Whether some module, under its default device id, has this param or
/// setting (`dist.drive`, `dist.mode`).
fn is_known_param(id: &str) -> bool {
    let Some((prefix, name)) = id.split_once('.') else {
        return false;
    };
    let registry = Registry::builtin();
    registry
        .instruments()
        .chain(registry.effects())
        .any(|m| m.prefix == prefix && (m.param(name).is_some() || m.setting(name).is_some()))
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

/// A table in the document, written either with a header (or dotted keys)
/// or inline (`{ … }`). Inline tables need their own spacing rules when a key
/// is appended.
enum Slot<'a> {
    Table(&'a mut Table),
    Inline(&'a mut InlineTable),
}

impl<'a> Slot<'a> {
    fn of(item: &'a mut Item) -> Option<Self> {
        match item {
            Item::Table(t) => Some(Self::Table(t)),
            Item::Value(toml_edit::Value::InlineTable(t)) => Some(Self::Inline(t)),
            _ => None,
        }
    }

    /// The table under `key`.
    fn child(self, key: &str) -> Option<Slot<'a>> {
        match self {
            Self::Table(t) => Slot::of(t.get_mut(key)?),
            Self::Inline(t) => t.get_mut(key)?.as_inline_table_mut().map(Self::Inline),
        }
    }

    /// The table under `key`, created as an inline table when absent.
    fn child_or_new(mut self, key: &str) -> Option<Slot<'a>> {
        let present = match &self {
            Self::Table(t) => t.contains_key(key),
            Self::Inline(t) => t.contains_key(key),
        };
        if !present {
            self.put(key, InlineTable::new().into());
        }
        self.child(key)
    }

    /// Sets `key` to `new`. An existing value keeps its surrounding
    /// whitespace and trailing comment; a new key is appended.
    fn put(&mut self, key: &str, mut new: toml_edit::Value) {
        let old = match self {
            Self::Table(t) => t.get_mut(key).and_then(Item::as_value_mut),
            Self::Inline(t) => t.get_mut(key),
        };
        if let Some(old) = old {
            *new.decor_mut() = old.decor().clone();
            *old = new;
            return;
        }
        match self {
            Self::Table(t) => {
                t.insert(key, Item::Value(new));
            }
            Self::Inline(t) => {
                // The space before `}` is the last value's suffix; hand it to
                // the new last value so the table reads `{ a = 1, b = 2 }`.
                if let Some((_, last)) = t.iter_mut().last() {
                    let suffix = last.decor().suffix().and_then(|s| s.as_str());
                    if let Some(suffix) =
                        suffix.filter(|s| s.trim().is_empty() && !s.contains('\n'))
                    {
                        let suffix = suffix.to_owned();
                        last.decor_mut().set_suffix("");
                        new.decor_mut().set_suffix(suffix);
                    }
                }
                t.insert(key, new);
            }
        }
    }

    fn remove(&mut self, key: &str) {
        match self {
            Self::Table(t) => {
                t.remove(key);
            }
            Self::Inline(t) => {
                t.remove(key);
            }
        }
    }
}

/// The table under `key` of the root table.
fn root_child<'a>(doc: &'a mut DocumentMut, key: &str) -> Option<Slot<'a>> {
    Slot::of(doc.as_table_mut().get_mut(key)?)
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

/// Element `index` of a list of tables, written either as `[[x]]` or as an
/// array of inline tables.
fn element_mut(item: &mut Item, index: usize) -> Option<Slot<'_>> {
    match item {
        Item::ArrayOfTables(aot) => aot.get_mut(index).map(Slot::Table),
        Item::Value(toml_edit::Value::Array(array)) => inline_element(array, index),
        _ => None,
    }
}

fn inline_element(array: &mut toml_edit::Array, index: usize) -> Option<Slot<'_>> {
    array
        .get_mut(index)?
        .as_inline_table_mut()
        .map(Slot::Inline)
}

fn sections_item(doc: &mut DocumentMut) -> Result<&mut Item, EditError> {
    doc.as_table_mut()
        .get_mut("sections")
        .ok_or_else(|| EditError::NoSuchTarget(SongPath::Sections.to_string()))
}

/// Reorders the values of an inline array with `f`, keeping each slot's
/// whitespace so the array reads as before.
fn reshape(array: &mut toml_edit::Array, f: impl FnOnce(&mut Vec<toml_edit::Value>)) {
    let mut decors: Vec<toml_edit::Decor> = array.iter().map(|v| v.decor().clone()).collect();
    let mut values: Vec<toml_edit::Value> = array.iter().cloned().collect();
    f(&mut values);
    // A new slot looks like the last one after the first, or like the first
    // when it starts a line; otherwise it is one space after the comma.
    let extra = match decors.as_slice() {
        [.., _, last] => last.clone(),
        [first] if prefix(first).contains('\n') => first.clone(),
        _ => toml_edit::Decor::new(" ", ""),
    };
    decors.resize(values.len(), extra);
    array.clear();
    for (mut v, decor) in values.into_iter().zip(decors) {
        *v.decor_mut() = decor;
        array.push_formatted(v);
    }
}

fn prefix(decor: &toml_edit::Decor) -> &str {
    decor.prefix().and_then(|p| p.as_str()).unwrap_or("")
}

// Table positions.
//
// `toml_edit` renders a document's tables sorted by position, where a table
// without one inherits the position of the table visited before it (visit
// order: a table, then its child tables and arrays of tables, depth first,
// in key order). Moving a `[[sections]]` element within its array changes
// the visit order but not the positions, so the section ops first give every
// table an explicit, distinct position matching the current render order,
// then assign positions to the moved tables.

/// Calls `f` on `table` and every table nested in it that gets a header when
/// rendered (dotted tables are skipped), in visit order.
fn walk<'t>(table: &'t Table, f: &mut dyn FnMut(&'t Table)) {
    if !table.is_dotted() {
        f(table);
    }
    for (_, item) in table.iter() {
        match item {
            Item::Table(t) => walk(t, f),
            Item::ArrayOfTables(aot) => aot.iter().for_each(|t| walk(t, f)),
            _ => {}
        }
    }
}

/// [`walk`] with mutable access, in the same order.
fn walk_mut(table: &mut Table, f: &mut dyn FnMut(&mut Table)) {
    if !table.is_dotted() {
        f(table);
    }
    for (_, item) in table.iter_mut() {
        match item {
            Item::Table(t) => walk_mut(t, f),
            Item::ArrayOfTables(aot) => aot.iter_mut().for_each(|t| walk_mut(t, f)),
            _ => {}
        }
    }
}

/// Gives every table the rank it renders at as its position, so positions
/// are explicit and distinct and the rendered text is unchanged.
fn normalize_positions(doc: &mut DocumentMut) {
    // The same keys the renderer sorts by: the root first, then the
    // inherited position; the sort is stable, so ties keep visit order.
    let mut keys = Vec::new();
    let mut last = 0;
    walk(doc.as_table(), &mut |t| {
        if let Some(p) = t.position() {
            last = p;
        }
        keys.push((!keys.is_empty(), last));
    });
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by_key(|&i| keys[i]);
    let mut rank = vec![0; keys.len()];
    for (r, &i) in order.iter().enumerate() {
        rank[i] = isize::try_from(r).expect("fewer tables than isize::MAX");
    }
    let mut ranks = rank.into_iter();
    walk_mut(doc.as_table_mut(), &mut |t| t.set_position(ranks.next()));
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

    #[test]
    fn normalize_keeps_text() {
        let src =
            "a = 1\n\n[x]\nk = 1\n\n[[s]]\nn = 1\n\n[y]\nk = 2\n\n[[s]]\nn = 2\n\n[[s.t]]\nq = 1\n";
        let mut doc: DocumentMut = src.parse().unwrap();
        normalize_positions(&mut doc);
        assert_eq!(doc.to_string(), src);
    }
}
