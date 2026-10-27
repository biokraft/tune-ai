//! Writing track and device edits into the song text: a track's table, an
//! entry of its `fx` list, a device's id and the param keys that follow it.
//!
//! Each edit runs the `tune_song` function for it and writes the result
//! into the text (see [`SongDoc::add_track`] and its siblings). The text
//! changes only where the edit does: the other tables, comments and
//! the other `fx` entries keep their bytes.

use serde_json::Value as Json;
use toml_edit::{Decor, DocumentMut, InlineTable, Item, Key, Table, Value};
use tune_song::toml_doc::{Slot, normalize_positions, reshape, root_child, walk, walk_mut};
use tune_song::{
    Clip, ClipKind, Device, EditOpError, Edited, Lane, LaneOrigin, Placement, Registry, Removed,
    Track, clip_table_toml, fx_entry_toml, parse, track_table_toml,
};

use super::arrange::{block_start, rerank, write_automation, write_plays};
use super::{EditError, SongDoc, fx_table, notes_value, put_instrument_setting, track_table};

impl SongDoc {
    /// Appends track `track` playing a new `module` instrument (see
    /// [`tune_song::add_track`]). Its table is written right after the last
    /// track table.
    pub fn add_track(&mut self, track: &str, module: &str) -> Result<(), EditError> {
        let edited = tune_song::add_track(&self.song, Registry::builtin(), track, module)?;
        let table = edited.arranged.song.tracks[track].clone();
        self.write_edited(edited, |doc| insert_track(doc, track, &table))?;
        Ok(())
    }

    /// Removes track `track` with the comments above its table, its lane
    /// stretches and an explicit `mix.duck_source` that names it (see
    /// [`tune_song::remove_track`]). Returns what went with it.
    pub fn remove_track(&mut self, track: &str) -> Result<Removed, EditError> {
        let edited = tune_song::remove_track(&self.song, track)?;
        let clears_source = edited.removed.duck_source.is_some();
        self.write_edited(edited, |doc| {
            remove_track_table(doc, track)?;
            if clears_source && let Some(mut mix) = root_child(doc, "mix") {
                mix.remove("duck_source");
            }
            Some(())
        })
    }

    /// Renames track `track` to `to` in place: its table keeps its comments
    /// and position; its lane stretches and an explicit `mix.duck_source`
    /// follow (see [`tune_song::rename_track`]).
    pub fn rename_track(&mut self, track: &str, to: &str) -> Result<(), EditError> {
        let edited = tune_song::rename_track(&self.song, track, to)?;
        let renames_source = self.song.mix.duck_source.as_deref() == Some(track);
        self.write_edited(edited, |doc| {
            rename_keys(root_child(doc, "tracks")?, |k| {
                (k == track).then(|| to.to_owned())
            })?;
            if renames_source && let Some(mut mix) = root_child(doc, "mix") {
                mix.put("duck_source", Value::from(to));
            }
            Some(())
        })?;
        Ok(())
    }

    /// Inserts a new `module` effect into the `fx` list of `track` at
    /// `index` (the end by default) and returns its id (see
    /// [`tune_song::add_effect`]). The other entries keep their text.
    pub fn add_effect(
        &mut self,
        track: &str,
        module: &str,
        id: Option<&str>,
        index: Option<usize>,
    ) -> Result<String, EditError> {
        let edited =
            tune_song::add_effect(&self.song, Registry::builtin(), track, module, id, index)?;
        let at = index.unwrap_or(self.song.tracks[track].fx.len());
        let device = edited.arranged.song.tracks[track].fx[at].clone();
        self.write_edited(edited, |doc| insert_fx(doc, track, at, &device))?;
        Ok(device.id)
    }

    /// Removes effect `device` of `track` and its lane stretches (see
    /// [`tune_song::remove_effect`]). Returns what went with it.
    pub fn remove_effect(&mut self, track: &str, device: &str) -> Result<Removed, EditError> {
        let edited = tune_song::remove_effect(&self.song, track, device)?;
        let at = self.song.tracks[track]
            .fx
            .iter()
            .position(|d| d.id == device)
            .expect("the effect exists");
        self.write_edited(edited, |doc| remove_fx(doc, track, at))
    }

