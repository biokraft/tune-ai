//! `tune set`'s write path: apply edits by path, write once, log them.

use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;
use tune_song::{FORMAT_VERSION, Migration};

use crate::diff::{Change, diff_songs, version_change};
use crate::doc::SongDoc;
use crate::error::{ErrorView, StudioError};
use crate::log::{EditLog, Source, text_hash};
use crate::path::SongPath;

/// Splits `PATH=VALUE` at the first `=`. The value is a TOML value
/// (`820`, `true`, `"hard"`); text that is not one is a string (`hard`); an
/// empty value is `null`.
pub fn parse_assignment(s: &str) -> Result<(SongPath, Value), String> {
    let (path, value) = s
        .split_once('=')
        .ok_or_else(|| format!("bad assignment `{s}`: expected PATH=VALUE"))?;
    let path = path.trim();
    let path: SongPath = path.parse().map_err(|e: String| {
        if looks_like_track_field(path) {
            format!(
                "`{path}` does not exist in this song (a track param id is \
                 <prefix>.<name>, e.g. tracks.acid.acid303.cutoff; `tune params` lists them)"
            )
        } else {
            e
        }
    })?;
    Ok((path, parse_value(value.trim())))
}

/// `tracks.<track>.<word>` with a one-word field: a misspelt track field or
/// a param id missing its prefix.
fn looks_like_track_field(path: &str) -> bool {
    path.strip_prefix("tracks.")
        .and_then(|r| r.split_once('.'))
        .is_some_and(|(track, field)| {
            !track.is_empty() && !field.is_empty() && !field.contains('.')
        })
}

fn parse_value(text: &str) -> Value {
    if text.is_empty() {
        return Value::Null;
    }
    text.parse::<toml_edit::Value>()
        .ok()
        .and_then(|v| from_toml(&v))
        .unwrap_or_else(|| Value::String(text.to_owned()))
}

fn from_toml(v: &toml_edit::Value) -> Option<Value> {
    use toml_edit::Value as T;
    Some(match v {
        T::String(s) => Value::String(s.value().clone()),
        T::Integer(i) => Value::from(*i.value()),
        T::Float(f) => serde_json::Number::from_f64(*f.value())?.into(),
        T::Boolean(b) => Value::Bool(*b.value()),
        T::Array(a) => Value::Array(a.iter().map(from_toml).collect::<Option<_>>()?),
        T::Datetime(_) | T::InlineTable(_) => return None,
    })
}

/// What a [`set_paths`] call changed.
#[derive(Clone, Debug, PartialEq)]
pub struct SetOutcome {
    pub changes: Vec<Change>,
    /// FNV-1a hash of the file after the write.
    pub file_hash: String,
    /// The format version the file was in when the write migrated it to
    /// the newest; `None` when it was already the newest or nothing was
    /// written.
    pub migrated_from: Option<u32>,
}

/// Applies every assignment to one document, writes the file once and logs
/// one entry per change, all carrying the file's hash. If any assignment
/// fails nothing is written. A file in an older format is written in the
/// newest, with a `song.version` change first; when no value changes,
/// nothing is written and the file keeps its format.
pub fn set_paths(
    song_path: &Path,
    assignments: &[(SongPath, Value)],
    source: Source,
) -> Result<SetOutcome, StudioError> {
    let text = fs::read_to_string(song_path)
        .map_err(|e| StudioError::Io(format!("cannot read {}: {e}", song_path.display())))?;
    let mut doc = SongDoc::parse(&text)
        .map_err(|errs| StudioError::Load(errs.iter().map(ErrorView::from).collect()))?;
    let before = doc.song().clone();
    // The text as parsed: the migrated one for a file in an older format.
    let start = doc.text();
    for (path, value) in assignments {
        doc.set(path, value)?;
    }
    let new_text = doc.text();
    if new_text == start {
        return Ok(SetOutcome {
            changes: Vec::new(),
            file_hash: text_hash(&text),
            migrated_from: None,
        });
    }
    let migrated_from = doc.migrated_from();
    let changes: Vec<Change> = migrated_from
        .and_then(|from| version_change(from, FORMAT_VERSION))
        .into_iter()
        .chain(diff_songs(&before, doc.song()))
        .collect();
    write_atomic(song_path, &new_text)
        .map_err(|e| StudioError::Io(format!("cannot write {}: {e}", song_path.display())))?;
    let file_hash = text_hash(&new_text);
    EditLog::for_song(song_path)
        .append_changes(source, &changes, &file_hash)
        .map_err(|e| StudioError::Io(format!("cannot write the edit log: {e}")))?;
    Ok(SetOutcome {
        changes,
        file_hash,
        migrated_from,
    })
}

/// What [`migrate_file`] did.
#[derive(Clone, Debug, PartialEq)]
pub enum MigrateOutcome {
    /// The file is already in the newest format; nothing was touched.
    Current,
    Migrated {
        /// The format the file was in.
        from: u32,
        /// The file in the newest format (written, unless it was a dry run).
        text: String,
        /// FNV-1a hash of the written file; `None` on a dry run.
        file_hash: Option<String>,
    },
}

/// Upgrades a song file to the newest format, keeping its comments. Writes
/// atomically and logs one entry `{path: "song.version", old: from, new:
/// <newest>}` with `source`, unless `dry_run`, which writes and logs nothing.
/// An invalid file is left as it is and its errors are returned.
pub fn migrate_file(
    song_path: &Path,
    source: Source,
    dry_run: bool,
) -> Result<MigrateOutcome, StudioError> {
    let text = fs::read_to_string(song_path)
        .map_err(|e| StudioError::Io(format!("cannot read {}: {e}", song_path.display())))?;
    let migration = tune_song::migrate(&text)
        .map_err(|errs| StudioError::Load(errs.iter().map(ErrorView::from).collect()))?;
    let Migration::Migrated { from, text } = migration else {
        return Ok(MigrateOutcome::Current);
    };
    if dry_run {
        return Ok(MigrateOutcome::Migrated {
            from,
            text,
            file_hash: None,
        });
    }
    write_atomic(song_path, &text)
        .map_err(|e| StudioError::Io(format!("cannot write {}: {e}", song_path.display())))?;
    let file_hash = text_hash(&text);
    let changes: Vec<Change> = version_change(from, FORMAT_VERSION).into_iter().collect();
    EditLog::for_song(song_path)
        .append_changes(source, &changes, &file_hash)
        .map_err(|e| StudioError::Io(format!("cannot write the edit log: {e}")))?;
    Ok(MigrateOutcome::Migrated {
        from,
        text,
        file_hash: Some(file_hash),
    })
}

/// Writes `text` to `<dir>/.song.toml.tune-tmp`, then renames it over
/// `song_path`, so a reader never sees a half-written file. The temp file is
/// removed when anything fails.
pub fn write_atomic(song_path: &Path, text: &str) -> io::Result<()> {
    let dir = song_path.parent().unwrap_or_else(|| Path::new(""));
    let tmp = dir.join(".song.toml.tune-tmp");
    let result = fs::write(&tmp, text).and_then(|()| fs::rename(&tmp, song_path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
