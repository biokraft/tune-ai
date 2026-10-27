//! Writing scene edits into the song text: a scene's `[scenes.<id>]` table
//! and the slots in it.
//!
//! Each edit runs the `tune_song` function for it and writes the result into
//! the text (see [`SongDoc::add_scene`] and its siblings). The text changes
//! only where the edit does: a removed scene takes the comments above its
//! table with it, a moved one carries them, and every other byte stays.

use toml_edit::{DocumentMut, Item};
use tune_song::toml_doc::{Slot, normalize_positions, root_child, walk, walk_mut};
use tune_song::{Removed, RemovedSlot, Scene, Song, scene_table_toml};

use super::items::{Place, insert_table, rename_keys};
use super::{EditError, SongDoc, put_slot};

/// Where the first scene table goes: after the last of these tables.
const SCENE_ANCHORS: &[&str] = &["song", "mix", "tracks", "clips"];

impl SongDoc {
    /// Adds an empty scene `scene` before scene `index` (0-based), or last
    /// when `index` is `None` or the scene count (see
    /// [`tune_song::add_scene`]). Its table is written right before that
    /// scene's table and the comments above it, else after the last scene
    /// table, else after the last clip table.
    pub fn add_scene(&mut self, scene: &str, index: Option<usize>) -> Result<(), EditError> {
        self.check_scenes_form("add_scene")?;
        let edited = tune_song::add_scene(&self.song, scene, index)?;
        let text = scene_table_toml(scene, &edited.arranged.song.scenes[scene]);
        // The scene now at `index` is the one the new table goes before.
        let next: Option<String> = edited
            .arranged
            .song
            .scenes
            .get_index_of(scene)
            .and_then(|at| self.song.scenes.get_index(at))
            .map(|(next, _)| next.clone());
        self.write_edited(edited, |doc| {
            let place = next.as_deref().map_or(Place::Last, Place::Before);
            insert_table(doc, "scenes", scene, &text, place, SCENE_ANCHORS)
        })?;
        Ok(())
    }

    /// Removes scene `scene` with the comments above its table (see
    /// [`tune_song::remove_scene`]). Returns the slots that went with it.
    pub fn remove_scene(&mut self, scene: &str) -> Result<Removed, EditError> {
        let edited = tune_song::remove_scene(&self.song, scene)?;
        self.write_edited(edited, |doc| {
            root_child(doc, "scenes")?.remove(scene);
            Some(())
        })
    }

    /// Renames scene `scene` to `to` in place: its table keeps its comments
    /// and position (see [`tune_song::rename_scene`]).
    pub fn rename_scene(&mut self, scene: &str, to: &str) -> Result<(), EditError> {
        let edited = tune_song::rename_scene(&self.song, scene, to)?;
        self.write_edited(edited, |doc| {
            rename_keys(root_child(doc, "scenes")?, |k| {
                (k == scene).then(|| to.to_owned())
            })
        })?;
        Ok(())
    }

    /// Moves scene `scene` to index `index` (0-based), as a removal then an
    /// insertion (see [`tune_song::move_scene`]). Its table, the tables
    /// under it and the comments above it move as they are.
    pub fn move_scene(&mut self, scene: &str, index: usize) -> Result<(), EditError> {
        // The model first, so a missing scene is said before a form.
        let edited = tune_song::move_scene(&self.song, scene, index)?;
        self.check_scene_tables("move_scene")?;
        let expected = edited.arranged.song;
        // Scene equality ignores order, so a move is checked by the keys.
        let after: Vec<String> = expected.scenes.keys().cloned().collect();
        if self.song.scenes.keys().eq(after.iter()) {
            return Ok(());
        }
        let mut doc = self.doc.clone();
        move_table(&mut doc, &self.song, scene, &after).ok_or_else(|| broken("written into"))?;
        self.commit_scenes(doc, &expected)
    }

    /// Puts `clip` in the slot of `track` in scene `scene`, or empties the
    /// slot for `None` (see [`tune_song::set_slot`]). A scene without
    /// `slots` gets the key; emptying the last slot of an inline `slots`
    /// takes the key away.
    pub fn set_slot(
        &mut self,
        scene: &str,
        track: &str,
        clip: Option<&str>,
    ) -> Result<(), EditError> {
        let edited = tune_song::set_slot(&self.song, scene, track, clip)?;
        self.write_edited(edited, |doc| {
            let table = root_child(doc, "scenes")?.child(scene)?;
            put_slot(table, track, clip)
        })?;
        Ok(())
    }

