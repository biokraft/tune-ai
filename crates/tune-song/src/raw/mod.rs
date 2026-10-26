//! First pass: the spanned TOML document to a typed mirror of the file shape.
//!
//! Plain `serde` with `deny_unknown_fields` stops at the first problem, but a
//! song file must report every mistake at once. So this pass walks toml's
//! spanned document ([`DeTable`]) by hand: an unknown key, a missing key or a
//! value of the wrong type is recorded in [`Errors`] and the walk carries on.
//! Each leaf value is still converted by serde (through toml's
//! [`ValueDeserializer`]), so TOML's number, string and boolean rules are
//! toml's own. Every field keeps the byte span of its value for
//! [`validate`](crate::validate), which checks meaning (ranges, references).

mod fields;

use std::ops::Range;

use toml::Spanned;
use toml::de::{DeTable, DeValue};
use tune_modules::Registry;

use crate::error::Errors;

pub(crate) use fields::At;
use fields::{Fields, Kind, leaf, sub_tables, tables_in_array};

pub(crate) struct RawSong {
    pub song: Option<RawHeader>,
    /// `None` when the file has no `[mix]` table (or it is not a table).
    pub mix: Option<RawMix>,
    pub tracks: Vec<(At<String>, RawTrack)>,
    pub patterns: Vec<(At<String>, RawPattern)>,
    /// `None` when the file has no `sections` key at all.
    pub sections: Option<Vec<RawSection>>,
}

pub(crate) struct RawHeader {
    pub name: Option<At<String>>,
    pub bpm: Option<At<f64>>,
    pub seed: Option<At<u64>>,
    pub sample_rate: Option<At<i64>>,
}

pub(crate) struct RawMix {
    pub master: Option<At<String>>,
    pub duck_source: Option<At<String>>,
    pub params: Vec<(At<String>, At<f64>)>,
}

pub(crate) struct RawTrack {
    pub instrument: Option<At<String>>,
    pub params: Vec<(At<String>, At<f64>)>,
    pub gain_db: Option<At<f64>>,
    pub pan: Option<At<f64>>,
    pub mute: Option<At<bool>>,
    pub solo: Option<At<bool>>,
    pub fx: Vec<RawFx>,
}

pub(crate) struct RawFx {
    pub span: Range<usize>,
    pub kind: Option<At<String>>,
    /// The setting keys the entry has (`mode`), by name.
    pub settings: Vec<(&'static str, At<String>)>,
    pub params: Vec<(At<String>, At<f64>)>,
}

pub(crate) struct RawPattern {
    pub track: Option<At<String>>,
    pub steps: Option<At<String>>,
}

pub(crate) struct RawSection {
    pub name: Option<At<String>>,
    pub bars: Option<At<i64>>,
    pub play: Vec<(At<String>, At<String>)>,
    pub automation: Vec<RawLane>,
}

pub(crate) struct RawLane {
    pub param: Option<At<String>>,
    /// `None` when `points` is missing or not an array.
    pub points: Option<At<Vec<RawPoint>>>,
}

pub(crate) struct RawPoint {
    pub bar: At<f64>,
    pub value: At<f64>,
}

/// Walks the whole document. Returns what could be read; every problem with
/// the file's shape is in `errors`. An fx entry may have every setting of
/// any effect in `registry` as a key.
pub(crate) fn read(
    doc: &Spanned<DeTable<'_>>,
    registry: &Registry,
    errors: &mut Errors,
) -> RawSong {
    let root = Fields::new(
        doc.get_ref(),
        doc.span(),
        "the song file",
        &["song", "mix", "tracks", "patterns", "sections"],
        errors,
    );

    let song = match root.get("song") {
        None => {
            errors.global("missing [song] table (it needs `name` and `bpm`)");
            None
        }
        Some(v) => Fields::open(v, "[song]", HEADER_KEYS, errors).map(|f| RawHeader {
            name: f.req(errors, "name", Kind::Str),
            bpm: f.req(errors, "bpm", Kind::Num),
            seed: f.opt(errors, "seed", Kind::U64),
            sample_rate: f.opt(errors, "sample_rate", Kind::Int),
        }),
    };

    let mix = root.get("mix").and_then(|v| {
        Fields::open(v, "[mix]", MIX_KEYS, errors).map(|f| RawMix {
            master: f.opt(errors, "master", Kind::Str),
            duck_source: f.opt(errors, "duck_source", Kind::Str),
            params: f.num_table(errors, "params"),
        })
    });

    let fx_keys = fx_keys(registry);
    let tracks = sub_tables(&root, "tracks", errors)
        .into_iter()
        .filter_map(|(id, v)| {
            let what = format!("[tracks.{}]", id.value);
            let f = Fields::open(v, &what, TRACK_KEYS, errors)?;
            Some((
                id,
                RawTrack {
                    instrument: f.req(errors, "instrument", Kind::Str),
                    params: f.num_table(errors, "params"),
                    gain_db: f.opt(errors, "gain_db", Kind::Num),
                    pan: f.opt(errors, "pan", Kind::Num),
                    mute: f.opt(errors, "mute", Kind::Bool),
                    solo: f.opt(errors, "solo", Kind::Bool),
                    fx: read_fx(&f, &what, &fx_keys, errors),
                },
            ))
        })
        .collect();

    let patterns = sub_tables(&root, "patterns", errors)
        .into_iter()
        .filter_map(|(id, v)| {
            let what = format!("[patterns.{}]", id.value);
            let f = Fields::open(v, &what, PATTERN_KEYS, errors)?;
            Some((
                id,
                RawPattern {
                    track: f.req(errors, "track", Kind::Str),
                    steps: f.req(errors, "steps", Kind::Str),
                },
            ))
        })
        .collect();

    let sections = root.get("sections").map(|v| {
        tables_in_array(v, "`sections`", errors)
            .into_iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let what = format!("section {}", i + 1);
                let f = Fields::open(v, &what, SECTION_KEYS, errors)?;
                Some(RawSection {
                    name: f.req(errors, "name", Kind::Str),
                    bars: f.req(errors, "bars", Kind::Int),
                    play: f.str_table(errors, "play"),
                    automation: read_lanes(&f, &what, errors),
                })
            })
            .collect()
    });

    RawSong {
        song,
        mix,
        tracks,
        patterns,
        sections,
    }
}

