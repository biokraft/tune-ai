//! What changed between two songs, by edit path.
//!
//! Values come from [`effective_value`], so a param the file leaves out
//! compares at its default: deleting an explicit value that equals the
//! default is a change of nothing, and deleting one that does not shows the
//! default as the new value.

use std::collections::HashMap;
use std::ops::Range;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tune_dsp::STRIP_PARAMS;
use tune_song::{Device, MixUnit, Song, Track, lane_value, round_split_value};

use crate::doc::effective_value;
use crate::path::{SongPath, TrackField};

/// One value that differs between two songs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ts_rs::TS)]
pub struct Change {
    #[ts(type = "string")]
    pub path: SongPath,
    pub old: Value,
    pub new: Value,
}

/// Every difference between `old` and `new`, in song-file order (the mix
/// right after `song.bpm`, as `[mix]` follows `[song]`).
///
/// After the clips come the scenes: `scenes.<s>` for a scene in one song
/// only, `scenes` when the scenes in both are in another relative order, and
/// one `scenes.<s>.slots.<t>` per slot whose clip differs.
///
/// A changed section count or order gives one `sections` change. Otherwise
/// sections are compared field by field (`bars`, `name`, `play.<t>`,
/// `lanes`). What a section plays and its lanes are compared by their parts
/// inside the section, relative to its start, so they count as changed only
/// when they changed within the section: not when the section only moved,
/// nor when a placement or stretch was split at its edge. A track that appears or disappears gives one `tracks.<t>`
/// change with `null` on the missing side; a track in both songs whose
/// devices differ (a module, an id, the fx order) gives a `tracks.<t>`
/// change first, then its param and setting changes.
pub fn diff_songs(old: &Song, new: &Song) -> Vec<Change> {
    let mut out = Vec::new();
    let mut push = |path: SongPath, a: Option<Value>, b: Option<Value>| {
        let (a, b) = (a.unwrap_or(Value::Null), b.unwrap_or(Value::Null));
        if a != b {
            out.push(Change {
                path,
                old: a,
                new: b,
            });
        }
    };
    let both = |path: &SongPath| (effective_value(old, path), effective_value(new, path));

    let p = SongPath::Bpm;
    let (a, b) = both(&p);
    push(p, a, b);

    let mix_params = MixUnit::ALL.into_iter().flat_map(|unit| {
        unit.params().iter().map(|spec| SongPath::MixParam {
            param: spec.id.to_owned(),
        })
    });
    for p in [SongPath::MixMaster, SongPath::MixDuckSource]
        .into_iter()
        .chain(mix_params)
    {
        let (a, b) = both(&p);
        push(p, a, b);
    }

    for name in union(old.tracks.keys(), new.tracks.keys()) {
        match (old.tracks.get(&name), new.tracks.get(&name)) {
            (Some(o), Some(n)) => {
                // The devices themselves: modules and ids, in chain order.
                let p = SongPath::Track {
                    track: name.clone(),
                };
                let (a, b) = both(&p);
                push(p, a, b);
                for field in TrackField::ALL {
                    let p = SongPath::TrackField {
                        track: name.clone(),
                        field,
                    };
                    let (a, b) = both(&p);
                    push(p, a, b);
                }
                let mut ids = param_ids(o);
                for id in param_ids(n) {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
                for id in ids {
                    let p = SongPath::TrackParam {
                        track: name.clone(),
                        param: id,
                    };
                    let (a, b) = both(&p);
                    push(p, a, b);
                }
            }
            _ => {
                let p = SongPath::Track { track: name };
                let (a, b) = both(&p);
                push(p, a, b);
            }
        }
    }

    for name in union(old.clips.keys(), new.clips.keys()) {
        for p in [
            SongPath::Clip { clip: name.clone() },
            SongPath::ClipBars { clip: name.clone() },
            SongPath::ClipNotes { clip: name },
        ] {
            let (a, b) = both(&p);
            push(p, a, b);
        }
    }

    for name in union(old.scenes.keys(), new.scenes.keys()) {
        if old.scenes.contains_key(&name) != new.scenes.contains_key(&name) {
            let p = SongPath::Scene { scene: name };
            let (a, b) = both(&p);
            push(p, a, b);
        }
    }
    if scenes_reordered(old, new) {
        let p = SongPath::Scenes;
        let (a, b) = both(&p);
        push(p, a, b);
    }
    for (name, o) in &old.scenes {
        let Some(n) = new.scenes.get(name) else {
            continue;
        };
        for track in union(o.slots.keys(), n.slots.keys()) {
            if o.slots.get(&track) == n.slots.get(&track) {
                continue;
            }
            let p = SongPath::SceneSlot {
                scene: name.clone(),
                track,
            };
            let (a, b) = both(&p);
            push(p, a, b);
        }
    }

    if sections_restructured(old, new) {
        let p = SongPath::Sections;
        let (a, b) = both(&p);
        push(p, a, b);
    } else {
        for i in 0..new.sections.len() {
            for p in [SongPath::SectionBars(i), SongPath::SectionName(i)] {
                let (a, b) = both(&p);
                push(p, a, b);
            }
            // Tracks in both songs, in the new song's order; one that is
            // silent in both compares equal and is left out.
            let plays = new.tracks.keys().filter(|t| old.tracks.contains_key(*t));
            for track in plays {
                if relative_play(old, i, track) == relative_play(new, i, track) {
                    continue;
                }
                let p = SongPath::SectionPlay {
                    section: i,
                    track: track.clone(),
                };
                let (a, b) = both(&p);
                push(p, a, b);
            }
            if relative_lanes(old, i) != relative_lanes(new, i) {
                let p = SongPath::SectionLanes(i);
                let (a, b) = both(&p);
                push(p, a, b);
            }
        }
    }
    out
}

/// The `song.version` change of a file whose format went from version `old`
/// to `new`, if it changed.
pub(crate) fn version_change(old: u32, new: u32) -> Option<Change> {
    (old != new).then(|| Change {
        path: SongPath::Version,
        old: Value::from(old),
        new: Value::from(new),
    })
}

/// The changes between two texts of a song file, given the format version
/// each was written in and the song it describes: `song.version` first when
/// the version differs, then [`diff_songs`].
pub(crate) fn diff_files(
    old_version: u32,
    old: &Song,
    new_version: u32,
    new: &Song,
) -> Vec<Change> {
    version_change(old_version, new_version)
        .into_iter()
        .chain(diff_songs(old, new))
        .collect()
}

/// Names in `a` order, then those only in `b`.
fn union<'a>(
    a: impl Iterator<Item = &'a String>,
    b: impl Iterator<Item = &'a String>,
) -> Vec<String> {
    let mut out: Vec<String> = a.cloned().collect();
    for k in b {
        if !out.contains(k) {
            out.push(k.clone());
        }
    }
    out
}

