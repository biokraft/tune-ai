//! A validated [`Song`] compiled into the immutable form the engine plays:
//! every id resolved to an index, every param table filled in, every clip
//! turned into engine cells and the arrangement cut into regions.

use tune_dsp::acid303::AcidStep;
use tune_dsp::acid303::math::db2amp;
use tune_dsp::{DrumHit, ParamSpec, STRIP_PARAMS, find_spec};
use tune_modules::{ACCENT_VELOCITY, ModuleDescriptor, NoteEvent, NoteId};
use tune_song::{
    Clip, Device, MIX_TRACK_ID, MasterMode, MixUnit, NoteClip, Song, TICKS_PER_STEP, Track,
    find_mix_param,
};

use crate::clock::{Clock, track_seed};
use crate::mixer::pan_gains;

/// One step of a track's clip, as the engine plays it: the engine turns
/// each into note events for the track's instrument.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Rest,
    /// A drum hit.
    Hit {
        accent: bool,
    },
    /// An acid step that holds a note.
    Note(AcidStep),
}

/// The id bit of a note-clip note: its [`NoteId`] is `NOTE_CLIP_IDS | index`,
/// the note's position in its clip, which no step note's id (below 2^31)
/// shares.
pub const NOTE_CLIP_IDS: u32 = 0x8000_0000;

/// One clip as the engine plays it.
#[derive(Clone, Debug, PartialEq)]
pub enum CompiledClip {
    /// An acid or drum clip: one cell per step.
    Steps(Vec<Cell>),
    /// A notes clip: its note events.
    Notes(NoteClipEvents),
}

/// A notes clip compiled to its events, in the order the engine sends them.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteClipEvents {
    /// The clip's id in the song file.
    pub id: String,
    /// The clip's length in 16th steps.
    pub steps: u64,
    /// Sorted by tick, then Offs before Ons, then note index.
    pub events: Vec<ClipEvent>,
    /// `steps + 1` indices into `events`: entry `s` is the first event with
    /// `tick >= 24 * s` (the last entry is `events.len()`).
    pub step_starts: Vec<usize>,
}

/// One event of a notes clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipEvent {
    /// Ticks from the clip start, `0 .. 24 * steps`.
    pub tick: u32,
    pub event: NoteEvent,
}

impl NoteClipEvents {
    /// Compiles `clip`, called `id`. Note `i` is an On at its start and an
    /// Off at its end (an Off at the clip's end moves to tick 0, where the
    /// loop wraps); its On slides when a later note starts inside it.
    pub fn new(id: &str, clip: &NoteClip) -> Self {
        let steps = u64::from(clip.steps());
        let clip_end = clip.steps() * TICKS_PER_STEP;
        // (tick, is_on, note index, event)
        let mut events: Vec<(u32, bool, usize, NoteEvent)> = Vec::new();
        for (i, n) in clip.notes.iter().enumerate() {
            let id = NoteId(NOTE_CLIP_IDS | u32::try_from(i).expect("at most 2^31 notes"));
            let slide = clip.notes[i + 1..]
                .iter()
                .any(|later| later.start > n.start && later.start < n.end());
            events.push((
                n.start,
                true,
                i,
                NoteEvent::On {
                    id,
                    pitch: n.pitch,
                    velocity: n.velocity,
                    accent: n.velocity >= ACCENT_VELOCITY,
                    slide,
                },
            ));
            let off = if n.end() >= clip_end { 0 } else { n.end() };
            events.push((off, false, i, NoteEvent::Off { id, pitch: n.pitch }));
        }
        events.sort_by_key(|&(tick, on, i, _)| (tick, on, i));
        let events: Vec<ClipEvent> = events
            .into_iter()
            .map(|(tick, _, _, event)| ClipEvent { tick, event })
            .collect();
        let step_starts = (0..=clip.steps())
            .map(|s| events.partition_point(|e| e.tick < s * TICKS_PER_STEP))
            .collect();
        Self {
            id: id.to_owned(),
            steps,
            events,
            step_starts,
        }
    }
}

