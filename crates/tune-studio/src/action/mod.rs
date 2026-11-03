//! The action layer: every song edit, transport control and undo as one
//! named, typed [`Action`].
//!
//! The studio UI, `tune do` and `tune set` all call the same actions. Each
//! has one serde form, `{"cmd": "<snake_case name>", "args": {…}}` (`args`
//! left out for an action without arguments), and one entry in the
//! [`ACTIONS`] catalog that names its kind, its arguments and their help, so
//! `tune actions` can list them and [`action_from_cli`] can read them from
//! `name=value` arguments.

mod apply;
mod catalog;
mod cli;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tune_dsp::DrumHit;
use tune_song::Registry;

use crate::diff::Change;
use crate::log::Source;
use crate::path::{SongPath, TrackField};

pub(crate) use apply::{ApplyCtx, apply_action, apply_all};
pub use catalog::{ACTIONS, find_action};
pub use cli::{action_from_cli, parse_value};

/// One action. Optional arguments may be left out and serialize as `null`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
#[derive(ts_rs::TS)]
pub enum Action {
    // ----- live -----
    /// Starts playback at `from_bar` (0-based), else at the cursor.
    Play {
        #[serde(default)]
        from_bar: Option<u32>,
    },
    Stop,
    /// Moves the cursor to `bar` (0-based).
    SeekBar {
        bar: u32,
    },
    /// Loops the half-open bar range `[start, end)` (0-based), or stops
    /// looping.
    SetLoop {
        #[serde(default)]
        bars: Option<[u32; 2]>,
    },
    // ----- live session (each lands at the next bar line) -----
    /// Launches the clip in the slot of scene `scene` for `track`; the track
    /// leaves the arrangement.
    LaunchClip {
        track: String,
        scene: String,
    },
    /// Launches every slot of `scene`: a slot with a clip plays it, an empty
    /// slot stops its track.
    LaunchScene {
        scene: String,
    },
    /// Silences `track` until it goes back to the arrangement.
    StopClip {
        track: String,
    },
    /// Sends `track`, or every track when `None`, back to the arrangement.
    BackToArrangement {
        #[serde(default)]
        track: Option<String>,
    },
    // ----- history -----
    Undo,
    Redo,
    // ----- song edits -----
    /// Sets the value at `path`, as `tune set` does.
    SetValue {
        #[ts(type = "string")]
        path: SongPath,
        value: Value,
    },
    /// A knob. `commit: false` is a live drag that is heard but not written;
    /// `commit: true` (the release, and the default) writes the value,
    /// rounded to 4 significant digits.
    SetParam {
        track: String,
        param: String,
        value: f64,
        #[serde(default = "yes")]
        commit: bool,
    },
    /// Sets the setting `setting` of the track's device `device` (its id,
    /// e.g. `dist`): `tracks.<t>.<device>.<setting>`.
    SetDeviceSetting {
        track: String,
        device: String,
        setting: String,
        value: String,
    },
    /// Sets one step of an acid clip. A rest (`note: None`) carries no
    /// accent or slide.
    SetAcidStep {
        clip: String,
        index: usize,
        #[serde(default)]
        note: Option<u8>,
        accent: bool,
        slide: bool,
    },
    SetDrumStep {
        clip: String,
        index: usize,
        hit: DrumHitDto,
    },
    /// Moves section `from` to index `to` (remove, then insert).
    MoveSection {
        from: usize,
        to: usize,
    },
    ResizeSection {
        index: usize,
        bars: u32,
    },
    DuplicateSection {
        index: usize,
    },
    DeleteSection {
        index: usize,
    },
    /// Which clip a track plays in a section; `None` silences it there.
    SetSectionPlay {
        section: usize,
        track: String,
        #[serde(default)]
        clip: Option<String>,
    },
    /// Sets each given mixer field; numbers are rounded to 4 significant
    /// digits.
    SetTrackMix {
        track: String,
        #[serde(default)]
        gain_db: Option<f64>,
        #[serde(default)]
        pan: Option<f64>,
        #[serde(default)]
        mute: Option<bool>,
        #[serde(default)]
        solo: Option<bool>,
    },
    // ----- tracks and devices -----
    /// Appends track `track` playing a new `instrument` module.
    AddTrack {
        track: String,
        instrument: String,
    },
    /// Removes a track with its placements and lane stretches.
    RemoveTrack {
        track: String,
    },
    RenameTrack {
        track: String,
        to: String,
    },
    /// Inserts an effect of `module` into the track's chain at `index`
    /// (the end by default); `id` defaults to the module's next free id.
    AddEffect {
        track: String,
        module: String,
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        index: Option<usize>,
    },
    RemoveEffect {
        track: String,
        device: String,
    },
    /// Moves an effect to `index` of the chain.
    MoveEffect {
        track: String,
        device: String,
        index: usize,
    },
    /// Renames a device (the instrument or an effect) of a track.
    RenameDevice {
        track: String,
        device: String,
        to: String,
    },
    /// Swaps the instrument of a track for a new `module` one, with the
    /// clean-up that goes with it (see [`tune_song::set_instrument`]).
    SetInstrument {
        track: String,
        module: String,
    },
    // ----- clips -----
    /// Adds a clip of `kind` (`acid`, `drum` or `notes`). `steps` is for
    /// acid and drum clips (16 rests by default), `bars` and `notes` for
    /// notes clips (1 bar and no notes by default).
    AddClip {
        clip: String,
        kind: String,
        #[serde(default)]
        steps: Option<String>,
        #[serde(default)]
        bars: Option<u32>,
        #[serde(default)]
        notes: Option<Value>,
    },
    /// Removes a clip with its placements.
    RemoveClip {
        clip: String,
    },
    /// Renames a clip; its placements follow.
    RenameClip {
        clip: String,
        to: String,
    },
    /// Copies a clip as `to`, right after the original.
    DuplicateClip {
        clip: String,
        to: String,
    },
    // ----- placements and sections (`at` and `to` are 1-based bars) -----
    /// Adds a placement of `clip` on `track` from bar `at` for `bars` bars,
    /// starting `offset` bars into the clip (0 by default).
    AddPlacement {
        track: String,
        clip: String,
        at: u32,
        bars: u32,
        #[serde(default)]
        offset: Option<u32>,
    },
    /// Moves the placement of `track` that starts at bar `at` to bar `to`.
    MovePlacement {
        track: String,
        at: u32,
        to: u32,
    },
    /// Makes the placement of `track` that starts at bar `at` `bars` long.
    ResizePlacement {
        track: String,
        at: u32,
        bars: u32,
    },
    /// Removes the placement of `track` that starts at bar `at`.
    RemovePlacement {
        track: String,
        at: u32,
    },
    /// Inserts an empty section before section `index` (the end by default).
    AddSection {
        name: String,
        bars: u32,
        #[serde(default)]
        index: Option<usize>,
    },
    RenameSection {
        index: usize,
        name: String,
    },
    // ----- scenes (`index` is 0-based) -----
    /// Adds an empty scene before scene `index` (the end by default).
    AddScene {
        scene: String,
        #[serde(default)]
        index: Option<usize>,
    },
    /// Removes a scene with its slots.
    RemoveScene {
        scene: String,
    },
    /// Renames a scene, keeping its place.
    RenameScene {
        scene: String,
        to: String,
    },
    /// Moves a scene to `index`, as a removal then an insertion.
    MoveScene {
        scene: String,
        index: usize,
    },
    /// Puts `clip` in a scene's slot for `track`; no clip empties the slot.
    SetSlot {
        scene: String,
        track: String,
        #[serde(default)]
        clip: Option<String>,
    },
    // ----- lanes (`param` as in the file; `at` is a 1-based bar) -----
    /// Adds a stretch of the lane for `param` from bar `at` for `bars` bars.
    /// `points` are `[bar offset from at, value 0 to 1]`.
    AddLane {
        param: String,
        at: u32,
        bars: u32,
        points: Vec<[f64; 2]>,
    },
    /// Removes the stretch of `param` that starts at bar `at`.
    RemoveLane {
        param: String,
        at: u32,
    },
    /// Replaces the points of the stretch of `param` that starts at bar `at`.
    SetLanePoints {
        param: String,
        at: u32,
        points: Vec<[f64; 2]>,
    },
    // ----- runs -----
    /// Undoes every edit of run `run` that nobody changed since (see
    /// [`crate::undo_run`]); its result is the [`crate::RunUndo`].
    UndoRun {
        run: String,
    },
}

