//! Rewrites a version 1 song file as version 2, keeping its comments and its
//! table order.
//!
//! The file is edited as a `toml_edit` document, so everything the
//! migration does not touch keeps its text. What changes: `[song]` gets
//! `version = 2` first; `[patterns.<id>]` become `[clips.<id>]` with `kind`
//! in place of `track`; every track that plays gets a `play` list; and each
//! section loses `play` and `automation`, whose lanes become top-level
//! `[[automation]]` stretches placed right after their section. The result is
//! parsed again and must be exactly the song the v1 file describes.

use toml_edit::{ArrayOfTables, Decor, DocumentMut, Item, Key, Table, Value};

use crate::toml_doc::{Slot, element_mut, normalize_positions, root_child, walk, walk_mut};
use crate::{FORMAT_VERSION, Parsed, Song, SongError, parse, placements_toml};

/// What [`migrate`] made of a song file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Migration {
    /// The file is already in the newest format.
    Current,
    /// The file was in format `from`; `text` is the same song in the newest
    /// format, with the file's comments and table order.
    Migrated { from: u32, text: String },
}

/// The message when the rewritten text is not the v1 song: a bug here, never
/// a problem with the file.
const MISMATCH: &str = "internal error: the migrated file does not match the v1 song; \
                        the file was not changed (please report this)";

/// How far apart table positions are spread before the migration adds
/// tables, so that new tables fit between existing ones.
const SPREAD: isize = 1024;

/// Migrates a song file to the newest format. v1 errors are reported at
/// their place in `src`.
pub fn migrate(src: &str) -> Result<Migration, Vec<SongError>> {
    let Parsed { song, version } = parse(src)?;
    if version == FORMAT_VERSION {
        return Ok(Migration::Current);
    }
    let mismatch = || {
        vec![SongError {
            line: None,
            column: None,
            message: MISMATCH.to_owned(),
        }]
    };
    let mut doc: DocumentMut = src.parse().map_err(|_| mismatch())?;
    rewrite(&mut doc, &song).ok_or_else(mismatch)?;
    let text = doc.to_string();
    match parse(&text) {
        Ok(new) if new.version == FORMAT_VERSION && same_song(&new.song, &song) => {
            Ok(Migration::Migrated {
                from: version,
                text,
            })
        }
        _ => Err(mismatch()),
    }
}

/// Whether two songs are the same, in the same order. `Song`'s `==`
/// compares its `IndexMap`s ignoring order, but track order sets the
/// engine's mix and stem order; the canonical text writes every map (tracks,
/// clips, mix, device and strip params) in model order.
fn same_song(a: &Song, b: &Song) -> bool {
    a == b && a.to_toml() == b.to_toml()
}

/// Rewrites the v1 document of `old` as v2. `None` when the document does
/// not have a shape the v1 reader accepted (which would be a bug).
fn rewrite(doc: &mut DocumentMut, old: &Song) -> Option<()> {
    header(doc)?;
    clips(doc, old)?;
    tracks(doc, old)?;
    sections(doc, old)
}

/// `[song]`: `version = 2` as its first key.
fn header(doc: &mut DocumentMut) -> Option<()> {
    let mut song = root_child(doc, "song")?;
    let version = Value::from(i64::from(FORMAT_VERSION));
    let first = |a: &Key, b: &Key| (a.get() != "version").cmp(&(b.get() != "version"));
    if keys(&song).iter().any(|k| k == "version") {
        // `version = 1`: the new value keeps the old one's decor.
        song.put("version", version);
    } else {
        match &mut song {
            Slot::Table(t) => {
                t.insert("version", Item::Value(version));
            }
            Slot::Inline(t) => {
                t.insert("version", version);
            }
        }
    }
    match song {
        Slot::Table(t) => t.sort_values_by(|a, _, b, _| first(a, b)),
        Slot::Inline(t) => {
            // The space before `}` is the last value's suffix: it stays at
            // the end, and the moved `version` reads `{ version = 2, …`.
            let closing = t
                .iter()
                .last()
                .and_then(|(_, v)| v.decor().suffix().and_then(|s| s.as_str()))
                .filter(|s| s.trim().is_empty() && !s.contains('\n'))
                .map(str::to_owned);
            t.sort_values_by(|a, _, b, _| first(a, b));
            if let Some(mut key) = t.key_mut("version") {
                *key.leaf_decor_mut() = Decor::new(" ", " ");
            }
            if let Some(version) = t.get_mut("version") {
                *version.decor_mut() = Decor::new(" ", "");
            }
            if let (Some(closing), Some((_, last))) = (closing, t.iter_mut().last()) {
                last.decor_mut().set_suffix(closing);
            }
        }
    }
    Some(())
}

