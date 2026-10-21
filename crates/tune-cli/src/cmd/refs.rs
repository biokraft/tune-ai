//! `tune refs`.

use std::fmt::Write as _;
use std::process::ExitCode;

use serde_json::{Value, json};

use crate::refs::{RefSource, find_refs};
use crate::report::{Failure, Success, finish};

pub fn run(json: bool) -> ExitCode {
    finish(json, list())
}

fn list() -> Result<Success, Vec<Failure>> {
    let cwd = std::env::current_dir().map_err(|e| vec![Failure::msg(e.to_string())])?;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let refs = find_refs(&cwd, home.as_deref()).map_err(|m| vec![Failure::msg(m)])?;
    let text = if refs.entries.is_empty() {
        "no references: add [refs] to tune.local.toml or put files in refs/".to_string()
    } else {
        let mut t = String::new();
        for e in &refs.entries {
            if !t.is_empty() {
                t.push('\n');
            }
            let _ = write!(
                t,
                "{}  {}  {}",
                e.name,
                if e.exists { "ok" } else { "missing" },
                e.configured
            );
        }
        t
    };
    let list: Vec<Value> = refs
        .entries
        .iter()
        .map(|e| {
            json!({
                "name": e.name,
                "path": e.configured,
                "resolved": e.path.display().to_string(),
                "exists": e.exists,
                "from": match e.from {
                    RefSource::Config => "config",
                    RefSource::RefsDir => "refs_dir",
                },
            })
        })
        .collect();
    Ok(Success {
        text,
        json: json!({
            "root": refs.root.map(|p| p.display().to_string()),
            "config": refs.config.map(|p| p.display().to_string()),
            "refs": list,
        }),
    })
}
