//! Tracks: the instrument, its fx chain and their params, as devices of the
//! registry's modules.

use std::ops::RangeInclusive;

use indexmap::{IndexMap, IndexSet};
use tune_dsp::{ParamSpec, STRIP_PARAMS, find_spec};
use tune_modules::{ModuleDescriptor, Registry};

use crate::error::Errors;
use crate::model::{Device, Track};
use crate::raw::{At, RawFx, RawTrack};

const GAIN_DB: RangeInclusive<f64> = -60.0..=12.0;
const PAN: RangeInclusive<f64> = -1.0..=1.0;

pub(super) fn track(
    id: &At<String>,
    t: &RawTrack,
    registry: &Registry,
    errors: &mut Errors,
) -> Option<Track> {
    let module = t.instrument.as_ref().and_then(|name| {
        let module = instrument_module(registry, &name.value);
        if module.is_none() {
            let expected: Vec<&str> = registry.instruments().map(|m| m.name).collect();
            errors.at(
                &name.span,
                format!(
                    "unknown instrument `{}` (expected one of {})",
                    name.value,
                    expected.join(", ")
                ),
            );
        }
        module
    });

    // The track's `params` hold the instrument's params and the strip's.
    let params = module.map(|m| {
        let values = param_values(&t.params, &[m.params, STRIP_PARAMS], m.name, errors);
        let mut instrument = Device::new(m.prefix, m);
        let mut strip = IndexMap::new();
        for (id, v) in values {
            match local_param(m, &id) {
                Some(local) => instrument.params.insert(local.to_owned(), v),
                None => strip.insert(id, v),
            };
        }
        (instrument, strip)
    });

    let gain_db = ranged(
        t.gain_db.as_ref(),
        0.0,
        &GAIN_DB,
        "gain_db must be -60 to +12 dB",
        errors,
    );
    let pan = ranged(t.pan.as_ref(), 0.0, &PAN, "pan must be -1 to +1", errors);

    let mut fx = Vec::new();
    let mut seen_types: IndexSet<&str> = IndexSet::new();
    let mut fx_ok = true;
    for raw in &t.fx {
        // v1 names no device ids: a second entry of one type would repeat
        // the first one's id.
        if let Some(kind) = &raw.kind
            && effect_module(registry, &kind.value).is_some()
            && !seen_types.insert(kind.value.as_str())
        {
            errors.at(
                &raw.span,
                format!(
                    "track `{}` may have only one {} (merge the entries)",
                    id.value, kind.value
                ),
            );
            fx_ok = false;
        }
        match fx_device(raw, registry, errors) {
            Some(device) => fx.push(device),
            None => fx_ok = false,
        }
    }

    let (instrument, strip) = params?;
    Some(Track {
        instrument,
        fx: fx_ok.then_some(fx)?,
        strip,
        gain_db: gain_db?,
        pan: pan?,
        mute: t.mute.as_ref().is_some_and(|m| m.value),
        solo: t.solo.as_ref().is_some_and(|s| s.value),
        // Filled in from the sections.
        play: Vec::new(),
    })
}

/// The instrument module called `name`.
pub(super) fn instrument_module(
    registry: &Registry,
    name: &str,
) -> Option<&'static ModuleDescriptor> {
    registry.get(name).filter(|m| m.is_instrument())
}

/// The effect module called `name`.
pub(super) fn effect_module(registry: &Registry, name: &str) -> Option<&'static ModuleDescriptor> {
    registry.get(name).filter(|m| !m.is_instrument())
}

/// The local name of `module`'s param with full id `id` (`<prefix>.<local>`).
fn local_param<'a>(module: &ModuleDescriptor, id: &'a str) -> Option<&'a str> {
    let local = id.strip_prefix(module.prefix)?.strip_prefix('.')?;
    module.param(local).map(|_| local)
}

