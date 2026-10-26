//! The arrangement: patterns, sections and their automation lanes.

use indexmap::{IndexMap, IndexSet};
use tune_dsp::acid303::{AcidPattern, PatternError};
use tune_dsp::{DrumPattern, DrumPatternError, ParamSpec, STRIP_DUCK_DEPTH, STRIP_PARAMS};

use super::tracks::{TrackShape, find_in, unknown_param};
use super::{BARS, Duck, unknown_track};
use crate::error::Errors;
use crate::model::{Lane, MIX_TRACK_ID, MixUnit, Pattern, PatternSteps, Section};
use crate::raw::{At, RawLane, RawPattern, RawSection};

pub(super) fn pattern(
    id: &At<String>,
    p: &RawPattern,
    shapes: &IndexMap<&str, TrackShape>,
    errors: &mut Errors,
) -> Option<Pattern> {
    let track = p.track.as_ref()?;
    let Some(shape) = shapes.get(track.value.as_str()) else {
        errors.at(&track.span, unknown_track(&track.value));
        return None;
    };
    let steps = p.steps.as_ref()?;
    // An unknown instrument already has its own error.
    let kind = shape.kind?;
    let parsed = if kind.is_drum() {
        DrumPattern::parse(&steps.value)
            .map(PatternSteps::Drum)
            .map_err(|e| match e {
                DrumPatternError::Length(n) => Err(n),
                other => Ok(other.to_string()),
            })
    } else {
        AcidPattern::parse(&steps.value)
            .map(PatternSteps::Acid)
            .map_err(|e| match e {
                PatternError::Length(n) => Err(n),
                other => Ok(other.to_string()),
            })
    };
    match parsed {
        Ok(steps) => Some(Pattern {
            track: track.value.clone(),
            steps,
        }),
        Err(Err(n)) => {
            errors.at(
                &steps.span,
                format!("pattern `{}` needs 1 to 64 steps, got {n}", id.value),
            );
            None
        }
        Err(Ok(why)) => {
            errors.at(
                &steps.span,
                format!("pattern `{}` has a bad step: {why}", id.value),
            );
            None
        }
    }
}

pub(super) fn section(
    index: usize,
    s: &RawSection,
    shapes: &IndexMap<&str, TrackShape>,
    pattern_owner: &IndexMap<&str, Option<&str>>,
    duck: &Duck<'_>,
    errors: &mut Errors,
) -> Option<Section> {
    let label = match &s.name {
        Some(n) => format!("section `{}`", n.value),
        None => format!("section {}", index + 1),
    };
    let bars = s.bars.as_ref().and_then(|b| {
        let ok = BARS.contains(&b.value);
        if !ok {
            errors.at(&b.span, format!("bars must be 1 to 1024, got {}", b.value));
        }
        ok.then(|| u32::try_from(b.value).expect("checked range"))
    });

    let before = errors.len();
    let mut play = IndexMap::new();
    for (track, pat) in &s.play {
        if !shapes.contains_key(track.value.as_str()) {
            errors.at(&track.span, unknown_track(&track.value));
            continue;
        }
        match pattern_owner.get(pat.value.as_str()) {
            None => errors.at(&pat.span, format!("unknown pattern `{}`", pat.value)),
            Some(Some(owner)) if *owner != track.value => errors.at(
                &pat.span,
                format!(
                    "pattern `{}` belongs to track `{owner}`, not `{}`",
                    pat.value, track.value
                ),
            ),
            _ => {
                play.insert(track.value.clone(), pat.value.clone());
            }
        }
    }

    let mut automation = Vec::new();
    let mut seen: IndexSet<(String, String)> = IndexSet::new();
    for raw in &s.automation {
        if let Some(lane) = lane(raw, bars, shapes, duck, errors) {
            let key = (lane.track.clone(), lane.param.clone());
            if !seen.insert(key) {
                let param = raw.param.as_ref().expect("a built lane has a param");
                errors.at(
                    &param.span,
                    format!(
                        "{label} already has a lane for `{}` (one lane per param per section)",
                        param.value
                    ),
                );
                continue;
            }
            automation.push(lane);
        }
    }

    (errors.len() == before).then_some(())?;
    Some(Section {
        name: s.name.as_ref()?.value.clone(),
        bars: bars?,
        play,
        automation,
    })
}

fn lane(
    raw: &RawLane,
    bars: Option<u32>,
    shapes: &IndexMap<&str, TrackShape>,
    duck: &Duck<'_>,
    errors: &mut Errors,
) -> Option<Lane> {
    let before = errors.len();
    let target = raw.param.as_ref().and_then(|p| resolve(p, shapes, errors));
    if let (Some((track, param)), Some(p)) = (&target, &raw.param)
        && track != MIX_TRACK_ID
        && param == STRIP_PARAMS[STRIP_DUCK_DEPTH].id
        && let Some(message) = duck.depth_error(track)
    {
        errors.at(&p.span, message);
    }

    let points = raw.points.as_ref()?;
    if points.value.is_empty() {
        errors.at(&points.span, "a lane needs at least one point");
    }
    let mut prev: Option<f64> = None;
    for point in &points.value {
        let bar = point.bar.value;
        if let Some(bars) = bars
            && !(0.0..=f64::from(bars)).contains(&bar)
        {
            errors.at(
                &point.bar.span,
                format!("bar {bar} is outside the section (0..={bars})"),
            );
        }
        if let Some(p) = prev
            && bar < p
        {
            errors.at(
                &point.bar.span,
                format!("points go backwards: bar {bar} comes after bar {p}"),
            );
        }
        prev = Some(bar);
        let value = point.value.value;
        if !(0.0..=1.0).contains(&value) {
            errors.at(
                &point.value.span,
                format!("value {value} is outside 0..=1 (lane values are normalized)"),
            );
        }
    }

    let (track, param) = target?;
    (errors.len() == before).then_some(())?;
    Some(Lane {
        track,
        param,
        points: points
            .value
            .iter()
            .map(|p| (p.bar.value, p.value.value))
            .collect(),
    })
}

/// Splits `<track id>.<param id>` at the first `.` and resolves the param
/// against the mix tables for the `mix` pseudo-track, else the instrument
/// table, the strip table, then each fx table of the track.
fn resolve(
    p: &At<String>,
    shapes: &IndexMap<&str, TrackShape>,
    errors: &mut Errors,
) -> Option<(String, String)> {
    let Some((track_id, param_id)) = p.value.split_once('.') else {
        errors.at(
            &p.span,
            format!(
                "automation param `{}` must be `<track id>.<param id>`, e.g. `acid.acid303.cutoff`",
                p.value
            ),
        );
        return None;
    };
    if track_id == MIX_TRACK_ID {
        let tables: Vec<&'static [ParamSpec]> = MixUnit::ALL.iter().map(|u| u.params()).collect();
        if find_in(&tables, param_id).is_none() {
            errors.at(&p.span, unknown_param(param_id, MIX_TRACK_ID, &tables));
            return None;
        }
        return Some((track_id.to_owned(), param_id.to_owned()));
    }
    let Some(shape) = shapes.get(track_id) else {
        errors.at(&p.span, unknown_track(track_id));
        return None;
    };
    // An unknown instrument already has its own error.
    let kind = shape.kind?;
    let mut tables = vec![kind.params(), STRIP_PARAMS];
    tables.extend(shape.fx_tables.iter().copied());
    if find_in(&tables, param_id).is_none() {
        errors.at(
            &p.span,
            unknown_param(param_id, kind.name(), &[kind.params(), STRIP_PARAMS]),
        );
        return None;
    }
    Some((track_id.to_owned(), param_id.to_owned()))
}