/// A song ready to play. Tracks, clips and sections keep song-file order.
#[derive(Clone, Debug)]
pub struct CompiledSong {
    pub name: String,
    pub clock: Clock,
    pub total_bars: u32,
    pub tracks: Vec<CompiledTrack>,
    /// One compiled clip per `Song::clips` entry, in the same order.
    pub clips: Vec<CompiledClip>,
    /// The named sections, for bar maps; the engine plays `regions`.
    pub sections: Vec<CompiledSection>,
    /// The arrangement cut wherever a section, a placement or a lane
    /// stretch starts or ends: in time order, the first at bar 0, together
    /// covering the whole song.
    pub regions: Vec<CompiledRegion>,
    pub mix: CompiledMix,
}

/// The `track` of a lane (or a knob override) aimed at the mix: a mix param
/// rather than one of a track's.
pub const MIX_TRACK: usize = usize::MAX;

/// The `[mix]` table resolved: what the master stage does, whom the duckers
/// follow and the param tables of the buses the song needs.
#[derive(Clone, Debug)]
pub struct CompiledMix {
    pub master: MasterMode,
    /// Track index of `Song::duck_source`.
    pub duck_source: Option<usize>,
    /// The full delay param table in plain units, defaults filled in; `None`
    /// when no track sends to the delay (`Song::bus_active`).
    pub delay: Option<Vec<f64>>,
    /// The full reverb param table, like `delay`.
    pub reverb: Option<Vec<f64>>,
    /// The full master param table in plain units, defaults filled in.
    pub master_params: Vec<f64>,
}

/// One mixer channel.
#[derive(Clone, Debug)]
pub struct CompiledTrack {
    pub id: String,
    pub instrument: CompiledDevice,
    /// The full strip param table (`STRIP_PARAMS`: sends and ducking) in
    /// plain units, defaults filled in.
    pub strip: Vec<f64>,
    /// The fx chain in slot order.
    pub fx: Vec<CompiledDevice>,
    /// Linear gain (`db2amp(gain_db)`).
    pub gain: f64,
    /// Constant-power `(left, right)` gains of the track's pan.
    pub pan_gains: (f64, f64),
    pub mute: bool,
    pub solo: bool,
    /// `track_seed(song seed, id)`.
    pub seed: u64,
}

/// One device: the instrument or an fx slot.
#[derive(Clone, Debug)]
pub struct CompiledDevice {
    /// Unique within the track; the prefix of its param ids.
    pub id: String,
    pub module: &'static ModuleDescriptor,
    /// Every setting, in `module.settings` order: what the factory takes.
    pub settings: Vec<&'static str>,
    /// The full param table (`module.params`) in plain units, defaults
    /// filled in.
    pub params: Vec<f64>,
}

impl CompiledDevice {
    fn new(device: &Device) -> Self {
        let module = device.module;
        let mut params: Vec<f64> = module.params.iter().map(|s| s.default).collect();
        for (local, &v) in &device.params {
            let (index, _) = module.param(local).unwrap_or_else(|| {
                panic!(
                    "unknown param `{local}` of `{}` in a validated song",
                    module.name
                )
            });
            params[index] = v;
        }
        Self {
            id: device.id.clone(),
            module,
            settings: device.settings.clone(),
            params,
        }
    }

    /// Resolves the param with local name `local`.
    fn param(&self, local: &str) -> Option<(usize, &'static ParamSpec)> {
        self.module.param(local)
    }
}

/// A named stretch of the arrangement.
#[derive(Clone, Debug)]
pub struct CompiledSection {
    pub name: String,
    pub start_bar: u32,
    pub bars: u32,
}

/// Where a track's cells come from in a region: the placement playing
/// there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellSource {
    /// Index into `CompiledSong::clips`.
    pub clip: usize,
    /// The placement's first step (`16 * start_bar`).
    pub start_step: u64,
    /// Steps into the clip at the placement's first step
    /// (`16 * offset_bars`).
    pub offset_steps: u64,
}

/// A stretch of bars inside which no section starts and no placement or
/// lane stretch starts or ends; it runs up to the next region's
/// `start_bar` (or the song's end).
#[derive(Clone, Debug)]
pub struct CompiledRegion {
    pub start_bar: u32,
    /// Per track (indexed like `CompiledSong::tracks`); `None` when no
    /// placement of the track covers the region.
    pub cells: Vec<Option<CellSource>>,
    /// The lane stretches covering the region, in `Song::automation` order.
    pub lanes: Vec<CompiledLane>,
}

