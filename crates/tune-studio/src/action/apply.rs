//! Applying an edit action to a document.

use serde_json::{Value, json};
use tune_dsp::DrumPattern;
use tune_dsp::acid303::{AcidPattern, AcidStep};
use tune_song::{Clip, Removed};

use std::slice;

use jiff::Timestamp;
use tune_song::FORMAT_VERSION;

use super::{Action, RunTag};
use crate::diff::{Change, diff_songs, version_change};
use crate::doc::{EditError, SongDoc, num, placement_json};
use crate::error::StudioError;
use crate::log::EditLog;
use crate::log::{EditEntry, Source, entries_for};
use crate::path::{SongPath, TrackField};

/// Committed knob and mix values keep this many significant digits.
const SIGNIFICANT_DIGITS: usize = 4;

/// What an action may read besides the document.
pub(crate) struct ApplyCtx<'a> {
    /// The song's edit log. Task 11's `undo_run` reads it.
    #[expect(dead_code, reason = "no action reads the edit log yet")]
    pub log: Option<&'a EditLog>,
}

/// Actions applied in order to one document: each one's result, and every
/// change with the name of the action that made it.
pub(crate) struct AppliedAll {
    pub results: Vec<Value>,
    pub changes: Vec<Change>,
    /// One per change: its action's name; `None` for the `song.version`
    /// change of a migration.
    pub names: Vec<Option<&'static str>>,
}

impl AppliedAll {
    /// One log entry per change, all stamped `ts`, each under its action's
    /// name and in `run`.
    pub fn entries(
        &self,
        ts: Timestamp,
        source: Source,
        file_hash: &str,
        run: Option<&RunTag>,
    ) -> Vec<EditEntry> {
        self.changes
            .iter()
            .zip(&self.names)
            .flat_map(|(c, name)| {
                entries_for(ts, source, slice::from_ref(c), file_hash, *name, run)
            })
            .collect()
    }
}

/// Applies `actions` to `doc` in order, all or nothing (on an error `doc`
/// may hold some of them: apply to a copy). Each action's changes are its
/// own diff, so each log entry can name its action. A text in format
/// `from_version` older than the newest gets the `song.version` change
/// first. `map_err` turns an action's error into the caller's.
pub(crate) fn apply_all(
    doc: &mut SongDoc,
    actions: &[Action],
    ctx: &ApplyCtx<'_>,
    from_version: u32,
    map_err: impl Fn(StudioError, &Action) -> StudioError,
) -> Result<AppliedAll, StudioError> {
    let mut all = AppliedAll {
        results: Vec::with_capacity(actions.len()),
        changes: Vec::new(),
        names: Vec::new(),
    };
    if let Some(change) = version_change(from_version, FORMAT_VERSION) {
        all.changes.push(change);
        all.names.push(None);
    }
    for action in actions {
        let before = doc.song().clone();
        let result = apply_action(doc, action, ctx).map_err(|e| map_err(e, action))?;
        all.results.push(result);
        for change in diff_songs(&before, doc.song()) {
            all.changes.push(change);
            all.names.push(Some(action.name()));
        }
    }
    Ok(all)
}