/// `value` (or `default` when absent) when it lies in `range`.
fn ranged(
    value: Option<&At<f64>>,
    default: f64,
    range: &RangeInclusive<f64>,
    rule: &str,
    errors: &mut Errors,
) -> Option<f64> {
    let Some(v) = value else {
        return Some(default);
    };
    if range.contains(&v.value) {
        Some(v.value)
    } else {
        errors.at(&v.span, format!("{rule}, got {}", v.value));
        None
    }
}

/// Checks `{ "<param id>" = plain }` against `tables` (the first one that
/// has the id wins); `owner` names them in messages ("unknown parameter `x`
/// for kick").
pub(super) fn param_values(
    values: &[(At<String>, At<f64>)],
    tables: &[&'static [ParamSpec]],
    owner: &str,
    errors: &mut Errors,
) -> IndexMap<String, f64> {
    let mut out = IndexMap::new();
    for (id, v) in values {
        match find_in(tables, &id.value) {
            None => errors.at(&id.span, unknown_param(&id.value, owner, tables)),
            Some(spec) => match spec.check(v.value) {
                Err(e) => errors.at(&v.span, e.to_string()),
                Ok(()) => {
                    out.insert(id.value.clone(), v.value);
                }
            },
        }
    }
    out
}

/// The spec with `id` in the first of `tables` that has it.
pub(super) fn find_in(tables: &[&'static [ParamSpec]], id: &str) -> Option<&'static ParamSpec> {
    tables
        .iter()
        .find_map(|table| find_spec(table, id).ok().map(|(_, spec)| spec))
}

pub(super) fn unknown_param(id: &str, owner: &str, tables: &[&[ParamSpec]]) -> String {
    let known: Vec<&str> = tables.iter().flat_map(|t| t.iter()).map(|s| s.id).collect();
    format!(
        "unknown parameter `{id}` for {owner} (known: {})",
        known.join(", ")
    )
}

/// One fx entry as a device of its effect module, with the module's prefix
/// as id.
fn fx_device(raw: &RawFx, registry: &Registry, errors: &mut Errors) -> Option<Device> {
    let kind = raw.kind.as_ref()?;
    let Some(module) = effect_module(registry, &kind.value) else {
        let expected: Vec<&str> = registry.effects().map(|m| m.name).collect();
        errors.at(
            &kind.span,
            format!(
                "unknown fx type `{}` (expected one of {})",
                kind.value,
                expected.join(", ")
            ),
        );
        return None;
    };
    let before = errors.len();
    let mut device = Device::new(module.prefix, module);
    for (name, value) in &raw.settings {
        let Some((index, spec)) = module.setting(name) else {
            errors.at(&value.span, format!("{} has no {name}", module.name));
            continue;
        };
        match spec.values.iter().find(|v| **v == value.value) {
            Some(v) => device.settings[index] = v,
            None => errors.at(
                &value.span,
                format!(
                    "unknown {} {name} `{}` (expected {})",
                    module.name,
                    value.value,
                    or_list(spec.values)
                ),
            ),
        }
    }
    let params = param_values(&raw.params, &[module.params], module.name, errors);
    (errors.len() == before).then_some(())?;
    for (id, v) in params {
        let local = local_param(module, &id).expect("checked against the module's table");
        device.params.insert(local.to_owned(), v);
    }
    Some(device)
}

/// `a`, `a or b`, `a, b or c`.
fn or_list(values: &[&str]) -> String {
    match values {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

/// What a declared track's raw entry says about the params it has.
pub(super) struct TrackShape {
    /// `None` for a missing or unknown instrument.
    pub(super) instrument: Option<&'static ModuleDescriptor>,
    /// The modules of its fx entries whose `type` is known.
    pub(super) fx: Vec<&'static ModuleDescriptor>,
}

impl TrackShape {
    pub(super) fn of(t: &RawTrack, registry: &Registry) -> Self {
        Self {
            instrument: t
                .instrument
                .as_ref()
                .and_then(|i| instrument_module(registry, &i.value)),
            fx: t
                .fx
                .iter()
                .filter_map(|fx| effect_module(registry, &fx.kind.as_ref()?.value))
                .collect(),
        }
    }
}