/// A lane stretch with its target resolved.
#[derive(Clone, Debug)]
pub struct CompiledLane {
    pub track: usize,
    pub target: ParamTarget,
    pub spec: &'static ParamSpec,
    pub start_bar: u32,
    /// `clock.bar_start(start_bar)`.
    pub start_sample: u64,
    /// `(bar offset from start_bar, normalized value)`, bars never
    /// decreasing.
    pub points: Vec<(f64, f64)>,
}

/// Which parameter of a track (or of the mix) a lane or a knob override
/// drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamTarget {
    /// Index into the instrument's param table.
    Instrument(usize),
    /// Index into the param table of fx `slot`.
    Fx { slot: usize, index: usize },
    /// Index into the track's strip table (`STRIP_PARAMS`).
    Strip(usize),
    /// Index into the param table of a mix unit; the track is [`MIX_TRACK`].
    Mix { unit: MixUnit, index: usize },
}

impl CompiledSong {
    /// Compiles `song`, which must come from `tune_song::parse_song` (or be
    /// otherwise valid). Panics on a dangling reference, which validation
    /// rules out.
    pub fn new(song: &Song, sample_rate: u32) -> Self {
        let tracks: Vec<CompiledTrack> = song
            .tracks
            .iter()
            .map(|(id, t)| compile_track(id, t, song.seed))
            .collect();

        let clips = song
            .clips
            .iter()
            .map(|(id, clip)| match clip {
                Clip::Acid(acid) => CompiledClip::Steps(
                    acid.steps()
                        .iter()
                        .map(|s| match s.note {
                            Some(_) => Cell::Note(s.clone()),
                            None => Cell::Rest,
                        })
                        .collect(),
                ),
                Clip::Drum(drum) => CompiledClip::Steps(
                    drum.hits()
                        .iter()
                        .map(|h| match h {
                            DrumHit::Rest => Cell::Rest,
                            DrumHit::Hit => Cell::Hit { accent: false },
                            DrumHit::Accent => Cell::Hit { accent: true },
                        })
                        .collect(),
                ),
                Clip::Notes(notes) => CompiledClip::Notes(NoteClipEvents::new(id, notes)),
            })
            .collect();

        let track_index = |id: &str| {
            song.tracks
                .get_index_of(id)
                .unwrap_or_else(|| panic!("unknown track `{id}` in a validated song"))
        };
        let clock = Clock::new(song.bpm, sample_rate);
        let starts = song.section_start_bars();
        let sections = song
            .sections
            .iter()
            .zip(&starts)
            .map(|(s, &start_bar)| CompiledSection {
                name: s.name.clone(),
                start_bar,
                bars: s.bars,
            })
            .collect();
        let lanes: Vec<CompiledLane> = song
            .automation
            .iter()
            .map(|lane| {
                let unknown = || panic!("unknown lane param `{}` in a validated song", lane.param);
                let (track, target, spec) = if lane.track == MIX_TRACK_ID {
                    let (target, spec) = resolve_mix(&lane.param).unwrap_or_else(unknown);
                    (MIX_TRACK, target, spec)
                } else {
                    let track = track_index(&lane.track);
                    let (target, spec) =
                        resolve(&tracks[track], &lane.param).unwrap_or_else(unknown);
                    (track, target, spec)
                };
                CompiledLane {
                    track,
                    target,
                    spec,
                    start_bar: lane.start_bar,
                    start_sample: clock.bar_start(lane.start_bar),
                    points: lane.points.clone(),
                }
            })
            .collect();
        let regions = regions(song, &starts, &lanes);

        let mix = compile_mix(song, &track_index);
        Self {
            name: song.name.clone(),
            clock,
            total_bars: song.total_bars(),
            tracks,
            clips,
            sections,
            regions,
            mix,
        }
    }

