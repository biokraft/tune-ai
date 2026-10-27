//! Writing a section operation's result into the song text: the
//! `[[sections]]` tables, the `play` lists of the tracks whose placements
//! changed, and the root `[[automation]]` tables, each kept stretch keeping
//! its table and comments. The section and automation tables are then laid
//! out as one block, each section followed by the stretches that start in
//! it. Comments inside a rewritten `play` list are not kept.

use toml_edit::{ArrayOfTables, DocumentMut, InlineTable, Item, Table};
use tune_song::toml_doc::{Slot, reshape, walk, walk_mut};
use tune_song::{Arranged, Lane, LaneOrigin, Song, placements_toml};

use super::{as_integer, section_table, track_table};

/// What a section operation does to the `[[sections]]` tables themselves.
#[derive(Clone, Copy)]
pub(super) enum Tables<'a> {
    /// Nothing: only what plays changes.
    Keep,
    /// Moves section `.0` to index `.1`.
    Move(usize, usize),
    /// Puts a copy of section `.0` right after it.
    Duplicate(usize),
    Delete(usize),
    /// Section `.0` gets `bars = .1`.
    Bars(usize, u32),
    /// A new section named `.1` of `.2` bars goes in at index `.0`.
    Insert(usize, &'a str, u32),
}

/// Applies `tables` to the sections, written as `[[sections]]` or as an
/// inline array. The indices were checked by the section operation.
pub(super) fn section_tables(doc: &mut DocumentMut, tables: Tables<'_>) -> Option<()> {
    match tables {
        Tables::Keep => return Some(()),
        Tables::Bars(index, bars) => {
            section_table(doc, index)?.put("bars", toml_edit::Value::from(i64::from(bars)));
            return Some(());
        }
        Tables::Move(..) | Tables::Duplicate(_) | Tables::Delete(_) | Tables::Insert(..) => {}
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
            Tables::Insert(index, name, bars) => {
                let mut table = Table::new();
                table.decor_mut().set_prefix("\n");
                table.insert("name", toml_edit::value(name));
                table.insert("bars", toml_edit::value(i64::from(bars)));
                aot.insert(index, table);
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
            Tables::Insert(index, name, bars) => {
                let mut section = InlineTable::new();
                section.insert("name", toml_edit::Value::from(name));
                section.insert("bars", toml_edit::Value::from(i64::from(bars)));
                section.fmt();
                values.insert(index, section.into());
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
pub(super) fn write_plays(doc: &mut DocumentMut, old: &Song, new: &Song) -> Option<()> {
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
/// gone are dropped. A song with no `automation` yet gets the list with a
/// new table per stretch.
pub(super) fn write_automation(
    doc: &mut DocumentMut,
    old: &Song,
    arranged: &Arranged,
) -> Option<()> {
    let root = doc.as_table_mut();
    let reused = reused_tables(&arranged.lanes);
    let lanes = arranged.song.automation.iter().zip(reused);
    let empty = arranged.lanes.is_empty();
    match root.get_mut("automation") {
        None if empty => return Some(()),
        // The first stretches of a song: every one is a new table.
        None => {
            let mut new = ArrayOfTables::new();
            for lane in &arranged.song.automation {
                let mut table = Table::new();
                table.decor_mut().set_prefix("\n");
                put_lane(Slot::Table(&mut table), None, lane)?;
                new.push(table);
            }
            root.insert("automation", Item::ArrayOfTables(new));
            return Some(());
        }
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
            LaneOrigin::Split(_) | LaneOrigin::New => None,
        })
        .collect()
}

/// Writes stretch `lane` into its table. `before` is the stretch the table
/// held: only the values that differ from it are put, so the others keep
/// their text. A new table (`before` is `None`) gets every key.
fn put_lane(mut slot: Slot<'_>, before: Option<&Lane>, lane: &Lane) -> Option<()> {
    if before.is_none_or(|b| b.track != lane.track || b.param != lane.param) {
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
///
/// [`normalize_positions`]: tune_song::toml_doc::normalize_positions
pub(super) fn block_start(doc: &DocumentMut) -> Option<isize> {
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
pub(super) fn rerank(doc: &mut DocumentMut, song: &Song, start: isize) {
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

#[cfg(test)]
mod tests {
    use tune_song::parse_song;

    use super::*;

    const SONG: &str = "[song]\nversion = 2\nname = \"t\"\nbpm = 120\n\n\
        [tracks.kick]\ninstrument = \"kick\"\n\n\
        [[sections]]\nname = \"a\"\nbars = 4\n\n\
        # the first\n[[automation]]\nparam = \"kick.kick.decay\"\nat = 1\nbars = 2\npoints = [[0, 0], [2, 1]]\n";

    /// A stretch with no old table gets a table of its own, written after
    /// the kept one, which keeps its comment.
    #[test]
    fn a_new_stretch_gets_a_new_table() {
        let old = parse_song(SONG).expect("parses");
        let mut new = old.clone();
        new.automation.push(Lane {
            track: "kick".into(),
            param: "kick.tune".into(),
            start_bar: 2,
            bars: 2,
            points: vec![(0.0, 0.5), (2.0, 0.25)],
        });
        let arranged = Arranged {
            song: new.clone(),
            lanes: vec![LaneOrigin::Kept(0), LaneOrigin::New],
        };
        let mut doc: DocumentMut = SONG.parse().expect("parses");
        write_automation(&mut doc, &old, &arranged).expect("written");
        let text = doc.to_string();
        assert!(text.starts_with(SONG), "{text}");
        assert!(
            text.ends_with(
                "\n[[automation]]\nparam = \"kick.kick.tune\"\nat = 3\nbars = 2\npoints = [[0, 0.5], [2, 0.25]]\n"
            ),
            "{text}"
        );
        assert_eq!(parse_song(&text).expect("parses"), new);
    }

    /// A kept stretch whose param changed gets the new `param`.
    #[test]
    fn a_kept_stretch_follows_its_new_param() {
        let old = parse_song(SONG).expect("parses");
        let mut new = old.clone();
        new.automation[0].param = "kick.pitch".into();
        let arranged = Arranged {
            song: new.clone(),
            lanes: vec![LaneOrigin::Kept(0)],
        };
        let mut doc: DocumentMut = SONG.parse().expect("parses");
        write_automation(&mut doc, &old, &arranged).expect("written");
        let text = doc.to_string();
        assert_eq!(text, SONG.replace("kick.kick.decay", "kick.kick.pitch"));
    }

    /// A song with no `automation` yet gets the list, one new table per
    /// stretch, after the text it had.
    #[test]
    fn the_first_stretches_create_the_list() {
        let bare = SONG
            .split("# the first")
            .next()
            .expect("has a lane")
            .to_owned();
        let bare = format!("{}\n", bare.trim_end());
        let old = parse_song(&bare).expect("parses");
        let mut new = old.clone();
        new.automation.push(Lane {
            track: "kick".into(),
            param: "kick.decay".into(),
            start_bar: 0,
            bars: 2,
            points: vec![(0.0, 0.0), (2.0, 1.0)],
        });
        let arranged = Arranged {
            song: new.clone(),
            lanes: vec![LaneOrigin::New],
        };
        let mut doc: DocumentMut = bare.parse().expect("parses");
        write_automation(&mut doc, &old, &arranged).expect("written");
        let text = doc.to_string();
        assert_eq!(
            text,
            format!(
                "{bare}\n[[automation]]\nparam = \"kick.kick.decay\"\nat = 1\nbars = 2\n\
                 points = [[0, 0], [2, 1]]\n"
            )
        );
        assert_eq!(parse_song(&text).expect("parses"), new);

        // Nothing to write, nothing created.
        let mut doc: DocumentMut = bare.parse().expect("parses");
        let none = Arranged {
            song: old.clone(),
            lanes: Vec::new(),
        };
        write_automation(&mut doc, &old, &none).expect("written");
        assert_eq!(doc.to_string(), bare);
    }
}
