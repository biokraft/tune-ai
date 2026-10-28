//! Edits of the session scenes: add, remove, rename or move a scene and set
//! or clear one of its slots.
//!
//! Like the other edits these do not validate their result (the caller
//! re-parses it): `set_slot` checks that the scene, the track and the clip
//! exist, and whether the track's instrument can play the clip is the
//! validator's. A scene `index` is 0-based.

use crate::edit::{
    EditOpError, Edited, Removed, RemovedSlot, clip_of, track_of, untouched_lanes, well_formed,
};
use crate::model::{Scene, Song};

/// Adds an empty scene `id` before scene `index`, or last when `index` is
/// `None` or the scene count.
pub fn add_scene(song: &Song, id: &str, index: Option<usize>) -> Result<Edited, EditOpError> {
    check_new_scene(song, id)?;
    let len = song.scenes.len();
    let at = index.unwrap_or(len);
    if at > len {
        return Err(EditOpError::SceneIndexOutOfBounds { index: at, len });
    }
    let mut song = song.clone();
    song.scenes
        .shift_insert(at, id.to_owned(), Scene::default());
    Ok(untouched_lanes(song, Removed::default()))
}

/// Removes scene `id`; its slots are listed in `removed.slots`.
pub fn remove_scene(song: &Song, id: &str) -> Result<Edited, EditOpError> {
    scene_of(song, id)?;
    let mut song = song.clone();
    let scene = song.scenes.shift_remove(id).expect("the scene exists");
    let removed = Removed {
        slots: scene
            .slots
            .into_iter()
            .map(|(track, clip)| RemovedSlot {
                scene: id.to_owned(),
                track,
                clip,
            })
            .collect(),
        ..Removed::default()
    };
    Ok(untouched_lanes(song, removed))
}

/// Renames scene `id` to `to`, keeping its place.
pub fn rename_scene(song: &Song, id: &str, to: &str) -> Result<Edited, EditOpError> {
    scene_of(song, id)?;
    if id == to {
        return Ok(untouched_lanes(song.clone(), Removed::default()));
    }
    check_new_scene(song, to)?;
    let mut song = song.clone();
    song.scenes = std::mem::take(&mut song.scenes)
        .into_iter()
        .map(|(k, v)| if k == id { (to.to_owned(), v) } else { (k, v) })
        .collect();
    Ok(untouched_lanes(song, Removed::default()))
}

/// Moves scene `id` to index `index`, as a removal then an insertion.
pub fn move_scene(song: &Song, id: &str, index: usize) -> Result<Edited, EditOpError> {
    scene_of(song, id)?;
    let len = song.scenes.len();
    if index >= len {
        return Err(EditOpError::SceneIndexOutOfBounds { index, len });
    }
    let mut song = song.clone();
    let scene = song.scenes.shift_remove(id).expect("the scene exists");
    song.scenes.shift_insert(index, id.to_owned(), scene);
    Ok(untouched_lanes(song, Removed::default()))
}

/// Puts `clip` in the slot of `track` in scene `scene`, or empties the slot
/// for `None`. A new slot goes last; an existing one keeps its place.
pub fn set_slot(
    song: &Song,
    scene: &str,
    track: &str,
    clip: Option<&str>,
) -> Result<Edited, EditOpError> {
    scene_of(song, scene)?;
    track_of(song, track)?;
    if let Some(clip) = clip {
        clip_of(song, clip)?;
    }
    let mut song = song.clone();
    let slots = &mut song.scenes[scene].slots;
    match clip {
        Some(clip) => {
            slots.insert(track.to_owned(), clip.to_owned());
        }
        None => {
            slots.shift_remove(track);
        }
    }
    Ok(untouched_lanes(song, Removed::default()))
}

fn scene_of<'s>(song: &'s Song, id: &str) -> Result<&'s Scene, EditOpError> {
    song.scenes
        .get(id)
        .ok_or_else(|| EditOpError::NoSuchScene(id.to_owned()))
}

/// Checks that `id` is a well-formed scene id nobody has. The existing scene
/// is reported first, so a duplicate id reads as that.
fn check_new_scene(song: &Song, id: &str) -> Result<(), EditOpError> {
    if song.scenes.contains_key(id) {
        return Err(EditOpError::SceneExists(id.to_owned()));
    }
    if !well_formed(id) {
        return Err(EditOpError::BadId {
            what: "scene",
            id: id.to_owned(),
        });
    }
    Ok(())
}