fn yes() -> bool {
    true
}

/// What an action does: change the song, drive the transport, or walk the
/// undo history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionKind {
    Edit,
    Live,
    History,
}

impl ActionKind {
    /// The kind as `tune actions` prints it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Live => "live",
            Self::History => "history",
        }
    }
}

/// The type of an action's argument, which says how `tune do` reads it.
/// `Id`, `ParamId`, `Path` and `Text` take the text as given (a TOML string
/// literal is unquoted); the others are TOML values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgType {
    /// A track, clip, device or setting id.
    Id,
    /// A param id such as `acid303.cutoff`.
    ParamId,
    /// An edit path such as `tracks.acid.acid303.cutoff`.
    Path,
    Text,
    Int,
    Number,
    Bool,
    /// Any TOML value.
    Json,
}

impl ArgType {
    /// The type as `tune actions` prints it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::ParamId => "param_id",
            Self::Path => "path",
            Self::Text => "text",
            Self::Int => "int",
            Self::Number => "number",
            Self::Bool => "bool",
            Self::Json => "json",
        }
    }

    /// Whether `tune do` takes the argument's text as given.
    pub fn is_text(self) -> bool {
        matches!(self, Self::Id | Self::ParamId | Self::Path | Self::Text)
    }
}

/// One argument of an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: &'static str,
    pub ty: ArgType,
    pub required: bool,
    pub help: &'static str,
}

