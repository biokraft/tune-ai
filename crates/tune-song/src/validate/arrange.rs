//! The arrangement: clips, sections, placements and lane stretches, read
//! from either format. A v1 file binds patterns to tracks and keeps what
//! plays and the lanes inside each section; v2 has a clip pool, placements
//! per track and top-level stretches.

use indexmap::{IndexMap, IndexSet};
use tune_dsp::acid303::{AcidPattern, PatternError};
use tune_dsp::{DrumPattern, DrumPatternError, ParamSpec, STRIP_DUCK_DEPTH, STRIP_PARAMS};
use tune_modules::ClipKind;

use super::tracks::{TrackShape, find_in, or_list, param_ids, spec_ids, unknown_param};
use super::{BARS, Duck, check_id, unknown_track};
use crate::error::Errors;
use crate::model::{Clip, Lane, MIX_TRACK_ID, MixUnit, Placement, Section, Track};
use crate::raw::{At, RawLane, RawPlacement, RawSection, RawSong};

/// The arrangement of a song: its clip pool, sections and lane stretches.
/// Placements go straight into the tracks.
pub(super) struct Arrangement {
    pub clips: IndexMap<String, Clip>,
    pub sections: Vec<Section>,
    pub automation: Vec<Lane>,
}

/// The highest `offset` of a placement, in bars.
const MAX_OFFSET: i64 = 1024;

/// Reads a v1 arrangement: each pattern becomes a clip of the kind its
/// track plays, and each section's `play` entries become placements over
/// the whole section, appended to their tracks in section order; its lanes
/// become stretches over it, in file order. Once a section is broken the
/// later start bars are off, but then no song is built.
pub(super) fn v1(
    raw: &RawSong,
    shapes: &IndexMap<&str, TrackShape>,
    duck: &Duck<'_>,
    tracks: &mut IndexMap<String, Track>,
    errors: &mut Errors,
) -> Arrangement {
    let pattern_owner: IndexMap<&str, Option<&str>> = raw
        .patterns
        .iter()
        .map(|(id, p)| {
            (
                id.value.as_str(),
                p.track.as_ref().map(|t| t.value.as_str()),
            )
        })
        .collect();

    let mut clips = IndexMap::new();
    for (id, p) in &raw.patterns {
        check_id(id, "pattern", errors);
        let Some(track) = p.track.as_ref() else {
            continue;
        };
        let Some(shape) = shapes.get(track.value.as_str()) else {
            errors.at(&track.span, unknown_track(&track.value));
            continue;
        };
        // An unknown instrument already has its own error.
        let (Some(steps), Some(module)) = (p.steps.as_ref(), shape.module()) else {
            continue;
        };
        let kind = if module.plays(ClipKind::Drum) {
            ClipKind::Drum
        } else {
            ClipKind::Acid
        };
        if let Some(clip) = clip_steps("pattern", id, kind, steps, errors) {
            clips.insert(id.value.clone(), clip);
        }
    }

    let mut sections = Vec::new();
    let mut automation = Vec::new();
    let mut start_bar = 0;
    match &raw.sections {
        Some(list) if !list.is_empty() => {
            for (i, s) in list.iter().enumerate() {
                let Some(v1) = section_v1(i, start_bar, s, shapes, &pattern_owner, duck, errors)
                else {
                    continue;
                };
                start_bar += v1.section.bars;
                sections.push(v1.section);
                for (track, placement) in v1.play {
                    if let Some(t) = tracks.get_mut(&track) {
                        t.play.push(placement);
                    }
                }
                automation.extend(v1.lanes);
            }
        }
        _ => no_sections(errors),
    }
    Arrangement {
        clips,
        sections,
        automation,
    }
}

