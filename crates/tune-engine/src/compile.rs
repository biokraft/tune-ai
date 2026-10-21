//! A validated [`Song`] compiled into the immutable form the engine plays:
//! every id resolved to an index, every param table filled in, every
//! pattern turned into engine cells.

use tune_dsp::acid303::math::db2amp;
use tune_dsp::{DrumHit, ParamSpec, find_spec};
use tune_song::{FxSpec, InstrumentKind, PatternSteps, Song};

use crate::clock::{Clock, track_seed};
use crate::instrument::Cell;
use crate::mixer::pan_gains;

/// A song ready to play. Tracks, patterns and sections keep song-file order.
#[derive(Clone, Debug)]
pub struct CompiledSong {
    pub name: String,
    pub clock: Clock,
    pub total_bars: u32,
    pub tracks: Vec<CompiledTrack>,
    /// One cell per pattern step, indexed like `Song::patterns`.
    pub patterns: Vec<Vec<Cell>>,
    pub sections: Vec<CompiledSection>,
}

/// One mixer channel.
#[derive(Clone, Debug)]
pub struct CompiledTrack {
    pub id: String,
    pub kind: InstrumentKind,
    /// The full instrument param table in plain units, defaults filled in.
    pub params: Vec<f64>,
    pub fx: Vec<CompiledFx>,
    /// Linear gain (`db2amp(gain_db)`).
    pub gain: f64,
    /// Constant-power `(left, right)` gains of the track's pan.
    pub pan_gains: (f64, f64),
    pub mute: bool,
    pub solo: bool,
    /// `track_seed(song seed, id)`.
    pub seed: u64,
}

/// One effect slot.
#[derive(Clone, Debug)]
pub struct CompiledFx {
    pub spec: FxSpec,
    /// The full fx param table in plain units, defaults filled in.
    pub params: Vec<f64>,
}

/// A stretch of the arrangement.
#[derive(Clone, Debug)]
pub struct CompiledSection {
    pub name: String,
    pub start_bar: u32,
    pub bars: u32,
    /// Pattern index per track (indexed like `CompiledSong::tracks`);
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

/// Which parameter of a track a lane (or a knob override) drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamTarget {
    /// Index into the instrument's param table.
    Instrument(usize),
    /// Index into the param table of fx `slot`.
    Fx { slot: usize, index: usize },
}

