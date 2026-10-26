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
//!
//! The `[song]` table's `version` is read first: it picks the shapes the rest
//! of the file is read with (format 1 or 2).

mod fields;

use std::ops::Range;

use toml::Spanned;
use toml::de::{DeTable, DeValue};
use tune_modules::{ModuleDescriptor, Registry};

use crate::error::Errors;

pub(crate) use fields::At;
use fields::{Fields, Kind, leaf, sub_tables, tables_in_array};

pub(crate) struct RawSong {
    /// The format version the file is written in (1 or 2).
    pub version: u32,
    pub song: Option<RawHeader>,
    /// `None` when the file has no `[mix]` table (or it is not a table).
    pub mix: Option<RawMix>,
    pub tracks: Vec<(At<String>, RawTrack)>,
    /// v1 only.
    pub patterns: Vec<(At<String>, RawPattern)>,
    /// v2 only.
    pub clips: Vec<(At<String>, RawClip)>,
    /// `None` when the file has no `sections` key at all.
    pub sections: Option<Vec<RawSection>>,
    /// v2 only: the top-level `[[automation]]` stretches, in file order.
    pub automation: Vec<RawLane>,
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
    pub instrument: Option<RawDevice>,
    pub params: Vec<(At<String>, At<f64>)>,
    pub gain_db: Option<At<f64>>,
    pub pan: Option<At<f64>>,
    pub mute: Option<At<bool>>,
    pub solo: Option<At<bool>>,
    pub fx: Vec<RawDevice>,
    /// v2 only: the placements, in file order.
    pub play: Vec<RawPlacement>,
}

/// An instrument or an fx entry. A v1 instrument is a module name only.
pub(crate) struct RawDevice {
    /// The whole entry.
    pub span: Range<usize>,
    /// `type` (the instrument's module name for `instrument = "<module>"`).
    pub kind: Option<At<String>>,
    /// v2 only; `None` means the module's prefix.
    pub id: Option<At<String>>,
    /// The setting keys the entry has (`mode`), by name.
    pub settings: Vec<(&'static str, At<String>)>,
    /// An fx entry's own `params` (an instrument's live in the track's).
    pub params: Vec<(At<String>, At<f64>)>,
}

pub(crate) struct RawPattern {
    pub track: Option<At<String>>,
    pub steps: Option<At<String>>,
}

pub(crate) struct RawClip {
    pub kind: Option<At<String>>,
    pub steps: Option<At<String>>,
}

pub(crate) struct RawPlacement {
    pub at: Option<At<i64>>,
    pub clip: Option<At<String>>,
    pub bars: Option<At<i64>>,
    pub offset: Option<At<i64>>,
}

pub(crate) struct RawSection {
    pub name: Option<At<String>>,
    pub bars: Option<At<i64>>,
    /// v1 only.
    pub play: Vec<(At<String>, At<String>)>,
    /// v1 only.
    pub automation: Vec<RawLane>,
}

pub(crate) struct RawLane {
    pub param: Option<At<String>>,
    /// v2 only (a v1 lane covers its section).
    pub at: Option<At<i64>>,
    /// v2 only.
    pub bars: Option<At<i64>>,
    /// `None` when `points` is missing or not an array.
    pub points: Option<At<Vec<RawPoint>>>,
}

pub(crate) struct RawPoint {
    pub bar: At<f64>,
    pub value: At<f64>,
}

/// The format versions this reader knows.
const VERSIONS: [i64; 2] = [1, 2];

/// The `version` the file declares in `[song]` (absent means 1), or `None`
/// (with its error recorded) when it is not a version this reader knows.
fn version(doc: &Spanned<DeTable<'_>>, errors: &mut Errors) -> Option<u32> {
    let Some(v) = doc
        .get_ref()
        .get("song")
        .and_then(|song| song.get_ref().as_table())
        .and_then(|song| song.get("version"))
    else {
        return Some(1);
    };
    let version: At<i64> = leaf(v, "`version`", Kind::Int, errors)?.map();
    if !VERSIONS.contains(&version.value) {
        errors.at(
            &version.span,
            format!(
                "unknown song format version {} (this tune reads versions 1 and 2)",
                version.value
            ),
        );
        return None;
    }
    Some(u32::try_from(version.value).expect("a known version"))
}

/// Walks the whole document. Returns what could be read; every problem with
/// the file's shape is in `errors`. `None` when the file's format version is
/// unknown: then nothing else is read. An instrument or fx entry may have
/// every setting of any instrument or effect in `registry` as a key.
pub(crate) fn read(
    doc: &Spanned<DeTable<'_>>,
    registry: &Registry,
    errors: &mut Errors,
) -> Option<RawSong> {
    let version = version(doc, errors)?;
    let v2 = version == 2;
    let root = Fields::hinted(
        doc.get_ref(),
        doc.span(),
        "the song file",
        if v2 { ROOT_KEYS_V2 } else { ROOT_KEYS_V1 },
        if v2 { ROOT_HINTS_V2 } else { &[] },
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

    let keys = DeviceKeys::of(registry, v2);
    let tracks = sub_tables(&root, "tracks", errors)
        .into_iter()
        .filter_map(|(id, v)| {
            let what = format!("[tracks.{}]", id.value);
            let f = Fields::open(
                v,
                &what,
                if v2 { TRACK_KEYS_V2 } else { TRACK_KEYS_V1 },
                errors,
            )?;
            let instrument = if v2 {
                read_instrument(&f, &what, &keys.instrument, errors)
            } else {
                f.req(errors, "instrument", Kind::Str)
                    .map(|kind| RawDevice {
                        span: kind.span.clone(),
                        kind: Some(kind),
                        id: None,
                        settings: Vec::new(),
                        params: Vec::new(),
                    })
            };
            Some((
                id,
                RawTrack {
                    instrument,
                    params: f.num_table(errors, "params"),
                    gain_db: f.opt(errors, "gain_db", Kind::Num),
                    pan: f.opt(errors, "pan", Kind::Num),
                    mute: f.opt(errors, "mute", Kind::Bool),
                    solo: f.opt(errors, "solo", Kind::Bool),
                    fx: read_fx(&f, &what, &keys.fx, errors),
                    play: if v2 {
                        read_play(&f, &what, errors)
                    } else {
                        Vec::new()
                    },
                },
            ))
        })
        .collect();

    let (patterns, clips) = if v2 {
        let clips = sub_tables(&root, "clips", errors)
            .into_iter()
            .filter_map(|(id, v)| {
                let what = format!("[clips.{}]", id.value);
                let f = Fields::open(v, &what, CLIP_KEYS, errors)?;
                Some((
                    id,
                    RawClip {
                        kind: f.req(errors, "kind", Kind::Str),
                        steps: f.req(errors, "steps", Kind::Str),
                    },
                ))
            })
            .collect();
        (Vec::new(), clips)
    } else {
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
        (patterns, Vec::new())
    };

    let sections = root.get("sections").map(|v| {
        tables_in_array(v, "`sections`", errors)
            .into_iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let what = format!("section {}", i + 1);
                if v2 {
                    let f =
                        Fields::open_hinted(v, &what, SECTION_KEYS_V2, SECTION_HINTS_V2, errors)?;
                    Some(RawSection {
                        name: f.req(errors, "name", Kind::Str),
                        bars: f.req(errors, "bars", Kind::Int),
                        play: Vec::new(),
                        automation: Vec::new(),
                    })
                } else {
                    let f = Fields::open(v, &what, SECTION_KEYS_V1, errors)?;
                    Some(RawSection {
                        name: f.req(errors, "name", Kind::Str),
                        bars: f.req(errors, "bars", Kind::Int),
                        play: f.str_table(errors, "play"),
                        automation: read_section_lanes(&f, &what, errors),
                    })
                }
            })
            .collect()
    });