/// Applies one edit action to `doc`, all or nothing, and returns its result
/// (`null` for most edits; `{"id": …}` for `add_effect`, `{"removed": …}` for the removals and `set_instrument`). Works on any document, so a pending
/// action can be applied again onto a newer file. Live and history actions
/// (and a knob drag, `set_param` with `commit: false`) give
/// [`StudioError::NotAnEdit`].
pub(crate) fn apply_action(
    doc: &mut SongDoc,
    action: &Action,
    _ctx: &ApplyCtx<'_>,
) -> Result<Value, StudioError> {
    match action {
        Action::SetValue { path, value } => doc.set(path, value)?,
        Action::SetParam {
            track,
            param,
            value,
            commit: true,
        } => {
            let path = SongPath::param(track, param);
            doc.set(&path, &Value::from(round_significant(*value)))?;
        }
        Action::SetDeviceSetting {
            track,
            device,
            setting,
            value,
        } => {
            let path = SongPath::TrackParam {
                track: track.clone(),
                param: format!("{device}.{setting}"),
            };
            doc.set(&path, &Value::from(value.as_str()))?;
        }
        Action::SetAcidStep {
            clip,
            index,
            note,
            accent,
            slide,
        } => {
            let path = SongPath::Clip { clip: clip.clone() };
            let mut steps = match clip_of(doc, &path, clip)? {
                Clip::Acid(p) => p.steps().to_vec(),
                Clip::Drum(_) | Clip::Notes(_) => {
                    return Err(bad(&path, "not an acid pattern").into());
                }
            };
            let step = step_mut(&mut steps, *index)?;
            *step = AcidStep {
                note: *note,
                accent: note.is_some() && *accent,
                slide: note.is_some() && *slide,
            };
            let p = AcidPattern::new(steps).map_err(|e| bad(&path, &e.to_string()))?;
            doc.set(&path, &Value::from(p.to_string()))?;
        }
        Action::SetDrumStep { clip, index, hit } => {
            let path = SongPath::Clip { clip: clip.clone() };
            let mut hits = match clip_of(doc, &path, clip)? {
                Clip::Drum(p) => p.hits().to_vec(),
                Clip::Acid(_) | Clip::Notes(_) => {
                    return Err(bad(&path, "not a drum pattern").into());
                }
            };
            *step_mut(&mut hits, *index)? = (*hit).into();
            let p = DrumPattern::new(hits).map_err(|e| bad(&path, &e.to_string()))?;
            doc.set(&path, &Value::from(p.to_string()))?;
        }
        Action::MoveSection { from, to } => doc.move_section(*from, *to)?,
        Action::ResizeSection { index, bars } => {
            doc.set(&SongPath::SectionBars(*index), &Value::from(*bars))?;
        }
        Action::DuplicateSection { index } => doc.duplicate_section(*index)?,
        Action::DeleteSection { index } => doc.delete_section(*index)?,
        Action::SetSectionPlay {
            section,
            track,
            clip,
        } => {
            let path = SongPath::SectionPlay {
                section: *section,
                track: track.clone(),
            };
            let value = clip.as_deref().map_or(Value::Null, Value::from);
            doc.set(&path, &value)?;
        }
        Action::SetTrackMix {
            track,
            gain_db,
            pan,
            mute,
            solo,
        } => {
            // One copy for all fields, so a refused field leaves none set.
            let mut next = doc.clone();
            let field = |field| SongPath::TrackField {
                track: track.clone(),
                field,
            };
            let numbers = [(TrackField::GainDb, gain_db), (TrackField::Pan, pan)];
            for (f, v) in numbers {
                if let Some(v) = v {
                    next.set(&field(f), &Value::from(round_significant(*v)))?;
                }
            }
            for (f, v) in [(TrackField::Mute, mute), (TrackField::Solo, solo)] {
                if let Some(v) = v {
                    next.set(&field(f), &Value::Bool(*v))?;
                }
            }
            *doc = next;
        }
        Action::AddTrack { track, instrument } => doc.add_track(track, instrument)?,
        Action::RemoveTrack { track } => {
            return Ok(removed_value(&doc.remove_track(track)?));
        }
        Action::RenameTrack { track, to } => doc.rename_track(track, to)?,
        Action::AddEffect {
            track,
            module,
            id,
            index,
        } => {
            let id = doc.add_effect(track, module, id.as_deref(), *index)?;
            return Ok(json!({ "id": id }));
        }
        Action::RemoveEffect { track, device } => {
            return Ok(removed_value(&doc.remove_effect(track, device)?));
        }
        Action::MoveEffect {
            track,
            device,
            index,
        } => doc.move_effect(track, device, *index)?,
        Action::RenameDevice { track, device, to } => doc.rename_device(track, device, to)?,
        Action::SetInstrument { track, module } => {
            return Ok(removed_value(&doc.set_instrument(track, module)?));
        }
        Action::SetParam { commit: false, .. }
        | Action::Play { .. }
        | Action::Stop
        | Action::SeekBar { .. }
        | Action::SetLoop { .. }
        | Action::Undo
        | Action::Redo => return Err(StudioError::NotAnEdit),
    }
    Ok(Value::Null)
}

/// What an edit removed, as the action's result: `{"removed": {"lanes":
/// […], "placements": […], "duck_source": …}}`. A lane is `{"param": "<track>.<param>",
/// "at", "bars", "points"}` (`at` 1-based), a placement
/// `{"track", "at", "clip", "bars", "offset"}`.
fn removed_value(removed: &Removed) -> Value {
    let lanes: Vec<Value> = removed
        .lanes
        .iter()
        .map(|l| {
            let points: Vec<Value> = l
                .points
                .iter()
                .map(|&(x, v)| json!([num(x), num(v)]))
                .collect();
            json!({
                "param": format!("{}.{}", l.track, l.param),
                "at": l.start_bar + 1,
                "bars": l.bars,
                "points": points,
            })
        })
        .collect();
    let placements: Vec<Value> = removed
        .placements
        .iter()
        .map(|(track, p)| {
            let mut value = json!({ "track": track });
            if let (Some(map), Value::Object(fields)) = (value.as_object_mut(), placement_json(p)) {
                map.extend(fields);
            }
            value
        })
        .collect();
    json!({ "removed": {
        "lanes": lanes,
        "placements": placements,
        "duck_source": removed.duck_source,
    } })
}

fn clip_of<'d>(doc: &'d SongDoc, path: &SongPath, clip: &str) -> Result<&'d Clip, EditError> {
    doc.song()
        .clips
        .get(clip)
        .ok_or_else(|| EditError::NoSuchTarget(path.to_string()))
}

fn step_mut<T>(steps: &mut [T], index: usize) -> Result<&mut T, EditError> {
    let len = steps.len();
    steps
        .get_mut(index)
        .ok_or(EditError::OutOfBounds { index, len })
}

fn bad(path: &SongPath, message: &str) -> EditError {
    EditError::BadValue {
        path: path.to_string(),
        message: message.to_owned(),
    }
}

/// `v` rounded to [`SIGNIFICANT_DIGITS`] significant digits, as the nearest
/// double to the short decimal (`823.456789` gives `823.5`).
fn round_significant(v: f64) -> f64 {
    if !v.is_finite() || v == 0.0 {
        return v;
    }
    format!("{v:.prec$e}", prec = SIGNIFICANT_DIGITS - 1)
        .parse()
        .unwrap_or(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_to_four_significant_digits() {
        for (v, want) in [
            (823.456789, 823.5),
            (2394.0, 2394.0),
            (12_346.0, 12_350.0),
            (-6.123456, -6.123),
            (0.333333, 0.3333),
            (0.000_123_456, 0.000_123_5),
            (500.0, 500.0),
            (0.0, 0.0),
        ] {
            assert_eq!(round_significant(v), want, "{v}");
        }
        assert!(round_significant(f64::NAN).is_nan());
    }
}