/// Reads a v2 arrangement: the clip pool, the sections, each track's
/// placements and the top-level stretches.
pub(super) fn v2(
    raw: &RawSong,
    shapes: &IndexMap<&str, TrackShape>,
    duck: &Duck<'_>,
    tracks: &mut IndexMap<String, Track>,
    errors: &mut Errors,
) -> Arrangement {
    // Every declared clip, with its kind when that is valid.
    let mut kinds: IndexMap<&str, Option<ClipKind>> = IndexMap::new();
    let mut clips = IndexMap::new();
    for (id, c) in &raw.clips {
        check_id(id, "clip", errors);
        let kind = c.kind.as_ref().and_then(|k| {
            let kind = ClipKind::from_name(&k.value);
            if kind.is_none() {
                let expected: Vec<&str> = ClipKind::ALL.iter().map(|k| k.name()).collect();
                errors.at(
                    &k.span,
                    format!(
                        "unknown clip kind `{}` (expected {})",
                        k.value,
                        or_list(&expected)
                    ),
                );
            }
            kind
        });
        kinds.insert(id.value.as_str(), kind);
        if let (Some(kind), Some(steps)) = (kind, &c.steps)
            && let Some(clip) = clip_steps("clip", id, kind, steps, errors)
        {
            clips.insert(id.value.clone(), clip);
        }
    }

    let mut sections = Vec::new();
    match &raw.sections {
        Some(list) if !list.is_empty() => {
            for s in list {
                let bars = section_bars(s, errors);
                if let (Some(name), Some(bars)) = (&s.name, bars) {
                    sections.push(Section {
                        name: name.value.clone(),
                        bars,
                    });
                }
            }
        }
        _ => no_sections(errors),
    }
    // The song's length, when there are sections and every one is valid; the
    // checks against it are skipped otherwise (a broken section, or none at
    // all, has its own error).
    let total = raw
        .sections
        .as_ref()
        .is_some_and(|list| !list.is_empty() && list.len() == sections.len())
        .then(|| sections.iter().map(|s| s.bars).sum::<u32>());

    for (id, t) in &raw.tracks {
        let Some(shape) = shapes.get(id.value.as_str()) else {
            continue;
        };
        let play = placements(&id.value, &t.play, shape, &kinds, total, errors);
        if let (Some(play), Some(track)) = (play, tracks.get_mut(&id.value)) {
            track.play = play;
        }
    }

    let mut automation: Vec<Lane> = Vec::new();
    for raw_lane in &raw.automation {
        let Some(lane) = stretch(raw_lane, total, shapes, duck, errors) else {
            continue;
        };
        let clash = automation.iter().find(|l| {
            l.track == lane.track
                && l.param == lane.param
                && l.start_bar < lane.span().end
                && lane.start_bar < l.span().end
        });
        if let Some(other) = clash {
            let at = raw_lane.at.as_ref().expect("a built stretch has `at`");
            errors.at(
                &at.span,
                format!(
                    "automation for `{}.{}` at bars {} overlaps the stretch at bars {}",
                    lane.track,
                    lane.param,
                    bar_range(lane.start_bar, lane.bars),
                    bar_range(other.start_bar, other.bars)
                ),
            );
            continue;
        }
        automation.push(lane);
    }

    Arrangement {
        clips,
        sections,
        automation,
    }
}

fn no_sections(errors: &mut Errors) {
    errors.global("a song needs at least one section ([[sections]])");
}

/// `3-4`: the 1-based first and last bar of `bars` bars from 0-based
/// `start_bar`.
fn bar_range(start_bar: u32, bars: u32) -> String {
    format!("{}-{}", start_bar + 1, start_bar + bars)
}

/// Parses `steps` of clip (or v1 pattern, as `what` says) `id` in the
/// notation of `kind`.
fn clip_steps(
    what: &str,
    id: &At<String>,
    kind: ClipKind,
    steps: &At<String>,
    errors: &mut Errors,
) -> Option<Clip> {
    let parsed = match kind {
        ClipKind::Drum => DrumPattern::parse(&steps.value)
            .map(Clip::Drum)
            .map_err(|e| match e {
                DrumPatternError::Length(n) => Err(n),
                other => Ok(other.to_string()),
            }),
        ClipKind::Acid => AcidPattern::parse(&steps.value)
            .map(Clip::Acid)
            .map_err(|e| match e {
                PatternError::Length(n) => Err(n),
                other => Ok(other.to_string()),
            }),
    };
    match parsed {
        Ok(clip) => Some(clip),
        Err(Err(n)) => {
            errors.at(
                &steps.span,
                format!("{what} `{}` needs 1 to 64 steps, got {n}", id.value),
            );
            None
        }
        Err(Ok(why)) => {
            errors.at(
                &steps.span,
                format!("{what} `{}` has a bad step: {why}", id.value),
            );
            None
        }
    }
}

/// A section's `bars`, when it is valid.
fn section_bars(s: &RawSection, errors: &mut Errors) -> Option<u32> {
    s.bars.as_ref().and_then(|b| {
        let ok = BARS.contains(&b.value);
        if !ok {
            errors.at(&b.span, format!("bars must be 1 to 1024, got {}", b.value));
        }
        ok.then(|| u32::try_from(b.value).expect("checked range"))
    })
}

