//! `tune params`.

use std::fmt::Write as _;
use std::process::ExitCode;

use serde_json::{Map, Value, json};
use tune_dsp::{Curve, ParamSpec, STRIP_PARAMS};
use tune_modules::{ClipKind, ModuleDescriptor, Registry, SettingSpec};
use tune_song::MixUnit;

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

fn setting_json(s: &SettingSpec) -> Value {
    json!({
        "name": s.name,
        "values": s.values,
        "default": s.default,
        "help": s.help,
    })
}

/// The clip kinds an instrument plays, in `ClipKind::ALL` order.
fn plays(m: &ModuleDescriptor) -> Vec<&'static str> {
    ClipKind::ALL
        .into_iter()
        .filter(|k| m.plays(*k))
        .map(ClipKind::name)
        .collect()
}

pub fn run(json: bool) -> ExitCode {
    let registry = Registry::builtin();
    let instrument_modules: Vec<&ModuleDescriptor> = registry.instruments().collect();
    let effect_modules: Vec<&ModuleDescriptor> = registry.effects().collect();
    let tables = |modules: &[&ModuleDescriptor]| -> Vec<(&str, &[ParamSpec])> {
        modules.iter().map(|m| (m.name, m.params)).collect()
    };
    let instruments = tables(&instrument_modules);
    let effects = tables(&effect_modules);
    let mut text = String::new();
    for (heading, modules) in [
        ("instrument", &instrument_modules),
        ("effect", &effect_modules),
    ] {
        for m in modules {
            let _ = writeln!(text, "{heading} {}", m.name);
            if heading == "instrument" {
                let _ = writeln!(text, "  plays: {}", plays(m).join(", "));
            }
            for s in m.settings {
                let _ = writeln!(
                    text,
                    "  setting {}: {} (default {})\n      {}",
                    s.name,
                    s.values.join("|"),
                    s.default,
                    s.help
                );
            }
            for p in m.params {
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
                "plays": instrument_modules
                    .iter()
                    .map(|m| (m.name.to_string(), json!(plays(m))))
                    .collect::<Map<_, _>>(),
                "strip": STRIP_PARAMS.iter().map(spec_json).collect::<Vec<_>>(),
                "mix": group(&mix),
                "settings": registry
                    .instruments()
                    .chain(registry.effects())
                    .filter(|m| !m.settings.is_empty())
                    .map(|m| {
                        (
                            m.name.to_string(),
                            Value::Array(m.settings.iter().map(setting_json).collect()),
                        )
                    })
                    .collect::<Map<_, _>>(),
            }),
        }),
    )
}
