//! The song file as an editable document.
//!
//! Claude writes commented song files for the producer to learn from, so an
//! edit must change only the value it names. [`SongDoc`] keeps the file as a
//! `toml_edit` document next to its validated [`Song`]. Every mutation runs
//! on a copy, re-parses the copy's text with [`parse`] and is committed
//! only when that succeeds, so the two always agree and the text is always a
//! valid song.
//!
//! The document is always in the newest format: a version 1 file is migrated
//! in memory when parsed (see [`SongDoc::migrated_from`]), so the first edit
//! writes it as version 2.
//!
//! Section edits (moving, copying, deleting or resizing a section, or what a
//! track plays in one) run the `tune_song` section operations on the song
//! and write their result into the text (see [`arrange`]).

use serde_json::{Value, json};
use toml_edit::{DocumentMut, InlineTable, Item, Table};
use tune_dsp::{ParamSpec, STRIP_PARAMS, find_spec};
use tune_song::toml_doc::{
    Slot, element_mut, inline_element, normalize_positions, root_child, walk, walk_mut,
};
use tune_song::{
    ArrangeError, Arranged, Clip, Device, FORMAT_VERSION, MIX_TRACK_ID, Migration, Note, Placement,
    Registry, SectionPlay, SettingSpec, Song, SongError, TICKS_PER_STEP, Track, find_mix_param,
    migrate, parse, pitch_name, section_bars_error,
};

use crate::path::{SongPath, TrackField};

mod arrange;

use arrange::{Tables, block_start, rerank, section_tables, write_automation, write_plays};

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
    /// A section operation would put a bar past `u32::MAX` (never for a
    /// song the validator accepts).
    #[error("the arrangement would be too long")]
    TooLong,
    #[error("the edit would make the song invalid:\n{}", summary(.0))]
    Invalid(Vec<SongError>),
    /// The studio could not carry out an edit it accepted: a bug, never a
    /// problem with the file.
    #[error("internal error: {0}; the song was not changed (please report this)")]
    Internal(String),
}

/// Why a section operation was refused, as an edit error. `path` is the
/// edit's path when it has one (`sections[<i>].bars`,
/// `sections[<i>].play.<t>`): a section or track it names that does not
/// exist is then [`EditError::NoSuchTarget`], as for any other path.
fn arrange_error(e: ArrangeError, path: Option<&SongPath>) -> EditError {
    match (e, path) {
        (ArrangeError::OutOfBounds { .. } | ArrangeError::NoSuchTrack(_), Some(path)) => {
            EditError::NoSuchTarget(path.to_string())
        }
        (ArrangeError::OutOfBounds { index, len }, None) => EditError::OutOfBounds { index, len },
        (ArrangeError::NoSuchTrack(track), None) => {
            EditError::NoSuchTarget(SongPath::Track { track }.to_string())
        }
        (ArrangeError::LastSection, _) => EditError::LastSection,
        // The validator's own wording (`section_bars_error`).
        (e @ ArrangeError::Bars { .. }, path) => EditError::BadValue {
            path: path.map_or_else(|| SongPath::Sections.to_string(), SongPath::to_string),
            message: e.to_string(),
        },
        (ArrangeError::TooLong, _) => EditError::TooLong,
    }
}

