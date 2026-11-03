//! Structural edits of a song's tracks, devices and clips: add, remove or
//! rename a track, add, remove, move or rename an effect, rename a device,
//! swap an instrument, add, remove, rename or duplicate a clip, add, move,
//! resize or remove a placement, add, remove or reshape a lane stretch.
//!
//! Like the section operations, these do not validate their result (the
//! caller re-parses it, and so gets the validator's positioned messages for
//! what they leave to it, such as a `duck.depth` with no kick to follow).
//! They refuse what they cannot carry out on their own: a missing track or
//! device, an id that is taken, reserved or malformed, an unknown module, an
//! index past the chain.
//!
//! What an edit removes along with its target (a track's lane stretches, an
//! effect's, a cleared `duck_source`) is reported in [`Removed`], so the
//! caller can show it and undo can restore it.

use indexmap::IndexMap;
use tune_dsp::STRIP_PARAMS;
use tune_modules::{ClipKind, ModuleDescriptor, Registry};

use crate::arrange::{Arranged, LaneOrigin};
use crate::model::{Clip, Device, Lane, MIX_TRACK_ID, Placement, Song, Track};

/// The song an edit made, where its stretches came from, and what it
/// removed.
#[derive(Clone, Debug, PartialEq)]
pub struct Edited {
    pub arranged: Arranged,
    pub removed: Removed,
}

/// What an edit removed besides its target.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Removed {
    /// Lane stretches of a removed track or device.
    pub lanes: Vec<Lane>,
    /// Placements as `(track id, placement)`.
    pub placements: Vec<(String, Placement)>,
    /// The `mix.duck_source` a removed track was, now cleared.
    pub duck_source: Option<String>,
    /// Scene slots that went with a removed scene, in scene and slot order.
    pub slots: Vec<RemovedSlot>,
}

/// A scene slot an edit removed: the clip `track` held in `scene`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedSlot {
    pub scene: String,
    pub track: String,
    pub clip: String,
}

/// Why an edit was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EditOpError {
    #[error("track `{0}` already exists")]
    TrackExists(String),
    #[error("there is no track `{0}`")]
    NoSuchTrack(String),
    #[error("`mix` is reserved for the mix params")]
    MixReserved,
    /// `what` is `track`, `device`, `clip` or `scene`.
    #[error("a {what} id is lower-case letters, digits and `_`, got `{id}`")]
    BadId { what: &'static str, id: String },
    #[error("unknown instrument `{name}` (expected {expected})")]
    UnknownInstrument { name: String, expected: String },
    #[error("unknown effect `{name}` (expected {expected})")]
    UnknownEffect { name: String, expected: String },
    #[error("`{0}` is an effect, not an instrument")]
    NotAnInstrument(String),
    #[error("`{0}` is an instrument, not an effect")]
    NotAnEffect(String),
    #[error("track `{track}` has no device `{device}`")]
    NoSuchDevice { track: String, device: String },
    #[error("track `{track}` already has a device `{device}`")]
    DeviceExists { track: String, device: String },
    /// A chain edit named the instrument.
    #[error("`{device}` is the instrument of track `{track}`, not an effect")]
    NotAnEffectDevice { track: String, device: String },
    /// A new instrument's id is its module's prefix, and an effect has it.
    #[error("track `{track}` already has a device `{device}`; rename it first")]
    InstrumentIdTaken { track: String, device: String },
    #[error("clip `{0}` already exists")]
    ClipExists(String),
    #[error("there is no clip `{0}`")]
    NoSuchClip(String),
    #[error("unknown clip kind `{name}` (expected {expected})")]
    UnknownClipKind { name: String, expected: String },
    #[error("`send` and `duck` are reserved for the strip params")]
    StripReserved,
    /// `bar` is 0-based, as the placement edits take it; the message counts
    /// from 1.
    #[error("track `{track}` has no placement at bar {}", .bar + 1)]
    NoSuchPlacement { track: String, bar: u32 },
    /// `bar` is 0-based, as the lane edits take it; the message counts from 1.
    #[error("there is no lane `{track}.{param}` at bar {}", .bar + 1)]
    NoSuchLane {
        track: String,
        param: String,
        bar: u32,
    },
    #[error("index {index} is out of bounds (the chain has {len})")]
    IndexOutOfBounds { index: usize, len: usize },
    #[error("scene `{0}` already exists")]
    SceneExists(String),
    #[error("there is no scene `{0}`")]
    NoSuchScene(String),
    /// A launch of a slot that holds no clip.
    #[error("scene `{scene}` has no clip for track `{track}`")]
    EmptySlot { scene: String, track: String },
    /// `index` is 0-based, as the scene edits take it.
    #[error("index {index} is out of bounds (the song has {len} scenes)")]
    SceneIndexOutOfBounds { index: usize, len: usize },
}

