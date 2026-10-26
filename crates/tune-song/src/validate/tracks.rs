//! Tracks: the instrument, its fx chain and their params.

use std::ops::RangeInclusive;

use indexmap::{IndexMap, IndexSet};
use tune_dsp::{DIST_PARAMS, DistortionMode, EQ_PARAMS, ParamSpec, STRIP_PARAMS, find_spec};

use crate::error::Errors;
use crate::model::{FxSpec, InstrumentKind, Track};
use crate::raw::{At, RawFx, RawTrack};

const GAIN_DB: RangeInclusive<f64> = -60.0..=12.0;
const PAN: RangeInclusive<f64> = -1.0..=1.0;

pub(super) fn track(id: &At<String>, t: &RawTrack, errors: &mut Errors) -> Option<Track> {
    let instrument = t.instrument.as_ref().and_then(|name| {
        let kind = InstrumentKind::from_name(&name.value);
        if kind.is_none() {
            let expected: Vec<&str> = InstrumentKind::ALL.iter().map(|k| k.name()).collect();
            errors.at(
                &name.span,
                format!(
                    "unknown instrument `{}` (expected one of {})",
                    name.value,
                    expected.join(", ")
                ),
            );
        }
        kind
    });

    let params = instrument.map(|kind| {
        param_values(
            &t.params,
            &[kind.params(), STRIP_PARAMS],
            kind.name(),
            errors,
        )
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
        if let Some(kind) = &raw.kind
            && FxSpec::TYPE_NAMES.contains(&kind.value.as_str())
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
        match fx_spec(raw, errors) {
            Some(spec) => fx.push(spec),
            None => fx_ok = false,
        }
    }

    Some(Track {
        instrument: instrument?,
        params: params?,
        gain_db: gain_db?,
        pan: pan?,
        mute: t.mute.as_ref().is_some_and(|m| m.value),
        solo: t.solo.as_ref().is_some_and(|s| s.value),
        fx: fx_ok.then_some(fx)?,
    })
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

fn fx_spec(raw: &RawFx, errors: &mut Errors) -> Option<FxSpec> {
    let kind = raw.kind.as_ref()?;
    match kind.value.as_str() {
        "distortion" => {
            let mode = match &raw.mode {
                None => Some(DistortionMode::Soft),
                Some(m) => {
                    let mode = DistortionMode::from_name(&m.value);
                    if mode.is_none() {
                        errors.at(
                            &m.span,
                            format!(
                                "unknown distortion mode `{}` (expected soft, hard or overdrive)",
                                m.value
                            ),
                        );
                    }
                    mode
                }
            };
            let before = errors.len();
            let params = param_values(&raw.params, &[DIST_PARAMS], "distortion", errors);
            (errors.len() == before).then_some(())?;
            Some(FxSpec::Distortion {
                mode: mode?,
                params,
            })
        }
        "eq" => {
            let before = errors.len();
            if let Some(m) = &raw.mode {
                errors.at(&m.span, "eq has no mode");
            }
            let params = param_values(&raw.params, &[EQ_PARAMS], "eq", errors);
            (errors.len() == before).then_some(())?;
            Some(FxSpec::Eq { params })
        }
        other => {
            errors.at(
                &kind.span,
                format!(
                    "unknown fx type `{other}` (expected one of {})",
                    FxSpec::TYPE_NAMES.join(", ")
                ),
            );
            None
        }
    }
}

/// What a declared track's raw entry says about the params it has.
pub(super) struct TrackShape {
    /// `None` for a missing or unknown instrument.
    pub(super) kind: Option<InstrumentKind>,
    /// Param tables of its fx entries whose `type` is known.
    pub(super) fx_tables: Vec<&'static [ParamSpec]>,
}

impl TrackShape {
    pub(super) fn of(t: &RawTrack) -> Self {
        Self {
            kind: t
                .instrument
                .as_ref()
                .and_then(|i| InstrumentKind::from_name(&i.value)),
            fx_tables: t
                .fx
                .iter()
                .filter_map(|fx| match fx.kind.as_ref()?.value.as_str() {
                    "distortion" => Some(DIST_PARAMS),
                    "eq" => Some(EQ_PARAMS),
                    _ => None,
                })
                .collect(),
        }
    }
}
