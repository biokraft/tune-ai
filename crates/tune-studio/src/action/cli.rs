//! Actions from `name=value` arguments, as `tune do` reads them, and back.

use serde_json::{Map, Value, json};

use super::{Action, ArgType, find_action};

/// Reads the action `name` from `name=value` arguments.
///
/// Each argument splits at its first `=`. One argument without a name is
/// the action's first argument. An empty value is `null`. The value of an
/// `id`, `param_id`, `path` or `text` argument is the text as given (a TOML
/// string literal is unquoted); any other value is read by
/// [`parse_value`].
pub fn action_from_cli(name: &str, args: &[String]) -> Result<Action, String> {
    let spec =
        find_action(name).ok_or_else(|| format!("unknown action `{name}` (see `tune actions`)"))?;
    let takes = || {
        if spec.args.is_empty() {
            "none".to_owned()
        } else {
            spec.args
                .iter()
                .map(|a| a.name)
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    let mut given = Map::new();
    let mut unnamed_seen = false;
    for raw in args {
        let (key, text) = match raw.split_once('=') {
            Some(pair) => pair,
            None => {
                let Some(first) = spec.args.first() else {
                    return Err(format!(
                        "`{name}` has no argument `{raw}` (it takes {})",
                        takes()
                    ));
                };
                if unnamed_seen {
                    return Err(format!(
                        "only the first argument (`{}`) may be given without a name",
                        first.name
                    ));
                }
                unnamed_seen = true;
                (first.name, raw.as_str())
            }
        };
        let arg =
            spec.args.iter().find(|a| a.name == key).ok_or_else(|| {
                format!("`{name}` has no argument `{key}` (it takes {})", takes())
            })?;
        if given.contains_key(key) {
            return Err(format!("`{key}` is given twice"));
        }
        given.insert(key.to_owned(), arg_value(arg.ty, text));
    }
    // A required text argument left empty (`track=`) is as good as missing;
    // the others take an empty value as `null`, which can mean something.
    let absent = |a: &&super::ArgSpec| {
        a.required
            && match given.get(a.name) {
                None => true,
                Some(value) => value.is_null() && a.ty.is_text(),
            }
    };
    if let Some(missing) = spec.args.iter().find(absent) {
        return Err(format!("`{name}` needs `{}`", missing.name));
    }
    let wire = if spec.args.is_empty() {
        json!({ "cmd": name })
    } else {
        json!({ "cmd": name, "args": given })
    };
    let action: Action =
        serde_json::from_value(wire).map_err(|e| format!("bad arguments for `{name}`: {e}"))?;
    if let Action::SetParam { commit: false, .. } = action {
        return Err(
            "`set_param` with commit=false is a knob preview for the studio UI; give the value \
             without it"
                .to_owned(),
        );
    }
    Ok(action)
}

/// One argument's value from its text.
fn arg_value(ty: ArgType, text: &str) -> Value {
    if text.is_empty() {
        Value::Null
    } else if ty.is_text() {
        Value::String(unquote(text).unwrap_or_else(|| text.to_owned()))
    } else {
        parse_value(text)
    }
}

/// The string a TOML string literal (`"hard"`, `'a b'`) holds.
fn unquote(text: &str) -> Option<String> {
    match text.parse::<toml_edit::Value>() {
        Ok(toml_edit::Value::String(s)) => Some(s.value().clone()),
        _ => None,
    }
}

impl Action {
    /// `name=value` arguments [`action_from_cli`] reads back as this
    /// action. Optional arguments left out (`null`) are not given.
    pub fn to_cli_args(&self) -> Vec<String> {
        let wire = serde_json::to_value(self).expect("an action serializes");
        let given = wire.get("args").and_then(Value::as_object);
        self.spec()
            .args
            .iter()
            .filter_map(|arg| {
                let value = given.and_then(|m| m.get(arg.name)).unwrap_or(&Value::Null);
                if value.is_null() && !arg.required {
                    return None;
                }
                Some(format!("{}={}", arg.name, cli_text(arg.ty, value)))
            })
            .collect()
    }
}

/// The text `tune do` reads back as `value`.
fn cli_text(ty: ArgType, value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        // Text that is empty or reads as a TOML string literal is quoted;
        // the rest goes as it is.
        Value::String(s) if ty.is_text() && !s.is_empty() && unquote(s).is_none() => s.clone(),
        other => to_toml(other).map_or_else(|| other.to_string(), |v| v.to_string()),
    }
}

/// `value` as a TOML value; `None` for a `null` inside it, which TOML
/// cannot hold.
fn to_toml(value: &Value) -> Option<toml_edit::Value> {
    Some(match value {
        Value::Null => return None,
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.into(),
            None => n.as_f64()?.into(),
        },
        Value::String(s) => s.as_str().into(),
        Value::Array(items) => items
            .iter()
            .map(to_toml)
            .collect::<Option<toml_edit::Array>>()?
            .into(),
        Value::Object(fields) => fields
            .iter()
            .map(|(k, v)| Some((k.as_str(), to_toml(v)?)))
            .collect::<Option<toml_edit::InlineTable>>()?
            .into(),
    })
}

/// Reads a value as `tune set` does: a TOML value (`820`, `true`,
/// `"hard"`); text that is not one is a string (`hard`); an empty text is
/// `null`.
pub fn parse_value(text: &str) -> Value {
    if text.is_empty() {
        return Value::Null;
    }
    text.parse::<toml_edit::Value>()
        .ok()
        .and_then(|v| from_toml(&v))
        .unwrap_or_else(|| Value::String(text.to_owned()))
}

fn from_toml(v: &toml_edit::Value) -> Option<Value> {
    use toml_edit::Value as T;
    Some(match v {
        T::String(s) => Value::String(s.value().clone()),
        T::Integer(i) => Value::from(*i.value()),
        T::Float(f) => serde_json::Number::from_f64(*f.value())?.into(),
        T::Boolean(b) => Value::Bool(*b.value()),
        T::Array(a) => Value::Array(a.iter().map(from_toml).collect::<Option<_>>()?),
        T::InlineTable(t) => Value::Object(
            t.iter()
                .map(|(k, v)| Some((k.to_owned(), from_toml(v)?)))
                .collect::<Option<_>>()?,
        ),
        T::Datetime(_) => return None,
    })
}
