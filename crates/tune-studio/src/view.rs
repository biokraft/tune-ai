//! What the UI renders: the studio's state and the open song, flattened
//! into plain serializable views.

use indexmap::IndexMap;
use serde::Serialize;
use tune_dsp::{Curve, ParamSpec, STRIP_PARAMS};
use tune_song::{
    Clip, ClipKind, Device, Lane, MasterMode, MixUnit, Placement, SectionPlay, Song, TICKS_PER_STEP,
};

use crate::action::DrumHitDto;
use crate::diff::Change;
use crate::error::ErrorView;

/// Everything the UI shows outside the song itself.
#[derive(Clone, Debug, PartialEq, Serialize, ts_rs::TS)]
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
    /// The format version the song file was migrated from when it was
    /// opened, while one undo would revert that migration; else `None`.
    pub migrated_from: Option<u32>,
}

/// The open song.
#[derive(Clone, Debug, PartialEq, Serialize, ts_rs::TS)]
pub struct SongView {
    pub name: String,
    pub bpm: f64,
    pub total_bars: u32,
    pub tracks: Vec<TrackView>,
    /// The clip pool, in song-file order.
    pub patterns: Vec<PatternView>,
    pub sections: Vec<SectionView>,
    pub mix: MixView,
}

/// The `[mix]` table: the master stage, the duck source and the shared
/// buses, with defaults filled in.
#[derive(Clone, Debug, PartialEq, Serialize, ts_rs::TS)]
pub struct MixView {
    /// `"clip"` or `"limit"`.
    pub master: String,
    /// Every master mode, in the order the UI offers them.
    pub masters: Vec<String>,
    /// The track every `duck.depth` follows: the file's `duck_source`, else
    /// the first kick; `None` when there is neither.
    pub duck_source: Option<String>,
    pub delay: Vec<ParamView>,
    pub reverb: Vec<ParamView>,
    pub master_params: Vec<ParamView>,
    /// Whether some track sends to the bus (explicitly or by a lane).
    pub delay_active: bool,
    pub reverb_active: bool,
}

/// One mixer channel.
#[derive(Clone, Debug, PartialEq, Serialize, ts_rs::TS)]
pub struct TrackView {
    pub id: String,
    /// The instrument's module name, e.g. `acid303`.
    pub instrument: String,
    /// The instrument's device id: the module name unless the file names
    /// another, and the prefix of its param ids.
    pub instrument_id: String,
    /// The clip kinds the instrument plays (`"acid"`, `"drum"`, `"notes"`).
    pub plays: Vec<String>,
    pub gain_db: f64,
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    /// The instrument's params, in table order.
    pub params: Vec<ParamView>,
    /// The strip params (sends and ducking), in `STRIP_PARAMS` order.
    pub strip: Vec<ParamView>,
    pub fx: Vec<FxView>,
    /// What the track plays, in time order.
    pub placements: Vec<PlacementView>,
}

/// One clip placed on a track.
#[derive(Clone, Debug, PartialEq, Serialize, ts_rs::TS)]
pub struct PlacementView {
    /// 0-based.
    pub start_bar: u32,
    pub bars: u32,
    pub clip: String,
    /// Bars into the clip at which the placement starts playing it.
    pub offset: u32,
    /// The section whose `play` shows this placement (it alone plays there,
    /// from the section start and the clip's first step, over the whole
    /// section); `None` when no section's does. Any later section it
    /// covers plays it mid-clip and does not show it.
    pub in_section: Option<usize>,
}

/// A param's spec plus its value in this song: everything a knob needs.
#[derive(Clone, Debug, PartialEq, Serialize, ts_rs::TS)]
pub struct ParamView {
    /// Full id, e.g. `acid303.cutoff`.
    pub id: String,
    pub label: String,
    pub unit: String,
    pub min: f64,
    pub max: f64,
    /// `"linear"` or `"exponential"`.
    #[ts(type = "\"linear\" | \"exponential\"")]
    pub curve: String,
    pub default: f64,
    pub help: String,
    /// The plain value: the file's, else the default.
    pub value: f64,
    /// Whether the file sets the value.
    pub explicit: bool,
}

