//! The generic spanned-TOML helpers the raw reader is built from: values
//! with their spans, leaf conversion with positioned messages, and a table
//! reader that reports unknown and missing keys.

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

/// The `[<key>.<id>]` sub-tables of a root key, as (id, value) pairs.
pub(super) fn sub_tables<'a, 'i>(
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
pub(super) fn tables_in_array<'a, 'i>(
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
pub(super) enum Kind {
    Str,
    Num,
    Int,
    U64,
    Bool,
}

/// A converted leaf value.
pub(super) enum Leaf {
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
pub(super) fn leaf(
    v: &Spanned<DeValue<'_>>,
    what: &str,
    kind: Kind,
    errors: &mut Errors,
) -> Option<At<Leaf>> {
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
pub(super) trait FromLeaf: Sized {
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
    pub(super) fn map<T: FromLeaf>(self) -> At<T> {
        At {
            span: self.span,
            value: T::from_leaf(self.value),
        }
    }

    pub(super) fn map_num(self) -> At<f64> {
        self.map()
    }
}

/// One TOML table being read: knows its allowed keys and where it starts.
pub(super) struct Fields<'a, 'i> {
    table: &'a DeTable<'i>,
    span: Range<usize>,
    what: String,
}

impl<'a, 'i> Fields<'a, 'i> {
    /// Wraps `table`, recording every key not in `allowed` as an error.
    pub(super) fn new(
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
    pub(super) fn open(
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

    pub(super) fn get(&self, key: &str) -> Option<&'a Spanned<DeValue<'i>>> {
        self.table.get(key)
    }

    pub(super) fn req_raw(
        &self,
        key: &str,
        errors: &mut Errors,
    ) -> Option<&'a Spanned<DeValue<'i>>> {
        let v = self.get(key);
        if v.is_none() {
            errors.at(
                &self.span,
                format!("missing field `{key}` in {}", self.what),
            );
        }
        v
    }

    pub(super) fn opt<T: FromLeaf>(
        &self,
        errors: &mut Errors,
        key: &str,
        kind: Kind,
    ) -> Option<At<T>> {
        let v = self.get(key)?;
        leaf(v, &format!("`{key}`"), kind, errors).map(At::map)
    }

    pub(super) fn req<T: FromLeaf>(
        &self,
        errors: &mut Errors,
        key: &str,
        kind: Kind,
    ) -> Option<At<T>> {
        let v = self.req_raw(key, errors)?;
        leaf(v, &format!("`{key}`"), kind, errors).map(At::map)
    }

    /// An inline `{ "<id>" = value, … }` table of leaves of one kind.
    pub(super) fn leaf_table<T: FromLeaf>(
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

    pub(super) fn num_table(&self, errors: &mut Errors, key: &str) -> Vec<(At<String>, At<f64>)> {
        self.leaf_table(errors, key, Kind::Num)
    }

    pub(super) fn str_table(
        &self,
        errors: &mut Errors,
        key: &str,
    ) -> Vec<(At<String>, At<String>)> {
        self.leaf_table(errors, key, Kind::Str)
    }
}