    let automation = match root.get("automation") {
        Some(v) if v2 => tables_in_array(v, "`automation`", errors)
            .into_iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let what = format!("automation {}", i + 1);
                let f = Fields::open(v, &what, LANE_KEYS_V2, errors)?;
                Some(RawLane {
                    param: f.req(errors, "param", Kind::Str),
                    at: f.req(errors, "at", Kind::Int),
                    bars: f.req(errors, "bars", Kind::Int),
                    points: f
                        .req_raw("points", errors)
                        .and_then(|v| read_points(v, errors)),
                })
            })
            .collect(),
        _ => Vec::new(),
    };

    Some(RawSong {
        version,
        song,
        mix,
        tracks,
        patterns,
        clips,
        sections,
        automation,
    })
}

const ROOT_KEYS_V1: &[&str] = &["song", "mix", "tracks", "patterns", "sections"];
const ROOT_KEYS_V2: &[&str] = &["song", "mix", "tracks", "clips", "sections", "automation"];
const ROOT_HINTS_V2: &[(&str, &str)] = &[(
    "patterns",
    "a version 2 file keeps patterns as [clips.<id>] with `kind`",
)];
const HEADER_KEYS: &[&str] = &["version", "name", "bpm", "seed", "sample_rate"];
const MIX_KEYS: &[&str] = &["master", "duck_source", "params"];
const TRACK_KEYS_V1: &[&str] = &[
    "instrument",
    "params",
    "gain_db",
    "pan",
    "mute",
    "solo",
    "fx",
];
const TRACK_KEYS_V2: &[&str] = &[
    "instrument",
    "params",
    "gain_db",
    "pan",
    "mute",
    "solo",
    "fx",
    "play",
];
const PATTERN_KEYS: &[&str] = &["track", "steps"];
const CLIP_KEYS: &[&str] = &["kind", "steps"];
const PLACEMENT_KEYS: &[&str] = &["at", "clip", "bars", "offset"];
const SECTION_KEYS_V1: &[&str] = &["name", "bars", "play", "automation"];
const SECTION_KEYS_V2: &[&str] = &["name", "bars"];
const SECTION_HINTS_V2: &[(&str, &str)] = &[
    (
        "play",
        "in version 2, what a track plays lives in the track's `play` list",
    ),
    (
        "automation",
        "in version 2, automation is top-level [[automation]] with `at` and `bars`",
    ),
];
const LANE_KEYS_V1: &[&str] = &["param", "points"];
const LANE_KEYS_V2: &[&str] = &["param", "at", "bars", "points"];

