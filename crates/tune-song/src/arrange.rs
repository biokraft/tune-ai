//! Section operations: move, duplicate, delete or resize a section, or set
//! what one track plays in it, carrying the placements and lane stretches
//! along.
//!
//! Every operation first splits the placements and stretches that cross the
//! edges it names, so each item lies wholly inside or outside the edited
//! bars, and then moves whole items. A split keeps the clip phase and the
//! lane values, so every bar outside the edit plays as before.
//!
//! Results are not validated here (the caller re-parses them): clip
//! existence and compatibility are the validator's.

use std::ops::Range;

use crate::model::{Lane, Placement, SECTION_BARS, Song, lane_value, section_bars_error};

/// Where each stretch of the edited song came from, index-aligned with the
/// new `song.automation`. Each holds an index into the old `automation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneOrigin {
    /// The stretch itself, or the first piece of a split one.
    Kept(usize),
    /// A later piece of a split stretch.
    Split(usize),
    /// A copy made by [`duplicate_section`].
    Copy(usize),
}

impl LaneOrigin {
    /// The index into the old `automation`.
    fn source(self) -> usize {
        match self {
            Self::Kept(i) | Self::Split(i) | Self::Copy(i) => i,
        }
    }
}

/// The song a section operation made, and where its stretches came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Arranged {
    pub song: Song,
    /// One entry per stretch of `song.automation`, in the same order.
    pub lanes: Vec<LaneOrigin>,
}

/// Why a section operation was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ArrangeError {
    #[error("there is no section {index} (the song has {len})")]
    OutOfBounds { index: usize, len: usize },
    #[error("the last section cannot be deleted")]
    LastSection,
    #[error("there is no track `{0}`")]
    NoSuchTrack(String),
    #[error("{}", section_bars_error(i64::from(*.bars)))]
    Bars { bars: u32 },
    /// The result would hold a bar past `u32::MAX` (never for a song the
    /// validator accepts).
    #[error("the arrangement would be too long")]
    TooLong,
}

/// Moves section `from` to index `to`, as `Vec::remove` then `insert`, with
/// everything it plays.
pub fn move_section(song: &Song, from: usize, to: usize) -> Result<Arranged, ArrangeError> {
    let moved = check(song, from, 0)?;
    let target = check(song, to, 0)?;
    let mut edit = Edit::new(song);
    if to == from {
        return Ok(edit.finish());
    }
    let len = i64::from(moved.end - moved.start);
    // The insertion edge, the bars between the old and the new place, how
    // far those move, and how far the moved section moves.
    let (edge, between, by, delta) = if to > from {
        let delta = i64::from(target.end - moved.end);
        (target.end, moved.end..target.end, -len, delta)
    } else {
        let delta = -i64::from(moved.start - target.start);
        (target.start, target.start..moved.start, len, delta)
    };
    edit.split(moved.start);
    edit.split(moved.end);
    edit.split(edge);
    edit.remap(|span| {
        if moved.contains(&span.start) {
            Fate::shifted(span, delta)
        } else if between.contains(&span.start) {
            Fate::shifted(span, by)
        } else {
            Fate::keep(span)
        }
    });
    let section = edit.song.sections.remove(from);
    edit.song.sections.insert(to, section);
    Ok(edit.finish())
}

/// Puts a copy of section `index`, with everything it plays, right after it.
pub fn duplicate_section(song: &Song, index: usize) -> Result<Arranged, ArrangeError> {
    let grows = song.sections.get(index).map_or(0, |s| s.bars);
    let bars = check(song, index, grows)?;
    let len = i64::from(bars.end - bars.start);
    let mut edit = Edit::new(song);
    edit.split(bars.start);
    edit.split(bars.end);
    edit.remap(|span| {
        if bars.contains(&span.start) {
            Fate {
                copy: Some(shift(span.clone(), len)),
                ..Fate::keep(span)
            }
        } else if span.start >= bars.end {
            Fate::shifted(span, len)
        } else {
            Fate::keep(span)
        }
    });
    let section = edit.song.sections[index].clone();
    edit.song.sections.insert(index + 1, section);
    Ok(edit.finish())
}

/// Removes section `index` and everything inside it; the rest closes up.
/// The last section is never deleted.
pub fn delete_section(song: &Song, index: usize) -> Result<Arranged, ArrangeError> {
    let bars = check(song, index, 0)?;
    if song.sections.len() == 1 {
        return Err(ArrangeError::LastSection);
    }
    let len = i64::from(bars.end - bars.start);
    let mut edit = Edit::new(song);
    edit.split(bars.start);
    edit.split(bars.end);
    edit.remap(|span| {
        if bars.contains(&span.start) {
            Fate::DROP
        } else if span.start >= bars.end {
            Fate::shifted(span, -len)
        } else {
            Fate::keep(span)
        }
    });
    edit.song.sections.remove(index);
    Ok(edit.finish())
}

