//! `toml_edit` document helpers shared by the migration and the studio's
//! in-place edits: tables reached through either written form (header or
//! inline), inline arrays reshaped without losing their layout, and table
//! positions made explicit so tables can be moved or added.

use toml_edit::{DocumentMut, InlineTable, Item, Table};

/// A table in the document, written either with a header (or dotted keys)
/// or inline (`{ … }`). Inline tables need their own spacing rules when a key
/// is appended.
pub enum Slot<'a> {
    Table(&'a mut Table),
    Inline(&'a mut InlineTable),
}

impl<'a> Slot<'a> {
    pub fn of(item: &'a mut Item) -> Option<Self> {
        match item {
            Item::Table(t) => Some(Self::Table(t)),
            Item::Value(toml_edit::Value::InlineTable(t)) => Some(Self::Inline(t)),
            _ => None,
        }
    }

    /// The table under `key`.
    pub fn child(self, key: &str) -> Option<Slot<'a>> {
        match self {
            Self::Table(t) => Slot::of(t.get_mut(key)?),
            Self::Inline(t) => t.get_mut(key)?.as_inline_table_mut().map(Self::Inline),
        }
    }

    /// The table under `key`, created as an inline table when absent.
    pub fn child_or_new(mut self, key: &str) -> Option<Slot<'a>> {
        let present = match &self {
            Self::Table(t) => t.contains_key(key),
            Self::Inline(t) => t.contains_key(key),
        };
        if !present {
            self.put(key, InlineTable::new().into());
        }
        self.child(key)
    }

    /// Sets `key` to `new`. An existing value keeps its surrounding
    /// whitespace and trailing comment; a new key is appended.
    pub fn put(&mut self, key: &str, mut new: toml_edit::Value) {
        let old = match self {
            Self::Table(t) => t.get_mut(key).and_then(Item::as_value_mut),
            Self::Inline(t) => t.get_mut(key),
        };
        if let Some(old) = old {
            *new.decor_mut() = old.decor().clone();
            *old = new;
            return;
        }
        match self {
            Self::Table(t) => {
                t.insert(key, Item::Value(new));
            }
            Self::Inline(t) => {
                // The space before `}` is the last value's suffix; hand it to
                // the new last value so the table reads `{ a = 1, b = 2 }`.
                if let Some((_, last)) = t.iter_mut().last() {
                    let suffix = last.decor().suffix().and_then(|s| s.as_str());
                    if let Some(suffix) =
                        suffix.filter(|s| s.trim().is_empty() && !s.contains('\n'))
                    {
                        let suffix = suffix.to_owned();
                        last.decor_mut().set_suffix("");
                        new.decor_mut().set_suffix(suffix);
                    }
                }
                t.insert(key, new);
            }
        }
    }

    pub fn remove(&mut self, key: &str) {
        match self {
            Self::Table(t) => {
                t.remove(key);
            }
            Self::Inline(t) => {
                t.remove(key);
            }
        }
    }
}

/// The table under `key` of the root table.
pub fn root_child<'a>(doc: &'a mut DocumentMut, key: &str) -> Option<Slot<'a>> {
    Slot::of(doc.as_table_mut().get_mut(key)?)
}

/// Element `index` of a list of tables, written either as `[[x]]` or as an
/// array of inline tables.
pub fn element_mut(item: &mut Item, index: usize) -> Option<Slot<'_>> {
    match item {
        Item::ArrayOfTables(aot) => aot.get_mut(index).map(Slot::Table),
        Item::Value(toml_edit::Value::Array(array)) => inline_element(array, index),
        _ => None,
    }
}

pub fn inline_element(array: &mut toml_edit::Array, index: usize) -> Option<Slot<'_>> {
    array
        .get_mut(index)?
        .as_inline_table_mut()
        .map(Slot::Inline)
}

/// Reorders the values of an inline array with `f`, keeping each slot's
/// whitespace so the array reads as before.
pub fn reshape(array: &mut toml_edit::Array, f: impl FnOnce(&mut Vec<toml_edit::Value>)) {
    let mut decors: Vec<toml_edit::Decor> = array.iter().map(|v| v.decor().clone()).collect();
    let mut values: Vec<toml_edit::Value> = array.iter().cloned().collect();
    f(&mut values);
    // A new slot looks like the last one after the first, or like the first
    // when it starts a line; otherwise it is one space after the comma.
    let extra = match decors.as_slice() {
        [.., _, last] => last.clone(),
        [first] if prefix(first).contains('\n') => first.clone(),
        _ => toml_edit::Decor::new(" ", ""),
    };
    decors.resize(values.len(), extra);
    array.clear();
    for (mut v, decor) in values.into_iter().zip(decors) {
        *v.decor_mut() = decor;
        array.push_formatted(v);
    }
}

pub fn prefix(decor: &toml_edit::Decor) -> &str {
    decor.prefix().and_then(|p| p.as_str()).unwrap_or("")
}

// Table positions.
//
// `toml_edit` renders a document's tables sorted by position, where a table
// without one inherits the position of the table visited before it (visit
// order: a table, then its child tables and arrays of tables, depth first,
// in key order). Moving a `[[sections]]` element within its array changes
// the visit order but not the positions, so the section ops first give every
// table an explicit, distinct position matching the current render order,
// then assign positions to the moved tables.

/// Calls `f` on `table` and every table nested in it that gets a header when
/// rendered (dotted tables are skipped), in visit order.
pub fn walk<'t>(table: &'t Table, f: &mut dyn FnMut(&'t Table)) {
    if !table.is_dotted() {
        f(table);
    }
    for (_, item) in table.iter() {
        match item {
            Item::Table(t) => walk(t, f),
            Item::ArrayOfTables(aot) => aot.iter().for_each(|t| walk(t, f)),
            _ => {}
        }
    }
}

/// [`walk`] with mutable access, in the same order.
pub fn walk_mut(table: &mut Table, f: &mut dyn FnMut(&mut Table)) {
    if !table.is_dotted() {
        f(table);
    }
    for (_, item) in table.iter_mut() {
        match item {
            Item::Table(t) => walk_mut(t, f),
            Item::ArrayOfTables(aot) => aot.iter_mut().for_each(|t| walk_mut(t, f)),
            _ => {}
        }
    }
}

/// Gives every table the rank it renders at as its position, so positions
/// are explicit and distinct and the rendered text is unchanged.
pub fn normalize_positions(doc: &mut DocumentMut) {
    // The same keys the renderer sorts by: the root first, then the
    // inherited position; the sort is stable, so ties keep visit order.
    let mut keys = Vec::new();
    let mut last = 0;
    walk(doc.as_table(), &mut |t| {
        if let Some(p) = t.position() {
            last = p;
        }
        keys.push((!keys.is_empty(), last));
    });
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by_key(|&i| keys[i]);
    let mut rank = vec![0; keys.len()];
    for (r, &i) in order.iter().enumerate() {
        rank[i] = isize::try_from(r).expect("fewer tables than isize::MAX");
    }
    let mut ranks = rank.into_iter();
    walk_mut(doc.as_table_mut(), &mut |t| t.set_position(ranks.next()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_keeps_text() {
        let src = "a = 1

[x]
k = 1

[[s]]
n = 1

[y]
k = 2

[[s]]
n = 2

[[s.t]]
q = 1
";
        let mut doc: DocumentMut = src.parse().unwrap();
        normalize_positions(&mut doc);
        assert_eq!(doc.to_string(), src);
    }
}
