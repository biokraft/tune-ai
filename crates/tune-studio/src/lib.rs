//! The studio core for tune-ai: edits to a song file, made in place.
//!
//! The producer (through the app) and Claude (as text) edit the same
//! `song.toml`. Claude's files carry comments for the producer to learn
//! from, so every edit here changes only the value it names and keeps the
//! rest of the file byte for byte. The [`Studio`] actor serves the open song
//! to any number of clients, plays it live and hot-reloads it. Headless: no
//! UI, audio device code or async runtime.

#![forbid(unsafe_code)]

mod action;
mod command;
mod control;
mod core;
mod diff;
mod doc;
mod error;
mod log;
mod path;
mod run_undo;
mod session;
mod set;
mod studio;
mod target;
mod view;
mod wire;

pub use action::{
    ACTIONS, Action, ActionKind, ActionSpec, ActionsOutcome, ArgSpec, ArgType, DrumHitDto, Origin,
    RunTag, Via, action_from_cli, find_action, parse_value,
};
pub use command::{Command, StudioCommand};
pub use control::{ControlServer, STUDIO_DIR_ENV, StudioLink, find_studio, studio_dir};
pub use core::{
    ExternalChange, OWN_WRITE_RING, PendingEdit, StudioCore, UNDO_DEPTH, UndoEntry, WRITE_DEBOUNCE,
};
pub use diff::{Change, diff_songs, is_whole_replace};
pub use doc::{EditError, SongDoc, effective_value, placement_json};
pub use error::{ErrorView, StudioError};
pub use log::{ActionRecord, EditEntry, EditLog, Source, text_hash};
pub use path::{SongPath, TrackField};
pub use run_undo::{KeptReason, KeptTarget, RunUndo, TargetItem, undo_run};
pub use set::{
    MigrateOutcome, SetOutcome, apply_to_file, migrate_file, parse_assignment, set_paths,
    write_atomic,
};
pub use studio::{Event, PLAYHEAD_INTERVAL, Studio, StudioConfig, StudioHandle};
pub use target::{Target, TargetChange, target_changes, target_value};
pub use view::{
    AcidStepView, FxView, LaneView, MixView, NoteView, ParamView, PatternView, SectionView,
    SongView, StudioState, TrackView,
};
pub use wire::wire_types_ts;
