//! `tune changes`: read the edit log.

use std::path::Path;
use std::process::ExitCode;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde_json::json;
use tune_studio::{EditEntry, EditLog, Source};

use crate::cmd::set::show;
use crate::report::{Failure, Success, finish};

pub struct Args<'a> {
    pub song: &'a Path,
    pub since: Option<&'a str>,
    pub source: Option<Source>,
    pub json: bool,
}

pub fn run(a: &Args) -> ExitCode {
    finish(a.json, changes(a))
}

fn rfc3339(ts: &Timestamp) -> String {
    ts.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn changes(a: &Args) -> Result<Success, Vec<Failure>> {
    let since = a
        .since
        .map(|s| {
            s.parse::<Timestamp>()
                .map_err(|e| vec![Failure::msg(format!("bad --since `{s}`: {e}"))])
        })
        .transpose()?;
    let all = EditLog::for_song(a.song)
        .read_since(since)
        .map_err(|e| vec![Failure::msg(format!("cannot read the edit log: {e}"))])?;
    // `until` ignores the source filter, so a follow-up `--since` misses nothing.
    let until = all.last().map(|e| e.ts).or(since).map(|t| rfc3339(&t));
    let entries: Vec<&EditEntry> = all
        .iter()
        .filter(|e| a.source.is_none_or(|s| e.source == s))
        .collect();
    let tz = TimeZone::system();
    let text = entries
        .iter()
        .map(|e| {
            let source = serde_json::to_value(e.source)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let run = e
                .run
                .as_ref()
                .map_or(String::new(), |r| format!(" (run {r})"));
            format!(
                "{} {} {} {} -> {}{run}",
                e.ts.to_zoned(tz.clone()).strftime("%H:%M:%S"),
                source,
                e.path,
                show(&e.old),
                show(&e.new)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Success {
        text,
        json: json!({ "until": until, "changes": entries }),
    })
}