/// One action in the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSpec {
    pub name: &'static str,
    pub kind: ActionKind,
    pub help: &'static str,
    pub args: &'static [ArgSpec],
}

impl Action {
    /// The action's wire name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Play { .. } => "play",
            Self::Stop => "stop",
            Self::SeekBar { .. } => "seek_bar",
            Self::SetLoop { .. } => "set_loop",
            Self::LaunchClip { .. } => "launch_clip",
            Self::LaunchScene { .. } => "launch_scene",
            Self::StopClip { .. } => "stop_clip",
            Self::BackToArrangement { .. } => "back_to_arrangement",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::SetValue { .. } => "set_value",
            Self::SetParam { .. } => "set_param",
            Self::SetDeviceSetting { .. } => "set_device_setting",
            Self::SetAcidStep { .. } => "set_acid_step",
            Self::SetDrumStep { .. } => "set_drum_step",
            Self::MoveSection { .. } => "move_section",
            Self::ResizeSection { .. } => "resize_section",
            Self::DuplicateSection { .. } => "duplicate_section",
            Self::DeleteSection { .. } => "delete_section",
            Self::SetSectionPlay { .. } => "set_section_play",
            Self::SetTrackMix { .. } => "set_track_mix",
            Self::AddTrack { .. } => "add_track",
            Self::RemoveTrack { .. } => "remove_track",
            Self::RenameTrack { .. } => "rename_track",
            Self::AddEffect { .. } => "add_effect",
            Self::RemoveEffect { .. } => "remove_effect",
            Self::MoveEffect { .. } => "move_effect",
            Self::RenameDevice { .. } => "rename_device",
            Self::SetInstrument { .. } => "set_instrument",
            Self::AddClip { .. } => "add_clip",
            Self::RemoveClip { .. } => "remove_clip",
            Self::RenameClip { .. } => "rename_clip",
            Self::DuplicateClip { .. } => "duplicate_clip",
            Self::AddPlacement { .. } => "add_placement",
            Self::MovePlacement { .. } => "move_placement",
            Self::ResizePlacement { .. } => "resize_placement",
            Self::RemovePlacement { .. } => "remove_placement",
            Self::AddSection { .. } => "add_section",
            Self::RenameSection { .. } => "rename_section",
            Self::AddScene { .. } => "add_scene",
            Self::RemoveScene { .. } => "remove_scene",
            Self::RenameScene { .. } => "rename_scene",
            Self::MoveScene { .. } => "move_scene",
            Self::SetSlot { .. } => "set_slot",
            Self::AddLane { .. } => "add_lane",
            Self::RemoveLane { .. } => "remove_lane",
            Self::SetLanePoints { .. } => "set_lane_points",
            Self::UndoRun { .. } => "undo_run",
        }
    }

    /// The action's catalog entry.
    pub fn spec(&self) -> &'static ActionSpec {
        find_action(self.name()).expect("every action is in the catalog")
    }

    pub fn kind(&self) -> ActionKind {
        self.spec().kind
    }

    /// Edit paths this action changes, in the UI's highlight-key form.
    pub fn targets(&self) -> Vec<String> {
        match self {
            Self::SetValue { path, .. } => vec![path.to_string()],
            Self::SetParam { track, param, .. } => vec![SongPath::param(track, param).to_string()],
            Self::SetDeviceSetting {
                track,
                device,
                setting,
                ..
            } => vec![format!("tracks.{track}.{device}.{setting}")],
            Self::SetAcidStep { clip, index, .. } | Self::SetDrumStep { clip, index, .. } => {
                vec![format!("clips.{clip}.steps[{index}]")]
            }
            Self::ResizeSection { index, .. } => vec![format!("sections[{index}].bars")],
            Self::SetSectionPlay { section, track, .. } => {
                vec![format!("sections[{section}].play.{track}")]
            }
            Self::SetTrackMix {
                track,
                gain_db,
                pan,
                mute,
                solo,
            } => [
                (TrackField::GainDb, gain_db.is_some()),
                (TrackField::Pan, pan.is_some()),
                (TrackField::Mute, mute.is_some()),
                (TrackField::Solo, solo.is_some()),
            ]
            .into_iter()
            .filter(|(_, given)| *given)
            .map(|(field, _)| format!("tracks.{track}.{field}"))
            .collect(),
            Self::AddTrack { track, .. }
            | Self::RemoveTrack { track }
            | Self::RenameTrack { track, .. } => vec![format!("tracks.{track}")],
            Self::AddEffect { track, .. }
            | Self::RemoveEffect { track, .. }
            | Self::MoveEffect { track, .. } => vec![format!("tracks.{track}.fx")],
            // The action names a device by id, not by role: a device whose
            // id is an instrument's module name reads as the instrument.
            Self::RenameDevice { track, device, .. } => {
                let is_instrument = Registry::builtin()
                    .instruments()
                    .any(|m| m.prefix == device);
                let role = if is_instrument { "instrument" } else { "fx" };
                vec![format!("tracks.{track}.{role}")]
            }
            Self::SetInstrument { track, .. } => vec![format!("tracks.{track}.instrument")],
            Self::AddClip { clip, .. } | Self::RemoveClip { clip } => {
                vec![format!("clips.{clip}")]
            }
            Self::RenameClip { clip, to } | Self::DuplicateClip { clip, to } => {
                vec![format!("clips.{clip}"), format!("clips.{to}")]
            }
            Self::AddPlacement { track, .. }
            | Self::MovePlacement { track, .. }
            | Self::ResizePlacement { track, .. }
            | Self::RemovePlacement { track, .. } => vec![format!("tracks.{track}.play")],
            Self::RenameSection { index, .. } => vec![format!("sections[{index}].name")],
            Self::AddScene { scene, .. } | Self::RemoveScene { scene } => {
                vec![format!("scenes.{scene}")]
            }
            Self::RenameScene { scene, to } => {
                vec![format!("scenes.{scene}"), format!("scenes.{to}")]
            }
            Self::MoveScene { .. } => vec!["scenes".to_owned()],
            Self::SetSlot { scene, track, .. } | Self::LaunchClip { track, scene } => {
                vec![format!("scenes.{scene}.slots.{track}")]
            }
            Self::LaunchScene { scene } => vec![format!("scenes.{scene}")],
            Self::StopClip { track } | Self::BackToArrangement { track: Some(track) } => {
                vec![format!("tracks.{track}")]
            }
            Self::AddLane { param, .. }
            | Self::RemoveLane { param, .. }
            | Self::SetLanePoints { param, .. } => vec![format!("automation.{param}")],
            Self::MoveSection { .. }
            | Self::DuplicateSection { .. }
            | Self::DeleteSection { .. }
            | Self::AddSection { .. } => {
                vec!["sections".to_owned()]
            }
            // Which targets a run undo writes, the edit log says.
            Self::UndoRun { .. }
            | Self::BackToArrangement { track: None }
            | Self::Play { .. }
            | Self::Stop
            | Self::SeekBar { .. }
            | Self::SetLoop { .. }
            | Self::Undo
            | Self::Redo => Vec::new(),
        }
    }
}

