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

use std::ops::Range;

use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};
use toml::Spanned;
use toml::de::{DeTable, DeValue, ValueDeserializer};

use crate::error::Errors;

/// A value with the byte span it came from.
#[derive(Clone, Debug)]
pub(crate) struct At<T> {
    pub span: Range<usize>,
    pub value: T,
}

/// A number written either as a TOML integer or as a float.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Num(pub f64);

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct NumVisitor;
        impl Visitor<'_> for NumVisitor {
            type Value = Num;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an integer or a float")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Num, E> {
                Ok(Num(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Num, E> {
                Ok(Num(v as f64))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Num, E> {
                Ok(Num(v))
            }
        }
        d.deserialize_any(NumVisitor)
    }
}

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
    pub mode: Option<At<String>>,
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
/// the file's shape is in `errors`.
pub(crate) fn read(doc: &Spanned<DeTable<'_>>, errors: &mut Errors) -> RawSong {
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
                    fx: read_fx(&f, &what, errors),
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
const FX_KEYS: &[&str] = &["type", "mode", "params"];
const PATTERN_KEYS: &[&str] = &["track", "steps"];
const SECTION_KEYS: &[&str] = &["name", "bars", "play", "automation"];
const LANE_KEYS: &[&str] = &["param", "points"];

fn read_fx(f: &Fields<'_, '_>, what: &str, errors: &mut Errors) -> Vec<RawFx> {
    let Some(v) = f.get("fx") else {
        return Vec::new();
    };
    tables_in_array(v, &format!("`fx` of {what}"), errors)
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let what = format!("fx {} of {what}", i + 1);
            let fx = Fields::open(v, &what, FX_KEYS, errors)?;
            Some(RawFx {
                span: v.span(),
                kind: fx.req(errors, "type", Kind::Str),
                mode: fx.opt(errors, "mode", Kind::Str),
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

/// The `[<key>.<id>]` sub-tables of a root key, as (id, value) pairs.
fn sub_tables<'a, 'i>(
    root: &Fields<'a, 'i>,
    key: &str,
    errors: &mut Errors,
) -> Vec<(At<String>, &'a Spanned<DeValue<'i>>)> {
    let Some(v) = root.get(key) else {
        return Vec::new();
    };
    let Some(table) = v.get_ref().as_table() else {
        errors.at(
            &v.span(),
            format!("`{key}` must be a table of [{key}.<id>] tables"),
        );
        return Vec::new();
    };
    table
        .iter()
        .map(|(k, v)| {
            (
                At {
                    span: k.span(),
                    value: k.get_ref().to_string(),
                },
                v,
            )
        })
        .collect()
}

/// The tables inside an array value (`[[x]]` or `x = [{…}, …]`).
fn tables_in_array<'a, 'i>(
    v: &'a Spanned<DeValue<'i>>,
    what: &str,
    errors: &mut Errors,
) -> Vec<&'a Spanned<DeValue<'i>>> {
    match v.get_ref().as_array() {
        Some(items) => items.iter().collect(),
        None => {
            errors.at(&v.span(), format!("{what} must be a list of tables"));
            Vec::new()
        }
    }
}

/// What a leaf value must be.
#[derive(Clone, Copy)]
enum Kind {
    Str,
    Num,
    Int,
    U64,
    Bool,
}

/// A converted leaf value.
enum Leaf {
    Str(String),
    Num(f64),
    Int(i64),
    U64(u64),
    Bool(bool),
}

impl Kind {
    fn expected(self) -> &'static str {
        match self {
            Kind::Str => "a string",
            Kind::Num => "a number",
            Kind::Int => "an integer",
            Kind::U64 => "a non-negative integer",
            Kind::Bool => "true or false",
        }
    }

    fn convert(self, v: &Spanned<DeValue<'_>>) -> Option<Leaf> {
        let de = || ValueDeserializer::from(v.clone());
        match self {
            Kind::Str => String::deserialize(de()).ok().map(Leaf::Str),
            Kind::Num => Num::deserialize(de()).ok().map(|n| Leaf::Num(n.0)),
            Kind::Int => i64::deserialize(de()).ok().map(Leaf::Int),
            Kind::U64 => u64::deserialize(de()).ok().map(Leaf::U64),
            Kind::Bool => bool::deserialize(de()).ok().map(Leaf::Bool),
        }
    }
}

/// Converts `v` to `kind`, or records "`<what>` must be …" at its span.
fn leaf(v: &Spanned<DeValue<'_>>, what: &str, kind: Kind, errors: &mut Errors) -> Option<At<Leaf>> {
    match kind.convert(v) {
        Some(value) => Some(At {
            span: v.span(),
            value,
        }),
        None => {
            errors.at(
                &v.span(),
                format!(
                    "{what} must be {}, found {}",
                    kind.expected(),
                    v.get_ref().type_str()
                ),
            );
            None
        }
    }
}

/// Unwraps a [`Leaf`] into the field type the raw structs use.
trait FromLeaf: Sized {
    fn from_leaf(leaf: Leaf) -> Self;
}

impl FromLeaf for String {
    fn from_leaf(leaf: Leaf) -> Self {
        match leaf {
            Leaf::Str(s) => s,
            _ => unreachable!("converted as Kind::Str"),
        }
    }
}

impl FromLeaf for f64 {
    fn from_leaf(leaf: Leaf) -> Self {
        match leaf {
            Leaf::Num(n) => n,
            _ => unreachable!("converted as Kind::Num"),
        }
    }
}

impl FromLeaf for i64 {
    fn from_leaf(leaf: Leaf) -> Self {
        match leaf {
            Leaf::Int(n) => n,
            _ => unreachable!("converted as Kind::Int"),
        }
    }
}

impl FromLeaf for u64 {
    fn from_leaf(leaf: Leaf) -> Self {
        match leaf {
            Leaf::U64(n) => n,
            _ => unreachable!("converted as Kind::U64"),
        }
    }
}

impl FromLeaf for bool {
    fn from_leaf(leaf: Leaf) -> Self {
        match leaf {
            Leaf::Bool(b) => b,
            _ => unreachable!("converted as Kind::Bool"),
        }
    }
}

impl At<Leaf> {
    fn map<T: FromLeaf>(self) -> At<T> {
        At {
            span: self.span,
            value: T::from_leaf(self.value),
        }
    }

    fn map_num(self) -> At<f64> {
        self.map()
    }
}

/// One TOML table being read: knows its allowed keys and where it starts.
struct Fields<'a, 'i> {
    table: &'a DeTable<'i>,
    span: Range<usize>,
    what: String,
}

impl<'a, 'i> Fields<'a, 'i> {
    /// Wraps `table`, recording every key not in `allowed` as an error.
    fn new(
        table: &'a DeTable<'i>,
        span: Range<usize>,
        what: &str,
        allowed: &[&str],
        errors: &mut Errors,
    ) -> Self {
        for key in table.keys() {
            let k: &str = key.get_ref();
            if !allowed.contains(&k) {
                let expected = allowed
                    .iter()
                    .map(|a| format!("`{a}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                errors.at(
                    &key.span(),
                    format!("unknown field `{k}` in {what}, expected one of {expected}"),
                );
            }
        }
        Self {
            table,
            span,
            what: what.to_owned(),
        }
    }

    /// Like [`new`](Self::new) for a value that must be a table.
    fn open(
        v: &'a Spanned<DeValue<'i>>,
        what: &str,
        allowed: &[&str],
        errors: &mut Errors,
    ) -> Option<Self> {
        match v.get_ref().as_table() {
            Some(t) => Some(Self::new(t, v.span(), what, allowed, errors)),
            None => {
                errors.at(
                    &v.span(),
                    format!("{what} must be a table, found {}", v.get_ref().type_str()),
                );
                None
            }
        }
    }

    fn get(&self, key: &str) -> Option<&'a Spanned<DeValue<'i>>> {
        self.table.get(key)
    }

    fn req_raw(&self, key: &str, errors: &mut Errors) -> Option<&'a Spanned<DeValue<'i>>> {
        let v = self.get(key);
        if v.is_none() {
            errors.at(
                &self.span,
                format!("missing field `{key}` in {}", self.what),
            );
        }
        v
    }

    fn opt<T: FromLeaf>(&self, errors: &mut Errors, key: &str, kind: Kind) -> Option<At<T>> {
        let v = self.get(key)?;
        leaf(v, &format!("`{key}`"), kind, errors).map(At::map)
    }

    fn req<T: FromLeaf>(&self, errors: &mut Errors, key: &str, kind: Kind) -> Option<At<T>> {
        let v = self.req_raw(key, errors)?;
        leaf(v, &format!("`{key}`"), kind, errors).map(At::map)
    }

    /// An inline `{ "<id>" = value, … }` table of leaves of one kind.
    fn leaf_table<T: FromLeaf>(
        &self,
        errors: &mut Errors,
        key: &str,
        kind: Kind,
    ) -> Vec<(At<String>, At<T>)> {
        let Some(v) = self.get(key) else {
            return Vec::new();
        };
        let Some(table) = v.get_ref().as_table() else {
            errors.at(
                &v.span(),
                format!(
                    "`{key}` in {} must be a table, found {}",
                    self.what,
                    v.get_ref().type_str()
                ),
            );
            return Vec::new();
        };
        table
            .iter()
            .filter_map(|(k, v)| {
                let id = At {
                    span: k.span(),
                    value: k.get_ref().to_string(),
                };
                let what = format!("`{}` in `{key}`", id.value);
                let value = leaf(v, &what, kind, errors)?.map();
                Some((id, value))
            })
            .collect()
    }

    fn num_table(&self, errors: &mut Errors, key: &str) -> Vec<(At<String>, At<f64>)> {
        self.leaf_table(errors, key, Kind::Num)
    }

    fn str_table(&self, errors: &mut Errors, key: &str) -> Vec<(At<String>, At<String>)> {
        self.leaf_table(errors, key, Kind::Str)
    }
}