/// Appends track `id` playing a new `module` instrument: its device id is
/// the module's prefix, gain and pan are 0, it is not muted or soloed and
/// has no effects or placements.
pub fn add_track(
    song: &Song,
    registry: &Registry,
    id: &str,
    module: &str,
) -> Result<Edited, EditOpError> {
    check_track_id(id)?;
    if song.tracks.contains_key(id) {
        return Err(EditOpError::TrackExists(id.to_owned()));
    }
    let module = instrument_module(registry, module)?;
    let mut song = song.clone();
    song.tracks.insert(
        id.to_owned(),
        Track {
            instrument: Device::new(module.prefix, module),
            fx: Vec::new(),
            strip: IndexMap::new(),
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            play: Vec::new(),
        },
    );
    Ok(untouched_lanes(song, Removed::default()))
}

/// Removes track `id`, its lane stretches (listed in `removed.lanes`) and,
/// when it is the explicit `mix.duck_source`, that setting
/// (`removed.duck_source`). Its placements go with it and are not listed; its
/// scene slots go too and are listed in `removed.slots`.
pub fn remove_track(song: &Song, id: &str) -> Result<Edited, EditOpError> {
    track_of(song, id)?;
    let mut song = song.clone();
    song.tracks.shift_remove(id);
    let mut removed = Removed::default();
    if song.mix.duck_source.as_deref() == Some(id) {
        removed.duck_source = song.mix.duck_source.take();
    }
    let kept = take_lanes(&mut song, &mut removed, |l| l.track == id);
    take_slots(&mut song, &mut removed, |track, _| track == id);
    Ok(edited(song, kept, removed))
}

/// Renames track `id` to `to`, keeping its place among the tracks and
/// following it in its lane stretches, its scene slots (the key keeps its
/// place in the scene) and an explicit `mix.duck_source`.
pub fn rename_track(song: &Song, id: &str, to: &str) -> Result<Edited, EditOpError> {
    track_of(song, id)?;
    if id == to {
        return Ok(untouched_lanes(song.clone(), Removed::default()));
    }
    check_track_id(to)?;
    if song.tracks.contains_key(to) {
        return Err(EditOpError::TrackExists(to.to_owned()));
    }
    let mut song = song.clone();
    song.tracks = std::mem::take(&mut song.tracks)
        .into_iter()
        .map(|(k, v)| if k == id { (to.to_owned(), v) } else { (k, v) })
        .collect();
    for lane in song.automation.iter_mut().filter(|l| l.track == id) {
        lane.track = to.to_owned();
    }
    for scene in song.scenes.values_mut() {
        if scene.slots.contains_key(id) {
            scene.slots = std::mem::take(&mut scene.slots)
                .into_iter()
                .map(|(k, v)| if k == id { (to.to_owned(), v) } else { (k, v) })
                .collect();
        }
    }
    if song.mix.duck_source.as_deref() == Some(id) {
        song.mix.duck_source = Some(to.to_owned());
    }
    Ok(untouched_lanes(song, Removed::default()))
}