    /// Moves effect `device` of `track` to `index` of the chain (see
    /// [`tune_song::move_effect`]); its text moves with it.
    pub fn move_effect(
        &mut self,
        track: &str,
        device: &str,
        index: usize,
    ) -> Result<(), EditError> {
        let edited = tune_song::move_effect(&self.song, track, device, index)?;
        let at = self.song.tracks[track]
            .fx
            .iter()
            .position(|d| d.id == device)
            .expect("the effect exists");
        self.write_edited(edited, |doc| move_fx(doc, track, at, index))?;
        Ok(())
    }

    /// Renames device `device` of `track` (its instrument or an effect) to
    /// `to`: writes its `id`, renames its param keys and its lane stretches
    /// follow (see [`tune_song::rename_device`]).
    pub fn rename_device(&mut self, track: &str, device: &str, to: &str) -> Result<(), EditError> {
        let edited = tune_song::rename_device(&self.song, track, device, to)?;
        let old = self.song.tracks[track].clone();
        self.write_edited(edited, |doc| rename_device_id(doc, track, &old, device, to))?;
        Ok(())
    }

    /// Swaps the instrument of `track` for a new `module` one: the
    /// `instrument` value, the old instrument's params, the placements the
    /// new module cannot play and its lane stretches (see
    /// [`tune_song::set_instrument`]). Returns what went with it.
    pub fn set_instrument(&mut self, track: &str, module: &str) -> Result<Removed, EditError> {
        let edited = tune_song::set_instrument(&self.song, Registry::builtin(), track, module)?;
        let old = self.song.clone();
        let new = edited.arranged.song.clone();
        self.write_edited(edited, |doc| {
            let device = &new.tracks[track].instrument;
            track_table(doc, track)?.put("instrument", Value::from(device.module.name));
            remove_params(doc, track, &format!("{}.", old.tracks[track].instrument.id))?;
            write_plays(doc, &old, &new)
        })
    }

    /// Appends clip `clip` of `kind` (`acid`, `drum` or `notes`) to the pool
    /// (see [`tune_song::add_clip`]). Its table goes right after the last
    /// clip table, else after the last track table. The keys are written as
    /// given, so the validator judges them: `steps` of a notes clip or
    /// `bars` and `notes` of a steps clip are refused with its hints.
    /// `steps` defaults to 16 rests, `bars` to 1.
    pub fn add_clip(
        &mut self,
        clip: &str,
        kind: &str,
        steps: Option<&str>,
        bars: Option<u32>,
        notes: Option<&Json>,
    ) -> Result<(), EditError> {
        let kind = ClipKind::from_name(kind).ok_or_else(|| {
            let expected: Vec<&str> = ClipKind::ALL.iter().map(|k| k.name()).collect();
            EditOpError::UnknownClipKind {
                name: kind.to_owned(),
                expected: format!("{} or {}", expected[..2].join(", "), expected[2]),
            }
        })?;
        tune_song::check_new_clip(&self.song, clip)?;
        let text = new_clip_table(clip, kind, steps, bars, notes)?;
        // Parse the table alone first: the validator's message is the answer
        // for a bad key, step or note, and the parsed clip is the edit's.
        let mut trial = self.doc.clone();
        let broken =
            || EditError::Internal("the edit could not be written into the file".to_owned());
        insert_table(&mut trial, "clips", clip, &text, None, CLIP_ANCHORS).ok_or_else(broken)?;
        let made = parse(&trial.to_string())
            .map_err(EditError::Invalid)?
            .song
            .clips
            .get(clip)
            .cloned()
            .ok_or_else(broken)?;
        let edited = tune_song::add_clip(&self.song, clip, made)?;
        self.write_edited(edited, |doc| {
            insert_table(doc, "clips", clip, &text, None, CLIP_ANCHORS)
        })?;
        Ok(())
    }

    /// Removes clip `clip` with the comments above its table and every
    /// placement of it (see [`tune_song::remove_clip`]). Returns the
    /// placements.
    pub fn remove_clip(&mut self, clip: &str) -> Result<Removed, EditError> {
        let edited = tune_song::remove_clip(&self.song, clip)?;
        let old = self.song.clone();
        let new = edited.arranged.song.clone();
        self.write_edited(edited, |doc| {
            match doc.as_table_mut().get_mut("clips")? {
                Item::Table(clips) => clips.remove(clip)?,
                _ => return None,
            };
            write_plays(doc, &old, &new)
        })
    }