    /// Refuses a file whose `scenes` is a root inline table (`scenes = { …
    /// }`): a valid form the writer cannot add or move a scene in.
    fn check_scenes_form(&self, action: &'static str) -> Result<(), EditError> {
        match self.doc.as_table().get("scenes") {
            Some(Item::Value(_)) => Err(EditError::ScenesByHand { action }),
            _ => Ok(()),
        }
    }

    /// Refuses a file whose scenes are not each their own `[scenes.<id>]`
    /// table, the only form [`move_table`] moves.
    fn check_scene_tables(&self, action: &'static str) -> Result<(), EditError> {
        self.check_scenes_form(action)?;
        if let Some(Item::Table(scenes)) = self.doc.as_table().get("scenes")
            && scenes
                .iter()
                .any(|(_, item)| !matches!(item, Item::Table(t) if !t.is_dotted()))
        {
            return Err(EditError::ScenesByHand { action });
        }
        Ok(())
    }

    /// Commits `doc` as the song `expected` on a copy, and keeps the copy
    /// only when the file has the scenes in `expected`'s order too: `Song`
    /// equality ignores the scene order, so a reorder that did not reach
    /// the text would pass for no change.
    fn commit_scenes(&mut self, doc: DocumentMut, expected: &Song) -> Result<(), EditError> {
        let mut next = self.clone();
        next.commit_as(doc, Some(expected))?;
        if next.song.scenes.keys().ne(expected.scenes.keys()) {
            return Err(broken("read back from"));
        }
        *self = next;
        Ok(())
    }
}

/// The writers of [`SongDoc::put`] for the scenes, with none of the
/// clean-up the edit actions do. Each commits through
/// [`commit_scenes`](SongDoc::commit_scenes), so the scene order is checked
/// in the file as well.
impl SongDoc {
    /// Removes scene `id`'s table and the comments above it (`None`), or
    /// writes `scene` as scene `id` at `index` of the scenes (replacing the
    /// one there) in the [`scene_table_toml`] layout: right before the
    /// table of the scene now at `index` and the comments above it, else
    /// after the last scene table, else after the last clip table.
    pub(crate) fn put_scene(
        &mut self,
        id: &str,
        scene: Option<(usize, Scene)>,
    ) -> Result<(), EditError> {
        let mut song = self.song.clone();
        let existed = song.scenes.shift_remove(id).is_some();
        if scene.is_none() && !existed {
            return Ok(());
        }
        if scene.is_some() {
            self.check_scenes_form("undo_run")?;
        }
        let mut doc = self.doc.clone();
        if existed {
            root_child(&mut doc, "scenes")
                .ok_or_else(|| broken("written into"))?
                .remove(id);
        }
        if let Some((index, scene)) = scene {
            let at = index.min(song.scenes.len());
            let next = song.scenes.get_index(at).map(|(next, _)| next.clone());
            let text = scene_table_toml(id, &scene);
            song.scenes.shift_insert(at, id.to_owned(), scene);
            let place = next.as_deref().map_or(Place::Last, Place::Before);
            insert_table(&mut doc, "scenes", id, &text, place, SCENE_ANCHORS)
                .ok_or_else(|| broken("written into"))?;
        }
        self.commit_scenes(doc, &song)
    }

    /// Puts the scene tables in the order of `order`: first its ids that
    /// are scenes of the song, then the scenes it does not name, in their
    /// order. Each scene's tables and the comments above them move as they
    /// are, as [`move_scene`](Self::move_scene) moves them.
    pub(crate) fn put_scene_order(&mut self, order: &[&str]) -> Result<(), EditError> {
        let mut after: Vec<String> = Vec::new();
        let named = order
            .iter()
            .copied()
            .filter(|id| self.song.scenes.contains_key(*id));
        for id in named
            .map(str::to_owned)
            .chain(self.song.scenes.keys().cloned())
        {
            if !after.contains(&id) {
                after.push(id);
            }
        }
        if self.song.scenes.keys().eq(after.iter()) {
            return Ok(());
        }
        self.check_scene_tables("undo_run")?;
        // Each scene in turn goes after all the others, so the last one
        // moved ends last.
        let mut doc = self.doc.clone();
        let mut now: Vec<String> = self.song.scenes.keys().cloned().collect();
        for id in &after {
            now.retain(|s| s != id);
            now.push(id.clone());
            move_table(&mut doc, &self.song, id, &now).ok_or_else(|| broken("written into"))?;
        }
        let mut expected = self.song.clone();
        expected.scenes = after
            .iter()
            .map(|id| (id.clone(), self.song.scenes[id].clone()))
            .collect();
        self.commit_scenes(doc, &expected)
    }
}