/// One effect in a track's chain.
#[derive(Clone, Debug, PartialEq, Serialize, ts_rs::TS)]
pub struct FxView {
    /// The device id, e.g. `eq` or `eq2`: the prefix of its param ids.
    pub id: String,
    /// The fx `type`, e.g. `distortion` or `eq`.
    pub kind: String,
    /// The distortion mode; empty for an eq, which has none.
    pub mode: String,
    /// The modes to offer; empty for an eq.
    pub modes: Vec<String>,
    pub params: Vec<ParamView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[derive(ts_rs::TS)]
pub enum PatternView {
    Acid {
        id: String,
        steps: Vec<AcidStepView>,
    },
    Drum {
        id: String,
        hits: Vec<DrumHitDto>,
    },
    Notes {
        id: String,
        bars: u32,
        notes: Vec<NoteView>,
    },
}

/// One note of a note clip, positions in 16th steps.
#[derive(Clone, Debug, PartialEq, Serialize, ts_rs::TS)]
pub struct NoteView {
    pub start: f64,
    /// MIDI note.
    pub pitch: u8,
    pub len: f64,
    pub vel: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, ts_rs::TS)]
pub struct AcidStepView {
    /// MIDI note, or `None` for a rest.
    pub note: Option<u8>,
    pub accent: bool,
    pub slide: bool,
}

/// A stretch of the arrangement.
#[derive(Clone, Debug, PartialEq, Serialize, ts_rs::TS)]
pub struct SectionView {
    pub name: String,
    /// 0-based.
    pub start_bar: u32,
    pub bars: u32,
    /// Track id to clip id, in track order, for each track that plays one
    /// clip over the whole section, from the section start with offset 0
    /// (see [`Song::section_play`]).
    pub play: IndexMap<String, String>,
    /// The lane stretches that overlap this section, in file order.
    pub lanes: Vec<LaneView>,
}

/// One automation stretch as one section shows it.
#[derive(Clone, Debug, PartialEq, Serialize, ts_rs::TS)]
pub struct LaneView {
    pub track: String,
    /// Full param id, e.g. `acid303.cutoff`.
    pub param: String,
    /// `[bar offset, normalized value]`, the offsets from the section start
    /// (below 0 or past the section for a stretch that crosses its edge).
    pub points: Vec<[f64; 2]>,
    /// The stretch's part of the section, in bars from its start:
    /// `0 <= from < to <= bars`.
    pub from: f64,
    pub to: f64,
}

/// The device setting an [`FxView`] shows as its `mode`.
const MODE: &str = "mode";

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
                    instrument: t.instrument.module.name.to_owned(),
                    instrument_id: t.instrument.id.clone(),
                    plays: ClipKind::ALL
                        .into_iter()
                        .filter(|&k| t.instrument.module.plays(k))
                        .map(|k| k.name().to_owned())
                        .collect(),
                    gain_db: t.gain_db,
                    pan: t.pan,
                    mute: t.mute,
                    solo: t.solo,
                    params: device_param_views(&t.instrument),
                    strip: param_views(STRIP_PARAMS, &t.strip),
                    fx: t.fx.iter().map(fx_view).collect(),
                    placements: t.play.iter().map(|p| placement_view(song, id, p)).collect(),
                })
                .collect(),
            patterns: song
                .clips
                .iter()
                .map(|(id, p)| match p {
                    Clip::Acid(a) => PatternView::Acid {
                        id: id.clone(),
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
                    Clip::Drum(d) => PatternView::Drum {
                        id: id.clone(),
                        hits: d.hits().iter().map(|&h| h.into()).collect(),
                    },
                    Clip::Notes(n) => PatternView::Notes {
                        id: id.clone(),
                        bars: n.bars,
                        notes: n
                            .notes
                            .iter()
                            .map(|note| NoteView {
                                start: f64::from(note.start) / f64::from(TICKS_PER_STEP),
                                pitch: note.pitch,
                                len: f64::from(note.len) / f64::from(TICKS_PER_STEP),
                                vel: note.velocity,
                            })
                            .collect(),
                    },
                })
                .collect(),
            sections: song
                .sections
                .iter()
                .zip(starts)
                .enumerate()
                .map(|(i, (s, start_bar))| SectionView {
                    name: s.name.clone(),
                    start_bar,
                    bars: s.bars,
                    play: song
                        .tracks
                        .keys()
                        .filter_map(|t| match song.section_play(i, t) {
                            SectionPlay::Clip(clip) => Some((t.clone(), clip.to_owned())),
                            _ => None,
                        })
                        .collect(),
                    lanes: song
                        .lanes_in(i)
                        .into_iter()
                        .map(|l| lane_view(l, start_bar, s.bars))
                        .collect(),
                })
                .collect(),
            mix: mix_view(song),
        }
    }
}