    /// Renames clip `clip` to `to` in place: its table keeps its comments
    /// and position, and the `play` lists that name it are rewritten (see
    /// [`tune_song::rename_clip`]).
    pub fn rename_clip(&mut self, clip: &str, to: &str) -> Result<(), EditError> {
        let edited = tune_song::rename_clip(&self.song, clip, to)?;
        let old = self.song.clone();
        let new = edited.arranged.song.clone();
        self.write_edited(edited, |doc| {
            rename_keys(root_child(doc, "clips")?, |k| {
                (k == clip).then(|| to.to_owned())
            })?;
            write_plays(doc, &old, &new)
        })?;
        Ok(())
    }

    /// Copies clip `clip` as `to`, written right after the original, with
    /// its `steps` as written but not its comments (see
    /// [`tune_song::duplicate_clip`]).
    pub fn duplicate_clip(&mut self, clip: &str, to: &str) -> Result<(), EditError> {
        let edited = tune_song::duplicate_clip(&self.song, clip, to)?;
        let copy = &edited.arranged.song.clips[to];
        let written = self
            .doc
            .get("clips")
            .and_then(|c| c.get(clip))
            .and_then(|c| c.get("steps"))
            .and_then(Item::as_str);
        let text = match (copy, written) {
            (Clip::Notes(_), _) | (_, None) => clip_table_toml(to, copy),
            (_, Some(steps)) => format!(
                "[clips.{to}]\nkind = {}\nsteps = {}\n",
                Value::from(copy.kind().name()),
                Value::from(steps)
            ),
        };
        self.write_edited(edited, |doc| {
            insert_table(doc, "clips", to, &text, Some(clip), CLIP_ANCHORS)
        })?;
        Ok(())
    }

    /// Adds a placement of `clip` on `track`, `bars` long from 0-based bar
    /// `start_bar`, starting `offset` bars into the clip (see
    /// [`tune_song::add_placement`]). The track's `play` list is rewritten,
    /// comments inside it gone; overlaps and clips the track cannot play are
    /// the validator's.
    pub fn add_placement(
        &mut self,
        track: &str,
        clip: &str,
        start_bar: u32,
        bars: u32,
        offset: u32,
    ) -> Result<(), EditError> {
        let placement = Placement {
            start_bar,
            bars,
            clip: clip.to_owned(),
            offset_bars: offset,
        };
        let edited = tune_song::add_placement(&self.song, track, placement)?;
        self.write_plays_of(edited)?;
        Ok(())
    }

    /// Moves the placement of `track` that starts at 0-based bar `start_bar`
    /// to start at `to` (see [`tune_song::move_placement`]).
    pub fn move_placement(
        &mut self,
        track: &str,
        start_bar: u32,
        to: u32,
    ) -> Result<(), EditError> {
        let edited = tune_song::move_placement(&self.song, track, start_bar, to)?;
        self.write_plays_of(edited)?;
        Ok(())
    }

    /// Makes the placement of `track` that starts at 0-based bar `start_bar`
    /// `bars` long (see [`tune_song::resize_placement`]).
    pub fn resize_placement(
        &mut self,
        track: &str,
        start_bar: u32,
        bars: u32,
    ) -> Result<(), EditError> {
        let edited = tune_song::resize_placement(&self.song, track, start_bar, bars)?;
        self.write_plays_of(edited)?;
        Ok(())
    }

    /// Removes the placement of `track` that starts at 0-based bar
    /// `start_bar` and returns it (see [`tune_song::remove_placement`]).
    pub fn remove_placement(&mut self, track: &str, start_bar: u32) -> Result<Removed, EditError> {
        let edited = tune_song::remove_placement(&self.song, track, start_bar)?;
        self.write_plays_of(edited)
    }