/// Track-relative ids of every param the track's instrument, strip and fx
/// chain have, then of every device setting (`dist.mode`).
fn param_ids(track: &Track) -> Vec<String> {
    let params = |d: &Device| {
        (0..d.module.params.len())
            .map(|i| d.param_id(d.module.local_name(i)))
            .collect::<Vec<_>>()
    };
    let mut ids = params(&track.instrument);
    ids.extend(STRIP_PARAMS.iter().map(|s| s.id.to_owned()));
    for fx in &track.fx {
        ids.extend(params(fx));
    }
    for d in std::iter::once(&track.instrument).chain(&track.fx) {
        ids.extend(d.module.settings.iter().map(|s| d.param_id(s.name)));
    }
    ids
}

/// The 0-based bars of section `i`, if the song has it.
fn section_range(song: &Song, i: usize) -> Option<Range<u32>> {
    let start = *song.section_start_bars().get(i)?;
    Some(start..start + song.sections[i].bars)
}

/// The part of the 0-based span `span` inside `section`, if any.
fn clip_span(span: Range<u32>, section: &Range<u32>) -> Option<Range<u32>> {
    let part = span.start.max(section.start)..span.end.min(section.end);
    (part.start < part.end).then_some(part)
}

/// The part of one placement inside a section: clip, start from the
/// section's start, bars and the offset into the clip there.
type PlayPart<'s> = (&'s str, u32, u32, u64);

