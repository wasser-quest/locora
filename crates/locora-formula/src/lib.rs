//! Locora's formula language.
//!
//! A small, safe, bounded expression language over workbook tables: no
//! `eval`, no IO, no ambient globals and no mutation. Every evaluation has
//! hard limits on source size, nesting, operations, reference depth,
//! function-call depth and result size, so a hostile or broken workbook
//! reports an error instead of hanging or exhausting memory.
//!
//! ```
//! use locora_formula::{Engine, Row, Value};
//! let rows = vec![Row::value("Inputs", "zahl", "487")];
//! let mut engine = Engine::new(&rows).unwrap();
//! assert_eq!(engine.evaluate("Integer($zahl) + 1").unwrap(), Value::Number(488.0));
//! ```

mod parse;
mod value;
pub mod workbook;

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

pub use parse::{Node, parse};
pub use value::{MAX_SAFE_INTEGER, Value};
use value::{bounded, number_text};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::new(message))
}

/// Names that could reach into a host object model are never allowed.
pub(crate) fn blocked(key: &str) -> bool {
    matches!(key, "__proto__" | "prototype" | "constructor")
}

/// `[A-Za-z][A-Za-z0-9_]{0,63}`
pub fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && text.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CellType {
    Text,
    Number,
    Boolean,
    Json,
    Formula,
    Asset,
    Palace,
    Room,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub column: String,
    #[serde(rename = "type")]
    pub kind: CellType,
    pub value: String,
}

/// One table row: `Table.key` with typed cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub table: String,
    pub key: String,
    pub cells: Vec<Cell>,
}

impl Row {
    /// A row with a single text cell `value`.
    pub fn value(table: &str, key: &str, value: &str) -> Self {
        Self::typed(table, key, CellType::Text, value)
    }

    /// A row with a single cell `value` of the given type.
    pub fn typed(table: &str, key: &str, kind: CellType, value: &str) -> Self {
        Self {
            table: table.into(),
            key: key.into(),
            cells: vec![Cell {
                column: "value".into(),
                kind,
                value: value.into(),
            }],
        }
    }

    fn cell(&self, column: &str) -> Option<&Cell> {
        self.cells.iter().find(|c| c.column == column)
    }