    /// Adds a stretch of the lane for `param` (`<track>.<param id>`, `mix.`
    /// for a mix param) from 0-based bar `start_bar` for `bars` bars (see
    /// [`tune_song::add_lane`]). It gets a table of its own, laid out right
    /// after the stretches of the section it starts in; the first one
    /// creates the root `[[automation]]` list. Overlaps, unknown params and
    /// points out of range are the validator's.
    pub fn add_lane(
        &mut self,
        param: &str,
        start_bar: u32,
        bars: u32,
        points: Vec<(f64, f64)>,
    ) -> Result<(), EditError> {
        let (track, param) = split_lane_param(param)?;
        let lane = Lane {
            track: track.to_owned(),
            param: param.to_owned(),
            start_bar,
            bars,
            points,
        };
        let edited = tune_song::add_lane(&self.song, lane)?;
        self.write_edited(edited, |_| Some(()))?;
        Ok(())
    }

    /// Removes the stretch of `param` that starts at 0-based bar `start_bar`,
    /// its table and the comments on it, and returns it (see
    /// [`tune_song::remove_lane`]).
    pub fn remove_lane(&mut self, param: &str, start_bar: u32) -> Result<Removed, EditError> {
        let (track, param) = split_lane_param(param)?;
        let edited = tune_song::remove_lane(&self.song, track, param, start_bar)?;
        self.write_edited(edited, |_| Some(()))
    }

    /// Replaces the `points` of the stretch of `param` that starts at
    /// 0-based bar `start_bar`, leaving the rest of its table as it is (see
    /// [`tune_song::set_lane_points`]).
    pub fn set_lane_points(
        &mut self,
        param: &str,
        start_bar: u32,
        points: Vec<(f64, f64)>,
    ) -> Result<(), EditError> {
        let (track, param) = split_lane_param(param)?;
        let edited = tune_song::set_lane_points(&self.song, track, param, start_bar, points)?;
        self.write_edited(edited, |_| Some(()))?;
        Ok(())
    }

    /// Writes the `play` lists an edit changed.
    fn write_plays_of(&mut self, edited: Edited) -> Result<Removed, EditError> {
        let old = self.song.clone();
        let new = edited.arranged.song.clone();
        self.write_edited(edited, |doc| write_plays(doc, &old, &new))
    }

    /// Writes an edit's result into a copy of the text with `write`, and
    /// the lane stretches it kept, and commits the copy only when it
    /// describes exactly the edited song. An edit that changes nothing
    /// writes nothing.
    fn write_edited(
        &mut self,
        edited: Edited,
        write: impl FnOnce(&mut DocumentMut) -> Option<()>,
    ) -> Result<Removed, EditError> {
        let Edited { arranged, removed } = edited;
        if arranged.song == self.song {
            return Ok(removed);
        }
        let broken =
            || EditError::Internal("the edit could not be written into the file".to_owned());
        let mut doc = self.doc.clone();
        write(&mut doc).ok_or_else(broken)?;
        if arranged.song.automation != self.song.automation {
            // A new stretch is laid out among the section tables, as a
            // section operation lays out its own.
            let new_tables = arranged.lanes.contains(&LaneOrigin::New);
            let start = if new_tables {
                normalize_positions(&mut doc);
                block_start(&doc)
            } else {
                None
            };
            write_automation(&mut doc, &self.song, &arranged).ok_or_else(broken)?;
            if let Some(start) = start {
                rerank(&mut doc, &arranged.song, start);
            }
        }
        self.commit_as(doc, Some(&arranged.song))?;
        Ok(removed)
    }
}

/// Splits a lane's `param` as the file spells it, `<track>.<param id>`, at
/// its first `.`; `mix` is the track of a mix param.
fn split_lane_param(param: &str) -> Result<(&str, &str), EditError> {
    param
        .split_once('.')
        .filter(|(track, id)| !track.is_empty() && !id.is_empty())
        .ok_or_else(|| EditError::BadValue {
            path: param.to_owned(),
            message: "a lane param is `<track>.<param id>`, such as `acid.acid303.cutoff` \
                      or `mix.reverb.size`"
                .to_owned(),
        })
}

/// Where the first clip table goes: after the last of these tables.
const CLIP_ANCHORS: &[&str] = &["song", "mix", "tracks"];