/// A run: the edits of one piece of work, named by Claude so the producer
/// can undo them together.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct RunTag {
    pub id: String,
    pub title: Option<String>,
}

impl RunTag {
    /// A run id is 1 to 64 letters, digits or `_.:-`; a title is one line of
    /// 1 to 200 characters.
    pub fn new(id: &str, title: Option<&str>) -> Result<Self, String> {
        let id_ok = (1..=64).contains(&id.chars().count())
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c));
        if !id_ok {
            return Err(format!(
                "run id must be 1 to 64 letters, digits or `_.:-`, got `{id}`"
            ));
        }
        if let Some(t) = title
            && (!(1..=200).contains(&t.chars().count()) || t.contains(['\n', '\r']))
        {
            return Err("a run title is one line of 1 to 200 characters".to_owned());
        }
        Ok(Self {
            id: id.to_owned(),
            title: title.map(str::to_owned),
        })
    }
}

/// Who is acting, and for which run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub source: Source,
    pub run: Option<RunTag>,
}

impl Origin {
    pub fn human() -> Self {
        Self {
            source: Source::Human,
            run: None,
        }
    }

    pub fn claude(run: Option<RunTag>) -> Self {
        Self {
            source: Source::Claude,
            run,
        }
    }
}

/// How a set of actions reached the song.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    /// Through the open studio.
    Studio,
    /// Straight into the file.
    File,
}

/// What a set of actions did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionsOutcome {
    pub via: Via,
    pub changes: Vec<Change>,
    /// One result per action, in order (`null` for most).
    pub results: Vec<Value>,
    /// FNV-1a hash of the file after the write; `None` when nothing was
    /// written.
    pub file_hash: Option<String>,
    /// The format version the file was migrated from, when the write did.
    pub migrated_from: Option<u32>,
}

/// One drum step on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(ts_rs::TS)]
pub enum DrumHitDto {
    Rest,
    Hit,
    Accent,
}

impl From<DrumHit> for DrumHitDto {
    fn from(h: DrumHit) -> Self {
        match h {
            DrumHit::Rest => Self::Rest,
            DrumHit::Hit => Self::Hit,
            DrumHit::Accent => Self::Accent,
        }
    }
}

impl From<DrumHitDto> for DrumHit {
    fn from(h: DrumHitDto) -> Self {
        match h {
            DrumHitDto::Rest => Self::Rest,
            DrumHitDto::Hit => Self::Hit,
            DrumHitDto::Accent => Self::Accent,
        }
    }
}