/// `patterns` becomes `clips` (the same item, so its tables keep their
/// place and decor), and each clip's `track` becomes `kind`.
fn clips(doc: &mut DocumentMut, old: &Song) -> Option<()> {
    let root = doc.as_table_mut();
    if !root.contains_key("patterns") {
        return Some(());
    }
    let order = renamed_keys(root.iter().map(|(k, _)| k), "patterns", "clips");
    let (key, item) = root.remove_entry("patterns")?;
    root.insert_formatted(&renamed(&key, "clips"), item);
    root.sort_values_by(|a, _, b, _| rank(&order, a).cmp(&rank(&order, b)));
    for (id, clip) in &old.clips {
        let mut slot = root_child(doc, "clips")?.child(id)?;
        rename(&mut slot, "track", "kind", Value::from(clip.kind().name()))?;
    }
    Some(())
}

/// Every track that plays gets `play = [...]` as its last key.
fn tracks(doc: &mut DocumentMut, old: &Song) -> Option<()> {
    for (id, track) in &old.tracks {
        if track.play.is_empty() {
            continue;
        }
        let play: Value = placements_toml(&track.play).parse().ok()?;
        root_child(doc, "tracks")?.child(id)?.put("play", play);
    }
    Some(())
}

/// Each section loses `play` and `automation` (their comments stay), and
/// each lane becomes a top-level `[[automation]]` stretch over its section,
/// right after it.
fn sections(doc: &mut DocumentMut, old: &Song) -> Option<()> {
    normalize_positions(doc);
    walk_mut(doc.as_table_mut(), &mut |t| {
        t.set_position(t.position().map(|p| p * SPREAD));
    });
    // A table made from an inline lane of an inline section goes after the
    // last table of the file.
    let mut last = 0;
    walk(doc.as_table(), &mut |t| {
        last = last.max(t.position().unwrap_or(last));
    });

    let starts = old.section_start_bars();
    let mut lanes = Vec::new();
    let item = doc.as_table_mut().get_mut("sections")?;
    for (index, section) in old.sections.iter().enumerate() {
        let mut slot = element_mut(item, index)?;
        let at = starts[index] + 1;
        let mut next = match &slot {
            Slot::Table(t) => t.position()?,
            Slot::Inline(_) => last,
        };
        for key in keys(&slot) {
            if key != "play" && key != "automation" {
                continue;
            }
            let removed = remove_keeping_comments(&mut slot, &key)?;
            if key != "automation" {
                continue;
            }
            let tables: Vec<Table> = match removed {
                Item::ArrayOfTables(aot) => aot.into_iter().collect(),
                Item::Value(Value::Array(array)) => {
                    let mut tables = Vec::new();
                    for value in array {
                        let Value::InlineTable(lane) = value else {
                            return None;
                        };
                        let mut table = lane.into_table();
                        next += 1;
                        table.set_position(Some(next));
                        tables.push(table);
                    }
                    tables
                }
                _ => return None,
            };
            for mut table in tables {
                stretch(&mut table, at, section.bars);
                lanes.push(table);
            }
        }
        if matches!(slot, Slot::Inline(_)) {
            last = next;
        }
    }
    if !lanes.is_empty() {
        let mut automation = ArrayOfTables::new();
        for lane in lanes {
            automation.push(lane);
        }
        doc.as_table_mut()
            .insert("automation", Item::ArrayOfTables(automation));
    }
    Some(())
}

