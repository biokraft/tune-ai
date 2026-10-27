//! Applying an edit action to a document.

use serde_json::Value;
use tune_dsp::DrumPattern;
use tune_dsp::acid303::{AcidPattern, AcidStep};
use tune_song::Clip;

use super::Action;
use crate::doc::{EditError, SongDoc};
use crate::error::StudioError;
use crate::log::EditLog;
use crate::path::{SongPath, TrackField};

/// Committed knob and mix values keep this many significant digits.
const SIGNIFICANT_DIGITS: usize = 4;

/// What an action may read besides the document.
pub(crate) struct ApplyCtx<'a> {
    /// The song's edit log. Task 11's `undo_run` reads it.
    #[expect(dead_code, reason = "no action reads the edit log yet")]
    pub log: Option<&'a EditLog>,
}

/// Applies one edit action to `doc`, all or nothing, and returns its result
/// (`null` for every edit so far). Works on any document, so a pending
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
