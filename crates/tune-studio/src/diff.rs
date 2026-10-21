//! What changed between two songs, by edit path.
//!
//! Values come from [`effective_value`], so a param the file leaves out
//! compares at its default: deleting an explicit value that equals the
//! default is a change of nothing, and deleting one that does not shows the
//! default as the new value.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tune_dsp::STRIP_PARAMS;
use tune_song::{MixUnit, Song, Track};

use crate::doc::effective_value;
use crate::path::{SongPath, TrackField};

/// One value that differs between two songs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub path: SongPath,
    pub old: Value,
    pub new: Value,
}

/// Every difference between `old` and `new`, in song-file order (the mix
/// right after `song.bpm`, as `[mix]` follows `[song]`).
///
/// A changed section count or order gives one `sections` change. Otherwise
/// sections are compared field by field (`bars`, `name`, `play.<t>`,
/// `lanes`). A track that appears or disappears gives one `tracks.<t>`
/// change with `null` on the missing side.
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
                let p = SongPath::FxMode {
                    track: name.clone(),
                };
                let (a, b) = both(&p);
                push(p, a, b);
            }
            _ => {
                let p = SongPath::Track { track: name };
                let (a, b) = both(&p);
                push(p, a, b);
            }
        }
    }

    for name in union(old.patterns.keys(), new.patterns.keys()) {
        let p = SongPath::Pattern { pattern: name };
        let (a, b) = both(&p);
        push(p, a, b);
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
            let plays = union(old.sections[i].play.keys(), new.sections[i].play.keys());
            for track in plays {
                if !old.tracks.contains_key(&track) || !new.tracks.contains_key(&track) {
                    continue;
                }
                let p = SongPath::SectionPlay { section: i, track };
                let (a, b) = both(&p);
                push(p, a, b);
            }
            let p = SongPath::SectionLanes(i);
            let (a, b) = both(&p);
            push(p, a, b);
        }
    }
    out
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

/// Full ids of every param the track's instrument, strip and fx chain have.
fn param_ids(track: &Track) -> Vec<String> {
    let mut ids: Vec<String> = track
        .instrument
        .params()
        .iter()
        .chain(STRIP_PARAMS)
        .map(|s| s.id.to_owned())
        .collect();
    for fx in &track.fx {
        ids.extend(fx.params_table().iter().map(|s| s.id.to_owned()));
    }
    ids
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
