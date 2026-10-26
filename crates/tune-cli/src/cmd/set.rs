//! `tune set`: Claude's tagged, all-or-nothing edits by path.

use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};
use tune_studio::{Source, StudioError, parse_assignment, set_paths};

use crate::report::{Failure, Success, finish};

pub fn run(song: &Path, assignments: &[String], json: bool) -> ExitCode {
    finish(json, set(song, assignments))
}

fn set(song: &Path, assignments: &[String]) -> Result<Success, Vec<Failure>> {
    let parsed = assignments
        .iter()
        .map(|a| parse_assignment(a))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|m| vec![Failure::msg(m)])?;
    let out = set_paths(song, &parsed, Source::Claude).map_err(|e| failures(song, e))?;
    let text = out
        .changes
        .iter()
        .map(|c| format!("set {} {} -> {}", c.path, show(&c.old), show(&c.new)))
        .collect::<Vec<_>>()
        .join("\n");
    let text = if text.is_empty() {
        "no changes".to_owned()
    } else {
        text
    };
    Ok(Success {
        text,
        json: json!({ "changes": out.changes, "file_hash": out.file_hash }),
    })
}

pub(crate) fn failures(song: &Path, e: StudioError) -> Vec<Failure> {
    match e {
        StudioError::Load(views) => views
            .into_iter()
            .map(|v| Failure {
                file: Some(song.display().to_string()),
                line: v.line,
                column: v.column,
                message: v.message,
            })
            .collect(),
        other => vec![Failure::msg(other.to_string())],
    }
}

/// A value as a person reads it: strings bare, everything else as JSON.
pub(crate) fn show(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