impl CompiledSong {
    /// Compiles `song`, which must come from `tune_song::parse_song` (or be
    /// otherwise valid). Panics on a dangling reference, which validation
    /// rules out.
    pub fn new(song: &Song, sample_rate: u32) -> Self {
        let tracks: Vec<CompiledTrack> = song
            .tracks
            .iter()
            .map(|(id, t)| CompiledTrack {
                id: id.clone(),
                kind: t.instrument,
                params: fill(t.instrument.params(), t.params.iter()),
                fx: t
                    .fx
                    .iter()
                    .map(|fx| CompiledFx {
                        spec: fx.clone(),
                        params: match fx {
                            FxSpec::Distortion { params, .. } => {
                                fill(fx.params_table(), params.iter())
                            }
                        },
                    })
                    .collect(),
                gain: db2amp(t.gain_db),
                pan_gains: pan_gains(t.pan),
                mute: t.mute,
                solo: t.solo,
                seed: track_seed(song.seed, id),
            })
            .collect();

        let patterns = song
            .patterns
            .values()
            .map(|p| match &p.steps {
                PatternSteps::Acid(acid) => acid
                    .steps()
                    .iter()
                    .map(|s| match s.note {
                        Some(_) => Cell::Note(s.clone()),
                        None => Cell::Rest,
                    })
                    .collect(),
                PatternSteps::Drum(drum) => drum
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
                        song.patterns
                            .get_index_of(pattern)
                            .unwrap_or_else(|| panic!("unknown pattern `{pattern}`")),
                    );
                }
                let lanes = s
                    .automation
                    .iter()
                    .map(|lane| {
                        let track = track_index(&lane.track);
                        let (target, spec) =
                            resolve(&tracks[track], &lane.param).unwrap_or_else(|| {
                                panic!("unknown lane param `{}` in a validated song", lane.param)
                            });
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

        Self {
            name: song.name.clone(),
            clock: Clock::new(song.bpm, sample_rate),
            total_bars: song.total_bars(),
            tracks,
            patterns,
            sections,
        }
    }

    /// Index of the track with `id`.
    pub fn track_index(&self, id: &str) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    /// Resolves `param_id` on track `track` against the instrument table,
    /// then each fx table in slot order. `None` for an unknown track or id.
    pub fn param_target(
        &self,
        track: usize,
        param_id: &str,
    ) -> Option<(ParamTarget, &'static ParamSpec)> {
        resolve(self.tracks.get(track)?, param_id)
    }

    /// The sample after the last one of the song.
    pub fn end_sample(&self) -> u64 {
        self.clock.bar_start(self.total_bars)
    }
}

/// The full `table` in plain units, with the explicit `values` set.
fn fill<'a>(
    table: &'static [ParamSpec],
    values: impl Iterator<Item = (&'a String, &'a f64)>,
) -> Vec<f64> {
    let mut params: Vec<f64> = table.iter().map(|s| s.default).collect();
    for (id, &v) in values {
        let (index, _) = find_spec(table, id)
            .unwrap_or_else(|_| panic!("unknown param `{id}` in a validated song"));
        params[index] = v;
    }
    params
}

/// Resolves a param id against the instrument table, then each fx table in
/// slot order (the order validation checks them in).
fn resolve(track: &CompiledTrack, id: &str) -> Option<(ParamTarget, &'static ParamSpec)> {
    if let Ok((index, spec)) = find_spec(track.kind.params(), id) {
        return Some((ParamTarget::Instrument(index), spec));
    }
    track.fx.iter().enumerate().find_map(|(slot, fx)| {
        find_spec(fx.spec.params_table(), id)
            .ok()
            .map(|(index, spec)| (ParamTarget::Fx { slot, index }, spec))
    })
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
        assert_eq!(kick.kind, InstrumentKind::Kick);
        assert_eq!(kick.gain, db2amp(-3.0));
        assert_eq!(kick.pan_gains, pan_gains(0.0));
        assert_eq!(kick.seed, track_seed(7, "kick"));
        assert!(kick.fx.is_empty());

        let acid = &s.tracks[1];
        assert_eq!(acid.params.len(), ACID303_PARAMS.len());
        assert_eq!(acid.params[2], 500.0);
        let set = [
            "acid303.cutoff",
            "acid303.resonance",
            "acid303.env_mod",
            "acid303.decay",
            "acid303.accent",
        ];
        for (i, spec) in ACID303_PARAMS.iter().enumerate() {
            if !set.contains(&spec.id) {
                assert_eq!(acid.params[i], spec.default, "{}", spec.id);
            }
        }
        assert_eq!(acid.gain, 1.0);
        assert_eq!(acid.fx.len(), 1);
        assert_eq!(acid.fx[0].params, vec![12.0, 1.0, -6.0]);
        assert_eq!(acid.fx[0].params.len(), DIST_PARAMS.len());

        let hat = &s.tracks[2];
        assert_eq!(hat.pan_gains, pan_gains(0.3));
        assert!(!hat.mute && !hat.solo);
    }

    #[test]
    fn patterns_become_cells() {
        let s = mini();
        assert_eq!(s.patterns.len(), 3);
        let k4 = &s.patterns[0];
        assert_eq!(k4.len(), 16);
        assert_eq!(k4[0], Cell::Hit { accent: false });
        assert_eq!(k4[1], Cell::Rest);
        assert_eq!(k4[12], Cell::Hit { accent: true });
        let a1 = &s.patterns[1];
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
