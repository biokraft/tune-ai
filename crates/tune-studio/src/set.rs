//! `tune set`'s write path: apply edits by path, write once, log them.

use std::fs;
use std::io;
use std::path::Path;

use jiff::Timestamp;
use serde_json::Value;

use crate::diff::{Change, diff_songs};
use crate::doc::SongDoc;
use crate::error::{ErrorView, StudioError};
use crate::log::{EditEntry, EditLog, Source, text_hash};
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
}

/// Applies every assignment to one document, writes the file once and logs
/// one entry per change, all carrying the file's hash. If any assignment
/// fails nothing is written.
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
    for (path, value) in assignments {
        doc.set(path, value)?;
    }
    let changes = diff_songs(&before, doc.song());
    let new_text = doc.text();
    let file_hash = text_hash(&new_text);
    if new_text == text {
        return Ok(SetOutcome { changes, file_hash });
    }
    write_atomic(song_path, &new_text)
        .map_err(|e| StudioError::Io(format!("cannot write {}: {e}", song_path.display())))?;
    let ts = Timestamp::now();
    let entries: Vec<EditEntry> = changes
        .iter()
        .map(|c| EditEntry {
            ts,
            source,
            path: c.path.to_string(),
            old: c.old.clone(),
            new: c.new.clone(),
            file_hash: Some(file_hash.clone()),
        })
        .collect();
    EditLog::for_song(song_path)
        .append(&entries)
        .map_err(|e| StudioError::Io(format!("cannot write the edit log: {e}")))?;
    Ok(SetOutcome { changes, file_hash })
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
