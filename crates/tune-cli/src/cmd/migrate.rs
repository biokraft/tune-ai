//! `tune migrate`: upgrade a song file to the newest format, keeping comments.

use std::path::Path;
use std::process::ExitCode;

use serde_json::json;
use tune_song::FORMAT_VERSION;
use tune_studio::{MigrateOutcome, Source, migrate_file};

use super::set::failures;
use crate::report::{Failure, Success, finish};

pub fn run(song: &Path, dry_run: bool, json: bool) -> ExitCode {
    finish(json, migrate(song, dry_run))
}

fn migrate(song: &Path, dry_run: bool) -> Result<Success, Vec<Failure>> {
    let outcome = migrate_file(song, Source::Claude, dry_run).map_err(|e| failures(song, e))?;
    Ok(match outcome {
        MigrateOutcome::Current => Success {
            text: format!("already version {FORMAT_VERSION}: {}", song.display()),
            json: json!({ "migrated": false, "version": FORMAT_VERSION }),
        },
        MigrateOutcome::Migrated {
            from,
            text,
            file_hash: None,
        } => Success {
            // `finish` ends the line; the file's own final newline is kept.
            text: text.strip_suffix('\n').unwrap_or(&text).to_owned(),
            json: json!({
                "migrated": true,
                "dry_run": true,
                "from": from,
                "to": FORMAT_VERSION,
                "text": text,
            }),
        },
        MigrateOutcome::Migrated {
            from,
            file_hash,
            text: _,
        } => Success {
            text: format!(
                "migrated {} from version {from} to {FORMAT_VERSION} (comments kept)",
                song.display()
            ),
            json: json!({
                "migrated": true,
                "from": from,
                "to": FORMAT_VERSION,
                "file_hash": file_hash,
            }),
        },
    })
}