const HEADER_KEYS: &[&str] = &["name", "bpm", "seed", "sample_rate"];
const MIX_KEYS: &[&str] = &["master", "duck_source", "params"];
const TRACK_KEYS: &[&str] = &[
    "instrument",
    "params",
    "gain_db",
    "pan",
    "mute",
    "solo",
    "fx",
];
const PATTERN_KEYS: &[&str] = &["track", "steps"];
const SECTION_KEYS: &[&str] = &["name", "bars", "play", "automation"];
const LANE_KEYS: &[&str] = &["param", "points"];

/// The keys an fx entry may have: `type`, every setting name of a registered
/// effect (in registry order, each once), then `params`.
fn fx_keys(registry: &Registry) -> Vec<&'static str> {
    let mut keys = vec!["type"];
    for name in registry.effects().flat_map(|m| m.settings).map(|s| s.name) {
        if !keys.contains(&name) {
            keys.push(name);
        }
    }
    keys.push("params");
    keys
}

fn read_fx(
    f: &Fields<'_, '_>,
    what: &str,
    keys: &[&'static str],
    errors: &mut Errors,
) -> Vec<RawFx> {
    let Some(v) = f.get("fx") else {
        return Vec::new();
    };
    tables_in_array(v, &format!("`fx` of {what}"), errors)
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let what = format!("fx {} of {what}", i + 1);
            let fx = Fields::open(v, &what, keys, errors)?;
            let kind = fx.req(errors, "type", Kind::Str);
            let settings = keys
                .iter()
                .filter(|k| !matches!(**k, "type" | "params"))
                .filter_map(|&name| Some((name, fx.opt(errors, name, Kind::Str)?)))
                .collect();
            Some(RawFx {
                span: v.span(),
                kind,
                settings,
                params: fx.num_table(errors, "params"),
            })
        })
        .collect()
}

fn read_lanes(f: &Fields<'_, '_>, what: &str, errors: &mut Errors) -> Vec<RawLane> {
    let Some(v) = f.get("automation") else {
        return Vec::new();
    };
    tables_in_array(v, &format!("`automation` of {what}"), errors)
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let what = format!("automation lane {} of {what}", i + 1);
            let lane = Fields::open(v, &what, LANE_KEYS, errors)?;
            Some(RawLane {
                param: lane.req(errors, "param", Kind::Str),
                points: lane
                    .req_raw("points", errors)
                    .and_then(|v| read_points(v, errors)),
            })
        })
        .collect()
}

/// `points = [[bar, normalized], …]`.
fn read_points(v: &Spanned<DeValue<'_>>, errors: &mut Errors) -> Option<At<Vec<RawPoint>>> {
    const SHAPE: &str = "must be a list of [bar, value] pairs, e.g. [[0, 0.1], [4, 0.8]]";
    let Some(items) = v.get_ref().as_array() else {
        errors.at(&v.span(), format!("`points` {SHAPE}"));
        return None;
    };
    let mut points = Vec::new();
    for item in items.iter() {
        let pair = item.get_ref().as_array().filter(|a| a.len() == 2);
        let Some(pair) = pair else {
            errors.at(&item.span(), format!("each point {SHAPE}"));
            continue;
        };
        let bar = leaf(&pair[0], "a point's bar", Kind::Num, errors);
        let value = leaf(&pair[1], "a point's value", Kind::Num, errors);
        if let (Some(bar), Some(value)) = (bar, value) {
            points.push(RawPoint {
                bar: bar.map_num(),
                value: value.map_num(),
            });
        }
    }
    Some(At {
        span: v.span(),
        value: points,
    })
}
