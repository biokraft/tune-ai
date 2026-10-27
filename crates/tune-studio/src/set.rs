//! `tune set`'s write path: apply edits by path, write once, log them.

use std::fs;
use std::io;
use std::path::Path;

use jiff::Timestamp;
use serde_json::Value;
use tune_song::{FORMAT_VERSION, Migration};

use crate::action::{Action, ActionsOutcome, ApplyCtx, Origin, Via, apply_all, parse_value};
use crate::diff::{Change, version_change};
use crate::doc::SongDoc;
use crate::error::{ErrorView, StudioError};
use crate::log::{ActionRecord, EditLog, Source, entries_for, text_hash};
use crate::path::SongPath;
use crate::target::target_changes;

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
/// nothing is written and the file keeps its format. A shorthand for
/// [`apply_to_file`] with one `set_value` per assignment.
pub fn set_paths(
    song_path: &Path,
    assignments: &[(SongPath, Value)],
    source: Source,
) -> Result<SetOutcome, StudioError> {
    let actions: Vec<Action> = assignments
        .iter()
        .map(|(path, value)| Action::SetValue {
            path: path.clone(),
            value: value.clone(),
        })
        .collect();
    let out = apply_to_file(song_path, &actions, &Origin { source, run: None })?;
    let file_hash = match out.file_hash {
        Some(h) => h,
        // Nothing written: the hash of the file as it is.
        None => {
            let text = fs::read_to_string(song_path).map_err(|e| {
                StudioError::Io(format!("cannot read {}: {e}", song_path.display()))
            })?;
            text_hash(&text)
        }
    };
    Ok(SetOutcome {
        changes: out.changes,
        file_hash,
        migrated_from: out.migrated_from,
    })
}

/// All or nothing: applies every action to one document, writes the file once
/// and logs one entry per change, each with its action's name and the
/// origin's run, then the write's record. A file in an older format is
/// written in the newest with a `song.version` change first (no action
/// name). No change: nothing written or logged. A live or history action
/// gives [`StudioError::NeedsStudio`].
pub fn apply_to_file(
    song_path: &Path,
    actions: &[Action],
    origin: &Origin,
) -> Result<ActionsOutcome, StudioError> {
    let text = fs::read_to_string(song_path)
        .map_err(|e| StudioError::Io(format!("cannot read {}: {e}", song_path.display())))?;
    let mut doc = SongDoc::parse(&text)
        .map_err(|errs| StudioError::Load(errs.iter().map(ErrorView::from).collect()))?;
    let log = EditLog::for_song(song_path);
    let ctx = ApplyCtx { log: Some(&log) };
    // The text as parsed: the migrated one for a file in an older format.
    let start = doc.text();
    let before = doc.song().clone();
    let migrated_from = doc.migrated_from();
    let applied = apply_all(
        &mut doc,
        actions,
        &ctx,
        migrated_from.unwrap_or(FORMAT_VERSION),
        |e, action| match e {
            StudioError::NotAnEdit => StudioError::NeedsStudio {
                action: action.name().to_owned(),
            },
            other => other,
        },
    )?;
    let new_text = doc.text();
    if new_text == start {
        return Ok(ActionsOutcome {
            via: Via::File,
            changes: Vec::new(),
            results: applied.results,
            file_hash: None,
            migrated_from: None,
        });
    }
    write_atomic(song_path, &new_text)
        .map_err(|e| StudioError::Io(format!("cannot write {}: {e}", song_path.display())))?;
    let file_hash = text_hash(&new_text);
    let ts = Timestamp::now();
    let run = origin.run.as_ref();
    let entries = applied.entries(ts, origin.source, &file_hash, run);
    let targets = target_changes(&before, doc.song());
    let names = actions.iter().map(Action::name);
    let record = ActionRecord::new(ts, origin.source, names, run, targets, &file_hash);
    log.append_logged(&entries, &record)
        .map_err(|e| StudioError::Io(format!("cannot write the edit log: {e}")))?;
    Ok(ActionsOutcome {
        via: Via::File,
        changes: applied.changes,
        results: applied.results,
        file_hash: Some(file_hash),
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
/// <newest>}` with `source` and a `migrate` record, unless `dry_run`, which
/// writes and logs nothing.
/// An invalid file is left as it is and its errors are returned.
pub fn migrate_file(
    song_path: &Path,
    source: Source,
    dry_run: bool,
) -> Result<MigrateOutcome, StudioError> {
    let text = fs::read_to_string(song_path)
        .map_err(|e| StudioError::Io(format!("cannot read {}: {e}", song_path.display())))?;
    let load = |errs: Vec<tune_song::SongError>| {
        StudioError::Load(errs.iter().map(ErrorView::from).collect())
    };
    let migration = tune_song::migrate(&text).map_err(load)?;
    let Migration::Migrated {
        from,
        text: migrated,
    } = migration
    else {
        return Ok(MigrateOutcome::Current);
    };
    if dry_run {
        return Ok(MigrateOutcome::Migrated {
            from,
            text: migrated,
            file_hash: None,
        });
    }
    // The format changes, not the song: no targets, unless a bug says so.
    let targets = target_changes(
        &tune_song::parse_song(&text).map_err(load)?,
        &tune_song::parse_song(&migrated).map_err(load)?,
    );
    write_atomic(song_path, &migrated)
        .map_err(|e| StudioError::Io(format!("cannot write {}: {e}", song_path.display())))?;
    let file_hash = text_hash(&migrated);
    let changes: Vec<Change> = version_change(from, FORMAT_VERSION).into_iter().collect();
    let ts = Timestamp::now();
    let entries = entries_for(ts, source, &changes, &file_hash, None, None);
    let record = ActionRecord::new(ts, source, ["migrate"], None, targets, &file_hash);
    EditLog::for_song(song_path)
        .append_logged(&entries, &record)
        .map_err(|e| StudioError::Io(format!("cannot write the edit log: {e}")))?;
    Ok(MigrateOutcome::Migrated {
        from,
        text: migrated,
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
