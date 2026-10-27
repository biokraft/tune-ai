//! Writing track and device edits into the song text: a track's table, an
//! entry of its `fx` list, a device's id and the param keys that follow it.
//!
//! Each edit runs the `tune_song` function for it and writes the result
//! into the text (see [`SongDoc::add_track`] and its siblings). The text
//! changes only where the edit does: the other tables, comments and
//! the other `fx` entries keep their bytes.

use toml_edit::{Decor, DocumentMut, InlineTable, Item, Key, Table, Value};
use tune_song::toml_doc::{Slot, normalize_positions, reshape, root_child, walk, walk_mut};
use tune_song::{Device, Edited, Registry, Removed, Track, fx_entry_toml, track_table_toml};

use super::arrange::{write_automation, write_plays};
use super::{EditError, SongDoc, fx_table, put_instrument_setting, track_table};

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
            write_automation(&mut doc, &self.song, &arranged).ok_or_else(broken)?;
        }
        self.commit_as(doc, Some(&arranged.song))?;
        Ok(removed)
    }
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
        holder.insert(id, Item::Value(Value::InlineTable(entry)));
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