/// Makes section `index` `bars` long. On a grow, the placements and
/// stretches that ran to its end run to the new end (a stretch holds its
/// last value); on a shrink, whatever crosses the new end is cut there and
/// whatever starts at or after it is dropped. Everything after the section
/// moves with its end. `bars` must be in [`SECTION_BARS`], as the validator
/// wants.
pub fn resize_section(song: &Song, index: usize, bars: u32) -> Result<Arranged, ArrangeError> {
    if !SECTION_BARS.contains(&bars) {
        return Err(ArrangeError::Bars { bars });
    }
    let old = check(song, index, bars)?;
    let delta = i64::from(bars) - i64::from(old.end - old.start);
    let new_end = old.start + bars;
    let mut edit = Edit::new(song);
    edit.split(old.start);
    edit.split(old.end);
    if delta < 0 {
        edit.split(new_end);
    }
    edit.remap(|span| {
        if span.start >= old.end {
            Fate::shifted(span, delta)
        } else if span.start < old.start {
            Fate::keep(span)
        } else if span.start >= new_end {
            Fate::DROP
        } else if delta > 0 && span.end == old.end {
            Fate::keep(span.start..new_end)
        } else {
            Fate::keep(span)
        }
    });
    edit.song.sections[index].bars = bars;
    Ok(edit.finish())
}

/// Makes track `track` play `clip` over all of section `section` from the
/// clip's start, or nothing there when `clip` is `None`. Lanes are untouched.
pub fn set_section_play(
    song: &Song,
    section: usize,
    track: &str,
    clip: Option<&str>,
) -> Result<Arranged, ArrangeError> {
    let bars = check(song, section, 0)?;
    if !song.tracks.contains_key(track) {
        return Err(ArrangeError::NoSuchTrack(track.to_owned()));
    }
    let mut edit = Edit::new(song);
    let play = &mut edit.song.tracks[track].play;
    split_placements(play, bars.start);
    split_placements(play, bars.end);
    play.retain(|p| !bars.contains(&p.start_bar));
    if let Some(clip) = clip {
        let at = play.partition_point(|p| p.start_bar < bars.start);
        play.insert(
            at,
            Placement {
                start_bar: bars.start,
                bars: bars.end - bars.start,
                clip: clip.to_owned(),
                offset_bars: 0,
            },
        );
    }
    Ok(edit.finish())
}

/// The 0-based bars of section `index`, or why there is no such section
/// or why an op that may push items up to `grows` bars later cannot run.
///
/// Every bar an op computes (a section start or end, an item's start or end,
/// a shifted or copied item, a split's offset) is at most the song's
/// furthest bar plus `grows`, or a placement's `offset_bars + bars`. When
/// those fit in `u32`, no bar arithmetic of the op can overflow.
fn check(song: &Song, index: usize, grows: u32) -> Result<Range<u32>, ArrangeError> {
    if index >= song.sections.len() {
        return Err(ArrangeError::OutOfBounds {
            index,
            len: song.sections.len(),
        });
    }
    let end = |start: u32, bars: u32| u64::from(start) + u64::from(bars);
    let total: u64 = song.sections.iter().map(|s| u64::from(s.bars)).sum();
    let placements = song.tracks.values().flat_map(|t| &t.play);
    let furthest = placements
        .clone()
        .map(|p| end(p.start_bar, p.bars))
        .chain(song.automation.iter().map(|l| end(l.start_bar, l.bars)))
        .fold(total, u64::max);
    let offsets = placements.map(|p| end(p.offset_bars, p.bars)).max();
    let max = u64::from(u32::MAX);
    if furthest + u64::from(grows) > max || offsets.is_some_and(|o| o > max) {
        return Err(ArrangeError::TooLong);
    }
    Ok(song.section_bars(index))
}

/// Where a whole item (a placement or a stretch lying inside one region
/// after the splits) goes: its new bars, or nowhere, and the bars of a copy.
struct Fate {
    to: Option<Range<u32>>,
    copy: Option<Range<u32>>,
}

impl Fate {
    const DROP: Self = Self {
        to: None,
        copy: None,
    };

    fn keep(span: Range<u32>) -> Self {
        Self {
            to: Some(span),
            copy: None,
        }
    }

    fn shifted(span: Range<u32>, by: i64) -> Self {
        Self::keep(shift(span, by))
    }
}

/// `span` moved `by` bars, in `i64`; [`check`] keeps the result in `u32`.
fn shift(span: Range<u32>, by: i64) -> Range<u32> {
    let start = u32::try_from(i64::from(span.start) + by).expect("checked: inside u32");
    start..start + (span.end - span.start)
}

/// A song being edited, with where each of its stretches came from.
struct Edit {
    song: Song,
    origins: Vec<LaneOrigin>,
}

