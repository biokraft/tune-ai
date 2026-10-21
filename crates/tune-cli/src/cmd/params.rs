//! `tune params`.

use std::fmt::Write as _;
use std::process::ExitCode;

use serde_json::{Map, Value, json};
use tune_dsp::{Curve, DIST_PARAMS, EQ_PARAMS, ParamSpec, STRIP_PARAMS};
use tune_song::{InstrumentKind, MixUnit};

use crate::report::{Success, finish};

fn curve(c: Curve) -> &'static str {
    match c {
        Curve::Linear => "linear",
        Curve::Exponential => "exponential",
    }
}

fn spec_json(p: &ParamSpec) -> Value {
    json!({
        "id": p.id,
        "label": p.label,
        "unit": p.unit,
        "min": p.min,
        "max": p.max,
        "curve": curve(p.curve),
        "default": p.default,
        "help": p.help,
    })
}

pub fn run(json: bool) -> ExitCode {
    let instruments: Vec<(&str, &[ParamSpec])> = InstrumentKind::ALL
        .iter()
        .map(|k| (k.name(), k.params()))
        .collect();
    let effects: Vec<(&str, &[ParamSpec])> = vec![("distortion", DIST_PARAMS), ("eq", EQ_PARAMS)];
    let mut text = String::new();
    for (heading, groups) in [("instrument", &instruments), ("effect", &effects)] {
        for (name, table) in groups {
            let _ = writeln!(text, "{heading} {name}");
            for p in *table {
                let _ = writeln!(
                    text,
                    "  {:<24} {} {}..{} {} {} default {}\n      {}",
                    p.id,
                    p.label,
                    p.min,
                    p.max,
                    p.unit,
                    curve(p.curve),
                    p.default,
                    p.help
                );
            }
        }
    }
    let mix: Vec<(&str, &[ParamSpec])> = MixUnit::ALL
        .iter()
        .map(|u| (u.name(), u.params()))
        .collect();
    let write_table = |text: &mut String, heading: String, table: &[ParamSpec]| {
        let _ = writeln!(text, "{heading}");
        for p in table {
            let _ = writeln!(
                text,
                "  {:<24} {} {}..{} {} {} default {}\n      {}",
                p.id,
                p.label,
                p.min,
                p.max,
                p.unit,
                curve(p.curve),
                p.default,
                p.help
            );
        }
    };
    write_table(&mut text, "strip".to_string(), STRIP_PARAMS);
    for (name, table) in &mix {
        write_table(&mut text, format!("mix {name}"), table);
    }
    let group = |groups: &[(&str, &[ParamSpec])]| {
        Value::Object(
            groups
                .iter()
                .map(|(n, t)| {
                    (
                        (*n).to_string(),
                        Value::Array(t.iter().map(spec_json).collect()),
                    )
                })
                .collect::<Map<_, _>>(),
        )
    };
    finish(
        json,
        Ok(Success {
            text: text.trim_end().to_string(),
            json: json!({
                "instruments": group(&instruments),
                "effects": group(&effects),
                "strip": STRIP_PARAMS.iter().map(spec_json).collect::<Vec<_>>(),
                "mix": group(&mix),
            }),
        }),
    )
}