/// A v1 lane table as a v2 stretch: `at` and `bars` right after `param`.
fn stretch(table: &mut Table, at: u32, bars: u32) {
    let mut order: Vec<String> = table.iter().map(|(k, _)| k.to_owned()).collect();
    let after = order
        .iter()
        .position(|k| k == "param")
        .map_or(order.len(), |i| i + 1);
    order.splice(after..after, ["at".to_owned(), "bars".to_owned()]);
    table.insert("at", Item::Value(Value::from(i64::from(at))));
    table.insert("bars", Item::Value(Value::from(i64::from(bars))));
    table.sort_values_by(|a, _, b, _| rank(&order, a).cmp(&rank(&order, b)));
}

/// The keys of a table, in order.
fn keys(slot: &Slot<'_>) -> Vec<String> {
    match slot {
        Slot::Table(t) => t.iter().map(|(k, _)| k.to_owned()).collect(),
        Slot::Inline(t) => t.iter().map(|(k, _)| k.to_owned()).collect(),
    }
}

/// `keys` in order, with `from` renamed to `to`.
fn renamed_keys<'k>(keys: impl Iterator<Item = &'k str>, from: &str, to: &str) -> Vec<String> {
    keys.map(|k| if k == from { to } else { k }.to_owned())
        .collect()
}

/// Where `key` sits in `order` (unknown keys last, in their own order).
fn rank(order: &[String], key: &Key) -> usize {
    order
        .iter()
        .position(|k| k == key.get())
        .unwrap_or(usize::MAX)
}

/// `key` under the name `to`, with its decor.
fn renamed(key: &Key, to: &str) -> Key {
    Key::new(to)
        .with_leaf_decor(key.leaf_decor().clone())
        .with_dotted_decor(key.dotted_decor().clone())
}

/// Replaces `from = …` by `to = value` at the same place, with the old
/// key's and value's decor.
fn rename(slot: &mut Slot<'_>, from: &str, to: &str, mut value: Value) -> Option<()> {
    match slot {
        Slot::Table(t) => {
            let order = renamed_keys(t.iter().map(|(k, _)| k), from, to);
            let (key, item) = t.remove_entry(from)?;
            *value.decor_mut() = item.as_value()?.decor().clone();
            t.insert_formatted(&renamed(&key, to), Item::Value(value));
            t.sort_values_by(|a, _, b, _| rank(&order, a).cmp(&rank(&order, b)));
        }
        Slot::Inline(t) => {
            let order = renamed_keys(t.iter().map(|(k, _)| k), from, to);
            let (key, old) = t.remove_entry(from)?;
            *value.decor_mut() = old.decor().clone();
            t.insert_formatted(&renamed(&key, to), value);
            t.sort_values_by(|a, _, b, _| rank(&order, a).cmp(&rank(&order, b)));
        }
    }
    Some(())
}

/// Removes `key` from the table and returns its item. The comment lines of a
/// removed value (above its key, inside it and after it) are kept as lines
/// after the value before it in the table, or above the key after it when it
/// was the first. An array of tables keeps its comments in its tables.
fn remove_keeping_comments(slot: &mut Slot<'_>, key: &str) -> Option<Item> {
    let order = keys(slot);
    let index = order.iter().position(|k| k == key)?;
    let (before, after) = (&order[..index], &order[index + 1..]);
    match slot {
        Slot::Table(t) => {
            let (k, item) = t.remove_entry(key)?;
            if item.is_array_of_tables() {
                return Some(item);
            }
            let mut lines = decor_comments(k.leaf_decor());
            lines.extend(item_comments(&item));
            if lines.is_empty() {
                return Some(item);
            }
            if let Some(prev) = before
                .iter()
                .rev()
                .find(|k| t.get(k.as_str()).is_some_and(Item::is_value))
            {
                let value = t.get_mut(prev)?.as_value_mut()?;
                append_lines(value.decor_mut(), &lines, "");
            } else {
                let next = after
                    .iter()
                    .find(|k| t.get(k.as_str()).is_some_and(Item::is_value))?;
                prepend_lines(t.key_mut(next)?.leaf_decor_mut(), &lines);
            }
            Some(item)
        }
        Slot::Inline(t) => {
            let last = after.is_empty();
            let (k, value) = t.remove_entry(key)?;
            let mut lines = decor_comments(k.leaf_decor());
            lines.extend(comments_in(&value.to_string()));
            if let Some(prev) = before.last() {
                let prev = t.get_mut(prev)?;
                if !lines.is_empty() {
                    append_lines(prev.decor_mut(), &lines, "\n");
                } else if last {
                    // The new last value takes the space before `}`.
                    let space = value.decor().suffix().and_then(|s| s.as_str());
                    if let Some(space) = space.filter(|s| s.trim().is_empty()) {
                        prev.decor_mut().set_suffix(space.to_owned());
                    }
                }
            } else if !lines.is_empty() {
                let next = after.first()?;
                prepend_lines(t.key_mut(next)?.leaf_decor_mut(), &lines);
            }
            Some(Item::Value(value))
        }
    }
}