/// The keys an instrument table and an fx entry may have.
struct DeviceKeys {
    /// v2: `type`, `id`, then every setting name of a registered instrument.
    instrument: Vec<&'static str>,
    /// `type`, `id` (v2), every setting name of a registered effect, then
    /// `params`.
    fx: Vec<&'static str>,
}

impl DeviceKeys {
    fn of(registry: &Registry, v2: bool) -> Self {
        let head: &[&'static str] = if v2 { &["type", "id"] } else { &["type"] };
        let with_settings = |modules: &mut dyn Iterator<Item = &'static ModuleDescriptor>| {
            let mut keys = head.to_vec();
            for name in modules.flat_map(|m| m.settings).map(|s| s.name) {
                if !keys.contains(&name) {
                    keys.push(name);
                }
            }
            keys
        };
        let instrument = with_settings(&mut registry.instruments());
        let mut fx = with_settings(&mut registry.effects());
        fx.push("params");
        Self { instrument, fx }
    }
}

/// The setting keys of a device table: every key of `keys` but `type`,
/// `id` and `params` that the table has.
fn read_settings(
    f: &Fields<'_, '_>,
    keys: &[&'static str],
    errors: &mut Errors,
) -> Vec<(&'static str, At<String>)> {
    keys.iter()
        .filter(|k| !matches!(**k, "type" | "id" | "params"))
        .filter_map(|&name| Some((name, f.opt(errors, name, Kind::Str)?)))
        .collect()
}

/// A v2 `instrument`: a module name, or a table with `type`, an optional
/// `id` and the module's settings.
fn read_instrument(
    f: &Fields<'_, '_>,
    what: &str,
    keys: &[&'static str],
    errors: &mut Errors,
) -> Option<RawDevice> {
    let v = f.req_raw("instrument", errors)?;
    if v.get_ref().as_table().is_none() {
        let kind: At<String> = leaf(v, "`instrument`", Kind::Str, errors)?.map();
        return Some(RawDevice {
            span: v.span(),
            kind: Some(kind),
            id: None,
            settings: Vec::new(),
            params: Vec::new(),
        });
    }
    let table = Fields::open(v, &format!("`instrument` of {what}"), keys, errors)?;
    Some(RawDevice {
        span: v.span(),
        kind: table.req(errors, "type", Kind::Str),
        id: table.opt(errors, "id", Kind::Str),
        settings: read_settings(&table, keys, errors),
        params: Vec::new(),
    })
}

fn read_fx(
    f: &Fields<'_, '_>,
    what: &str,
    keys: &[&'static str],
    errors: &mut Errors,
) -> Vec<RawDevice> {
    let Some(v) = f.get("fx") else {
        return Vec::new();
    };
    tables_in_array(v, &format!("`fx` of {what}"), errors)
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let what = format!("fx {} of {what}", i + 1);
            let fx = Fields::open(v, &what, keys, errors)?;
            Some(RawDevice {
                span: v.span(),
                kind: fx.req(errors, "type", Kind::Str),
                id: if keys.contains(&"id") {
                    fx.opt(errors, "id", Kind::Str)
                } else {
                    None
                },
                settings: read_settings(&fx, keys, errors),
                params: fx.num_table(errors, "params"),
            })
        })
        .collect()
}

/// A v2 track's `play = [{ at, clip, bars, offset }, …]`.
fn read_play(f: &Fields<'_, '_>, what: &str, errors: &mut Errors) -> Vec<RawPlacement> {
    let Some(v) = f.get("play") else {
        return Vec::new();
    };
    tables_in_array(v, &format!("`play` of {what}"), errors)
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let what = format!("placement {} of {what}", i + 1);
            let p = Fields::open(v, &what, PLACEMENT_KEYS, errors)?;
            Some(RawPlacement {
                at: p.req(errors, "at", Kind::Int),
                clip: p.req(errors, "clip", Kind::Str),
                bars: p.req(errors, "bars", Kind::Int),
                offset: p.opt(errors, "offset", Kind::Int),
            })
        })
        .collect()
}

/// A v1 section's `[[sections.automation]]` lanes.
fn read_section_lanes(f: &Fields<'_, '_>, what: &str, errors: &mut Errors) -> Vec<RawLane> {
    let Some(v) = f.get("automation") else {
        return Vec::new();
    };
    tables_in_array(v, &format!("`automation` of {what}"), errors)
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let what = format!("automation lane {} of {what}", i + 1);
            let lane = Fields::open(v, &what, LANE_KEYS_V1, errors)?;
            Some(RawLane {
                param: lane.req(errors, "param", Kind::Str),
                at: None,
                bars: None,
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
