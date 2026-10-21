//! What the UI renders: the studio's state and the open song, flattened
//! into plain serializable views.

use indexmap::IndexMap;
use serde::Serialize;
use tune_dsp::{Curve, DistortionMode, ParamSpec};
use tune_song::{FxSpec, PatternSteps, Song};

use crate::command::DrumHitDto;
use crate::diff::Change;
use crate::error::ErrorView;

/// Everything the UI shows outside the song itself.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StudioState {
    pub path: Option<String>,
    pub song: Option<SongView>,
    pub playing: bool,
    /// 0-based.
    pub cursor_bar: u32,
    /// Half-open `[start, end)`, 0-based.
    pub loop_bars: Option<[u32; 2]>,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Local changes an external edit overrode, offered for re-apply.
    pub offered: Vec<Change>,
    /// Problems in the song file on disk; edits wait until they are fixed.
    pub file_errors: Vec<ErrorView>,
}

/// The open song.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SongView {
    pub name: String,
    pub bpm: f64,
    pub total_bars: u32,
    pub tracks: Vec<TrackView>,
    pub patterns: Vec<PatternView>,
    pub sections: Vec<SectionView>,
}

/// One mixer channel.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TrackView {
    pub id: String,
    pub instrument: String,
    pub gain_db: f64,
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    /// The instrument's params, in table order.
    pub params: Vec<ParamView>,
    pub fx: Vec<FxView>,
}

/// A param's spec plus its value in this song: everything a knob needs.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ParamView {
    /// Full id, e.g. `acid303.cutoff`.
    pub id: String,
    pub label: String,
    pub unit: String,
    pub min: f64,
    pub max: f64,
    /// `"linear"` or `"exponential"`.
    pub curve: String,
    pub default: f64,
    pub help: String,
    /// The plain value: the file's, else the default.
    pub value: f64,
    /// Whether the file sets the value.
    pub explicit: bool,
}

/// One effect in a track's chain.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FxView {
    /// The fx `type`, e.g. `distortion`.
    pub kind: String,
    pub mode: String,
    pub modes: Vec<String>,
    pub params: Vec<ParamView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PatternView {
    Acid {
        id: String,
        track: String,
        steps: Vec<AcidStepView>,
    },
    Drum {
        id: String,
        track: String,
        hits: Vec<DrumHitDto>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AcidStepView {
    /// MIDI note, or `None` for a rest.
    pub note: Option<u8>,
    pub accent: bool,
    pub slide: bool,
}

/// A stretch of the arrangement.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SectionView {
    pub name: String,
    /// 0-based.
    pub start_bar: u32,
    pub bars: u32,
    /// Track id to pattern id.
    pub play: IndexMap<String, String>,
    pub lanes: Vec<LaneView>,
}

/// An automation lane: `points` are `[bar offset, normalized value]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LaneView {
    pub track: String,
    /// Full param id, e.g. `acid303.cutoff`.
    pub param: String,
    pub points: Vec<[f64; 2]>,
}

/// Every distortion mode, in the order the UI offers them.
const DIST_MODES: [DistortionMode; 3] = [
    DistortionMode::Soft,
    DistortionMode::Hard,
    DistortionMode::Overdrive,
];

impl From<&Song> for SongView {
    fn from(song: &Song) -> Self {
        let starts = song.section_start_bars();
        Self {
            name: song.name.clone(),
            bpm: song.bpm,
            total_bars: song.total_bars(),
            tracks: song
                .tracks
                .iter()
                .map(|(id, t)| TrackView {
                    id: id.clone(),
                    instrument: t.instrument.name().to_owned(),
                    gain_db: t.gain_db,
                    pan: t.pan,
                    mute: t.mute,
                    solo: t.solo,
                    params: param_views(t.instrument.params(), &t.params),
                    fx: t.fx.iter().map(fx_view).collect(),
                })
                .collect(),
            patterns: song
                .patterns
                .iter()
                .map(|(id, p)| match &p.steps {
                    PatternSteps::Acid(a) => PatternView::Acid {
                        id: id.clone(),
                        track: p.track.clone(),
                        steps: a
                            .steps()
                            .iter()
                            .map(|s| AcidStepView {
                                note: s.note,
                                accent: s.accent,
                                slide: s.slide,
                            })
                            .collect(),
                    },
                    PatternSteps::Drum(d) => PatternView::Drum {
                        id: id.clone(),
                        track: p.track.clone(),
                        hits: d.hits().iter().map(|&h| h.into()).collect(),
                    },
                })
                .collect(),
            sections: song
                .sections
                .iter()
                .zip(starts)
                .map(|(s, start_bar)| SectionView {
                    name: s.name.clone(),
                    start_bar,
                    bars: s.bars,
                    play: s.play.clone(),
                    lanes: s
                        .automation
                        .iter()
                        .map(|l| LaneView {
                            track: l.track.clone(),
                            param: l.param.clone(),
                            points: l.points.iter().map(|&(b, v)| [b, v]).collect(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

fn fx_view(fx: &FxSpec) -> FxView {
    match fx {
        FxSpec::Distortion { mode, params } => FxView {
            kind: fx.type_name().to_owned(),
            mode: mode.name().to_owned(),
            modes: DIST_MODES.iter().map(|m| m.name().to_owned()).collect(),
            params: param_views(fx.params_table(), params),
        },
        FxSpec::Eq { params } => FxView {
            kind: fx.type_name().to_owned(),
            mode: String::new(),
            modes: Vec::new(),
            params: param_views(fx.params_table(), params),
        },
    }
}

fn param_views(table: &[ParamSpec], explicit: &IndexMap<String, f64>) -> Vec<ParamView> {
    table
        .iter()
        .map(|spec| {
            let set = explicit.get(spec.id).copied();
            ParamView {
                id: spec.id.to_owned(),
                label: spec.label.to_owned(),
                unit: spec.unit.to_owned(),
                min: spec.min,
                max: spec.max,
                curve: match spec.curve {
                    Curve::Linear => "linear",
                    Curve::Exponential => "exponential",
                }
                .to_owned(),
                default: spec.default,
                help: spec.help.to_owned(),
                value: set.unwrap_or(spec.default),
                explicit: set.is_some(),
            }
        })
        .collect()
}
