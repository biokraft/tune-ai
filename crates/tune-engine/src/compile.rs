//! A validated [`Song`] compiled into the immutable form the engine plays:
//! every id resolved to an index, every param table filled in, every
//! pattern turned into engine cells.

use tune_dsp::acid303::math::db2amp;
use tune_dsp::{DrumHit, ParamSpec, STRIP_PARAMS, find_spec};
use tune_modules::{Cell, ModuleDescriptor};
use tune_song::{Clip, Device, MIX_TRACK_ID, MasterMode, MixUnit, Song, Track, find_mix_param};

use crate::clock::{Clock, track_seed};
use crate::mixer::pan_gains;

/// A song ready to play. Tracks, patterns and sections keep song-file order.
#[derive(Clone, Debug)]
pub struct CompiledSong {
    pub name: String,
    pub clock: Clock,
    pub total_bars: u32,
    pub tracks: Vec<CompiledTrack>,
    /// One cell per clip step, indexed like `Song::clips`.
    pub clips: Vec<Vec<Cell>>,
    pub sections: Vec<CompiledSection>,
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

/// A stretch of the arrangement.
#[derive(Clone, Debug)]
pub struct CompiledSection {
    pub name: String,
    pub start_bar: u32,
    pub bars: u32,
    /// Clip index per track (indexed like `CompiledSong::tracks`);
    /// `None` when the section does not play the track.
    pub play: Vec<Option<usize>>,
    pub lanes: Vec<CompiledLane>,
}

/// An automation lane with its target resolved.
#[derive(Clone, Debug)]
pub struct CompiledLane {
    pub track: usize,
    pub target: ParamTarget,
    pub spec: &'static ParamSpec,
    /// `(bar offset from the section start, normalized value)`, bars never
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
            .values()
            .map(|p| match p {
                Clip::Acid(acid) => acid
                    .steps()
                    .iter()
                    .map(|s| match s.note {
                        Some(_) => Cell::Note(s.clone()),
                        None => Cell::Rest,
                    })
                    .collect(),
                Clip::Drum(drum) => drum
                    .hits()
                    .iter()
                    .map(|h| match h {
                        DrumHit::Rest => Cell::Rest,
                        DrumHit::Hit => Cell::Hit { accent: false },
                        DrumHit::Accent => Cell::Hit { accent: true },
                    })
                    .collect(),
            })
            .collect();

        let track_index = |id: &str| {
            song.tracks
                .get_index_of(id)
                .unwrap_or_else(|| panic!("unknown track `{id}` in a validated song"))
        };
        let sections = song
            .sections
            .iter()
            .zip(song.section_start_bars())
            .map(|(s, start_bar)| {
                let mut play = vec![None; tracks.len()];
                for (track, pattern) in &s.play {
                    play[track_index(track)] = Some(
                        song.clips
                            .get_index_of(pattern)
                            .unwrap_or_else(|| panic!("unknown pattern `{pattern}`")),
                    );
                }
                let lanes = s
                    .automation
                    .iter()
                    .map(|lane| {
                        let unknown =
                            || panic!("unknown lane param `{}` in a validated song", lane.param);
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
                            points: lane.points.clone(),
                        }
                    })
                    .collect();
                CompiledSection {
                    name: s.name.clone(),
                    start_bar,
                    bars: s.bars,
                    play,
                    lanes,
                }
            })
            .collect();

        let mix = compile_mix(song, &track_index);
        Self {
            name: song.name.clone(),
            clock: Clock::new(song.bpm, sample_rate),
            total_bars: song.total_bars(),
            tracks,
            clips,
            sections,
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
        let k4 = &s.clips[0];
        assert_eq!(k4.len(), 16);
        assert_eq!(k4[0], Cell::Hit { accent: false });
        assert_eq!(k4[1], Cell::Rest);
        assert_eq!(k4[12], Cell::Hit { accent: true });
        let a1 = &s.clips[1];
        assert_eq!(a1.len(), 8);
        assert!(matches!(&a1[1], Cell::Note(n) if n.note == Some(36) && n.accent));
        assert!(matches!(&a1[2], Cell::Note(n) if n.note == Some(48) && n.slide));
        assert_eq!(a1[4], Cell::Rest);
    }

    #[test]
    fn sections_resolve_play_and_lanes() {
        let s = mini();
        assert_eq!(s.sections.len(), 2);
        let (a, b) = (&s.sections[0], &s.sections[1]);
        assert_eq!((a.name.as_str(), a.start_bar, a.bars), ("a", 0, 2));
        assert_eq!((b.name.as_str(), b.start_bar, b.bars), ("b", 2, 2));
        assert_eq!(a.play, vec![Some(0), None, Some(2)]);
        assert_eq!(b.play, vec![Some(0), Some(1), Some(2)]);
        assert!(a.lanes.is_empty());
        let lane = &b.lanes[0];
        assert_eq!(lane.track, 1);
        assert_eq!(lane.target, ParamTarget::Instrument(2));
        assert_eq!(lane.spec.id, "acid303.cutoff");
        assert_eq!(lane.points, vec![(0.0, 0.1), (2.0, 0.8)]);
    }
}