/// Inserts a new `module` effect into the chain of `track` at `index` (the
/// end by default). Its id is `id`, else [`Track::next_device_id`].
pub fn add_effect(
    song: &Song,
    registry: &Registry,
    track: &str,
    module: &str,
    id: Option<&str>,
    index: Option<usize>,
) -> Result<Edited, EditOpError> {
    let old = track_of(song, track)?;
    let module = effect_module(registry, module)?;
    let id = match id {
        Some(id) => {
            check_device_id(id)?;
            if old.device(id).is_some() {
                return Err(device_exists(track, id));
            }
            id.to_owned()
        }
        None => old.next_device_id(module),
    };
    let len = old.fx.len();
    let index = index.unwrap_or(len);
    if index > len {
        return Err(EditOpError::IndexOutOfBounds { index, len });
    }
    let mut song = song.clone();
    song.tracks[track]
        .fx
        .insert(index, Device::new(&id, module));
    Ok(untouched_lanes(song, Removed::default()))
}

/// Removes effect `device` of `track` with its lane stretches (listed in
/// `removed.lanes`).
pub fn remove_effect(song: &Song, track: &str, device: &str) -> Result<Edited, EditOpError> {
    let at = effect_index(track_of(song, track)?, track, device)?;
    let mut song = song.clone();
    song.tracks[track].fx.remove(at);
    let mut removed = Removed::default();
    let prefix = format!("{device}.");
    let kept = take_lanes(&mut song, &mut removed, |l| {
        l.track == track && l.param.starts_with(&prefix)
    });
    Ok(edited(song, kept, removed))
}

/// Moves effect `device` of `track` so that it stands at `index` of the
/// chain (as `Vec::remove` then `insert`).
pub fn move_effect(
    song: &Song,
    track: &str,
    device: &str,
    index: usize,
) -> Result<Edited, EditOpError> {
    let old = track_of(song, track)?;
    let at = effect_index(old, track, device)?;
    let len = old.fx.len();
    if index >= len {
        return Err(EditOpError::IndexOutOfBounds { index, len });
    }
    let mut song = song.clone();
    let fx = &mut song.tracks[track].fx;
    let moved = fx.remove(at);
    fx.insert(index, moved);
    Ok(untouched_lanes(song, Removed::default()))
}

/// Renames the device `device` of `track` (the instrument or an effect) to
/// `to`; its lane stretches follow. Its param values keep their local names,
/// so the song model holds no other change.
pub fn rename_device(
    song: &Song,
    track: &str,
    device: &str,
    to: &str,
) -> Result<Edited, EditOpError> {
    let old = track_of(song, track)?;
    if old.device(device).is_none() {
        return Err(no_such_device(track, device));
    }
    if device == to {
        return Ok(untouched_lanes(song.clone(), Removed::default()));
    }
    check_device_id(to)?;
    if old.device(to).is_some() {
        return Err(device_exists(track, to));
    }
    let mut song = song.clone();
    let t = &mut song.tracks[track];
    let renamed = if t.instrument.id == device {
        &mut t.instrument
    } else {
        t.fx.iter_mut()
            .find(|d| d.id == device)
            .expect("the device exists")
    };
    renamed.id = to.to_owned();
    let prefix = format!("{device}.");
    for lane in song.automation.iter_mut().filter(|l| l.track == track) {
        if let Some(rest) = lane.param.strip_prefix(&prefix) {
            lane.param = format!("{to}.{rest}");
        }
    }
    Ok(untouched_lanes(song, Removed::default()))
}