    /// Index of the track with `id`.
    pub fn track_index(&self, id: &str) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    /// Resolves `param_id` (`<device id>.<param>`, or a strip id) on track
    /// `track`: the instrument, the strip, then each fx in slot order. `None`
    /// for an unknown track or id.
    pub fn param_target(
        &self,
        track: usize,
        param_id: &str,
    ) -> Option<(ParamTarget, &'static ParamSpec)> {
        resolve(self.tracks.get(track)?, param_id)
    }

    /// Resolves `param_id` on the track with id `track_id`, which may be
    /// `"mix"` for a mix param: `(track index or MIX_TRACK, target, spec)`.
    /// `None` for an unknown track or id.
    pub fn resolve_param(
        &self,
        track_id: &str,
        param_id: &str,
    ) -> Option<(usize, ParamTarget, &'static ParamSpec)> {
        if track_id == MIX_TRACK_ID {
            let (target, spec) = resolve_mix(param_id)?;
            return Some((MIX_TRACK, target, spec));
        }
        let track = self.track_index(track_id)?;
        let (target, spec) = self.param_target(track, param_id)?;
        Some((track, target, spec))
    }

    /// The sample after the last one of the song.
    pub fn end_sample(&self) -> u64 {
        self.clock.bar_start(self.total_bars)
    }
}

/// Cuts the arrangement of `song` (its sections starting at `section_starts`)
/// into regions: bounds at bar 0, the end, every section start and every
/// start and end of a placement or lane stretch, clipped to the song.
/// `lanes` are `song.automation` compiled, in the same order.
fn regions(song: &Song, section_starts: &[u32], lanes: &[CompiledLane]) -> Vec<CompiledRegion> {
    let total = song.total_bars();
    let placements = song.tracks.values().flat_map(|t| &t.play);
    let mut bounds: Vec<u32> = [0, total]
        .into_iter()
        .chain(section_starts.iter().copied())
        .chain(placements.flat_map(|p| [p.start_bar, p.start_bar + p.bars]))
        .chain(
            song.automation
                .iter()
                .flat_map(|l| [l.start_bar, l.start_bar + l.bars]),
        )
        .map(|b| b.min(total))
        .collect();
    bounds.sort_unstable();
    bounds.dedup();
    // Every bound but the end starts a region.
    bounds.pop();
    bounds
        .into_iter()
        .map(|a| {
            let cells = song
                .tracks
                .values()
                .map(|t| {
                    let p = t.play.iter().find(|p| p.span().contains(&a))?;
                    let clip = song
                        .clips
                        .get_index_of(&p.clip)
                        .unwrap_or_else(|| panic!("unknown clip `{}`", p.clip));
                    Some(CellSource {
                        clip,
                        start_step: 16 * u64::from(p.start_bar),
                        offset_steps: 16 * u64::from(p.offset_bars),
                    })
                })
                .collect();
            let lanes = song
                .automation
                .iter()
                .zip(lanes)
                .filter(|(l, _)| l.span().contains(&a))
                .map(|(_, compiled)| compiled.clone())
                .collect();
            CompiledRegion {
                start_bar: a,
                cells,
                lanes,
            }
        })
        .collect()
}

/// Track `id` of a song seeded with `song_seed`, every table filled in.
fn compile_track(id: &str, t: &Track, song_seed: u64) -> CompiledTrack {
    CompiledTrack {
        id: id.to_owned(),
        instrument: CompiledDevice::new(&t.instrument),
        strip: fill(STRIP_PARAMS, &t.strip),
        fx: t.fx.iter().map(CompiledDevice::new).collect(),
        gain: db2amp(t.gain_db),
        pan_gains: pan_gains(t.pan),
        mute: t.mute,
        solo: t.solo,
        seed: track_seed(song_seed, id),
    }
}

/// The full `table` in plain units, with the explicit `values` (by full id)
/// set.
fn fill<'a>(
    table: &'static [ParamSpec],
    values: impl IntoIterator<Item = (&'a String, &'a f64)>,
) -> Vec<f64> {
    let mut params: Vec<f64> = table.iter().map(|s| s.default).collect();
    for (id, &v) in values {
        let (index, _) = find_spec(table, id)
            .unwrap_or_else(|_| panic!("unknown param `{id}` in a validated song"));
        params[index] = v;
    }
    params
}