/// Placement `p` of `track`, with the section that shows it: one it
/// overlaps where the track plays one clip over the whole section, which
/// can then only be this placement, starting there with offset 0.
fn placement_view(song: &Song, track: &str, p: &Placement) -> PlacementView {
    let span = p.span();
    let in_section = song
        .sections
        .iter()
        .zip(song.section_start_bars())
        .enumerate()
        .position(|(i, (s, start))| {
            start < span.end
                && span.start < start + s.bars
                && matches!(song.section_play(i, track), SectionPlay::Clip(_))
        });
    PlacementView {
        start_bar: p.start_bar,
        bars: p.bars,
        clip: p.clip.clone(),
        offset: p.offset_bars,
        in_section,
    }
}

/// Stretch `lane` as the section starting at `start` (0-based) and `bars`
/// long shows it: its points shifted to the section start, clipped to the
/// section by `from` and `to`.
fn lane_view(lane: &Lane, start: u32, bars: u32) -> LaneView {
    let shift = f64::from(lane.start_bar) - f64::from(start);
    let span = lane.span();
    LaneView {
        track: lane.track.clone(),
        param: lane.param.clone(),
        points: lane.points.iter().map(|&(b, v)| [b + shift, v]).collect(),
        from: f64::from(span.start.max(start) - start),
        to: f64::from(span.end.min(start + bars) - start),
    }
}

fn mix_view(song: &Song) -> MixView {
    let params = |unit: MixUnit| param_views(unit.params(), &song.mix.params);
    MixView {
        master: song.mix.master.name().to_owned(),
        masters: MasterMode::ALL
            .iter()
            .map(|m| m.name().to_owned())
            .collect(),
        duck_source: song.duck_source().map(str::to_owned),
        delay: params(MixUnit::Delay),
        reverb: params(MixUnit::Reverb),
        master_params: params(MixUnit::Master),
        delay_active: song.bus_active(MixUnit::Delay),
        reverb_active: song.bus_active(MixUnit::Reverb),
    }
}

fn fx_view(fx: &Device) -> FxView {
    let modes = fx
        .module
        .setting(MODE)
        .map_or(&[][..], |(_, spec)| spec.values);
    FxView {
        id: fx.id.clone(),
        kind: fx.module.name.to_owned(),
        mode: fx.setting(MODE).unwrap_or_default().to_owned(),
        modes: modes.iter().map(|m| (*m).to_owned()).collect(),
        params: device_param_views(fx),
    }
}

/// A device's params, with track-relative ids (`<device id>.<param>`).
fn device_param_views(device: &Device) -> Vec<ParamView> {
    let module = device.module;
    module
        .params
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let local = module.local_name(i);
            param_view(
                spec,
                device.param_id(local),
                device.params.get(local).copied(),
            )
        })
        .collect()
}

/// A table's params by full id (`send.delay`, `reverb.size`).
fn param_views(table: &[ParamSpec], explicit: &IndexMap<String, f64>) -> Vec<ParamView> {
    table
        .iter()
        .map(|spec| param_view(spec, spec.id.to_owned(), explicit.get(spec.id).copied()))
        .collect()
}

/// `spec` under `id`, with the file's value `set` if any.
fn param_view(spec: &ParamSpec, id: String, set: Option<f64>) -> ParamView {
    ParamView {
        id,
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
}