/// Swaps the instrument of `track` for a new `module` one. The new device is
/// called after the module's prefix, with default settings and no params;
/// the old instrument's param values go (the strip's stay), and so do:
///
/// - its lane stretches (`removed.lanes`),
/// - the placements of clips the new module cannot play, `removed.placements`
///   (the clips stay in the pool),
/// - the scene slots of the track holding such clips, `removed.slots`,
/// - an explicit `mix.duck_source` naming the track, when the new module
///   plays no drum clips (`removed.duck_source`).
///
/// The same module changes nothing, whatever its id.
pub fn set_instrument(
    song: &Song,
    registry: &Registry,
    track: &str,
    module: &str,
) -> Result<Edited, EditOpError> {
    let old = track_of(song, track)?;
    let module = instrument_module(registry, module)?;
    if old.instrument.module.name == module.name {
        return Ok(untouched_lanes(song.clone(), Removed::default()));
    }
    if old.fx.iter().any(|d| d.id == module.prefix) {
        return Err(EditOpError::InstrumentIdTaken {
            track: track.to_owned(),
            device: module.prefix.to_owned(),
        });
    }
    let old_prefix = format!("{}.", old.instrument.id);
    let mut song = song.clone();
    let mut removed = Removed::default();
    let t = &mut song.tracks[track];
    t.instrument = Device::new(module.prefix, module);
    let (played, gone): (Vec<Placement>, Vec<Placement>) =
        std::mem::take(&mut t.play).into_iter().partition(|p| {
            song.clips
                .get(&p.clip)
                .is_none_or(|clip| module.plays(clip.kind()))
        });
    song.tracks[track].play = played;
    removed.placements = gone.into_iter().map(|p| (track.to_owned(), p)).collect();
    let cannot: Vec<String> = song
        .clips
        .iter()
        .filter(|(_, clip)| !module.plays(clip.kind()))
        .map(|(id, _)| id.clone())
        .collect();
    take_slots(&mut song, &mut removed, |t, clip| {
        t == track && cannot.iter().any(|c| c == clip)
    });
    if song.mix.duck_source.as_deref() == Some(track) && !module.plays(ClipKind::Drum) {
        removed.duck_source = song.mix.duck_source.take();
    }
    let kept = take_lanes(&mut song, &mut removed, |l| {
        l.track == track && l.param.starts_with(&old_prefix)
    });
    Ok(edited(song, kept, removed))
}

/// Appends clip `id` to the pool. The clip is built by the caller, so it is
/// valid; its id is checked here.
pub fn add_clip(song: &Song, id: &str, clip: Clip) -> Result<Edited, EditOpError> {
    check_new_clip(song, id)?;
    let mut song = song.clone();
    song.clips.insert(id.to_owned(), clip);
    Ok(untouched_lanes(song, Removed::default()))
}

/// Removes clip `id` and every placement of it (listed in
/// `removed.placements`, track by track in song order) and every scene slot
/// holding it (`removed.slots`).
pub fn remove_clip(song: &Song, id: &str) -> Result<Edited, EditOpError> {
    clip_of(song, id)?;
    let mut song = song.clone();
    song.clips.shift_remove(id);
    let mut removed = Removed::default();
    for (track_id, track) in &mut song.tracks {
        let (gone, kept): (Vec<Placement>, Vec<Placement>) = std::mem::take(&mut track.play)
            .into_iter()
            .partition(|p| p.clip == id);
        track.play = kept;
        removed
            .placements
            .extend(gone.into_iter().map(|p| (track_id.clone(), p)));
    }
    take_slots(&mut song, &mut removed, |_, clip| clip == id);
    Ok(untouched_lanes(song, removed))
}

/// Renames clip `id` to `to`, keeping its place in the pool and following
/// it in every placement and scene slot.
pub fn rename_clip(song: &Song, id: &str, to: &str) -> Result<Edited, EditOpError> {
    clip_of(song, id)?;
    if id == to {
        return Ok(untouched_lanes(song.clone(), Removed::default()));
    }
    check_new_clip(song, to)?;
    let mut song = song.clone();
    song.clips = std::mem::take(&mut song.clips)
        .into_iter()
        .map(|(k, v)| if k == id { (to.to_owned(), v) } else { (k, v) })
        .collect();
    for placement in song.tracks.values_mut().flat_map(|t| &mut t.play) {
        if placement.clip == id {
            placement.clip = to.to_owned();
        }
    }
    for clip in song.scenes.values_mut().flat_map(|s| s.slots.values_mut()) {
        if clip == id {
            *clip = to.to_owned();
        }
    }
    Ok(untouched_lanes(song, Removed::default()))
}