impl Edit {
    fn new(song: &Song) -> Self {
        Self {
            song: song.clone(),
            origins: (0..song.automation.len()).map(LaneOrigin::Kept).collect(),
        }
    }

    /// Splits every placement and stretch that crosses 0-based bar `bar`.
    fn split(&mut self, bar: u32) {
        for track in self.song.tracks.values_mut() {
            split_placements(&mut track.play, bar);
        }
        let lanes = std::mem::take(&mut self.song.automation);
        let origins = std::mem::take(&mut self.origins);
        for (lane, origin) in lanes.into_iter().zip(origins) {
            if lane.start_bar < bar && bar < lane.span().end {
                let (left, right) = split_lane(&lane, bar);
                self.push(left, origin);
                self.push(right, LaneOrigin::Split(origin.source()));
            } else {
                self.push(lane, origin);
            }
        }
    }

    /// Moves, drops or copies every placement and stretch as `fate` says.
    /// A copy goes right after its original.
    fn remap(&mut self, fate: impl Fn(Range<u32>) -> Fate) {
        for track in self.song.tracks.values_mut() {
            let mut play = Vec::with_capacity(track.play.len());
            for p in track.play.drain(..) {
                let Fate { to, copy } = fate(p.span());
                for span in [to, copy].into_iter().flatten() {
                    play.push(Placement {
                        start_bar: span.start,
                        bars: span.end - span.start,
                        ..p.clone()
                    });
                }
            }
            play.sort_by_key(|p| p.start_bar);
            track.play = play;
        }
        let lanes = std::mem::take(&mut self.song.automation);
        let origins = std::mem::take(&mut self.origins);
        for (lane, origin) in lanes.into_iter().zip(origins) {
            let Fate { to, copy } = fate(lane.span());
            if let Some(span) = to {
                self.push(moved(&lane, span), origin);
            }
            if let Some(span) = copy {
                self.push(moved(&lane, span), LaneOrigin::Copy(origin.source()));
            }
        }
    }

    fn push(&mut self, lane: Lane, origin: LaneOrigin) {
        self.song.automation.push(lane);
        self.origins.push(origin);
    }

    /// The edited song, its stretches stably sorted by the section each
    /// starts in.
    fn finish(self) -> Arranged {
        let starts = self.song.section_start_bars();
        let section = |lane: &Lane| starts.partition_point(|&s| s <= lane.start_bar) - 1;
        let mut lanes: Vec<(Lane, LaneOrigin)> =
            self.song.automation.into_iter().zip(self.origins).collect();
        lanes.sort_by_key(|(lane, _)| section(lane));
        let (automation, origins) = lanes.into_iter().unzip();
        Arranged {
            song: Song {
                automation,
                ..self.song
            },
            lanes: origins,
        }
    }
}

/// `lane` over the bars `span`, with the same points.
fn moved(lane: &Lane, span: Range<u32>) -> Lane {
    Lane {
        start_bar: span.start,
        bars: span.end - span.start,
        ..lane.clone()
    }
}

/// Splits the placement of `play` that crosses 0-based bar `bar`, if any,
/// so that both pieces play the clip steps they played before.
fn split_placements(play: &mut Vec<Placement>, bar: u32) {
    let Some(i) = play
        .iter()
        .position(|p| p.start_bar < bar && bar < p.span().end)
    else {
        return;
    };
    let p = &mut play[i];
    let k = bar - p.start_bar;
    let right = Placement {
        start_bar: bar,
        bars: p.bars - k,
        clip: p.clip.clone(),
        // At most `offset_bars + bars`, which `check` keeps in `u32`.
        offset_bars: p.offset_bars + k,
    };
    p.bars = k;
    play.insert(i + 1, right);
}

/// Splits `lane` at 0-based bar `bar` (strictly inside it) into two stretches
/// whose values match the lane's, the jump at `bar` (two points there)
/// falling between them.
fn split_lane(lane: &Lane, bar: u32) -> (Lane, Lane) {
    let k = bar - lane.start_bar;
    let at = f64::from(k);
    let points = &lane.points;
    // The engine's rule: the later point wins where two share a bar.
    let v_at = lane_value(points, at);
    let v_left = points
        .iter()
        .find(|&&(b, _)| b == at)
        .map_or(v_at, |&(_, v)| v);
    let left = points
        .iter()
        .copied()
        .filter(|&(b, _)| b < at)
        .chain([(at, v_left)])
        .collect();
    let right = [(0.0, v_at)]
        .into_iter()
        .chain(
            points
                .iter()
                .filter(|&&(b, _)| b > at)
                .map(|&(b, v)| (b - at, v)),
        )
        .collect();
    (
        Lane {
            bars: k,
            points: left,
            ..lane.clone()
        },
        Lane {
            start_bar: bar,
            bars: lane.bars - k,
            points: right,
            ..lane.clone()
        },
    )
}