/// A v1 section read into the arrangement: the section itself, a
/// placement over all of it per `play` entry (track id first, in table
/// order) and a lane stretch over all of it per lane (in file order).
struct V1Section {
    section: Section,
    play: Vec<(String, Placement)>,
    lanes: Vec<Lane>,
}

/// Checks v1 section `index`, which starts at 0-based bar `start_bar`.
fn section_v1(
    index: usize,
    start_bar: u32,
    s: &RawSection,
    shapes: &IndexMap<&str, TrackShape>,
    pattern_owner: &IndexMap<&str, Option<&str>>,
    duck: &Duck<'_>,
    errors: &mut Errors,
) -> Option<V1Section> {
    let label = match &s.name {
        Some(n) => format!("section `{}`", n.value),
        None => format!("section {}", index + 1),
    };
    let bars = section_bars(s, errors);

    let before = errors.len();
    let mut play = Vec::new();
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
            _ => play.push((track.value.clone(), pat.value.clone())),
        }
    }

    let mut automation = Vec::new();
    let mut seen: IndexSet<(String, String)> = IndexSet::new();
    for raw in &s.automation {
        if let Some(lane) = lane(raw, start_bar, bars, "section", shapes, duck, errors) {
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
    let bars = bars?;
    let play = play
        .into_iter()
        .map(|(track, clip)| {
            let placement = Placement {
                start_bar,
                bars,
                clip,
                offset_bars: 0,
            };
            (track, placement)
        })
        .collect();
    Some(V1Section {
        section: Section {
            name: s.name.as_ref()?.value.clone(),
            bars,
        },
        play,
        lanes: automation,
    })
}

/// A 1-based `at` as a 0-based bar, when it is at least 1.
fn start_bar(at: &At<i64>, errors: &mut Errors) -> Option<i64> {
    if at.value < 1 {
        errors.at(&at.span, "`at` is a 1-based bar: it must be at least 1");
        return None;
    }
    Some(at.value - 1)
}

/// A placement's or stretch's `bars`, when it is at least 1.
fn length(bars: &At<i64>, errors: &mut Errors) -> Option<i64> {
    if bars.value < 1 {
        errors.at(
            &bars.span,
            format!("bars must be at least 1, got {}", bars.value),
        );
        return None;
    }
    Some(bars.value)
}

/// Checks the v2 placements of track `track` (of shape `shape`) against the
/// clip pool (`kinds`) and the song's length (`total` bars, when known).
/// `None` when any of them is broken.
fn placements(
    track: &str,
    raw: &[RawPlacement],
    shape: &TrackShape,
    kinds: &IndexMap<&str, Option<ClipKind>>,
    total: Option<u32>,
    errors: &mut Errors,
) -> Option<Vec<Placement>> {
    let before = errors.len();
    let mut play = Vec::new();
    // The 0-based start and end of the last placement in time order.
    let mut prev: Option<(i64, i64)> = None;
    for p in raw {
        if let Some(clip) = &p.clip {
            match kinds.get(clip.value.as_str()) {
                None => errors.at(&clip.span, format!("unknown clip `{}`", clip.value)),
                Some(Some(kind)) => {
                    if let Some(module) = shape.module()
                        && !module.plays(*kind)
                    {
                        let plays: Vec<&str> = ClipKind::ALL
                            .iter()
                            .filter(|k| module.plays(**k))
                            .map(|k| k.name())
                            .collect();
                        errors.at(
                            &clip.span,
                            format!(
                                "clip `{}` is an {} clip; track `{track}` ({}) plays {} clips",
                                clip.value,
                                kind.name(),
                                module.name,
                                or_list(&plays)
                            ),
                        );
                    }
                }
                // A clip with an unknown kind has its own error.
                Some(None) => {}
            }
        }
        let offset = match &p.offset {
            None => Some(0),
            Some(o) if (0..=MAX_OFFSET).contains(&o.value) => Some(o.value),
            Some(o) => {
                errors.at(
                    &o.span,
                    format!("offset must be 0 to {MAX_OFFSET}, got {}", o.value),
                );
                None
            }
        };
        let start = p.at.as_ref().and_then(|at| start_bar(at, errors));
        let bars = p.bars.as_ref().and_then(|b| length(b, errors));
        let (Some(start), Some(bars)) = (start, bars) else {
            continue;
        };
        let end = start.saturating_add(bars);
        let at = p.at.as_ref().expect("a valid start has `at`");
        match prev {
            Some((prev_start, _)) if start < prev_start => {
                errors.at(
                    &at.span,
                    format!("placements on track `{track}` must be in time order"),
                );
                continue;
            }
            Some((prev_start, prev_end)) if start < prev_end => {
                errors.at(
                    &at.span,
                    format!(
                        "placement at bar {} on track `{track}` overlaps the one at bar {}",
                        start + 1,
                        prev_start + 1
                    ),
                );
                continue;
            }
            _ => prev = Some((start, end)),
        }
        if let Some(total) = total
            && end > i64::from(total)
        {
            let b = p.bars.as_ref().expect("valid bars");
            errors.at(
                &b.span,
                format!(
                    "placement on track `{track}` ends at bar {end}, past the song's end (bar {total})"
                ),
            );
            continue;
        }
        if let (Some(clip), Some(offset), Some(_)) = (&p.clip, offset, total) {
            play.push(Placement {
                start_bar: u32::try_from(start).expect("inside the song"),
                bars: u32::try_from(bars).expect("inside the song"),
                clip: clip.value.clone(),
                offset_bars: u32::try_from(offset).expect("checked range"),
            });
        }
    }
    (errors.len() == before).then_some(play)
}

/// Checks one v2 `[[automation]]` stretch against the song's length
/// (`total` bars, when known).
fn stretch(
    raw: &RawLane,
    total: Option<u32>,
    shapes: &IndexMap<&str, TrackShape>,
    duck: &Duck<'_>,
    errors: &mut Errors,
) -> Option<Lane> {
    let before = errors.len();
    let start = raw.at.as_ref().and_then(|at| start_bar(at, errors));
    let bars = raw.bars.as_ref().and_then(|b| length(b, errors));
    let mut span = None;
    if let (Some(start), Some(bars), Some(total)) = (start, bars, total) {
        let end = start.saturating_add(bars);
        if end > i64::from(total) {
            let b = raw.bars.as_ref().expect("valid bars");
            errors.at(
                &b.span,
                format!(
                    "automation at bars {}-{end} runs past the song's end (bar {total})",
                    start + 1
                ),
            );
        } else {
            let start = u32::try_from(start).expect("inside the song");
            span = Some((start, u32::try_from(bars).expect("inside the song")));
        }
    }
    // The points are checked against `bars` even when the stretch is cut
    // off by a broken section or the song's end.
    let len = bars.and_then(|b| u32::try_from(b).ok());
    let lane = lane(
        raw,
        span.map_or(0, |(s, _)| s),
        len,
        "stretch",
        shapes,
        duck,
        errors,
    );
    let (start, bars) = span?;
    (errors.len() == before).then_some(())?;
    lane.map(|lane| Lane {
        start_bar: start,
        bars,
        ..lane
    })
}

/// Checks one lane stretch that starts at `start_bar` and is `bars` long
/// (`None` when its `bars` is broken); `region` names it in messages (a v1
/// lane covers its `section`, a v2 one is a `stretch`).
fn lane(
    raw: &RawLane,
    start_bar: u32,
    bars: Option<u32>,
    region: &str,
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
                format!("bar {bar} is outside the {region} (0..={bars})"),
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
        start_bar,
        // A broken `bars` has its own error, so the lane is never used.
        bars: bars.unwrap_or(0),
        points: points
            .value
            .iter()
            .map(|p| (p.bar.value, p.value.value))
            .collect(),
    })
}

/// Splits `<track id>.<param id>` at the first `.` and resolves the param
/// against the mix tables for the `mix` pseudo-track, else by device id:
/// the instrument, the strip, then the fx device with that id.
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
            errors.at(
                &p.span,
                unknown_param(param_id, MIX_TRACK_ID, &spec_ids(&tables)),
            );
            return None;
        }
        return Some((track_id.to_owned(), param_id.to_owned()));
    }
    let Some(shape) = shapes.get(track_id) else {
        errors.at(&p.span, unknown_track(track_id));
        return None;
    };
    // An unknown instrument already has its own error.
    let instrument = shape.instrument.as_ref()?;
    let found = instrument.has_param(param_id)
        || find_in(&[STRIP_PARAMS], param_id).is_some()
        || shape.fx.iter().any(|fx| fx.has_param(param_id));
    if !found {
        let mut known = param_ids(&instrument.id, instrument.module);
        known.extend(spec_ids(&[STRIP_PARAMS]));
        errors.at(
            &p.span,
            unknown_param(param_id, instrument.module.name, &known),
        );
        return None;
    }
    Some((track_id.to_owned(), param_id.to_owned()))
}
