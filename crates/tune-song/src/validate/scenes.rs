//! Scenes: each slot names a track of the song and a clip that track's
//! instrument can play.

use indexmap::IndexMap;
use tune_modules::ClipKind;

use super::arrange::cannot_play;
use super::tracks::TrackShape;
use super::{check_id, unknown_track};
use crate::error::Errors;
use crate::model::Scene;
use crate::raw::{At, RawScene};

/// Checks the `[scenes.<id>]` tables against the tracks (`shapes`) and the
/// declared clips (`kinds`), keeping the file order. A broken slot is left out
/// of the scene; its error is recorded.
pub(super) fn scenes(
    raw: &[(At<String>, RawScene)],
    shapes: &IndexMap<&str, TrackShape>,
    kinds: &IndexMap<&str, Option<ClipKind>>,
    errors: &mut Errors,
) -> IndexMap<String, Scene> {
    let mut scenes = IndexMap::new();
    for (id, scene) in raw {
        check_id(id, "scene", errors);
        let mut slots = IndexMap::new();
        for (track, clip) in &scene.slots {
            let Some(shape) = shapes.get(track.value.as_str()) else {
                errors.at(&track.span, unknown_track(&track.value));
                continue;
            };
            match kinds.get(clip.value.as_str()) {
                None => {
                    errors.at(&clip.span, format!("unknown clip `{}`", clip.value));
                    continue;
                }
                Some(Some(kind)) => {
                    // An unknown instrument already has its own error.
                    if let Some(module) = shape.module()
                        && !module.plays(*kind)
                    {
                        errors.at(
                            &clip.span,
                            cannot_play(&clip.value, *kind, &track.value, module),
                        );
                        continue;
                    }
                }
                // A clip with an unknown kind has its own error.
                Some(None) => {}
            }
            slots.insert(track.value.clone(), clip.value.clone());
        }
        scenes.insert(id.value.clone(), Scene { slots });
    }
    scenes
}
