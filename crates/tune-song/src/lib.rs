//! Song files for tune-ai: the typed song model, the TOML parser and its
//! validation.
//!
//! A song file is the shared source of truth that both the producer (through
//! the app) and Claude (as text) edit, so [`parse`] reports every mistake
//! at once, each with a 1-based `line:column`. It reads format versions 1
//! and 2; [`Song::to_toml`] writes version 2, and [`migrate`] rewrites a
//! version 1 file as version 2, keeping its comments. [`move_section`] and its
//! siblings edit the arrangement one section at a time.

#![forbid(unsafe_code)]

mod arrange;
mod error;
mod migrate;
mod model;
mod raw;
pub mod toml_doc;
mod validate;
mod watch;
mod write;

use std::path::Path;

pub use arrange::{
    ArrangeError, Arranged, LaneOrigin, delete_section, duplicate_section, move_section,
    resize_section, set_section_play,
};
pub use error::{LoadError, SongError};
pub use migrate::{Migration, migrate};
pub use model::{
    Clip, DEFAULT_VELOCITY, Device, Lane, MIX_TRACK_ID, MasterMode, Mix, MixUnit, NOTE_CLIP_BARS,
    Note, NoteClip, Placement, SECTION_BARS, Section, SectionPlay, Song, TICKS_PER_STEP, Track,
    find_mix_param, lane_value, parse_pitch, pitch_name, round_split_value, section_bars_error,
};
pub use tune_modules::{ClipKind, ModuleDescriptor, Registry, SettingSpec};
pub use watch::{RawReload, Reload, SongWatcher};
pub use write::{notes_toml, placements_toml};

/// The newest song format version, the one [`Song::to_toml`] writes.
pub const FORMAT_VERSION: u32 = 2;

/// A parsed song and the format version its text was written in.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub song: Song,
    pub version: u32,
}

/// Parses and validates a song file (format 1 or 2) against the built-in
/// modules. On failure every problem is returned, ordered by position.
pub fn parse(src: &str) -> Result<Parsed, Vec<SongError>> {
    parse_with(src, Registry::builtin())
}

/// Like [`parse`], with the modules of `registry`: an instrument or an fx
/// `type` must name one of them.
pub fn parse_with(src: &str, registry: &Registry) -> Result<Parsed, Vec<SongError>> {
    let mut errors = error::Errors::default();
    let doc = match toml::de::DeTable::parse(src) {
        Ok(doc) => doc,
        Err(e) => {
            match e.span() {
                Some(span) => errors.at(&span, e.message().trim_end()),
                None => errors.global(e.message().trim_end()),
            }
            return Err(errors.finish(src));
        }
    };
    let Some(raw) = raw::read(&doc, registry, &mut errors) else {
        return Err(errors.finish(src));
    };
    let version = raw.version;
    match validate::validate(raw, registry, &mut errors) {
        Some(song) if errors.is_empty() => Ok(Parsed { song, version }),
        _ => Err(errors.finish(src)),
    }
}

/// [`parse`], keeping only the song.
pub fn parse_song(src: &str) -> Result<Song, Vec<SongError>> {
    parse(src).map(|p| p.song)
}

/// Reads and parses the song file at `path`.
pub fn load(path: &Path) -> Result<Parsed, LoadError> {
    let src = std::fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_owned(),
        source,
    })?;
    parse(&src).map_err(LoadError::Invalid)
}

/// [`load`], keeping only the song.
pub fn load_song(path: &Path) -> Result<Song, LoadError> {
    load(path).map(|p| p.song)
}