fn summary(errors: &[SongError]) -> String {
    errors
        .iter()
        .map(SongError::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A song file kept as text with its comments and layout, plus the validated
/// song it describes. The two are always in agreement, and the text is
/// always in the newest format.
#[derive(Clone, Debug)]
pub struct SongDoc {
    doc: DocumentMut,
    song: Song,
    /// The format version of the parsed text, when it was older and so was
    /// migrated in memory.
    migrated_from: Option<u32>,
}

impl SongDoc {
    /// Parses and validates a song file, keeping its formatting for edits.
    /// A file in an older format is migrated to the newest in memory, with
    /// its comments: [`text`](Self::text) is then the migrated text and
    /// [`migrated_from`](Self::migrated_from) the file's version. Errors are
    /// positioned in `text` either way.
    pub fn parse(text: &str) -> Result<Self, Vec<SongError>> {
        // The given text is parsed first so that errors carry its positions.
        let parsed = parse(text)?;
        let (migrated, migrated_from) = if parsed.version == FORMAT_VERSION {
            (None, None)
        } else {
            match migrate(text)? {
                Migration::Migrated { from, text } => (Some(text), Some(from)),
                Migration::Current => (None, None),
            }
        };
        // The song is the one the document's own text describes, so the two
        // agree by construction.
        let (text, song) = match migrated.as_deref() {
            Some(migrated) => (migrated, parse(migrated)?.song),
            None => (text, parsed.song),
        };
        let doc = text.parse::<DocumentMut>().map_err(|e| {
            vec![SongError {
                line: None,
                column: None,
                message: e.message().trim_end().to_owned(),
            }]
        })?;
        Ok(Self {
            doc,
            song,
            migrated_from,
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

    /// The format version of the text [`parse`](Self::parse) was given,
    /// when it was older than the newest and so was migrated; `None` for a
    /// file already in the newest format.
    pub fn migrated_from(&self) -> Option<u32> {
        self.migrated_from
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
    /// the table right after `[song]`. `mix.duck_source = null` removes the
    /// key.
    ///
    /// `sections[<i>].bars` resizes the section as
    /// [`resize_section`](tune_song::resize_section) does, and
    /// `sections[<i>].play.<t>` makes the track play a clip over the whole
    /// section, or nothing there for `null`, as
    /// [`set_section_play`](tune_song::set_section_play) does.
    pub fn set(&mut self, path: &SongPath, value: &Value) -> Result<(), EditError> {
        if path.is_read_only() {
            return Err(EditError::ReadOnly(path.to_string()));
        }
        match path {
            SongPath::SectionBars(index) => {
                let bars = section_bars(path, value)?;
                if self
                    .song
                    .sections
                    .get(*index)
                    .is_some_and(|s| s.bars == bars)
                {
                    return Ok(());
                }
                let result = tune_song::resize_section(&self.song, *index, bars);
                self.arrange(result, Some(path), Tables::Bars(*index, bars))
            }
            SongPath::SectionPlay { section, track } => {
                let clip = match value {
                    Value::Null => None,
                    Value::String(clip) => Some(clip.as_str()),
                    _ => {
                        return Err(EditError::BadValue {
                            path: path.to_string(),
                            message: "expected a clip id or null".to_owned(),
                        });
                    }
                };
                let result = tune_song::set_section_play(&self.song, *section, track, clip);
                self.arrange(result, Some(path), Tables::Keep)
            }
            _ => {
                let mut doc = self.doc.clone();
                apply(&mut doc, &self.song, path, value)?;
                self.commit(doc)
            }
        }
    }

    /// Moves section `from` to index `to` (as `Vec::remove` then `insert`),
    /// with everything it plays. The section's leading comments and its
    /// `[[automation]]` tables move with it.
    pub fn move_section(&mut self, from: usize, to: usize) -> Result<(), EditError> {
        let result = tune_song::move_section(&self.song, from, to);
        self.arrange(result, None, Tables::Move(from, to))
    }

    /// Inserts a copy of section `index`, with everything it plays, right
    /// after it. The copy's tables (section and automation) carry the
    /// original's comments.
    pub fn duplicate_section(&mut self, index: usize) -> Result<(), EditError> {
        let result = tune_song::duplicate_section(&self.song, index);
        self.arrange(result, None, Tables::Duplicate(index))
    }

    /// Removes section `index` with its comments and everything inside it;
    /// the rest closes up. The last section cannot be removed.
    pub fn delete_section(&mut self, index: usize) -> Result<(), EditError> {
        let result = tune_song::delete_section(&self.song, index);
        self.arrange(result, None, Tables::Delete(index))
    }

    /// Writes the result of a section operation into the text (see the
    /// module docs) and commits it. A result equal to the current song
    /// changes nothing.
    fn arrange(
        &mut self,
        result: Result<Arranged, ArrangeError>,
        path: Option<&SongPath>,
        tables: Tables,
    ) -> Result<(), EditError> {
        let arranged = result.map_err(|e| arrange_error(e, path))?;
        if arranged.song == self.song {
            return Ok(());
        }
        let broken = || {
            EditError::Internal("the section edit could not be written into the file".to_owned())
        };
        let mut doc = self.doc.clone();
        normalize_positions(&mut doc);
        let start = block_start(&doc);
        section_tables(&mut doc, tables).ok_or_else(broken)?;
        write_plays(&mut doc, &self.song, &arranged.song).ok_or_else(broken)?;
        write_automation(&mut doc, &self.song, &arranged).ok_or_else(broken)?;
        if let Some(start) = start {
            rerank(&mut doc, &arranged.song, start);
        }
        self.commit_as(doc, Some(&arranged.song))
    }

    /// Validates the edited copy and keeps it only when it is a valid song.
    fn commit(&mut self, doc: DocumentMut) -> Result<(), EditError> {
        self.commit_as(doc, None)
    }

    /// [`commit`](Self::commit), and when `expected` is given, keeps the
    /// copy only when its text describes exactly that song.
    fn commit_as(&mut self, doc: DocumentMut, expected: Option<&Song>) -> Result<(), EditError> {
        let parsed = parse(&doc.to_string()).map_err(EditError::Invalid)?;
        if expected.is_some_and(|song| *song != parsed.song) {
            return Err(EditError::Internal(
                "the edited file does not describe the arranged song".to_owned(),
            ));
        }
        self.doc = doc;
        self.song = parsed.song;
        Ok(())
    }
}

/// The `bars` of a `sections[<i>].bars` edit.
fn section_bars(path: &SongPath, value: &Value) -> Result<u32, EditError> {
    let bad = |message: String| EditError::BadValue {
        path: path.to_string(),
        message,
    };
    let number = value
        .as_f64()
        .ok_or_else(|| bad("expected a number".to_owned()))?;
    let bars = as_integer(number).ok_or_else(|| bad("bars must be a whole number".to_owned()))?;
    u32::try_from(bars).map_err(|_| bad(section_bars_error(bars)))
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
        SongPath::Version => Value::from(FORMAT_VERSION),
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
        // A notes clip has no step notation: its value here is `null`.
        SongPath::Clip { clip } => song
            .clips
            .get(clip)?
            .steps_text()
            .map_or(Value::Null, Value::from),
        SongPath::ClipNotes { clip } => match song.clips.get(clip)? {
            Clip::Notes(n) => Value::Array(n.notes.iter().map(note_json).collect()),
            _ => Value::Null,
        },
        SongPath::ClipBars { clip } => match song.clips.get(clip)? {
            Clip::Notes(n) => Value::from(n.bars),
            _ => Value::Null,
        },
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

/// A note as JSON: `{"start", "pitch", "len", "vel"}`, `start` and `len` in
/// 16th steps, `pitch` a note name.
fn note_json(n: &Note) -> Value {
    let steps = |ticks: u32| num(f64::from(ticks) / f64::from(TICKS_PER_STEP));
    json!({
        "start": steps(n.start),
        "pitch": pitch_name(n.pitch),
        "len": steps(n.len),
        "vel": n.velocity,
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
                Target::Setting(Home::Instrument, spec) => {
                    let v = toml_edit::Value::from(string()?);
                    let slot = track_table(doc, track).ok_or_else(missing)?;
                    put_instrument_setting(slot, t.instrument.module.name, spec.name, v)
                        .ok_or_else(missing)?;
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
        SongPath::Clip { clip } => {
            let existing = song.clips.get(clip).ok_or_else(missing)?;
            if matches!(existing, Clip::Notes(_)) {
                return Err(bad(&format!(
                    "clip `{clip}` is a notes clip: it has no steps (set clips.{clip}.notes)"
                )));
            }
            let v = toml_edit::Value::from(string()?);
            root_child(doc, "clips")
                .and_then(|c| c.child(clip))
                .ok_or_else(missing)?
                .put("steps", v);
        }
        SongPath::ClipNotes { clip } | SongPath::ClipBars { clip } => {
            let existing = song.clips.get(clip).ok_or_else(missing)?;
            let field = if matches!(path, SongPath::ClipNotes { .. }) {
                "notes"
            } else {
                "bars"
            };
            if !matches!(existing, Clip::Notes(_)) {
                let kind = existing.kind().name();
                let article = if kind == "acid" { "an" } else { "a" };
                return Err(bad(&format!(
                    "clip `{clip}` is {article} {kind} clip: it has no {field}"
                )));
            }
            let v = if field == "notes" {
                notes_value(value).map_err(|m| bad(&m))?
            } else {
                let bars = number()?;
                as_integer(bars)
                    .map(toml_edit::Value::from)
                    .ok_or_else(|| bad("bars must be a whole number"))?
            };
            root_child(doc, "clips")
                .and_then(|c| c.child(clip))
                .ok_or_else(missing)?
                .put(field, v);
        }
        SongPath::SectionName(i) => {
            if *i >= song.sections.len() {
                return Err(missing());
            }
            let v = toml_edit::Value::from(string()?);
            section_table(doc, *i).ok_or_else(missing)?.put("name", v);
        }
        SongPath::SectionBars(_) | SongPath::SectionPlay { .. } => {
            unreachable!("SongDoc::set runs these as section operations")
        }
        SongPath::Version
        | SongPath::Track { .. }
        | SongPath::SectionLanes(_)
        | SongPath::Sections => {
            return Err(EditError::ReadOnly(path.to_string()));
        }
    }
    Ok(())
}

/// The TOML array for a `clips.<c>.notes` value, laid out as
/// [`notes_toml`](tune_song::notes_toml) lays notes out: `[]`, one note
/// inline, or one note per line with a trailing comma. Numbers are written
/// as given; the commit's validation judges them.
fn notes_value(value: &Value) -> Result<toml_edit::Value, String> {
    let notes = value
        .as_array()
        .ok_or("expected an array of notes, each {start, pitch, len, vel}")?;
    let entries = notes
        .iter()
        .map(|note| {
            let object = note.as_object().ok_or("expected an array of notes")?;
            if let Some(key) = object
                .keys()
                .find(|k| !["start", "pitch", "len", "vel"].contains(&k.as_str()))
            {
                return Err(format!("a note has start, pitch, len and vel, not `{key}`"));
            }
            let mut parts = Vec::new();
            for key in ["start", "pitch", "len", "vel"] {
                let Some(v) = object.get(key) else { continue };
                let text = if key == "pitch" {
                    let name = v.as_str().ok_or_else(|| {
                        "a note's pitch must be a note name, e.g. \"C3\"".to_owned()
                    })?;
                    toml_edit::Value::from(name).to_string()
                } else {
                    let n = v
                        .as_f64()
                        .ok_or_else(|| format!("a note's {key} must be a number"))?;
                    toml_number(n).to_string()
                };
                parts.push(format!("{key} = {}", text.trim()));
            }
            Ok(format!("{{ {} }}", parts.join(", ")))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let text = match entries.as_slice() {
        [] => "[]".to_owned(),
        [one] => format!("[{one}]"),
        many => format!(
            "[\n{}]",
            many.iter().map(|e| format!("  {e},\n")).collect::<String>()
        ),
    };
    text.parse::<toml_edit::Value>()
        .map_err(|e| format!("the notes do not make valid TOML: {e}"))
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

/// Sets `setting` on a track's instrument: a key of its table, or, for an
/// instrument written as a module name, the table `{ type = "<module>",
/// <setting> = <value> }` in its place (keeping the value's spacing and
/// trailing comment).
fn put_instrument_setting(
    mut track: Slot<'_>,
    module: &str,
    setting: &str,
    value: toml_edit::Value,
) -> Option<()> {
    let is_name = match &track {
        Slot::Table(t) => t.get("instrument").is_some_and(|i| i.as_str().is_some()),
        Slot::Inline(t) => t.get("instrument").is_some_and(|i| i.as_str().is_some()),
    };
    if is_name {
        let mut table = InlineTable::new();
        table.insert("type", toml_edit::Value::from(module));
        table.insert(setting, value);
        table.fmt();
        track.put("instrument", table.into());
    } else {
        track.child("instrument")?.put(setting, value);
    }
    Some(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_instrument_name_becomes_a_table_for_a_setting() {
        let mut doc: DocumentMut = "[tracks.a]\ninstrument = \"acid303\" # bass\n"
            .parse()
            .unwrap();
        let slot = track_table(&mut doc, "a").unwrap();
        put_instrument_setting(slot, "acid303", "mode", "x".into()).unwrap();
        assert_eq!(
            doc.to_string(),
            "[tracks.a]\ninstrument = { type = \"acid303\", mode = \"x\" } # bass\n"
        );
        let slot = track_table(&mut doc, "a").unwrap();
        put_instrument_setting(slot, "acid303", "mode", "y".into()).unwrap();
        assert_eq!(
            doc.to_string(),
            "[tracks.a]\ninstrument = { type = \"acid303\", mode = \"y\" } # bass\n"
        );
    }

    #[test]
    fn arrange_errors_read_as_edit_errors() {
        let bars = SongPath::SectionBars(0);
        let play = SongPath::SectionPlay {
            section: 3,
            track: "acid".into(),
        };
        let cases = [
            (
                arrange_error(ArrangeError::OutOfBounds { index: 3, len: 2 }, None),
                "index 3 is out of bounds (there are 2)",
            ),
            (
                arrange_error(ArrangeError::OutOfBounds { index: 3, len: 2 }, Some(&play)),
                "`sections[3].play.acid` does not exist in this song",
            ),
            (
                arrange_error(ArrangeError::NoSuchTrack("acid".into()), Some(&play)),
                "`sections[3].play.acid` does not exist in this song",
            ),
            (
                arrange_error(ArrangeError::NoSuchTrack("acid".into()), None),
                "`tracks.acid` does not exist in this song",
            ),
            (
                arrange_error(ArrangeError::LastSection, None),
                "a song needs at least one section",
            ),
            (
                arrange_error(ArrangeError::Bars { bars: 0 }, Some(&bars)),
                "bad value for `sections[0].bars`: bars must be 1 to 1024, got 0",
            ),
            (
                arrange_error(ArrangeError::TooLong, None),
                "the arrangement would be too long",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
        assert_eq!(
            EditError::Internal("x".into()).to_string(),
            "internal error: x; the song was not changed (please report this)"
        );
    }

    #[test]
    fn a_text_that_is_not_the_arranged_song_is_refused() {
        let src = "[song]\nversion = 2\nname = \"x\"\nbpm = 120\n\n\
                   [[sections]]\nname = \"a\"\nbars = 1\n";
        let mut d = SongDoc::parse(src).unwrap();
        let mut other = d.song().clone();
        other.bpm = 121.0;
        let edited: DocumentMut = src.replacen("bars = 1", "bars = 2", 1).parse().unwrap();
        let err = d.commit_as(edited, Some(&other)).unwrap_err();
        assert!(matches!(err, EditError::Internal(_)), "{err:?}");
        assert_eq!(d.text(), src);
        let same = d.song().clone();
        let edited: DocumentMut = src.parse().unwrap();
        d.commit_as(edited, Some(&same)).unwrap();
    }

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
