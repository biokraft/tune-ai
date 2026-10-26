//! Song files for tune-ai: the typed song model, the TOML parser and its
//! validation.
//!
//! A song file is the shared source of truth that both the producer (through
//! the app) and Claude (as text) edit, so [`parse_song`] reports every
//! mistake at once, each with a 1-based `line:column`.

#![forbid(unsafe_code)]

mod error;
mod model;
mod raw;
mod validate;
mod watch;
mod write;

use std::path::Path;

pub use error::{LoadError, SongError};
pub use model::{
    Clip, Device, Lane, MIX_TRACK_ID, MasterMode, Mix, MixUnit, Placement, Section, SectionPlay,
    Song, Track, find_mix_param, lane_value,
};
pub use tune_modules::{ClipKind, ModuleDescriptor, Registry, SettingSpec};
pub use watch::{RawReload, Reload, SongWatcher};

/// Parses and validates a song file against the built-in modules. On
/// failure every problem is returned, ordered by position.
pub fn parse_song(src: &str) -> Result<Song, Vec<SongError>> {
    parse_song_with(src, Registry::builtin())
}

/// Like [`parse_song`], with the modules of `registry`: an instrument or an
/// fx `type` must name one of them.
pub fn parse_song_with(src: &str, registry: &Registry) -> Result<Song, Vec<SongError>> {
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
    let raw = raw::read(&doc, registry, &mut errors);
    match validate::validate(raw, registry, &mut errors) {
        Some(song) if errors.is_empty() => Ok(song),
        _ => Err(errors.finish(src)),
    }
}

/// Reads and parses the song file at `path`.
pub fn load_song(path: &Path) -> Result<Song, LoadError> {
    let src = std::fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_owned(),
        source,
    })?;
    parse_song(&src).map_err(LoadError::Invalid)
}
