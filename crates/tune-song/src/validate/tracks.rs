//! Tracks: the instrument, its fx chain and their params, as devices of the
//! registry's modules. A device's params are addressed by its id
//! (`<device id>.<param>`); a v1 file names no ids, so each device has its
//! module's prefix.

use std::ops::RangeInclusive;

use indexmap::{IndexMap, IndexSet};
use tune_dsp::{ParamError, ParamSpec, STRIP_PARAMS, find_spec};
use tune_modules::{ModuleDescriptor, Registry};

use super::check_id;
use crate::error::Errors;
use crate::model::{Device, Track};
use crate::raw::{At, RawDevice, RawTrack};

const GAIN_DB: RangeInclusive<f64> = -60.0..=12.0;
const PAN: RangeInclusive<f64> = -1.0..=1.0;

/// Checks track `id`; `v2` selects the version 2 rules for devices.
pub(super) fn track(
    id: &At<String>,
    t: &RawTrack,
    v2: bool,
    registry: &Registry,
    errors: &mut Errors,
) -> Option<Track> {
    let before = errors.len();
    let instrument = t
        .instrument
        .as_ref()
        .and_then(|raw| instrument_device(raw, v2, registry, errors));

    // The track's `params` hold the instrument's params and the strip's.
    let params = instrument.map(|mut instrument| {
        let strip = track_params(&t.params, &mut instrument, v2, errors);
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
    for raw in &t.fx {
        // v1 names no device ids: a second entry of one type would repeat
        // the first one's id.
        if !v2
            && let Some(kind) = &raw.kind
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
        }
        if let Some(device) = fx_device(raw, v2, registry, errors) {
            fx.push(device);
        }
    }
    if v2 {
        check_unique_ids(id, t, registry, errors);
    }

    let (instrument, strip) = params?;
    (errors.len() == before).then_some(())?;
    Some(Track {
        instrument,
        fx,
        strip,
        gain_db: gain_db?,
        pan: pan?,
        mute: t.mute.as_ref().is_some_and(|m| m.value),
        solo: t.solo.as_ref().is_some_and(|s| s.value),
        // Filled in from the arrangement.
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

/// The track's instrument as a device, or `None` when its module is
/// missing or unknown. Problems with its id or settings are recorded but
/// still give a device, so the track's params are checked against it.
fn instrument_device(
    raw: &RawDevice,
    v2: bool,
    registry: &Registry,
    errors: &mut Errors,
) -> Option<Device> {
    let kind = raw.kind.as_ref()?;
    let Some(module) = instrument_module(registry, &kind.value) else {
        let effect = registry
            .effects()
            .any(|m| m.name == kind.value || m.prefix == kind.value);
        let message = if v2 && effect {
            format!("`{}` is an effect, not an instrument", kind.value)
        } else {
            let expected: Vec<&str> = registry.instruments().map(|m| m.name).collect();
            format!(
                "unknown instrument `{}` (expected one of {})",
                kind.value,
                expected.join(", ")
            )
        };
        errors.at(&kind.span, message);
        return None;
    };
    let mut device = Device::new(&device_id(raw, module, errors), module);
    apply_settings(&mut device, &raw.settings, errors);
    Some(device)
}

/// One fx entry as a device of its effect module.
fn fx_device(
    raw: &RawDevice,
    v2: bool,
    registry: &Registry,
    errors: &mut Errors,
) -> Option<Device> {
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
    let mut device = Device::new(&device_id(raw, module, errors), module);
    apply_settings(&mut device, &raw.settings, errors);
    for (id, v) in &raw.params {
        match device.local_param(&id.value) {
            Some(local) => {
                let (_, spec) = module.param(local).expect("a local param of the module");
                if check_value(id, v, spec, errors) {
                    device.params.insert(local.to_owned(), v.value);
                }
            }
            None => errors.at(&id.span, not_a_param(&id.value, &device, "fx", v2)),
        }
    }
    (errors.len() == before).then_some(device)
}

/// The id of `raw`'s device: the `id` it names (checked), else the
/// module's prefix.
fn device_id(raw: &RawDevice, module: &ModuleDescriptor, errors: &mut Errors) -> String {
    let Some(id) = &raw.id else {
        return module.prefix.to_owned();
    };
    let before = errors.len();
    check_id(id, "device", errors);
    if errors.len() == before && strip_prefixes().any(|p| p == id.value) {
        errors.at(
            &id.span,
            format!(
                "`{}` is reserved for the strip and cannot be a device id",
                id.value
            ),
        );
    }
    id.value.clone()
}

/// The device ids the strip's param ids start with (`send`, `duck`).
fn strip_prefixes() -> impl Iterator<Item = &'static str> {
    let mut seen = IndexSet::new();
    STRIP_PARAMS
        .iter()
        .filter_map(|s| s.id.split_once('.').map(|(p, _)| p))
        .filter(move |p| seen.insert(*p))
}

/// Sets each of `settings` on `device`, recording the ones its module lacks
/// or does not allow.
fn apply_settings(
    device: &mut Device,
    settings: &[(&'static str, At<String>)],
    errors: &mut Errors,
) {
    let module = device.module;
    for (name, value) in settings {
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
}

/// Splits a track's `params` into the instrument's (set on `instrument`,
/// by local name) and the strip's (returned, by full id).
fn track_params(
    values: &[(At<String>, At<f64>)],
    instrument: &mut Device,
    v2: bool,
    errors: &mut Errors,
) -> IndexMap<String, f64> {
    let mut strip = IndexMap::new();
    for (id, v) in values {
        if let Some(local) = instrument.local_param(&id.value) {
            let (_, spec) = instrument
                .module
                .param(local)
                .expect("a local param of the module");
            if check_value(id, v, spec, errors) {
                instrument.params.insert(local.to_owned(), v.value);
            }
        } else if let Ok((_, spec)) = find_spec(STRIP_PARAMS, &id.value) {
            if check_value(id, v, spec, errors) {
                strip.insert(id.value.clone(), v.value);
            }
        } else {
            errors.at(
                &id.span,
                not_a_param(&id.value, instrument, "instrument", v2),
            );
        }
    }
    strip
}

/// Whether `v` (the value of param `id`) lies in `spec`'s range; records
/// the problem otherwise.
fn check_value(id: &At<String>, v: &At<f64>, spec: &ParamSpec, errors: &mut Errors) -> bool {
    let ok = spec.check(v.value).is_ok();
    if !ok {
        let e = ParamError::OutOfRange {
            id: id.value.clone(),
            value: v.value,
            min: spec.min,
            max: spec.max,
        };
        errors.at(&v.span, e.to_string());
    }
    ok
}

/// Why `id` is not a param of `device` (the track's `role` device, `fx`
/// or `instrument`). In v2 a param of the module under another device id
/// gets told which ids the device reads.
fn not_a_param(id: &str, device: &Device, role: &str, v2: bool) -> String {
    let local = id.split_once('.').map(|(_, local)| local);
    if v2 && local.is_some_and(|l| device.module.param(l).is_some()) {
        return format!(
            "`{id}` is not a param of {role} `{}`: its params are `{}.<param>`",
            device.id, device.id
        );
    }
    let mut known = param_ids(&device.id, device.module);
    if role == "instrument" {
        known.extend(STRIP_PARAMS.iter().map(|s| s.id.to_owned()));
    }
    unknown_param(id, device.module.name, &known)
}

/// Every param id of a device of `module` called `device_id`
/// (`<device id>.<param>`), in table order.
pub(super) fn param_ids(device_id: &str, module: &ModuleDescriptor) -> Vec<String> {
    (0..module.params.len())
        .map(|i| format!("{device_id}.{}", module.local_name(i)))
        .collect()
}

/// In v2 every device of a track (instrument included) has its own id.
fn check_unique_ids<'a>(
    track: &At<String>,
    t: &'a RawTrack,
    registry: &Registry,
    errors: &mut Errors,
) {
    let devices: Vec<(&'a RawDevice, &'static ModuleDescriptor)> = t
        .instrument
        .iter()
        .filter_map(|raw| Some((raw, instrument_module(registry, &raw.kind.as_ref()?.value)?)))
        .chain(
            t.fx.iter()
                .filter_map(|raw| Some((raw, effect_module(registry, &raw.kind.as_ref()?.value)?))),
        )
        .collect();
    let id_of = |&(raw, module): &(&'a RawDevice, &'static ModuleDescriptor)| -> &'a str {
        raw.id
            .as_ref()
            .map_or(module.prefix, |id| id.value.as_str())
    };
    let all: IndexSet<&str> = devices.iter().map(id_of).collect();
    let mut seen: IndexSet<&str> = IndexSet::new();
    for device in &devices {
        let id = id_of(device);
        if seen.insert(id) {
            continue;
        }
        let suggestion = (2..)
            .map(|n| format!("{id}{n}"))
            .find(|s| !all.contains(s.as_str()))
            .expect("a track has finitely many devices");
        let (raw, _) = device;
        let span = match (&raw.id, &raw.kind) {
            (Some(at), _) | (None, Some(at)) => &at.span,
            (None, None) => &raw.span,
        };
        errors.at(
            span,
            format!(
                "track `{}` already has a device `{id}` (give the second one an `id`, e.g. `id = \"{suggestion}\"`)",
                track.value
            ),
        );
    }
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
/// for mix").
pub(super) fn param_values(
    values: &[(At<String>, At<f64>)],
    tables: &[&'static [ParamSpec]],
    owner: &str,
    errors: &mut Errors,
) -> IndexMap<String, f64> {
    let mut out = IndexMap::new();
    for (id, v) in values {
        match find_in(tables, &id.value) {
            None => errors.at(&id.span, unknown_param(&id.value, owner, &spec_ids(tables))),
            Some(spec) => {
                if check_value(id, v, spec, errors) {
                    out.insert(id.value.clone(), v.value);
                }
            }
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

/// Every id of `tables`, in order.
pub(super) fn spec_ids(tables: &[&[ParamSpec]]) -> Vec<String> {
    tables
        .iter()
        .flat_map(|t| t.iter())
        .map(|s| s.id.to_owned())
        .collect()
}

pub(super) fn unknown_param(id: &str, owner: &str, known: &[String]) -> String {
    format!(
        "unknown parameter `{id}` for {owner} (known: {})",
        known.join(", ")
    )
}

/// `a`, `a or b`, `a, b or c`.
pub(super) fn or_list(values: &[&str]) -> String {
    match values {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

/// What a declared track's raw entry says about its devices, as far as
/// their types are known.
pub(super) struct TrackShape {
    /// `None` for a missing or unknown instrument.
    pub(super) instrument: Option<ShapeDevice>,
    /// The fx entries whose `type` is known, in chain order.
    pub(super) fx: Vec<ShapeDevice>,
}

/// A device of a [`TrackShape`].
pub(super) struct ShapeDevice {
    pub(super) id: String,
    pub(super) module: &'static ModuleDescriptor,
}

impl ShapeDevice {
    /// Whether the track-relative param id `id` names one of this device's
    /// params.
    pub(super) fn has_param(&self, id: &str) -> bool {
        self.module.local_param(&self.id, id).is_some()
    }

    fn of(raw: &RawDevice, module: &'static ModuleDescriptor) -> Self {
        Self {
            id: raw
                .id
                .as_ref()
                .map_or(module.prefix, |id| id.value.as_str())
                .to_owned(),
            module,
        }
    }
}

impl TrackShape {
    pub(super) fn of(t: &RawTrack, registry: &Registry) -> Self {
        Self {
            instrument: t.instrument.as_ref().and_then(|raw| {
                let module = instrument_module(registry, &raw.kind.as_ref()?.value)?;
                Some(ShapeDevice::of(raw, module))
            }),
            fx: t
                .fx
                .iter()
                .filter_map(|raw| {
                    let module = effect_module(registry, &raw.kind.as_ref()?.value)?;
                    Some(ShapeDevice::of(raw, module))
                })
                .collect(),
        }
    }

    /// The instrument's module, when it is known.
    pub(super) fn module(&self) -> Option<&'static ModuleDescriptor> {
        self.instrument.as_ref().map(|d| d.module)
    }
}