/// Copies clip `id` as `to`, which stands right after it in the pool.
pub fn duplicate_clip(song: &Song, id: &str, to: &str) -> Result<Edited, EditOpError> {
    let at = song
        .clips
        .get_index_of(id)
        .ok_or_else(|| EditOpError::NoSuchClip(id.to_owned()))?;
    check_new_clip(song, to)?;
    let mut song = song.clone();
    let copy = song.clips[at].clone();
    song.clips.shift_insert(at + 1, to.to_owned(), copy);
    Ok(untouched_lanes(song, Removed::default()))
}

/// Adds `placement` to `track`'s `play`, before the first one that starts
/// later, so the list stays in time order. Whether it overlaps another, runs
/// past the song's end or names a clip the track cannot play is for the
/// validator.
pub fn add_placement(
    song: &Song,
    track: &str,
    placement: Placement,
) -> Result<Edited, EditOpError> {
    track_of(song, track)?;
    let mut song = song.clone();
    insert_placement(&mut song.tracks[track].play, placement);
    Ok(untouched_lanes(song, Removed::default()))
}

/// Moves the placement of `track` that starts at 0-based bar `start_bar` to
/// start at `to`, keeping its length and clip phase. The list is put back in
/// time order.
pub fn move_placement(
    song: &Song,
    track: &str,
    start_bar: u32,
    to: u32,
) -> Result<Edited, EditOpError> {
    let mut song = song.clone();
    let play = &mut song
        .tracks
        .get_mut(track)
        .ok_or_else(|| no_track(track))?
        .play;
    let at = placement_at(play, track, start_bar)?;
    let mut moved = play.remove(at);
    moved.start_bar = to;
    insert_placement(play, moved);
    Ok(untouched_lanes(song, Removed::default()))
}

/// Makes the placement of `track` that starts at 0-based bar `start_bar`
/// `bars` long. Whether `bars` is valid is for the validator.
pub fn resize_placement(
    song: &Song,
    track: &str,
    start_bar: u32,
    bars: u32,
) -> Result<Edited, EditOpError> {
    let mut song = song.clone();
    let play = &mut song
        .tracks
        .get_mut(track)
        .ok_or_else(|| no_track(track))?
        .play;
    let at = placement_at(play, track, start_bar)?;
    play[at].bars = bars;
    Ok(untouched_lanes(song, Removed::default()))
}

/// Removes the placement of `track` that starts at 0-based bar `start_bar`,
/// and lists it in `removed.placements`.
pub fn remove_placement(song: &Song, track: &str, start_bar: u32) -> Result<Edited, EditOpError> {
    let mut song = song.clone();
    let play = &mut song
        .tracks
        .get_mut(track)
        .ok_or_else(|| no_track(track))?
        .play;
    let at = placement_at(play, track, start_bar)?;
    let gone = play.remove(at);
    let removed = Removed {
        placements: vec![(track.to_owned(), gone)],
        ..Removed::default()
    };
    Ok(untouched_lanes(song, removed))
}

/// Adds stretch `lane`, after the stretches that start in the same section
/// or earlier (the order the file keeps them in). It is a new stretch with
/// no old table. Overlaps, params that do not exist and points out of range
/// are for the validator.
pub fn add_lane(song: &Song, lane: Lane) -> Result<Edited, EditOpError> {
    let mut song = song.clone();
    let mut lanes: Vec<(Lane, LaneOrigin)> = std::mem::take(&mut song.automation)
        .into_iter()
        .enumerate()
        .map(|(i, l)| (l, LaneOrigin::Kept(i)))
        .collect();
    lanes.push((lane, LaneOrigin::New));
    let starts = song.section_start_bars();
    lanes.sort_by_key(|(l, _)| starts.partition_point(|&s| s <= l.start_bar));
    let (automation, origins) = lanes.into_iter().unzip();
    song.automation = automation;
    Ok(Edited {
        arranged: Arranged {
            song,
            lanes: origins,
        },
        removed: Removed::default(),
    })
}

