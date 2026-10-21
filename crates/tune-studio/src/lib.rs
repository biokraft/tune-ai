//! The studio core for tune-ai: edits to a song file, made in place.
//!
//! The producer (through the app) and Claude (as text) edit the same
//! `song.toml`. Claude's files carry comments for the producer to learn
//! from, so every edit here changes only the value it names and keeps the
//! rest of the file byte for byte. Headless: no UI, audio or async runtime.

#![forbid(unsafe_code)]

mod command;
mod core;
mod diff;
mod doc;
mod error;
mod log;
mod path;
mod set;
mod view;

pub use command::{Command, DrumHitDto};
pub use core::{OWN_WRITE_RING, PendingEdit, StudioCore, UNDO_DEPTH, UndoEntry, WRITE_DEBOUNCE};
pub use diff::{Change, diff_songs, is_whole_replace};
pub use doc::{EditError, SongDoc, effective_value};
pub use error::{ErrorView, StudioError};
pub use log::{EditEntry, EditLog, Source, text_hash};
pub use path::{SongPath, TrackField};
pub use set::{SetOutcome, parse_assignment, set_paths, write_atomic};
pub use view::{
    AcidStepView, FxView, LaneView, ParamView, PatternView, SectionView, SongView, StudioState,
    TrackView,
};
