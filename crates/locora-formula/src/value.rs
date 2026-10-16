//! Formula values: JSON-like data with finite numbers.

use std::collections::BTreeMap;
use std::fmt;

use crate::{Error, Result};

/// Largest integer that is exactly representable (2^53 - 1).
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Number(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Number(v as f64)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Text(v.to_owned())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(v)
    }
}
impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::List(v.into_iter().map(Into::into).collect())
    }
}

/// Numbers print like in spreadsheets and JSON: `3`, `0.5`, `-12.25`.
pub(crate) fn number_text(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn json_string(s: &str, out: &mut String) {
    out.push_str(&serde_json::to_string(s).expect("strings serialize"));
}

impl Value {
    /// Compact JSON text.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out);
        out
    }

    fn write_json(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => out.push_str(&number_text(*n)),
            Value::Text(s) => json_string(s, out),
            Value::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write_json(out);
                }
                out.push(']');
            }
            Value::Record(map) => {
                out.push('{');
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    json_string(k, out);
                    out.push(':');
                    v.write_json(out);
                }
                out.push('}');
            }
        }
    }

    /// Parse a JSON cell: finite numbers, bounded size and depth, no forbidden keys.
    pub fn from_json(source: &str) -> Result<Value> {
        if source.chars().count() > 16_000 {
            return Err(Error::new("JSON cell is too large"));
        }
        let json: serde_json::Value =
            serde_json::from_str(source).map_err(|e| Error::new(format!("Invalid JSON: {e}")))?;
        let mut count = 0;
        let value = convert(&json, 0, &mut count)?;
        Ok(value)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::Text(_) => "text",
            Value::List(_) => "list",
            Value::Record(_) => "record",
        }
    }
}

fn convert(json: &serde_json::Value, depth: usize, count: &mut usize) -> Result<Value> {
    *count += 1;
    if *count > 4000 || depth > 24 {
        return Err(Error::new("JSON cell is too complex"));
    }
    Ok(match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            let v = n
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or_else(|| Error::new("JSON number is not finite"))?;
            Value::Number(v)
        }
        serde_json::Value::String(s) => Value::Text(s.clone()),
        serde_json::Value::Array(items) => Value::List(
            items
                .iter()
                .map(|i| convert(i, depth + 1, count))
                .collect::<Result<_>>()?,
        ),
        serde_json::Value::Object(map) => {
            let mut out = BTreeMap::new();
            for (k, v) in map {
                if crate::blocked(k) {
                    return Err(Error::new("Forbidden JSON key"));
                }
                out.insert(k.clone(), convert(v, depth + 1, count)?);
            }
            Value::Record(out)
        }
    })
}

impl fmt::Display for Value {
    /// Text as shown to people: text without quotes, everything else as JSON.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Text(s) => f.write_str(s),
            other => f.write_str(&other.to_json()),
        }
    }
}

/// Reject results that are too big, too deep or numerically unsafe. Counts the
/// expanded shape, so `[x, x]` chains cannot explode later in `Text()`.
pub(crate) fn bounded(value: Value) -> Result<Value> {
    let mut pending: Vec<(&Value, usize)> = vec![(&value, 0)];
    let mut nodes = 0;
    let mut text = 0;
    while let Some((v, depth)) = pending.pop() {
        nodes += 1;
        if nodes > 4000 || depth > 32 {
            return Err(Error::new("Result structure is too large"));
        }
        match v {
            Value::Number(n) if !n.is_finite() || n.abs() > MAX_SAFE_INTEGER => {
                return Err(Error::new(
                    "Numeric result is outside the supported finite range",
                ));
            }
            Value::Text(s) => {
                let len = s.chars().count();
                text += len;
                if len > 16_000 || text > 64_000 {
                    return Err(Error::new("Result text is too large"));
                }
            }
            Value::List(items) => {
                if items.len() > 1000 {
                    return Err(Error::new("Result is too large"));
                }
                pending.extend(items.iter().map(|i| (i, depth + 1)));
            }
            Value::Record(map) => {
                if map.len() > 1000 {
                    return Err(Error::new("Result is too large"));
                }
                pending.extend(map.values().map(|i| (i, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(value)
}
