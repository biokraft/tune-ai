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
//! and write their result into the text: the `[[sections]]` tables, the
//! `play` lists of the tracks whose placements changed, and the
//! `[[automation]]` tables, each kept stretch keeping its comments. The
//! section and automation tables are then laid out as one block, each
//! section followed by the stretches that start in it. Comments inside a
//! rewritten `play` list are not kept.

use serde_json::{Value, json};
use toml_edit::{ArrayOfTables, DocumentMut, InlineTable, Item, Table};
use tune_dsp::{ParamSpec, STRIP_PARAMS, find_spec};
use tune_song::toml_doc::{
    Slot, element_mut, inline_element, normalize_positions, reshape, root_child, walk, walk_mut,
};
use tune_song::{
    ArrangeError, Arranged, Device, FORMAT_VERSION, Lane, LaneOrigin, MIX_TRACK_ID, Migration,
    Placement, Registry, SectionPlay, SettingSpec, Song, SongError, Track, find_mix_param, migrate,
    parse, placements_toml,
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
    /// A section operation would put a bar past `u32::MAX` (never for a
    /// song the validator accepts).
    #[error("the arrangement would be too long")]
    TooLong,
    #[error("the edit would make the song invalid:\n{}", summary(.0))]
    Invalid(Vec<SongError>),
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
        // The validator's own wording, "bars must be 1 to 1024, got <n>".
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
        let parsed = parse(text)?;
        let (migrated, migrated_from) = if parsed.version == FORMAT_VERSION {
            (None, None)
        } else {
            match migrate(text)? {
                Migration::Migrated { from, text } => (Some(text), Some(from)),
                Migration::Current => (None, None),
            }
        };
        let doc = migrated
            .as_deref()
            .unwrap_or(text)
            .parse::<DocumentMut>()
            .map_err(|e| {
                vec![SongError {
                    line: None,
                    column: None,
                    message: e.message().trim_end().to_owned(),
                }]
            })?;
        Ok(Self {
            doc,
            song: parsed.song,
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
        let broken = || EditError::NoSuchTarget(SongPath::Sections.to_string());
        let mut doc = self.doc.clone();
        normalize_positions(&mut doc);
        let start = block_start(&doc);
        section_tables(&mut doc, tables).ok_or_else(broken)?;
        write_plays(&mut doc, &self.song, &arranged.song).ok_or_else(broken)?;
        write_automation(&mut doc, &self.song, &arranged).ok_or_else(broken)?;
        if let Some(start) = start {
            rerank(&mut doc, &arranged.song, start);
        }
        self.commit(doc)?;
        debug_assert_eq!(
            self.song, arranged.song,
            "the text describes the arranged song"
        );
        Ok(())
    }

    /// Validates the edited copy and keeps it only when it is a valid song.
    fn commit(&mut self, doc: DocumentMut) -> Result<(), EditError> {
        let parsed = parse(&doc.to_string()).map_err(EditError::Invalid)?;
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
    u32::try_from(bars).map_err(|_| bad(format!("bars must be 1 to 1024, got {bars}")))
}

/// What a section operation does to the `[[sections]]` tables themselves.
#[derive(Clone, Copy)]
enum Tables {
    /// Nothing: only what plays changes.
    Keep,
    /// Moves section `.0` to index `.1`.
    Move(usize, usize),
    /// Puts a copy of section `.0` right after it.
    Duplicate(usize),
    Delete(usize),
    /// Section `.0` gets `bars = .1`.
    Bars(usize, u32),
}

/// Applies `tables` to the sections, written as `[[sections]]` or as an
/// inline array. The indices were checked by the section operation.
fn section_tables(doc: &mut DocumentMut, tables: Tables) -> Option<()> {
    match tables {
        Tables::Keep => return Some(()),
        Tables::Bars(index, bars) => {
            section_table(doc, index)?.put("bars", toml_edit::Value::from(i64::from(bars)));
            return Some(());
        }
        Tables::Move(..) | Tables::Duplicate(_) | Tables::Delete(_) => {}
    }
    match doc.as_table_mut().get_mut("sections")? {
        Item::ArrayOfTables(aot) => match tables {
            Tables::Move(from, to) => {
                let moved = aot.remove(from);
                aot.insert(to, moved);
            }
            Tables::Duplicate(index) => {
                let copy = aot.get(index)?.clone();
                aot.insert(index + 1, copy);
            }
            Tables::Delete(index) => {
                aot.remove(index);
            }
            Tables::Keep | Tables::Bars(..) => {}
        },
        Item::Value(toml_edit::Value::Array(array)) => reshape(array, |values| match tables {
            Tables::Move(from, to) => {
                let moved = values.remove(from);
                values.insert(to, moved);
            }
            Tables::Duplicate(index) => {
                let copy = values[index].clone();
                values.insert(index + 1, copy);
            }
            Tables::Delete(index) => {
                values.remove(index);
            }
            Tables::Keep | Tables::Bars(..) => {}
        }),
        _ => return None,
    }
    Some(())
}

/// Rewrites the `play` list of every track whose placements differ between
/// `old` and `new`: the value is replaced in place (its key, spacing and
/// trailing comment kept), added as the track's last key, or removed when
/// the track no longer plays.
fn write_plays(doc: &mut DocumentMut, old: &Song, new: &Song) -> Option<()> {
    for (id, track) in &new.tracks {
        if old.tracks.get(id).is_some_and(|o| o.play == track.play) {
            continue;
        }
        let mut slot = track_table(doc, id)?;
        if track.play.is_empty() {
            slot.remove("play");
        } else {
            slot.put("play", placements_toml(&track.play).parse().ok()?);
        }
    }
    Some(())
}

/// Rebuilds the root `automation` list (`[[automation]]` tables or an inline
/// array) from the arranged stretches: a kept stretch reuses its old table,
/// comments included, with only the values that changed put in place; a
/// copy clones the original's table. Of a split stretch whose first piece
/// is gone, the first piece left reuses the old table, so its comments
/// stay; any other piece is a new table. Old tables of stretches that are
/// gone are dropped.
fn write_automation(doc: &mut DocumentMut, old: &Song, arranged: &Arranged) -> Option<()> {
    let root = doc.as_table_mut();
    let reused = reused_tables(&arranged.lanes);
    let lanes = arranged.song.automation.iter().zip(reused);
    let empty = arranged.lanes.is_empty();
    match root.get_mut("automation") {
        None => return empty.then_some(()),
        Some(Item::ArrayOfTables(aot)) => {
            let mut new = ArrayOfTables::new();
            for (lane, reuse) in lanes {
                let (table, before) = match reuse {
                    Some(i) => (aot.get(i)?.clone(), Some(old.automation.get(i)?)),
                    None => {
                        let mut table = Table::new();
                        table.decor_mut().set_prefix("\n");
                        (table, None)
                    }
                };
                let mut table = table;
                put_lane(Slot::Table(&mut table), before, lane)?;
                new.push(table);
            }
            *aot = new;
        }
        Some(Item::Value(toml_edit::Value::Array(array))) => {
            let mut new = Vec::new();
            for (lane, reuse) in lanes {
                let (mut value, before) = match reuse {
                    Some(i) => (array.get(i)?.clone(), Some(old.automation.get(i)?)),
                    None => (InlineTable::new().into(), None),
                };
                put_lane(Slot::Inline(value.as_inline_table_mut()?), before, lane)?;
                if before.is_none() {
                    value.as_inline_table_mut()?.fmt();
                }
                new.push(value);
            }
            reshape(array, |values| *values = new);
        }
        Some(_) => return None,
    }
    if empty {
        root.remove("automation");
    }
    Some(())
}

/// For each arranged stretch, the index of the old table it is written into:
/// its own for a kept stretch or a copy; for a split piece, the original's
/// when no kept piece of that stretch is left and no earlier piece took it;
/// `None` for a new table.
fn reused_tables(origins: &[LaneOrigin]) -> Vec<Option<usize>> {
    let kept: Vec<usize> = origins
        .iter()
        .filter_map(|o| match *o {
            LaneOrigin::Kept(i) => Some(i),
            _ => None,
        })
        .collect();
    let mut taken = Vec::new();
    origins
        .iter()
        .map(|o| match *o {
            LaneOrigin::Kept(i) | LaneOrigin::Copy(i) => Some(i),
            LaneOrigin::Split(i) if !kept.contains(&i) && !taken.contains(&i) => {
                taken.push(i);
                Some(i)
            }
            LaneOrigin::Split(_) => None,
        })
        .collect()
}

/// Writes stretch `lane` into its table. `before` is the stretch the table
/// held: only the values that differ from it are put, so the others keep
/// their text. A new table (`before` is `None`) gets every key.
fn put_lane(mut slot: Slot<'_>, before: Option<&Lane>, lane: &Lane) -> Option<()> {
    if before.is_none() {
        let param = format!("{}.{}", lane.track, lane.param);
        slot.put("param", toml_edit::Value::from(param));
    }
    if before.is_none_or(|b| b.start_bar != lane.start_bar) {
        slot.put("at", toml_edit::Value::from(i64::from(lane.start_bar) + 1));
    }
    if before.is_none_or(|b| b.bars != lane.bars) {
        slot.put("bars", toml_edit::Value::from(i64::from(lane.bars)));
    }
    if before.is_none_or(|b| b.points != lane.points) {
        let points: Vec<String> = lane
            .points
            .iter()
            .map(|&(bar, value)| format!("[{}, {}]", number_text(bar), number_text(value)))
            .collect();
        slot.put("points", format!("[{}]", points.join(", ")).parse().ok()?);
    }
    Some(())
}

/// A number as TOML text, exact integers as integers (`2`, `0.25`).
fn number_text(v: f64) -> String {
    match as_integer(v) {
        Some(i) => i.to_string(),
        None => v.to_string(),
    }
}

/// The rank (see [`normalize_positions`]) where the section block starts:
/// the first `[[sections]]` table's, else the first `[[automation]]`
/// table's; `None` when neither is written as an array of tables.
fn block_start(doc: &DocumentMut) -> Option<isize> {
    let first = |key: &str| match doc.as_table().get(key) {
        Some(Item::ArrayOfTables(aot)) => aot.iter().filter_map(Table::position).min(),
        _ => None,
    };
    first("sections").or_else(|| first("automation"))
}

/// Lays the `[[sections]]` and `[[automation]]` tables out as one block
/// from rank `start`: section 0, the stretches that start in it, section 1,
/// its stretches, and so on. Every other table at or after `start` follows
/// the block, in its order; the tables before it stay. The document's
/// positions must be normalized ranks (the moved and new tables' aside).
fn rerank(doc: &mut DocumentMut, song: &Song, start: isize) {
    let root = doc.as_table_mut();
    let is_block = |key: &str, item: &Item| {
        matches!(key, "sections" | "automation") && matches!(item, Item::ArrayOfTables(_))
    };
    let sizes = |key: &str| -> Vec<isize> {
        match root.get(key) {
            Some(Item::ArrayOfTables(aot)) => aot
                .iter()
                .map(|t| {
                    let mut n = 0;
                    walk(t, &mut |_| n += 1);
                    n
                })
                .collect(),
            _ => Vec::new(),
        }
    };
    let (section_sizes, lane_sizes) = (sizes("sections"), sizes("automation"));
    // (section, sections before stretches, index), in block order.
    let starts = song.section_start_bars();
    let section_of = |lane: &Lane| starts.partition_point(|&s| s <= lane.start_bar).max(1) - 1;
    let mut order: Vec<(usize, bool, usize)> = (0..section_sizes.len())
        .map(|i| (i, false, i))
        .chain(
            song.automation
                .iter()
                .take(lane_sizes.len())
                .enumerate()
                .map(|(j, lane)| (section_of(lane), true, j)),
        )
        .collect();
    order.sort_unstable();
    let mut section_at = vec![0; section_sizes.len()];
    let mut lane_at = vec![0; lane_sizes.len()];
    let mut next = start;
    for (_, is_lane, i) in order {
        let (at, size) = if is_lane {
            (&mut lane_at[i], lane_sizes[i])
        } else {
            (&mut section_at[i], section_sizes[i])
        };
        *at = next;
        next += size;
    }
    let block = next - start;
    for (key, item) in root.iter_mut() {
        let place: Option<&[isize]> = match key.get() {
            "sections" if is_block("sections", item) => Some(&section_at),
            "automation" if is_block("automation", item) => Some(&lane_at),
            _ => None,
        };
        let tables: Vec<&mut Table> = match item {
            Item::Table(t) => vec![t],
            Item::ArrayOfTables(aot) => aot.iter_mut().collect(),
            _ => Vec::new(),
        };
        for (i, table) in tables.into_iter().enumerate() {
            match place {
                Some(place) => {
                    let mut next = place[i];
                    walk_mut(table, &mut |t| {
                        t.set_position(Some(next));
                        next += 1;
                    });
                }
                None => walk_mut(table, &mut |t| {
                    if let Some(p) = t.position().filter(|p| *p >= start) {
                        t.set_position(Some(p + block));
                    }
                }),
            }
        }
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
        SongPath::Clip { clip } => Value::from(song.clips.get(clip)?.steps_text()),
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
            if !song.clips.contains_key(clip) {
                return Err(missing());
            }
            let v = toml_edit::Value::from(string()?);
            root_child(doc, "clips")
                .and_then(|c| c.child(clip))
                .ok_or_else(missing)?
                .put("steps", v);
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