/// Removes the stretch of `track`'s `param` that starts at 0-based bar
/// `start_bar`, and lists it in `removed.lanes`. `track` is `mix` for a mix
/// param.
pub fn remove_lane(
    song: &Song,
    track: &str,
    param: &str,
    start_bar: u32,
) -> Result<Edited, EditOpError> {
    let at = lane_at(song, track, param, start_bar)?;
    let mut song = song.clone();
    let mut removed = Removed::default();
    // A valid song has no two equal stretches (they would overlap).
    let gone = song.automation[at].clone();
    let kept = take_lanes(&mut song, &mut removed, |l| *l == gone);
    Ok(edited(song, kept, removed))
}

/// Replaces the points of the stretch of `track`'s `param` that starts at
/// 0-based bar `start_bar`. Whether the points are in range is for the
/// validator.
pub fn set_lane_points(
    song: &Song,
    track: &str,
    param: &str,
    start_bar: u32,
    points: Vec<(f64, f64)>,
) -> Result<Edited, EditOpError> {
    let at = lane_at(song, track, param, start_bar)?;
    let mut song = song.clone();
    song.automation[at].points = points;
    Ok(untouched_lanes(song, Removed::default()))
}

/// The index of the stretch of `param` that starts at `start_bar`.
fn lane_at(song: &Song, track: &str, param: &str, start_bar: u32) -> Result<usize, EditOpError> {
    song.automation
        .iter()
        .position(|l| l.track == track && l.param == param && l.start_bar == start_bar)
        .ok_or_else(|| EditOpError::NoSuchLane {
            track: track.to_owned(),
            param: param.to_owned(),
            bar: start_bar,
        })
}

fn no_track(id: &str) -> EditOpError {
    EditOpError::NoSuchTrack(id.to_owned())
}

/// Puts `placement` before the first one that starts later.
fn insert_placement(play: &mut Vec<Placement>, placement: Placement) {
    let at = play.partition_point(|p| p.start_bar <= placement.start_bar);
    play.insert(at, placement);
}

/// The index of the placement that starts at 0-based bar `start_bar`.
fn placement_at(play: &[Placement], track: &str, start_bar: u32) -> Result<usize, EditOpError> {
    play.iter()
        .position(|p| p.start_bar == start_bar)
        .ok_or_else(|| EditOpError::NoSuchPlacement {
            track: track.to_owned(),
            bar: start_bar,
        })
}

pub(crate) fn clip_of<'s>(song: &'s Song, id: &str) -> Result<&'s Clip, EditOpError> {
    song.clips
        .get(id)
        .ok_or_else(|| EditOpError::NoSuchClip(id.to_owned()))
}

