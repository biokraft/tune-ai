//! `tune set`: Claude's tagged, all-or-nothing edits by path, through the
//! open studio when there is one (see `tune do`).

use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};
use tune_studio::{
    Action, ActionsOutcome, Change, Origin, RunTag, SongPath, StudioError, parse_assignment,
    text_hash,
};

use super::do_action::run_actions;
use super::tag_migrated;
use crate::report::{Failure, Success, finish};

pub fn run(song: &Path, assignments: &[String], run: &RunFlags, json: bool) -> ExitCode {
    finish(json, set(song, assignments, run))
}

fn set(song: &Path, assignments: &[String], run: &RunFlags) -> Result<Success, Vec<Failure>> {
    let parsed = assignments
        .iter()
        .map(|a| parse_assignment(a))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|m| vec![Failure::msg(m)])?;
    let run = run.resolve().map_err(|m| vec![Failure::msg(m)])?;
    let actions: Vec<Action> = parsed
        .into_iter()
        .map(|(path, value)| Action::SetValue { path, value })
        .collect();
    let out = run_actions(song, &actions, &Origin::claude(run)).map_err(|e| failures(song, e))?;
    let text = change_lines(&out.changes);
    // `migrated_from` only for a migrated file, as every command reports it.
    let mut json = json!({
        "via": out.via,
        "changes": out.changes,
        "file_hash": file_hash(song, &out),
    });
    tag_migrated(&mut json, out.migrated_from);
    Ok(Success { text, json })
}

/// The run a command was given: `--run`/`--run-title`, else `TUNE_RUN` and
/// `TUNE_RUN_TITLE`.
#[derive(Clone, Debug, Default)]
pub struct RunFlags {
    pub run: Option<String>,
    pub run_title: Option<String>,
}

impl RunFlags {
    pub fn resolve(&self) -> Result<Option<RunTag>, String> {
        let env = |name| std::env::var(name).ok().filter(|v| !v.is_empty());
        if self.run.is_none() && self.run_title.is_some() {
            return Err("--run-title needs a run: give --run ID too".to_owned());
        }
        let Some(id) = self.run.clone().or_else(|| env("TUNE_RUN")) else {
            return Ok(None);
        };
        let title = self.run_title.clone().or_else(|| env("TUNE_RUN_TITLE"));
        RunTag::new(&id, title.as_deref()).map(Some)
    }
}

/// The file's hash after the call: the written one, else that of the file
/// as it is (nothing was written).
pub(crate) fn file_hash(song: &Path, out: &ActionsOutcome) -> Option<String> {
    out.file_hash
        .clone()
        .or_else(|| std::fs::read_to_string(song).ok().map(|t| text_hash(&t)))
}

/// One line per change as `tune set` prints them; "no changes" for none.
pub(crate) fn change_lines(changes: &[Change]) -> String {
    let text = changes
        .iter()
        .map(|c| match c.path {
            SongPath::Version => format!(
                "migrated the file from version {} to {}",
                show(&c.old),
                show(&c.new)
            ),
            _ => format!("set {} {} -> {}", c.path, show(&c.old), show(&c.new)),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        "no changes".to_owned()
    } else {
        text
    }
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
