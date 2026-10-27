//! `tune do`: run an action on a song, or a batch of song edits.
//!
//! With the song open in a studio (the app or `tune serve`), the actions go
//! through it: they show up at once and are one undo step each, and live
//! and history actions work. Without one, edits go straight into the file
//! and the others need the studio.

use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};
use tune_studio::{
    Action, ActionKind, ActionsOutcome, Origin, StudioError, action_from_cli, apply_to_file,
    find_studio, studio_dir,
};

use super::set::{RunFlags, change_lines, failures, file_hash};
use super::tag_migrated;
use crate::report::{Failure, Success, finish};

pub struct Args<'a> {
    pub song: &'a Path,
    pub action: Option<&'a str>,
    pub args: &'a [String],
    pub batch: bool,
    pub run: RunFlags,
    pub json: bool,
}

pub fn run(a: &Args) -> ExitCode {
    finish(a.json, do_actions(a))
}

fn do_actions(a: &Args) -> Result<Success, Vec<Failure>> {
    let fail = |m: String| vec![Failure::msg(m)];
    let run = a.run.resolve().map_err(fail)?;
    let actions = if a.batch {
        read_batch().map_err(fail)?
    } else {
        let name = a
            .action
            .ok_or_else(|| fail("give an action or --batch".into()))?;
        vec![action_from_cli(name, a.args).map_err(fail)?]
    };
    if a.batch
        && let Some(live) = actions.iter().find(|x| x.kind() != ActionKind::Edit)
    {
        return Err(fail(
            StudioError::EditsOnly {
                action: live.name().to_owned(),
            }
            .to_string(),
        ));
    }
    let out =
        run_actions(a.song, &actions, &Origin::claude(run)).map_err(|e| failures(a.song, e))?;
    let mut lines = Vec::new();
    match actions.as_slice() {
        // Undo and redo print what they changed, like an edit.
        [one] if one.kind() == ActionKind::Live => lines.push(format!("ran {}", one.name())),
        _ => lines.push(change_lines(&out.changes)),
    }
    lines.extend(out.results.iter().flat_map(result_lines));
    let mut json = json!({
        "via": out.via,
        "changes": out.changes,
        "results": out.results,
        "file_hash": file_hash(a.song, &out),
    });
    tag_migrated(&mut json, out.migrated_from);
    Ok(Success {
        text: lines.join("\n"),
        json,
    })
}

/// Runs `actions` through the studio that has `song` open, else edits into
/// the file (live and history actions then give
/// [`StudioError::NeedsStudio`]). An error once the studio has the request
/// (no answer, or its refusal) is the result: the file is never tried after.
pub(crate) fn run_actions(
    song: &Path,
    actions: &[Action],
    origin: &Origin,
) -> Result<ActionsOutcome, StudioError> {
    // No endpoint directory: no studio can have written one there.
    match studio_dir().ok().and_then(|dir| find_studio(&dir, song)) {
        Some(link) => link.run(actions, origin),
        None => apply_to_file(song, actions, origin),
    }
}

/// A batch: a JSON array of action objects on stdin.
fn read_batch() -> Result<Vec<Action>, String> {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read the batch from stdin: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("bad batch: {e}"))
}

/// What an action's result says, one line per fact; nothing for `null`.
fn result_lines(result: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(id) = result.get("id").and_then(Value::as_str) {
        lines.push(format!("added device {id}"));
    }
    if let Some(removed) = result.get("removed") {
        for lane in removed["lanes"].as_array().into_iter().flatten() {
            lines.push(format!(
                "removed lane {} at bar {}",
                lane["param"].as_str().unwrap_or("?"),
                lane["at"]
            ));
        }
        for p in removed["placements"].as_array().into_iter().flatten() {
            lines.push(format!(
                "removed placement {} on {} at bar {}",
                p["clip"].as_str().unwrap_or("?"),
                p["track"].as_str().unwrap_or("?"),
                p["at"]
            ));
        }
        if let Some(src) = removed["duck_source"].as_str() {
            lines.push(format!("cleared duck source {src}"));
        }
    }
    if lines.is_empty() && !result.is_null() {
        lines.push(result.to_string());
    }
    lines
}