/// The text of a new clip's table: the keys as given, `steps` and `bars`
/// defaulting for the kinds that take them.
fn new_clip_table(
    id: &str,
    kind: ClipKind,
    steps: Option<&str>,
    bars: Option<u32>,
    notes: Option<&Json>,
) -> Result<String, EditError> {
    let steps = match (kind, steps) {
        (_, Some(steps)) => Some(steps),
        (ClipKind::Acid, None) => Some(". . . . . . . . . . . . . . . ."),
        (ClipKind::Drum, None) => Some("................"),
        (ClipKind::Notes, None) => None,
    };
    let bars = match (kind, bars) {
        (_, Some(bars)) => Some(bars),
        (ClipKind::Notes, None) => Some(1),
        _ => None,
    };
    let mut out = format!("[clips.{id}]\nkind = {}\n", Value::from(kind.name()));
    if let Some(steps) = steps {
        out.push_str(&format!("steps = {}\n", Value::from(steps)));
    }
    if let Some(bars) = bars {
        out.push_str(&format!("bars = {bars}\n"));
    }
    if let Some(notes) = notes {
        let value = notes_value(notes).map_err(|message| EditError::BadValue {
            path: format!("clips.{id}.notes"),
            message,
        })?;
        out.push_str(&format!("notes = {}\n", value.to_string().trim()));
    }
    Ok(out)
}

/// Adds the table of track `id` right after the last `[tracks.*]` table,
/// with one blank line before it. A file whose track entries are inline
/// tables (`[tracks]` with `kick = { … }`) gets an inline entry; a file with
/// no tracks gets `[tracks.<id>]` after `[song]` and `[mix]`.
pub(super) fn insert_track(doc: &mut DocumentMut, id: &str, track: &Track) -> Option<()> {
    let text = track_table_toml(id, track);
    insert_table(doc, "tracks", id, &text, None, &["song", "mix"])
}

/// Adds the table `text` (`[<container>.<id>]` and its keys, as
/// [`track_table_toml`] writes them) to the
/// `container` tables, with one blank line before it. It goes right after
/// the table of `sibling` when that is given, else after the container's
/// last table; a container with no table yet is created after the last
/// table of the first of `anchors` that has one (in the file's order). A
/// container written as inline entries gets an inline entry.
fn insert_table(
    doc: &mut DocumentMut,
    container: &str,
    id: &str,
    text: &str,
    sibling: Option<&str>,
    anchors: &[&str],
) -> Option<()> {
    normalize_positions(doc);
    let mut parsed: DocumentMut = text.parse().ok()?;
    let Item::Table(mut tables) = parsed.as_table_mut().remove(container)? else {
        return None;
    };
    let Item::Table(mut table) = tables.remove(id)? else {
        return None;
    };
    let root = doc.as_table_mut();
    let inline = match root.get(container) {
        Some(Item::Table(t)) => t.iter().any(|(_, item)| item.is_value()),
        Some(_) => return None,
        None => false,
    };
    if inline {
        let Some(Item::Table(holder)) = root.get_mut(container) else {
            return None;
        };
        let mut entry = table.into_inline_table();
        entry.fmt();
        // Behind the sibling when there is one: the entries after it move
        // behind the new one.
        let later: Vec<String> = match sibling {
            Some(sibling) => holder
                .iter()
                .map(|(k, _)| k.to_owned())
                .skip_while(|k| k != sibling)
                .skip(1)
                .collect(),
            None => Vec::new(),
        };
        let moved: Vec<(Key, Item)> = later
            .iter()
            .filter_map(|k| holder.remove_entry(k))
            .collect();
        holder.insert(id, Item::Value(Value::InlineTable(entry)));
        for (key, item) in moved {
            holder.insert_formatted(&key, item);
        }
        return Some(());
    }
    let last = match root.get(container) {
        Some(Item::Table(t)) => match sibling {
            Some(sibling) => match t.get(sibling) {
                Some(Item::Table(s)) => max_position(s),
                _ => return None,
            },
            None => max_position(t),
        },
        _ => None,
    };
    // Where the new table goes: after `last`, else after the last table of
    // the anchors.
    let after = match last {
        Some(last) => last,
        None => anchors
            .iter()
            .filter_map(|key| match root.get(key) {
                Some(Item::Table(t)) => max_position(t),
                _ => None,
            })
            .max()
            .unwrap_or(0),
    };
    let created = last.is_none();
    let new = if created { 2 } else { 1 };
    walk_mut(root, &mut |t| {
        if let Some(p) = t.position().filter(|p| *p > after) {
            t.set_position(Some(p + new));
        }
    });
    table.set_position(Some(after + new));
    table.decor_mut().set_prefix("\n");
    if created {
        let mut holder = Table::new();
        holder.set_implicit(true);
        holder.set_position(Some(after + 1));
        root.insert(container, Item::Table(holder));
    }
    let Some(Item::Table(holder)) = root.get_mut(container) else {
        return None;
    };
    holder.insert(id, Item::Table(table));
    Some(())
}

