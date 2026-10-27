//! `tune actions`: the action catalog.

use std::fmt::Write as _;
use std::process::ExitCode;

use serde_json::{Value, json};
use tune_studio::{ACTIONS, ActionSpec, ArgSpec};

use crate::report::{Success, finish};

fn arg_json(a: &ArgSpec) -> Value {
    json!({
        "name": a.name,
        "type": a.ty.name(),
        "required": a.required,
        "help": a.help,
    })
}

fn action_json(a: &ActionSpec) -> Value {
    json!({
        "name": a.name,
        "kind": a.kind.name(),
        "help": a.help,
        "args": a.args.iter().map(arg_json).collect::<Vec<_>>(),
    })
}

pub fn run(json: bool) -> ExitCode {
    let mut text = String::new();
    for a in ACTIONS {
        let _ = writeln!(text, "{}  {}  {}", a.name, a.kind.name(), a.help);
        for arg in a.args {
            let optional = if arg.required { "" } else { ", optional" };
            let _ = writeln!(
                text,
                "    {}: {}{optional}  {}",
                arg.name,
                arg.ty.name(),
                arg.help
            );
        }
    }
    finish(
        json,
        Ok(Success {
            text: text.trim_end().to_owned(),
            json: json!({ "actions": ACTIONS.iter().map(action_json).collect::<Vec<_>>() }),
        }),
    )
}