/// Adds `lines` after the value `decor` belongs to, one per line, then
/// `end`.
fn append_lines(decor: &mut Decor, lines: &[String], end: &str) {
    let mut suffix = raw(decor.suffix()).to_owned();
    for line in lines {
        suffix.push('\n');
        suffix.push_str(line);
    }
    suffix.push_str(end);
    decor.set_suffix(suffix);
}

/// Adds `lines` above the key `decor` belongs to, one per line.
fn prepend_lines(decor: &mut Decor, lines: &[String]) {
    let mut prefix: String = lines.iter().map(|l| format!("{l}\n")).collect();
    prefix.push_str(raw(decor.prefix()));
    decor.set_prefix(prefix);
}

fn raw(s: Option<&toml_edit::RawString>) -> &str {
    s.and_then(|s| s.as_str()).unwrap_or("")
}

/// The comment lines of a decor (it holds only whitespace and comments).
fn decor_comments(decor: &Decor) -> Vec<String> {
    let mut lines = comments_in(raw(decor.prefix()));
    lines.extend(comments_in(raw(decor.suffix())));
    lines
}

/// The comment lines of an item, in text order.
fn item_comments(item: &Item) -> Vec<String> {
    match item {
        Item::None => Vec::new(),
        Item::Value(v) => comments_in(&v.to_string()),
        Item::Table(t) => table_comments(t),
        Item::ArrayOfTables(aot) => aot.iter().flat_map(table_comments).collect(),
    }
}

fn table_comments(table: &Table) -> Vec<String> {
    let mut lines = decor_comments(table.decor());
    for (key, item) in table.iter() {
        if let Some(k) = table.key(key) {
            lines.extend(decor_comments(k.leaf_decor()));
        }
        lines.extend(item_comments(item));
    }
    lines
}

/// The comments in TOML text, each from `#` to the end of its line,
/// trimmed. A `#` inside a string is not a comment.
fn comments_in(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut close: Option<&str> = None;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if let Some(delim) = close {
            if delim.starts_with('"') && c == '\\' {
                // Skip the escaped char along with the backslash.
                let mut chars = rest.chars();
                chars.next();
                chars.next();
                rest = chars.as_str();
                continue;
            }
            if let Some(after) = rest.strip_prefix(delim) {
                close = None;
                rest = after;
                continue;
            }
        } else if c == '#' {
            let end = rest.find('\n').unwrap_or(rest.len());
            lines.push(rest[..end].trim().to_owned());
            rest = &rest[end..];
            continue;
        } else if let Some(open) = ["\"\"\"", "'''", "\"", "'"]
            .into_iter()
            .find(|o| rest.starts_with(o))
        {
            close = Some(open);
            rest = &rest[open.len()..];
            continue;
        }
        rest = &rest[c.len_utf8()..];
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{comments_in, same_song};
    use crate::parse_song;

    #[test]
    fn same_song_sees_track_order() {
        let song = parse_song(include_str!("../tests/fixtures/mini.toml")).unwrap();
        let mut reordered = song.clone();
        reordered.tracks.reverse();
        assert_eq!(reordered, song, "IndexMap equality ignores order");
        assert!(!same_song(&reordered, &song));
        assert!(same_song(&song.clone(), &song));
    }

    #[test]
    fn comments_skip_strings() {
        let text = "a = \"# no\" # yes\nb = '#' # two\n# three\nc = \"\"\"\n# no\n\"\"\" # four\nd = \"\\\"#\"";
        assert_eq!(comments_in(text), ["# yes", "# two", "# three", "# four"]);
    }
}