/// The greatest position of a table in `table`, itself and what it holds.
fn max_position(table: &Table) -> Option<isize> {
    let mut max = None;
    walk(table, &mut |t| max = max.max(t.position()));
    max
}

/// Removes the table of track `id`; the comments above it go with it.
pub(super) fn remove_track_table(doc: &mut DocumentMut, id: &str) -> Option<()> {
    match doc.as_table_mut().get_mut("tracks")? {
        Item::Table(tracks) => tracks.remove(id).map(|_| ()),
        _ => None,
    }
}

/// Renames the keys of `slot` that `rename` gives a new name for, keeping
/// their order, spacing and values.
fn rename_keys(slot: Slot<'_>, rename: impl Fn(&str) -> Option<String>) -> Option<()> {
    let renamed = |key: Key| match rename(key.get()) {
        Some(new) => Key::new(new)
            .with_leaf_decor(key.leaf_decor().clone())
            .with_dotted_decor(key.dotted_decor().clone()),
        None => key,
    };
    match slot {
        Slot::Table(t) => {
            let keys: Vec<String> = t.iter().map(|(k, _)| k.to_owned()).collect();
            let entries = keys.iter().map(|k| t.remove_entry(k));
            let entries: Vec<(Key, Item)> = entries.collect::<Option<_>>()?;
            for (key, item) in entries {
                t.insert_formatted(&renamed(key), item);
            }
        }
        Slot::Inline(t) => {
            let keys: Vec<String> = t.iter().map(|(k, _)| k.to_owned()).collect();
            let entries = keys.iter().map(|k| t.remove_entry(k));
            let entries: Vec<(Key, Value)> = entries.collect::<Option<_>>()?;
            for (key, value) in entries {
                t.insert_formatted(&renamed(key), value);
            }
        }
    }
    Some(())
}

/// The `fx` array of a track.
fn fx_array<'a>(doc: &'a mut DocumentMut, track: &str) -> Option<&'a mut toml_edit::Array> {
    match track_table(doc, track)? {
        Slot::Table(t) => t.get_mut("fx")?.as_array_mut(),
        Slot::Inline(t) => t.get_mut("fx")?.as_array_mut(),
    }
}

/// Inserts `device` as entry `index` of the track's `fx` list, the others'
/// text unchanged. A track with no `fx` gets the list as a new key, placed
/// before its `play` list.
fn insert_fx(doc: &mut DocumentMut, track: &str, index: usize, device: &Device) -> Option<()> {
    let entry: Value = fx_entry_toml(device).parse().ok()?;
    if let Some(array) = fx_array(doc, track) {
        reshape(array, |values| values.insert(index, entry));
        return Some(());
    }
    let list: Value = format!("[{}]", fx_entry_toml(device)).parse().ok()?;
    let mut slot = track_table(doc, track)?;
    slot.put("fx", list);
    if let Slot::Table(t) = &mut slot {
        // `fx` goes before `play`: move `play` behind it.
        if let Some((key, item)) = t.remove_entry("play") {
            t.insert_formatted(&key, item);
        }
    }
    Some(())
}

/// Removes entry `index` of the track's `fx` list, and the key when it was
/// the last.
fn remove_fx(doc: &mut DocumentMut, track: &str, index: usize) -> Option<()> {
    let array = fx_array(doc, track)?;
    reshape(array, |values| {
        values.remove(index);
    });
    if array.is_empty() {
        track_table(doc, track)?.remove("fx");
    }
    Some(())
}