/// Resolves a track-relative param id. It splits at the first `.`: the
/// instrument's id gives its param, a strip id (`send.delay`) the strip's,
/// and an fx device's id that device's param.
fn resolve(track: &CompiledTrack, id: &str) -> Option<(ParamTarget, &'static ParamSpec)> {
    let (device, local) = id.split_once('.')?;
    if device == track.instrument.id {
        let (index, spec) = track.instrument.param(local)?;
        return Some((ParamTarget::Instrument(index), spec));
    }
    if let Ok((index, spec)) = find_spec(STRIP_PARAMS, id) {
        return Some((ParamTarget::Strip(index), spec));
    }
    let slot = track.fx.iter().position(|fx| fx.id == device)?;
    let (index, spec) = track.fx[slot].param(local)?;
    Some((ParamTarget::Fx { slot, index }, spec))
}

/// Resolves a mix param id (`reverb.size`, `master.gain`).
fn resolve_mix(id: &str) -> Option<(ParamTarget, &'static ParamSpec)> {
    find_mix_param(id).map(|(unit, index, spec)| (ParamTarget::Mix { unit, index }, spec))
}

/// The `[mix]` table of `song` resolved; `track_index` maps a track id to
/// its index.
fn compile_mix(song: &Song, track_index: &impl Fn(&str) -> usize) -> CompiledMix {
    let table = |unit: MixUnit| {
        let mut params: Vec<f64> = unit.params().iter().map(|s| s.default).collect();
        for (id, &v) in &song.mix.params {
            let (u, index, _) = find_mix_param(id)
                .unwrap_or_else(|| panic!("unknown mix param `{id}` in a validated song"));
            if u == unit {
                params[index] = v;
            }
        }
        params
    };
    let bus = |unit: MixUnit| song.bus_active(unit).then(|| table(unit));
    CompiledMix {
        master: song.mix.master,
        duck_source: song.duck_source().map(track_index),
        delay: bus(MixUnit::Delay),
        reverb: bus(MixUnit::Reverb),
        master_params: table(MixUnit::Master),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tune_dsp::DIST_PARAMS;
    use tune_dsp::acid303::ACID303_PARAMS;
    use tune_song::Note;

    const MINI: &str = include_str!("../../tune-song/tests/fixtures/mini.toml");

    fn mini() -> CompiledSong {
        CompiledSong::new(&tune_song::parse_song(MINI).unwrap(), 48_000)
    }

    #[test]
    fn header_and_tracks() {
        let s = mini();
        assert_eq!(s.name, "mini");
        assert_eq!(s.clock, Clock::new(120.0, 48_000));
        assert_eq!(s.total_bars, 4);
        assert_eq!(s.end_sample(), 384_000);
        let ids: Vec<_> = s.tracks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["kick", "acid", "hat"]);
        assert_eq!(s.track_index("hat"), Some(2));
        assert_eq!(s.track_index("nope"), None);

        let kick = &s.tracks[0];
        assert_eq!(kick.instrument.module.name, "kick");
        assert_eq!(kick.instrument.id, "kick");
        assert_eq!(kick.gain, db2amp(-3.0));
        assert_eq!(kick.pan_gains, pan_gains(0.0));
        assert_eq!(kick.seed, track_seed(7, "kick"));
        assert!(kick.fx.is_empty());

        let acid = &s.tracks[1];
        let params = &acid.instrument.params;
        assert_eq!(params.len(), ACID303_PARAMS.len());
        assert_eq!(params[2], 500.0);
        let set = [
            "acid303.cutoff",
            "acid303.resonance",
            "acid303.env_mod",
            "acid303.decay",
            "acid303.accent",
        ];
        for (i, spec) in ACID303_PARAMS.iter().enumerate() {
            if !set.contains(&spec.id) {
                assert_eq!(params[i], spec.default, "{}", spec.id);
            }
        }
        assert_eq!(acid.gain, 1.0);
        assert_eq!(acid.fx.len(), 1);
        assert_eq!(acid.fx[0].id, "dist");
        assert_eq!(acid.fx[0].settings, ["soft"]);
        assert_eq!(acid.fx[0].params, vec![12.0, 1.0, -6.0]);
        assert_eq!(acid.fx[0].params.len(), DIST_PARAMS.len());

        let hat = &s.tracks[2];
        assert_eq!(hat.pan_gains, pan_gains(0.3));
        assert!(!hat.mute && !hat.solo);
    }

    #[test]
    fn resolve_param_by_device_id() {
        const MIXED: &str = include_str!("../../tune-song/tests/fixtures/mixed.toml");
        let s = CompiledSong::new(&tune_song::parse_song(MIXED).unwrap(), 48_000);
        let target = |param: &str| {
            s.resolve_param("acid", param)
                .map(|(t, target, _)| (t, target))
        };
        assert_eq!(
            target("dist.drive"),
            Some((1, ParamTarget::Fx { slot: 0, index: 0 }))
        );
        assert_eq!(
            target("eq.low_cut"),
            Some((1, ParamTarget::Fx { slot: 1, index: 0 }))
        );
        assert_eq!(
            target("acid303.cutoff"),
            Some((1, ParamTarget::Instrument(2)))
        );
        assert_eq!(target("send.delay"), Some((1, ParamTarget::Strip(0))));
        assert_eq!(target("eq.nope"), None);
        assert_eq!(target("kick.tune"), None);
    }

    #[test]
    fn clips_become_cells() {
        let s = mini();
        assert_eq!(s.clips.len(), 3);
        let steps = |i: usize| match &s.clips[i] {
            CompiledClip::Steps(cells) => cells,
            other => panic!("not a step clip: {other:?}"),
        };
        let k4 = steps(0);
        assert_eq!(k4.len(), 16);
        assert_eq!(k4[0], Cell::Hit { accent: false });
        assert_eq!(k4[1], Cell::Rest);
        assert_eq!(k4[12], Cell::Hit { accent: true });
        let a1 = steps(1);
        assert_eq!(a1.len(), 8);
        assert!(matches!(&a1[1], Cell::Note(n) if n.note == Some(36) && n.accent));
        assert!(matches!(&a1[2], Cell::Note(n) if n.note == Some(48) && n.slide));
        assert_eq!(a1[4], Cell::Rest);
    }

    #[test]
    fn sections_keep_names_and_bars() {
        let s = mini();
        let sections: Vec<_> = s
            .sections
            .iter()
            .map(|x| (x.name.as_str(), x.start_bar, x.bars))
            .collect();
        assert_eq!(sections, [("a", 0, 2), ("b", 2, 2)]);
    }

    #[test]
    fn regions_resolve_cells_and_lanes() {
        let s = mini();
        assert_eq!(s.regions.len(), 2);
        let cell = |clip, start_step| {
            Some(CellSource {
                clip,
                start_step,
                offset_steps: 0,
            })
        };
        let (a, b) = (&s.regions[0], &s.regions[1]);
        assert_eq!(a.start_bar, 0);
        assert_eq!(a.cells, vec![cell(0, 0), None, cell(2, 0)]);
        assert!(a.lanes.is_empty());
        assert_eq!(b.start_bar, 2);
        assert_eq!(b.cells, vec![cell(0, 32), cell(1, 32), cell(2, 32)]);
        assert_eq!(b.lanes.len(), 1);
        let lane = &b.lanes[0];
        assert_eq!(lane.track, 1);
        assert_eq!(lane.target, ParamTarget::Instrument(2));
        assert_eq!(lane.spec.id, "acid303.cutoff");
        assert_eq!(lane.start_bar, 2);
        assert_eq!(lane.start_sample, 192_000);
        assert_eq!(lane.points, vec![(0.0, 0.1), (2.0, 0.8)]);
    }

    #[test]
    fn v1_regions_are_the_sections() {
        for src in [
            include_str!("../../tune-song/tests/fixtures/first-acid-v1.toml"),
            include_str!("../../tune-song/tests/fixtures/first-acid-mixed.toml"),
        ] {
            let song = tune_song::parse_song(src).unwrap();
            let s = CompiledSong::new(&song, 48_000);
            let regions: Vec<u32> = s.regions.iter().map(|r| r.start_bar).collect();
            assert_eq!(regions, song.section_start_bars(), "{}", song.name);
        }
    }

    #[test]
    fn region_bounds_follow_placements_and_stretches() {
        let mut song = tune_song::parse_song(MINI).unwrap();
        // The acid plays bar 3 only, with a one-bar offset; the lane covers
        // bars 2..3 and crosses into section `b`.
        song.tracks.get_mut("acid").unwrap().play = vec![tune_song::Placement {
            start_bar: 2,
            bars: 1,
            clip: "a1".into(),
            offset_bars: 1,
        }];
        song.automation[0].start_bar = 1;
        song.automation[0].bars = 2;
        let s = CompiledSong::new(&song, 48_000);
        let starts: Vec<u32> = s.regions.iter().map(|r| r.start_bar).collect();
        assert_eq!(starts, [0, 1, 2, 3]);
        let acid: Vec<_> = s.regions.iter().map(|r| r.cells[1]).collect();
        let played = Some(CellSource {
            clip: 1,
            start_step: 32,
            offset_steps: 16,
        });
        assert_eq!(acid, [None, None, played, None]);
        let lanes: Vec<usize> = s.regions.iter().map(|r| r.lanes.len()).collect();
        assert_eq!(lanes, [0, 1, 1, 0]);
        assert_eq!(s.regions[1].lanes[0].start_sample, 96_000);
    }

    #[test]
    fn note_clips_become_events() {
        let mut song = tune_song::parse_song(MINI).unwrap();
        let note = |start, len, pitch, velocity| Note {
            start,
            len,
            pitch,
            velocity,
        };
        // Notes {0, C3, 4}, {2.5, D#3, 2, vel 120} and {14, G3, 2}, in steps.
        let clip = NoteClip {
            bars: 1,
            notes: vec![
                note(0, 96, 48, 100),
                note(60, 48, 51, 120),
                note(336, 48, 55, 100),
            ],
        };
        song.clips.insert("lead".into(), Clip::Notes(clip));
        let s = CompiledSong::new(&song, 48_000);
        let CompiledClip::Notes(lead) = s.clips.last().unwrap() else {
            panic!("not a notes clip");
        };
        assert_eq!(lead.id, "lead");
        assert_eq!(lead.steps, 16);
        let on = |i, pitch, velocity, accent, slide| NoteEvent::On {
            id: NoteId(NOTE_CLIP_IDS | i),
            pitch,
            velocity,
            accent,
            slide,
        };
        let off = |i, pitch| NoteEvent::Off {
            id: NoteId(NOTE_CLIP_IDS | i),
            pitch,
        };
        let events: Vec<(u32, NoteEvent)> = lead.events.iter().map(|e| (e.tick, e.event)).collect();
        assert_eq!(
            events,
            [
                (0, off(2, 55)),
                (0, on(0, 48, 100, false, true)),
                (60, on(1, 51, 120, true, false)),
                (96, off(0, 48)),
                (108, off(1, 51)),
                (336, on(2, 55, 100, false, false)),
            ]
        );
        assert_eq!(
            lead.step_starts,
            [0, 2, 2, 3, 3, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 6, 6]
        );
    }

    #[test]
    fn chords_order_by_note_and_do_not_slide() {
        let clip = NoteClip {
            bars: 1,
            notes: vec![
                Note {
                    start: 0,
                    len: 24,
                    pitch: 48,
                    velocity: 100,
                },
                Note {
                    start: 0,
                    len: 48,
                    pitch: 52,
                    velocity: 100,
                },
                Note {
                    start: 24,
                    len: 24,
                    pitch: 55,
                    velocity: 100,
                },
            ],
        };
        let lead = NoteClipEvents::new("c", &clip);
        let ticks: Vec<(u32, bool)> = lead
            .events
            .iter()
            .map(|e| (e.tick, matches!(e.event, NoteEvent::On { .. })))
            .collect();
        assert_eq!(
            ticks,
            [
                (0, true),
                (0, true),
                (24, false),
                (24, true),
                (48, false),
                (48, false)
            ]
        );
        let slides: Vec<bool> = lead
            .events
            .iter()
            .filter_map(|e| match e.event {
                NoteEvent::On { slide, .. } => Some(slide),
                _ => None,
            })
            .collect();
        // The second chord note runs on into the third note's start.
        assert_eq!(slides, [false, true, false]);
    }
}