/// Checks that `id` is a well-formed clip id nobody has. The existing clip
/// is reported first, so a duplicate id reads as that.
pub fn check_new_clip(song: &Song, id: &str) -> Result<(), EditOpError> {
    if song.clips.contains_key(id) {
        return Err(EditOpError::ClipExists(id.to_owned()));
    }
    if !well_formed(id) {
        return Err(EditOpError::BadId {
            what: "clip",
            id: id.to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn track_of<'s>(song: &'s Song, id: &str) -> Result<&'s Track, EditOpError> {
    song.tracks
        .get(id)
        .ok_or_else(|| EditOpError::NoSuchTrack(id.to_owned()))
}

/// The position of effect `device` in `track`'s chain.
fn effect_index(track: &Track, id: &str, device: &str) -> Result<usize, EditOpError> {
    match track.fx.iter().position(|d| d.id == device) {
        Some(at) => Ok(at),
        None if track.instrument.id == device => Err(EditOpError::NotAnEffectDevice {
            track: id.to_owned(),
            device: device.to_owned(),
        }),
        None => Err(no_such_device(id, device)),
    }
}

fn no_such_device(track: &str, device: &str) -> EditOpError {
    EditOpError::NoSuchDevice {
        track: track.to_owned(),
        device: device.to_owned(),
    }
}

fn device_exists(track: &str, device: &str) -> EditOpError {
    EditOpError::DeviceExists {
        track: track.to_owned(),
        device: device.to_owned(),
    }
}

/// Whether `id` is lower-case letters, digits and `_`, as the validator
/// wants of a track, device or clip id.
pub(crate) fn well_formed(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn check_track_id(id: &str) -> Result<(), EditOpError> {
    if !well_formed(id) {
        return Err(EditOpError::BadId {
            what: "track",
            id: id.to_owned(),
        });
    }
    if id == MIX_TRACK_ID {
        return Err(EditOpError::MixReserved);
    }
    Ok(())
}

fn check_device_id(id: &str) -> Result<(), EditOpError> {
    if !well_formed(id) {
        return Err(EditOpError::BadId {
            what: "device",
            id: id.to_owned(),
        });
    }
    let strip = STRIP_PARAMS
        .iter()
        .any(|s| s.id.split_once('.').is_some_and(|(prefix, _)| prefix == id));
    if strip {
        return Err(EditOpError::StripReserved);
    }
    Ok(())
}

fn instrument_module(
    registry: &Registry,
    name: &str,
) -> Result<&'static ModuleDescriptor, EditOpError> {
    match registry.get(name) {
        Some(m) if m.is_instrument() => Ok(m),
        Some(m) => Err(EditOpError::NotAnInstrument(m.name.to_owned())),
        None => Err(EditOpError::UnknownInstrument {
            name: name.to_owned(),
            expected: one_of(registry.instruments()),
        }),
    }
}

fn effect_module(
    registry: &Registry,
    name: &str,
) -> Result<&'static ModuleDescriptor, EditOpError> {
    match registry.get(name) {
        Some(m) if !m.is_instrument() => Ok(m),
        Some(m) => Err(EditOpError::NotAnEffect(m.name.to_owned())),
        None => Err(EditOpError::UnknownEffect {
            name: name.to_owned(),
            expected: one_of(registry.effects()),
        }),
    }
}

/// `a`, `a or b`, `a, b or c`.
fn one_of(modules: impl Iterator<Item = &'static ModuleDescriptor>) -> String {
    let names: Vec<&str> = modules.map(|m| m.name).collect();
    match names.split_last() {
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// Moves the lane stretches `gone` selects into `removed.lanes`; returns the
/// old index of each one left, in order.
fn take_lanes(song: &mut Song, removed: &mut Removed, gone: impl Fn(&Lane) -> bool) -> Vec<usize> {
    let mut kept = Vec::new();
    let mut lanes = Vec::new();
    for (i, lane) in std::mem::take(&mut song.automation).into_iter().enumerate() {
        if gone(&lane) {
            removed.lanes.push(lane);
        } else {
            kept.push(i);
            lanes.push(lane);
        }
    }
    song.automation = lanes;
    kept
}

/// Moves the scene slots `gone` selects (given a slot's track and clip) into
/// `removed.slots`, in scene order and then the slot's order.
fn take_slots(song: &mut Song, removed: &mut Removed, gone: impl Fn(&str, &str) -> bool) {
    for (scene_id, scene) in &mut song.scenes {
        let slots = std::mem::take(&mut scene.slots);
        for (track, clip) in slots {
            if gone(&track, &clip) {
                removed.slots.push(RemovedSlot {
                    scene: scene_id.clone(),
                    track,
                    clip,
                });
            } else {
                scene.slots.insert(track, clip);
            }
        }
    }
}

/// `song` with every stretch still the old one, in the old order.
pub(crate) fn untouched_lanes(song: Song, removed: Removed) -> Edited {
    let kept = (0..song.automation.len()).collect();
    edited(song, kept, removed)
}

/// `song`, whose stretches are the old ones at the indices `kept`.
fn edited(song: Song, kept: Vec<usize>, removed: Removed) -> Edited {
    Edited {
        arranged: Arranged {
            song,
            lanes: kept.into_iter().map(LaneOrigin::Kept).collect(),
        },
        removed,
    }
}