/// Moves entry `from` of the track's `fx` list to `to`.
fn move_fx(doc: &mut DocumentMut, track: &str, from: usize, to: usize) -> Option<()> {
    reshape(fx_array(doc, track)?, |values| {
        let moved = values.remove(from);
        values.insert(to, moved);
    });
    Some(())
}

/// Gives the device `device` of `track` (as `old` has it) the id `to`: the
/// `id` key of its entry and the param keys that start with its old id.
fn rename_device_id(
    doc: &mut DocumentMut,
    track: &str,
    old: &Track,
    device: &str,
    to: &str,
) -> Option<()> {
    let prefix = format!("{device}.");
    let rekey = |key: &str| key.strip_prefix(&prefix).map(|rest| format!("{to}.{rest}"));
    if old.instrument.id == device {
        let slot = track_table(doc, track)?;
        let is_name = match &slot {
            Slot::Table(t) => t.get("instrument").is_some_and(|i| i.as_str().is_some()),
            Slot::Inline(t) => t.get("instrument").is_some_and(|i| i.as_str().is_some()),
        };
        if is_name {
            let module = old.instrument.module.name;
            put_instrument_setting(slot, module, "id", Value::from(to))?;
        } else {
            put_id(slot.child("instrument")?, to);
        }
        // The instrument's params share the track's `params` table.
        if let Some(params) = track_table(doc, track)?.child("params") {
            rename_keys(params, rekey)?;
        }
    } else {
        let at = old.fx.iter().position(|d| d.id == device)?;
        put_id(fx_table(doc, track, at)?, to);
        if let Some(params) = fx_table(doc, track, at)?.child("params") {
            rename_keys(params, rekey)?;
        }
    }
    Some(())
}

/// Sets the `id` of a device's table: replaced where it stands, else added
/// right after `type`.
fn put_id(mut slot: Slot<'_>, id: &str) {
    let value = Value::from(id);
    let Slot::Inline(table) = &mut slot else {
        return slot.put("id", value);
    };
    let after_type = table.get("id").is_none()
        && table.get("type").is_some()
        && table.iter().last().is_some_and(|(k, _)| k != "type");
    if !after_type {
        return slot.put("id", value);
    }
    insert_after(table, "type", "id", value);
}

/// Inserts `key = value` right after `after` in an inline table that has
/// more keys behind it, spaced like `{ a = 1, b = 2, c = 3 }`.
fn insert_after(table: &mut InlineTable, after: &str, key: &str, mut value: Value) {
    let keys: Vec<String> = table.iter().map(|(k, _)| k.to_owned()).collect();
    let mut entries: Vec<(Key, Value)> =
        keys.iter().filter_map(|k| table.remove_entry(k)).collect();
    let at = entries
        .iter()
        .position(|(k, _)| k.get() == after)
        .map_or(entries.len(), |i| i + 1);
    *value.decor_mut() = Decor::new(" ", "");
    let key = Key::new(key).with_leaf_decor(Decor::new(" ", " "));
    entries.insert(at, (key, value));
    for (key, value) in entries {
        table.insert_formatted(&key, value);
    }
}

/// Removes the entries of the track's `params` table whose key starts with
/// `prefix`, and the table when none is left. A table that keeps entries
/// keeps its spacing.
fn remove_params(doc: &mut DocumentMut, track: &str, prefix: &str) -> Option<()> {
    let Some(params) = track_table(doc, track)?.child("params") else {
        return Some(());
    };
    let left = match params {
        Slot::Table(t) => {
            t.retain(|k, _| !k.starts_with(prefix));
            t.len()
        }
        Slot::Inline(t) => {
            // The space before `}` is the last value's suffix: keep it there.
            let tail = t
                .iter()
                .last()
                .and_then(|(_, v)| v.decor().suffix().and_then(|s| s.as_str()))
                .map(str::to_owned);
            t.retain(|k, _| !k.starts_with(prefix));
            if let (Some(tail), Some((_, last))) = (tail, t.iter_mut().last()) {
                last.decor_mut().set_suffix(tail);
            }
            t.len()
        }
    };
    if left == 0 {
        track_table(doc, track)?.remove("params");
    }
    Some(())
}
