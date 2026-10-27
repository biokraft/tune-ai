//! The song commands. Each takes parsed arguments and returns the exit code.

use std::path::Path;

use serde_json::Value;
use tune_song::{FORMAT_VERSION, Song};

use crate::report::{Failure, Success, load_failures};

pub mod actions;
pub mod analyze;
pub mod changes;
pub mod compare;
pub mod do_action;
pub mod info;
pub mod migrate;
pub mod params;
pub mod play;
pub mod refs;
pub mod render;
pub mod serve;
pub mod set;
pub mod validate;

/// A loaded song, and the format version it was read from when that was not
/// the newest (it was migrated in memory; the file is untouched).
pub(crate) struct Loaded {
    pub song: Song,
    pub migrated_from: Option<u32>,
}

/// Loads a song, mapping failures to reportable ones.
pub(crate) fn load(path: &Path) -> Result<Loaded, Vec<Failure>> {
    let parsed = tune_song::load(path).map_err(|e| load_failures(path, e))?;
    Ok(Loaded {
        migrated_from: (parsed.version != FORMAT_VERSION).then_some(parsed.version),
        song: parsed.song,
    })
}

/// What a command tells the reader when it read a v1 song as v2 in memory.
pub(crate) fn migration_note(path: &Path, from: u32) -> String {
    format!(
        "note: {} is a version {from} song file; it was read as version {FORMAT_VERSION} in \
         memory (run `tune migrate {}` to upgrade the file)",
        path.display(),
        path.display()
    )
}

/// Adds `"migrated_from"` to a JSON object when the song was migrated in
/// memory, else leaves it alone.
pub(crate) fn tag_migrated(json: &mut Value, migrated_from: Option<u32>) {
    if let (Some(from), Value::Object(map)) = (migrated_from, json) {
        map.insert("migrated_from".into(), Value::from(from));
    }
}

/// Adds the migration to a successful result: `migrated_from` in its JSON,
/// and in text mode the note on stderr (stdout stays the result).
pub(crate) fn note_migration(
    path: &Path,
    migrated_from: Option<u32>,
    json: bool,
    success: &mut Success,
) {
    tag_migrated(&mut success.json, migrated_from);
    if let (Some(from), false) = (migrated_from, json) {
        eprintln!("{}", migration_note(path, from));
    }
}

/// Sample rate a song renders at.
pub(crate) fn sample_rate(song: &Song) -> u32 {
    song.sample_rate.unwrap_or(48_000)
}