/// What `track` plays inside section `i`, relative to the section's start;
/// `None` when the song has no such section or track.
fn relative_play<'s>(song: &'s Song, i: usize, track: &str) -> Option<Vec<PlayPart<'s>>> {
    let section = section_range(song, i)?;
    let placements = &song.tracks.get(track)?.play;
    Some(
        placements
            .iter()
            .filter_map(|p| {
                let part = clip_span(p.span(), &section)?;
                let offset = u64::from(p.offset_bars) + u64::from(part.start - p.start_bar);
                Some((
                    p.clip.as_str(),
                    part.start - section.start,
                    part.end - part.start,
                    offset,
                ))
            })
            .collect(),
    )
}

/// The part of one lane stretch inside a section: track, param, start from
/// the section's start, bars, and the breakpoints of that part (its value
/// at the start, the points strictly inside, its value at the end).
type LanePart<'s> = (&'s str, &'s str, u32, u32, Vec<(f64, f64)>);

/// The stretches inside section `i`, each cut to the section and relative
/// to its start, in a fixed order; `None` when the song has no such
/// section. A stretch split at the section's edge gives the same part as
/// the whole one (the values are computed as the split computes them).
fn relative_lanes(song: &Song, i: usize) -> Option<Vec<LanePart<'_>>> {
    let section = section_range(song, i)?;
    let mut parts: Vec<LanePart<'_>> = song
        .automation
        .iter()
        .filter_map(|l| {
            let part = clip_span(l.span(), &section)?;
            let from = f64::from(part.start - l.start_bar);
            let to = f64::from(part.end - l.start_bar);
            let points = &l.points;
            // At the end, the value just before it: the first point there
            // (the earlier side of a jump), else the lane's value.
            let end = points
                .iter()
                .find(|&&(bar, _)| bar == to)
                .map_or_else(|| round_split_value(lane_value(points, to)), |&(_, v)| v);
            let mut cut = vec![(0.0, round_split_value(lane_value(points, from)))];
            cut.extend(
                points
                    .iter()
                    .filter(|&&(bar, _)| from < bar && bar < to)
                    .map(|&(bar, v)| (bar - from, v)),
            );
            cut.push((to - from, end));
            Some((
                l.track.as_str(),
                l.param.as_str(),
                part.start - section.start,
                part.end - part.start,
                cut,
            ))
        })
        .collect();
    parts.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
    Some(parts)
}

/// Whether the scenes in both songs are in a different relative order.
fn scenes_reordered(old: &Song, new: &Song) -> bool {
    let shared = |a: &Song, b: &Song| -> Vec<String> {
        a.scenes
            .keys()
            .filter(|k| b.scenes.contains_key(*k))
            .cloned()
            .collect()
    };
    shared(old, new) != shared(new, old)
}

/// Whether the section list changed in count, or holds the same sections
/// (name and length) in another order.
fn sections_restructured(old: &Song, new: &Song) -> bool {
    if old.sections.len() != new.sections.len() {
        return true;
    }
    let key = |s: &Song| -> Vec<(String, u32)> {
        s.sections
            .iter()
            .map(|x| (x.name.clone(), x.bars))
            .collect()
    };
    let (a, b) = (key(old), key(new));
    if a == b {
        return false;
    }
    let (mut sa, mut sb) = (a, b);
    sa.sort();
    sb.sort();
    sa == sb
}

/// Whether more than half of the old text's non-blank lines are gone from
/// the new text: a different song replaced the file, not an edit.
pub fn is_whole_replace(old_text: &str, new_text: &str) -> bool {
    let mut available: HashMap<&str, usize> = HashMap::new();
    for line in new_text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        *available.entry(line).or_default() += 1;
    }
    let (mut total, mut gone) = (0usize, 0usize);
    for line in old_text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        total += 1;
        match available.get_mut(line) {
            Some(n) if *n > 0 => *n -= 1,
            _ => gone += 1,
        }
    }
    gone * 2 > total
}
