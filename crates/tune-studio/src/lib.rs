//! The studio core for tune-ai: edits to a song file, made in place.
//!
//! The producer (through the app) and Claude (as text) edit the same
//! `song.toml`. Claude's files carry comments for the producer to learn
//! from, so every edit here changes only the value it names and keeps the
//! rest of the file byte for byte. The [`Studio`] actor serves the open song
//! to any number of clients, plays it live and hot-reloads it. Headless: no
//! UI, audio device code or async runtime.

#![forbid(unsafe_code)]

mod command;
mod core;
mod diff;
mod doc;
mod error;
mod log;
mod path;
mod set;
mod studio;
mod view;

pub use command::{Command, DrumHitDto};
pub use core::{
    ExternalChange, OWN_WRITE_RING, PendingEdit, StudioCore, UNDO_DEPTH, UndoEntry, WRITE_DEBOUNCE,
};
pub use diff::{Change, diff_songs, is_whole_replace};
pub use doc::{EditError, SongDoc, effective_value, placement_json};
pub use error::{ErrorView, StudioError};
pub use log::{EditEntry, EditLog, Source, text_hash};
pub use path::{SongPath, TrackField};
pub use set::{
    MigrateOutcome, SetOutcome, migrate_file, parse_assignment, set_paths, write_atomic,
};
pub use studio::{Event, PLAYHEAD_INTERVAL, Studio, StudioConfig, StudioHandle};
pub use view::{
    AcidStepView, FxView, LaneView, MixView, ParamView, PatternView, SectionView, SongView,
    StudioState, TrackView,
};
