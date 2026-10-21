//! The song commands. Each takes parsed arguments and returns the exit code.

use std::path::Path;

use tune_song::Song;

use crate::report::{Failure, load_failures};

pub mod analyze;
pub mod changes;
pub mod info;
pub mod params;
pub mod play;
pub mod refs;
pub mod render;
pub mod serve;
pub mod set;
pub mod validate;

/// Loads a song, mapping failures to reportable ones.
pub(crate) fn load(path: &Path) -> Result<Song, Vec<Failure>> {
    tune_song::load_song(path).map_err(|e| load_failures(path, e))
}

/// Sample rate a song renders at.
pub(crate) fn sample_rate(song: &Song) -> u32 {
    song.sample_rate.unwrap_or(48_000)
}
