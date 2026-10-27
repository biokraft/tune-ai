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
    Action, ActionKind, ActionsOutcome, KeptReason, KeptTarget, Origin, RunUndo, STUDIO_DIR_ENV,
    Source, StudioError, action_from_cli, apply_to_file, find_studio, studio_dir,
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
        // A run undo says what it reverted and kept.
        [Action::UndoRun { .. }] => {}
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
///
/// A `TUNE_STUDIO_DIR` that cannot be used (a relative path) is an error:
/// a studio started with a good one may have the song open. With no
/// `TUNE_STUDIO_DIR` and no `HOME` there is no endpoint directory, so no
/// studio can be found: the file is edited, with a warning.
pub(crate) fn run_actions(
    song: &Path,
    actions: &[Action],
    origin: &Origin,
) -> Result<ActionsOutcome, StudioError> {
    let dir = match studio_dir() {
        Ok(dir) => Some(dir),
        Err(e) if std::env::var_os(STUDIO_DIR_ENV).is_some_and(|d| !d.is_empty()) => {
            return Err(StudioError::Io(e.to_string()));
        }
        Err(_) => {
            eprintln!("warning: HOME is not set; a running studio cannot be found");
            None
        }
    };
    match dir.and_then(|dir| find_studio(&dir, song)) {
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
    if let Ok(undo) = serde_json::from_value::<RunUndo>(result.clone()) {
        return run_undo_lines(&undo);
    }
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
        for slot in removed["slots"].as_array().into_iter().flatten() {
            lines.push(format!(
                "removed slot {} on {} in scene {}",
                slot["clip"].as_str().unwrap_or("?"),
                slot["track"].as_str().unwrap_or("?"),
                slot["scene"].as_str().unwrap_or("?")
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

/// `undid run r1: 4 reverted, 1 kept`, then one line per kept target.
fn run_undo_lines(undo: &RunUndo) -> Vec<String> {
    let head = format!(
        "undid run {}: {} reverted, {} kept",
        undo.run,
        undo.reverted.len(),
        undo.kept.len()
    );
    std::iter::once(head)
        .chain(undo.kept.iter().map(kept_line))
        .collect()
}

/// `kept <target>: <why>`; a placement or stretch is named by its bar.
fn kept_line(k: &KeptTarget) -> String {
    let at = k
        .item
        .as_ref()
        .and_then(|i| i.get("at"))
        .map_or_else(String::new, |at| format!(" at bar {at}"));
    let why = match (k.reason, k.by) {
        (KeptReason::StillUsed, _) => {
            format!("still used: {}", k.message.as_deref().unwrap_or("refused"))
        }
        (KeptReason::ChangedLater, Some(Source::Human)) => "changed later by human".to_owned(),
        (KeptReason::ChangedLater, Some(Source::Claude)) => {
            "changed later by another run".to_owned()
        }
        (KeptReason::ChangedLater, Some(Source::External) | None) => {
            "changed later outside the studio".to_owned()
        }
    };
    format!("kept {}{at}: {why}", k.target)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn kept_lines_say_why() {
        let kept = |item, reason, by, message: Option<&str>| KeptTarget {
            target: "tracks.acid.play".parse().unwrap(),
            item,
            reason,
            by,
            message: message.map(str::to_owned),
        };
        let changed = KeptReason::ChangedLater;
        let lines: Vec<String> = [
            kept(None, changed, Some(Source::Human), None),
            kept(None, changed, Some(Source::Claude), None),
            kept(None, changed, Some(Source::External), None),
            kept(Some(json!({ "at": 3 })), changed, None, None),
            kept(
                None,
                KeptReason::StillUsed,
                None,
                Some("unknown clip `lead`"),
            ),
        ]
        .iter()
        .map(kept_line)
        .collect();
        assert_eq!(
            lines,
            [
                "kept tracks.acid.play: changed later by human",
                "kept tracks.acid.play: changed later by another run",
                "kept tracks.acid.play: changed later outside the studio",
                "kept tracks.acid.play at bar 3: changed later outside the studio",
                "kept tracks.acid.play: still used: unknown clip `lead`",
            ]
        );
    }
}