    /// Check names, limits and literal cell values.
    pub fn validate(&self) -> Result<()> {
        if !is_identifier(&self.table) || blocked(&self.table) {
            return fail("Use an identifier for the table name");
        }
        if self.key.is_empty() || self.key.chars().count() > 64 || blocked(&self.key) {
            return fail("Invalid row key");
        }
        if self.cells.len() > 24 {
            return fail("A row has at most 24 cells");
        }
        let mut columns = HashSet::new();
        for cell in &self.cells {
            if !is_identifier(&cell.column)
                || blocked(&cell.column)
                || !columns.insert(&cell.column)
            {
                return fail("Invalid or duplicate column");
            }
            if cell.value.chars().count() > 16_000 {
                return fail("Cell is too large");
            }
            match cell.kind {
                CellType::Number => {
                    let n: f64 = cell
                        .value
                        .trim()
                        .parse()
                        .map_err(|_| Error::new("Invalid numeric cell"))?;
                    if !n.is_finite() || n.abs() > MAX_SAFE_INTEGER {
                        return fail("Invalid numeric cell");
                    }
                }
                CellType::Boolean if cell.value != "true" && cell.value != "false" => {
                    return fail("Boolean cells must be true or false");
                }
                CellType::Json => {
                    Value::from_json(&cell.value)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// The value of a non-formula cell.
pub fn literal_cell(cell: &Cell) -> Result<Value> {
    match cell.kind {
        CellType::Json => bounded(Value::from_json(&cell.value)?),
        CellType::Number => bounded(Value::Number(
            cell.value
                .trim()
                .parse()
                .map_err(|_| Error::new("Invalid numeric cell"))?,
        )),
        CellType::Boolean => Ok(Value::Bool(cell.value == "true")),
        _ => Ok(Value::Text(cell.value.clone())),
    }
}

const MAX_ROWS: usize = 1000;
const MAX_STEPS: usize = 10_000;
const MAX_CALL_DEPTH: usize = 16;
const MAX_REFERENCE_DEPTH: usize = 48;

/// Evaluates formulas against a set of rows. Cell results are cached; run-time
/// overrides (`Table.key.column` → value) replace saved values without
/// changing the rows.
pub struct Engine {
    rows: HashMap<String, Row>,
    order: Vec<String>,
    overrides: HashMap<String, Value>,
    cache: HashMap<String, Value>,
    active: Vec<String>,
    steps: usize,
    calls: usize,
}

type Locals = BTreeMap<String, Value>;

fn num(v: &Value) -> Result<f64> {
    match v {
        Value::Number(n) if n.is_finite() => Ok(*n),
        _ => fail("Expected a number (use Number(...) for digit strings)"),
    }
}

fn boolean(v: &Value) -> Result<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => fail("Expected true or false"),
    }
}

fn text(v: &Value) -> Result<&str> {
    match v {
        Value::Text(s) => Ok(s),
        _ => fail("Expected text"),
    }
}

/// `String(value)` semantics: text as is, numbers without `.0`, structures as JSON.
fn plain(v: &Value) -> String {
    match v {
        Value::Text(s) => s.clone(),
        Value::Number(n) => number_text(*n),
        other => other.to_json(),
    }
}

fn index_of(key: &Value) -> Option<usize> {
    match key {
        Value::Number(n) if *n >= 0.0 && n.fract() == 0.0 && *n <= MAX_SAFE_INTEGER => {
            Some(*n as usize)
        }
        Value::Text(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => s.parse().ok(),
        _ => None,
    }
}

fn member(value: &Value, key: &Value) -> Result<Value> {
    let key_text = match key {
        Value::Text(s) => s.clone(),
        Value::Number(n) => number_text(*n),
        _ => return fail("Invalid index"),
    };
    if blocked(&key_text) {
        return fail("Forbidden field");
    }
    let out_of_bounds = || Error::new(format!("Index {key_text} is out of bounds"));
    match value {
        Value::Text(s) => {
            let i = index_of(key).ok_or_else(out_of_bounds)?;
            s.chars()
                .nth(i)
                .map(|c| Value::Text(c.to_string()))
                .ok_or_else(out_of_bounds)
        }
        Value::List(items) => {
            let i = index_of(key).ok_or_else(out_of_bounds)?;
            items.get(i).cloned().ok_or_else(out_of_bounds)
        }
        Value::Record(map) => map
            .get(&key_text)
            .cloned()
            .ok_or_else(|| Error::new(format!("Unknown field '{key_text}'"))),
        _ => fail(format!("Unknown field '{key_text}'")),
    }
}

fn positive_mod(a: f64, b: f64) -> Result<f64> {
    if b <= 0.0 {
        return fail("Modulus must be positive");
    }
    Ok(((a % b) + b) % b)
}

impl Engine {
    pub fn new(rows: &[Row]) -> Result<Self> {
        Self::with_overrides(rows, HashMap::new())
    }

    /// `overrides` maps `Table.key.column` to a run-time value.
    pub fn with_overrides(rows: &[Row], overrides: HashMap<String, Value>) -> Result<Self> {
        if rows.len() > MAX_ROWS {
            return fail("Workbook row limit exceeded");
        }
        let mut map = HashMap::new();
        let mut order = Vec::new();
        for row in rows {
            row.validate()?;
            let id = format!("{}.{}", row.table, row.key);
            if map.insert(id.clone(), row.clone()).is_some() {
                return fail(format!("Duplicate row {id}"));
            }
            order.push(id);
        }
        Ok(Self {
            rows: map,
            order,
            overrides,
            cache: HashMap::new(),
            active: Vec::new(),
            steps: 0,
            calls: 0,
        })
    }

    /// Evaluate a formula.
    pub fn evaluate(&mut self, source: &str) -> Result<Value> {
        self.evaluate_with(source, &Locals::new())
    }

    /// Evaluate a formula with extra local names.
    pub fn evaluate_with(
        &mut self,
        source: &str,
        locals: &BTreeMap<String, Value>,
    ) -> Result<Value> {
        self.steps = 0;
        self.calls = 0;
        let node = parse(source)?;
        let value = self.run(&node, locals)?;
        bounded(value)
    }

    fn cell(&mut self, row_id: &str, column: &str) -> Result<Value> {
        let key = format!("{row_id}.{column}");
        if let Some(v) = self.overrides.get(&key) {
            return bounded(v.clone());
        }
        if let Some(v) = self.cache.get(&key) {
            return Ok(v.clone());
        }
        if self.active.contains(&key) {
            return fail(format!("Circular reference at {key}"));
        }
        if self.active.len() >= MAX_REFERENCE_DEPTH {
            return fail("Reference depth exceeded");
        }
        let cell = self
            .rows
            .get(row_id)
            .and_then(|r| r.cell(column))
            .cloned()
            .ok_or_else(|| Error::new(format!("Unknown cell {key}")))?;
        self.active.push(key.clone());
        let result = (|| {
            if cell.kind == CellType::Formula {
                let node = parse(&cell.value)?;
                bounded(self.run(&node, &Locals::new())?)
            } else {
                literal_cell(&cell)
            }
        })();
        self.active.pop();
        let result = result?;
        self.cache.insert(key, result.clone());
        Ok(result)
    }

    fn row_value(&mut self, row_id: &str) -> Result<Value> {
        let row = self
            .rows
            .get(row_id)
            .cloned()
            .ok_or_else(|| Error::new(format!("Unknown row {row_id}")))?;
        if row.cell("value").is_some() {
            return self.cell(row_id, "value");
        }
        let mut record = BTreeMap::new();
        for cell in &row.cells {
            record.insert(cell.column.clone(), self.cell(row_id, &cell.column)?);
        }
        bounded(Value::Record(record))
    }

    fn lookup(&mut self, parts: &[Value], locals: &Locals) -> Result<Value> {
        let name = text(&parts[0])?.to_owned();
        let mut tail = &parts[1..];
        let mut value = if let Some(v) = locals.get(&name) {
            bounded(v.clone())?
        } else if let Some(input) = name.strip_prefix('$') {
            let id = format!("Inputs.{input}");
            if !self.rows.contains_key(&id) {
                return fail(format!("Unknown input {name}"));
            }
            self.row_value(&id)?
        } else {
            let mut row = None;
            if let Some(first) = tail.first() {
                let id = format!("{name}.{}", plain(first));
                if self.rows.contains_key(&id) {
                    row = Some(id);
                    tail = &tail[1..];
                }
            } else {
                let matches: Vec<&String> = self
                    .order
                    .iter()
                    .filter(|id| {
                        let r = &self.rows[*id];
                        r.key == name && r.table != "Functions" && r.table != "Actions"
                    })
                    .collect();
                if matches.len() == 1 {
                    row = Some(matches[0].clone());
                }
            }
            let Some(row) = row else {
                let path: Vec<String> = parts.iter().map(plain).collect();
                return fail(format!("Unknown or ambiguous reference {}", path.join(".")));
            };
            match tail.first().map(plain) {
                Some(column) if self.rows[&row].cell(&column).is_some() => {
                    tail = &tail[1..];
                    self.cell(&row, &column)?
                }
                _ => self.row_value(&row)?,
            }
        };
        for key in tail {
            value = member(&value, key)?;
        }
        Ok(value)
    }

    fn path(&mut self, node: &Node, locals: &Locals) -> Result<Option<Vec<Value>>> {
        match node {
            Node::Name(name) => Ok(Some(vec![Value::Text(name.clone())])),
            Node::Member(object, key) => match self.path(object, locals)? {
                Some(mut base) => {
                    base.push(self.run(key, locals)?);
                    Ok(Some(base))
                }
                None => Ok(None),
            },
            _ => Ok(None),
        }
    }

    fn run(&mut self, node: &Node, locals: &Locals) -> Result<Value> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return fail("Formula operation budget exceeded");
        }
        match node {
            Node::Literal(v) => bounded(v.clone()),
            Node::Name(name) => self.lookup(&[Value::Text(name.clone())], locals),
            Node::Member(object, key) => match self.path(node, locals)? {
                Some(path) => self.lookup(&path, locals),
                None => {
                    let base = self.run(object, locals)?;
                    let key = self.run(key, locals)?;
                    member(&base, &key)
                }
            },
            Node::Array(items) => {
                let values = items
                    .iter()
                    .map(|n| self.run(n, locals))
                    .collect::<Result<Vec<_>>>()?;
                bounded(Value::List(values))
            }
            Node::Unary(op, right) => {
                let v = self.run(right, locals)?;
                match op.as_str() {
                    "!" | "not" => Ok(Value::Bool(!boolean(&v)?)),
                    "-" => bounded(Value::Number(-num(&v)?)),
                    _ => bounded(Value::Number(num(&v)?)),
                }
            }
            Node::Binary(op, left, right) => self.binary(op, left, right, locals),
            Node::Call(name, args) => self.call(name, args, locals),
        }
    }

    fn binary(&mut self, op: &str, left: &Node, right: &Node, locals: &Locals) -> Result<Value> {
        let a = self.run(left, locals)?;
        match op {
            "and" | "&&" => {
                return Ok(Value::Bool(
                    boolean(&a)? && boolean(&self.run(right, locals)?)?,
                ));
            }
            "or" | "||" => {
                return Ok(Value::Bool(
                    boolean(&a)? || boolean(&self.run(right, locals)?)?,
                ));
            }
            _ => {}
        }
        let b = self.run(right, locals)?;
        let number = |n: f64| bounded(Value::Number(n));
        match op {
            "=" | "==" => Ok(Value::Bool(a == b)),
            "!=" => Ok(Value::Bool(a != b)),
            "+" => match (&a, &b) {
                (Value::Text(x), Value::Text(y)) => bounded(Value::Text(format!("{x}{y}"))),
                _ => number(num(&a)? + num(&b)?),
            },
            "-" => number(num(&a)? - num(&b)?),
            "*" => number(num(&a)? * num(&b)?),
            "/" => {
                let d = num(&b)?;
                if d == 0.0 {
                    return fail("Division by zero");
                }
                number(num(&a)? / d)
            }
            "%" | "mod" => number(positive_mod(num(&a)?, num(&b)?)?),
            "^" => number(num(&a)?.powf(num(&b)?)),
            "<" => Ok(Value::Bool(num(&a)? < num(&b)?)),
            ">" => Ok(Value::Bool(num(&a)? > num(&b)?)),
            "<=" => Ok(Value::Bool(num(&a)? <= num(&b)?)),
            ">=" => Ok(Value::Bool(num(&a)? >= num(&b)?)),
            _ => fail(format!("Unknown operator {op}")),
        }
    }

    fn call(&mut self, name: &str, args: &[Node], locals: &Locals) -> Result<Value> {
        let lower = name.to_lowercase();
        if lower == "if" || lower == "wenn" {
            if args.len() != 3 {
                return fail("IF needs three arguments");
            }
            let condition = boolean(&self.run(&args[0], locals)?)?;
            return self.run(&args[if condition { 1 } else { 2 }], locals);
        }
        let values = args
            .iter()
            .map(|a| self.run(a, locals))
            .collect::<Result<Vec<_>>>()?;
        let arity = |n: usize| {
            if values.len() == n {
                Ok(())
            } else {
                fail(format!("{name} expects {n} arguments"))
            }
        };
        match lower.as_str() {
            "number" | "integer" => {
                arity(1)?;
                let n = match &values[0] {
                    Value::Number(n) => *n,
                    Value::Text(s) if !s.trim().is_empty() => s
                        .trim()
                        .parse::<f64>()
                        .map_err(|_| Error::new("Invalid number"))?,
                    _ => return fail("Invalid number"),
                };
                if lower == "integer" && (n.fract() != 0.0 || n.abs() > MAX_SAFE_INTEGER) {
                    return fail("Expected an integer");
                }
                return bounded(Value::Number(n));
            }
            "text" => {
                arity(1)?;
                let v = bounded(values[0].clone())?;
                return bounded(Value::Text(plain(&v)));
            }
            "floor" => {
                arity(1)?;
                return Ok(Value::Number(num(&values[0])?.floor()));
            }
            "abs" => {
                arity(1)?;
                return Ok(Value::Number(num(&values[0])?.abs()));
            }
            "mod" => {
                arity(2)?;
                return Ok(Value::Number(positive_mod(
                    num(&values[0])?,
                    num(&values[1])?,
                )?));
            }
            "min" | "max" => {
                if values.is_empty() || values.len() > 100 {
                    return fail("Invalid argument count");
                }
                let numbers = values.iter().map(num).collect::<Result<Vec<_>>>()?;
                let pick = if lower == "min" { f64::min } else { f64::max };
                return Ok(Value::Number(
                    numbers.into_iter().reduce(pick).expect("non-empty"),
                ));
            }
            "length" => {
                arity(1)?;
                return match &values[0] {
                    Value::Text(s) => Ok(Value::Number(s.chars().count() as f64)),
                    Value::List(l) => Ok(Value::Number(l.len() as f64)),
                    _ => fail("Length needs text or a list"),
                };
            }
            "digits" => {
                arity(1)?;
                let s = plain(&values[0]);
                if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                    return fail("Digits needs non-negative decimal digits");
                }
                return bounded(Value::List(
                    s.chars().map(|c| Value::Text(c.to_string())).collect(),
                ));
            }
            "reverse" => {
                arity(1)?;
                return match &values[0] {
                    Value::Text(s) => Ok(Value::Text(s.chars().rev().collect())),
                    Value::List(l) => Ok(Value::List(l.iter().rev().cloned().collect())),
                    _ => fail("Reverse needs text or a list"),
                };
            }
            "concat" => {
                arity(2)?;
                return match (&values[0], &values[1]) {
                    (Value::Text(a), Value::Text(b)) => bounded(Value::Text(format!("{a}{b}"))),
                    (Value::List(a), Value::List(b)) => {
                        bounded(Value::List(a.iter().chain(b).cloned().collect()))
                    }
                    _ => fail("Concat needs two lists or two strings"),
                };
            }
            "setat" => {
                arity(3)?;
                let index = num(&values[1])?;
                let Value::List(list) = &values[0] else {
                    return fail("SetAt needs a list and a valid index");
                };
                if index.fract() != 0.0 || index < 0.0 || index as usize >= list.len() {
                    return fail("SetAt needs a list and a valid index");
                }
                let mut out = list.clone();
                out[index as usize] = values[2].clone();
                return bounded(Value::List(out));
            }
            "major" => return self.major(&values),
            _ => {}
        }
        self.user_function(name, values)
    }

    /// `Major(size, digits, table?)`: split digits into groups of `size` and
    /// look each group up in the Major table (no implicit zero padding).
    fn major(&mut self, args: &[Value]) -> Result<Value> {
        if args.len() < 2 || args.len() > 3 {
            return fail("Major(size, digits, optionalTable)");
        }
        let size = num(&args[0])?;
        if size.fract() != 0.0 || !(1.0..=6.0).contains(&size) {
            return fail("Major group size must be 1–6");
        }
        let digits = match &args[1] {
            Value::Text(_) | Value::Number(_) => plain(&args[1]),
            _ => return fail("Major needs digits"),
        };
        if digits.is_empty() || digits.len() > 1000 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return fail("Major needs non-negative decimal digits");
        }
        let table = if args.len() == 3 {
            text(&args[2])?.to_owned()
        } else {
            "Major".into()
        };
        let mut result = Vec::new();
        for chunk in digits.as_bytes().chunks(size as usize) {
            let key = std::str::from_utf8(chunk).expect("ascii digits");
            let id = format!("{table}.{key}");
            if !self.rows.contains_key(&id) {
                return fail(format!(
                    "Missing Major entry {table}[\"{key}\"] (no implicit zero padding)"
                ));
            }
            let object = self.cell(&id, "object")?;
            let image = if self.rows[&id].cell("image").is_some() {
                self.cell(&id, "image")?
            } else {
                Value::Text(String::new())
            };
            let mut entry = BTreeMap::new();
            for (k, v) in [
                ("number", Value::Text(key.into())),
                ("zahl", Value::Text(key.into())),
                ("object", object.clone()),
                ("objekt", object),
                ("image", image.clone()),
                ("bild", image),
            ] {
                entry.insert(k.to_owned(), v);
            }
            result.push(Value::Record(entry));
        }
        bounded(Value::List(result))
    }

    fn user_function(&mut self, name: &str, args: Vec<Value>) -> Result<Value> {
        let id = format!("Functions.{name}");
        let row = self
            .rows
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::new(format!("Unknown function {name}")))?;
        let params: Vec<String> = row
            .cell("params")
            .map(|c| {
                c.value
                    .split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let body = row
            .cell("body")
            .ok_or_else(|| Error::new("Function body is missing"))?
            .value
            .clone();
        let unique: HashSet<&String> = params.iter().collect();
        if params.iter().any(|p| !is_identifier(p) || blocked(p))
            || unique.len() != params.len()
            || params.len() != args.len()
        {
            return fail("Invalid function parameters");
        }
        self.calls += 1;
        if self.calls > MAX_CALL_DEPTH {
            return fail("Function call depth exceeded");
        }
        let locals: Locals = params.into_iter().zip(args).collect();
        let result = parse(&body).and_then(|node| self.run(&node, &locals));
        self.calls -= 1;
        result
    }
}

#[cfg(test)]
mod tests;