/// The error of a scene write the writer could not do: a bug, not the
/// file's fault.
fn broken(what: &str) -> EditError {
    EditError::Internal(format!("the scene write could not be {what} the file"))
}

/// The `slots` table of scene `scene`, when the file has one.
fn slots_of<'a>(doc: &'a mut DocumentMut, scene: &str) -> Option<Slot<'a>> {
    root_child(doc, "scenes")?.child(scene)?.child("slots")
}

/// Takes the listed slots out of their scenes' tables, as emptying them with
/// [`SongDoc::set_slot`] does.
pub(super) fn clear_slots(doc: &mut DocumentMut, slots: &[RemovedSlot]) -> Option<()> {
    for slot in slots {
        let scene = root_child(doc, "scenes")?.child(&slot.scene)?;
        put_slot(scene, &slot.track, None)?;
    }
    Some(())
}

/// Renames the slot key `track` to `to` in every scene of `song` that has
/// it, in place: the key keeps its position, spacing and value.
pub(super) fn rename_slot_track(
    doc: &mut DocumentMut,
    song: &Song,
    track: &str,
    to: &str,
) -> Option<()> {
    for (id, scene) in &song.scenes {
        if scene.slots.contains_key(track) {
            let slots = slots_of(doc, id)?;
            rename_keys(slots, |k| (k == track).then(|| to.to_owned()))?;
        }
    }
    Some(())
}

/// Rewrites every slot value `clip` in the scenes of `song` to `to`, keeping
/// the spacing and comments around it.
pub(super) fn rename_slot_clip(
    doc: &mut DocumentMut,
    song: &Song,
    clip: &str,
    to: &str,
) -> Option<()> {
    for (id, scene) in &song.scenes {
        for (track, _) in scene.slots.iter().filter(|(_, c)| *c == clip) {
            slots_of(doc, id)?.put(track, toml_edit::Value::from(to));
        }
    }
    Some(())
}

/// Gives the tables of scene `scene` (its own and those under it) the place
/// right before the table of the scene that follows it in `after`, else
/// after the last table of any scene, renumbering every table's position.
fn move_table(
    doc: &mut DocumentMut,
    before: &tune_song::Song,
    scene: &str,
    after: &[String],
) -> Option<()> {
    normalize_positions(doc);
    let ranks_of = |doc: &DocumentMut, id: &str| -> Option<Vec<isize>> {
        let Item::Table(scenes) = doc.as_table().get("scenes")? else {
            return None;
        };
        let Item::Table(table) = scenes.get(id)? else {
            return None;
        };
        let mut ranks = Vec::new();
        walk(table, &mut |t| ranks.extend(t.position()));
        Some(ranks)
    };
    let moved = ranks_of(doc, scene)?;
    // Every rank, in render order, then the moved block taken out.
    let mut all = Vec::new();
    walk(doc.as_table(), &mut |t| all.extend(t.position()));
    all.sort_unstable();
    all.retain(|r| !moved.contains(r));
    let at = after.iter().position(|id| id == scene)?;
    let insert_at = match after.get(at + 1) {
        Some(next) => {
            // The table of the scene that now follows, as the file has it.
            let own = match doc.as_table().get("scenes")? {
                Item::Table(scenes) => match scenes.get(next)? {
                    Item::Table(t) => t.position()?,
                    _ => return None,
                },
                _ => return None,
            };
            all.iter().position(|r| *r == own)?
        }
        None => {
            // After the last table of the scenes that stay.
            let last = before
                .scenes
                .keys()
                .filter(|id| id.as_str() != scene)
                .filter_map(|id| ranks_of(doc, id))
                .flatten()
                .max()?;
            all.iter().position(|r| *r == last)? + 1
        }
    };
    let mut order = all;
    for (i, rank) in moved.iter().enumerate() {
        order.insert(insert_at + i, *rank);
    }
    let mut placed = true;
    walk_mut(doc.as_table_mut(), &mut |t| {
        if let Some(p) = t.position() {
            let new = order.iter().position(|r| *r == p);
            placed &= new.is_some();
            if let Some(new) = new.and_then(|n| isize::try_from(n).ok()) {
                t.set_position(Some(new));
            }
        }
    });
    // A table the walk met but the first pass did not is a bug, not a panic.
    placed.then_some(())
}
