//! Workbook run time: shared and local tables, run-local values, inputs,
//! explicit actions and formula bindings on loci.
//!
//! Saved tables never change while practising. Inputs and actions write into
//! a run-local value map that overrides cells for this run only.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{Cell, CellType, Engine, Error, Result, Row, Value, literal_cell};

/// Tables of a palace: shared rows, replaced by local rows with the same `Table.key`.
pub fn effective_rows(shared: &[Row], local: &[Row]) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    for row in shared.iter().chain(local) {
        match out
            .iter_mut()
            .find(|r| r.table == row.table && r.key == row.key)
        {
            Some(existing) => *existing = row.clone(),
            None => out.push(row.clone()),
        }
    }
    out
}

/// Locus fields a formula can compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BindingField {
    /// The place (Ort) shown while recalling.
    Place,
    /// What is stored at the place.
    Item,
    X,
    Y,
    /// Whether the marker is shown (does not change what is asked).
    Visible,
    /// Whether the locus is part of the route at all (decided at the start).
    Active,
}

impl BindingField {
    pub const ALL: [BindingField; 6] = [
        BindingField::Place,
        BindingField::Item,
        BindingField::X,
        BindingField::Y,
        BindingField::Visible,
        BindingField::Active,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BindingField::Place => "place",
            BindingField::Item => "item",
            BindingField::X => "x",
            BindingField::Y => "y",
            BindingField::Visible => "visible",
            BindingField::Active => "active",
        }
    }

    pub fn parse(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|f| f.name() == name)
            .ok_or_else(|| {
                Error::new(format!(
                    "Unknown binding field '{name}' (place, item, x, y, visible, active)"
                ))
            })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub field: BindingField,
    pub expression: String,
}

impl Binding {
    pub fn validate(&self) -> Result<()> {
        crate::parse(&self.expression).map(|_| ())
    }
}

/// A locus as stored plus its bindings.
#[derive(Debug, Clone, PartialEq)]
pub struct LocusInput<'a> {
    pub place: &'a str,
    pub item: &'a str,
    pub x: f64,
    pub y: f64,
    pub bindings: &'a [Binding],
}

/// A locus after evaluating its bindings.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLocus {
    pub place: String,
    pub item: String,
    pub x: f64,
    pub y: f64,
    pub visible: bool,
}

/// What an action did.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionOutcome {
    /// Run values were written.
    Set,
    /// Show the answer of the current locus.
    Reveal,
    /// All run values were reset to the saved tables.
    Reset,
}

/// An input asked before a run: a literal row in the `Inputs` table.
#[derive(Debug, Clone, PartialEq)]
pub struct InputSpec {
    pub key: String,
    pub kind: CellType,
    pub default: Value,
}

fn scalar_text(value: Value, field: &str) -> Result<String> {
    match value {
        Value::Text(s) => Ok(s),
        Value::Number(_) | Value::Bool(_) => Ok(value.to_string()),
        _ => Err(Error::new(format!(
            "{field} must be text, a number or true/false (use Text(...) for lists)"
        ))),
    }
}

/// One practice run over a palace's tables.
#[derive(Debug, Clone)]
pub struct Run {
    rows: Vec<Row>,
    /// `Table.key.column` → run-local value.
    values: HashMap<String, Value>,
}

impl Run {
    pub fn new(rows: Vec<Row>) -> Result<Self> {
        for row in &rows {
            row.validate()?;
        }
        Ok(Self {
            rows,
            values: HashMap::new(),
        })
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Run-local values written by inputs and actions.
    pub fn values(&self) -> &HashMap<String, Value> {
        &self.values
    }

    /// Formula engine for this run; `View.reveal` tells formulas whether the
    /// answer is being shown.
    pub fn engine(&self, reveal: bool) -> Result<Engine> {
        let mut rows = self.rows.clone();
        rows.retain(|r| !(r.table == "View" && r.key == "reveal"));
        rows.push(Row::typed(
            "View",
            "reveal",
            CellType::Boolean,
            if reveal { "true" } else { "false" },
        ));
        Engine::with_overrides(&rows, self.values.clone())
    }

    pub fn evaluate(&self, source: &str, reveal: bool) -> Result<Value> {
        self.engine(reveal)?.evaluate(source)
    }

    /// Literal `Inputs` rows with a `value` cell, in table order.
    pub fn inputs(&self) -> Result<Vec<InputSpec>> {
        let mut out = Vec::new();
        for row in self.rows.iter().filter(|r| r.table == "Inputs") {
            let Some(cell) = row.cells.iter().find(|c| c.column == "value") else {
                continue;
            };
            if matches!(
                cell.kind,
                CellType::Formula | CellType::Asset | CellType::Palace | CellType::Room
            ) {
                continue;
            }
            out.push(InputSpec {
                key: row.key.clone(),
                kind: cell.kind,
                default: literal_cell(cell)?,
            });
        }
        Ok(out)
    }

    /// Set an input from text typed by a person, converted to the cell type.
    pub fn set_input(&mut self, key: &str, text: &str) -> Result<()> {
        let spec = self
            .inputs()?
            .into_iter()
            .find(|i| i.key == key)
            .ok_or_else(|| Error::new(format!("Unknown input ${key}")))?;
        let probe = Cell {
            column: "value".into(),
            kind: spec.kind,
            value: text.trim().to_owned(),
        };
        let row = Row {
            table: "Inputs".into(),
            key: key.into(),
            cells: vec![probe.clone()],
        };
        row.validate()?;
        self.values
            .insert(format!("Inputs.{key}.value"), literal_cell(&probe)?);
        Ok(())
    }

    /// Keys of the rows in the `Actions` table.
    pub fn actions(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|r| r.table == "Actions")
            .map(|r| r.key.clone())
            .collect()
    }

    /// Run `Actions.<key>`: `set` computes all values first and only then
    /// replaces the run values (all or nothing).
    pub fn execute(&mut self, key: &str, reveal: bool) -> Result<ActionOutcome> {
        let row = self
            .rows
            .iter()
            .find(|r| r.table == "Actions" && r.key == key)
            .cloned()
            .ok_or_else(|| Error::new(format!("Unknown action {key}")))?;
        let get = |column: &str| {
            row.cells
                .iter()
                .find(|c| c.column == column)
                .map(|c| c.value.clone())
        };
        match get("kind").as_deref() {
            Some("reveal") => return Ok(ActionOutcome::Reveal),
            Some("reset") => {
                self.values.clear();
                return Ok(ActionOutcome::Reset);
            }
            Some("set") => {}
            _ => return Err(Error::new("Action kind must be set, reveal or reset")),
        }
        let expression =
            get("expression").ok_or_else(|| Error::new("Set action needs an expression"))?;
        let targets: Vec<String> = get("target")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        if targets.is_empty() || targets.len() > 24 {
            return Err(Error::new("Invalid targets"));
        }
        let result = self.evaluate(&expression, reveal)?;
        let outputs = if targets.len() == 1 {
            vec![result]
        } else {
            match result {
                Value::List(items) if items.len() == targets.len() => items,
                _ => {
                    return Err(Error::new(
                        "Multi-target set needs an equally sized result list",
                    ));
                }
            }
        };
        let mut next = self.values.clone();
        let mut seen = HashSet::new();
        for (target, value) in targets.iter().zip(outputs) {
            let parts: Vec<&str> = target.split('.').collect();
            let (table, key, column) = match parts.as_slice() {
                [s, k] => (*s, *k, "value"),
                [s, k, c] => (*s, *k, *c),
                _ => return Err(Error::new("Use Table.key.column for a write target")),
            };
            let cell = self
                .rows
                .iter()
                .find(|r| r.table == table && r.key == key)
                .and_then(|r| r.cells.iter().find(|c| c.column == column))
                .filter(|c| {
                    matches!(
                        c.kind,
                        CellType::Text | CellType::Number | CellType::Boolean | CellType::Json
                    )
                })
                .ok_or_else(|| Error::new("Writes target existing literal cells only"))?;
            let id = format!("{table}.{key}.{column}");
            if !seen.insert(id.clone()) {
                return Err(Error::new("Duplicate write target"));
            }
            let fits = match (cell.kind, &value) {
                (CellType::Text, Value::Text(_))
                | (CellType::Number, Value::Number(_))
                | (CellType::Boolean, Value::Bool(_)) => true,
                (CellType::Json, v) => Value::from_json(&v.to_json()).is_ok(),
                _ => false,
            };
            if !fits {
                return Err(Error::new(format!(
                    "Write result for {id} does not match the cell type"
                )));
            }
            next.insert(id, value);
        }
        self.values = next;
        Ok(ActionOutcome::Set)
    }

    /// Whether a locus belongs to the route. Decided with `View.reveal = true`.
    pub fn active(&self, bindings: &[Binding]) -> Result<bool> {
        match bindings.iter().find(|b| b.field == BindingField::Active) {
            None => Ok(true),
            Some(b) => match self.evaluate(&b.expression, true)? {
                Value::Bool(v) => Ok(v),
                _ => Err(Error::new("active must be true or false")),
            },
        }
    }

    /// Evaluate a locus's bindings for the given reveal state.
    pub fn resolve(&self, locus: &LocusInput<'_>, reveal: bool) -> Result<ResolvedLocus> {
        let mut out = ResolvedLocus {
            place: locus.place.to_owned(),
            item: locus.item.to_owned(),
            x: locus.x,
            y: locus.y,
            visible: true,
        };
        if locus.bindings.is_empty() {
            return Ok(out);
        }
        let mut engine = self.engine(reveal)?;
        for binding in locus.bindings {
            if binding.field == BindingField::Active {
                continue;
            }
            let value = engine.evaluate(&binding.expression)?;
            match binding.field {
                BindingField::Place => out.place = scalar_text(value, "place")?,
                BindingField::Item => out.item = scalar_text(value, "item")?,
                BindingField::X | BindingField::Y => match value {
                    Value::Number(n) if (0.0..=1.0).contains(&n) => {
                        if binding.field == BindingField::X {
                            out.x = n;
                        } else {
                            out.y = n;
                        }
                    }
                    _ => return Err(Error::new("x and y must be numbers between 0 and 1")),
                },
                BindingField::Visible => match value {
                    Value::Bool(v) => out.visible = v,
                    _ => return Err(Error::new("visible must be true or false")),
                },
                BindingField::Active => {}
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(column: &str, kind: CellType, value: &str) -> Cell {
        Cell {
            column: column.into(),
            kind,
            value: value.into(),
        }
    }

    fn adder() -> Vec<Row> {
        vec![
            Row::typed("Inputs", "a", CellType::Text, "47"),
            Row::typed("Inputs", "b", CellType::Text, "38"),
            Row::typed("RAM", "carry", CellType::Number, "0"),
            Row::typed("RAM", "step", CellType::Number, "0"),
            Row {
                table: "Actions".into(),
                key: "next".into(),
                cells: vec![
                    cell("kind", CellType::Text, "set"),
                    cell("target", CellType::Text, "RAM.carry, RAM.step"),
                    cell(
                        "expression",
                        CellType::Text,
                        "[floor((Integer($a) + Integer($b)) / 100), RAM.step + 1]",
                    ),
                ],
            },
            Row {
                table: "Actions".into(),
                key: "clear".into(),
                cells: vec![cell("kind", CellType::Text, "reset")],
            },
            Row {
                table: "Actions".into(),
                key: "bad".into(),
                cells: vec![
                    cell("kind", CellType::Text, "set"),
                    cell("target", CellType::Text, "RAM.carry"),
                    cell("expression", CellType::Text, "\"text\""),
                ],
            },
        ]
    }

    #[test]
    fn local_rows_shadow_shared_rows() {
        let shared = vec![
            Row::value("Major", "1", "Tee"),
            Row::value("Major", "2", "Noah"),
        ];
        let local = vec![Row::value("Major", "2", "Neo")];
        let rows = effective_rows(&shared, &local);
        assert_eq!(rows.len(), 2);
        let run = Run::new(rows).unwrap();
        assert_eq!(run.evaluate("Major[\"2\"]", false).unwrap(), "Neo".into());
    }

    #[test]
    fn inputs_and_actions_change_only_the_run() {
        let mut run = Run::new(adder()).unwrap();
        let inputs = run.inputs().unwrap();
        assert_eq!(
            inputs.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(inputs[0].default, "47".into());
        run.set_input("a", "75").unwrap();
        assert_eq!(
            run.evaluate("Integer($a) + Integer($b)", false).unwrap(),
            113.0.into()
        );

        assert_eq!(run.execute("next", false).unwrap(), ActionOutcome::Set);
        assert_eq!(
            run.evaluate("[RAM.carry, RAM.step]", false).unwrap(),
            vec![1.0, 1.0].into()
        );
        run.execute("next", false).unwrap();
        assert_eq!(run.evaluate("RAM.step", false).unwrap(), 2.0.into());

        // A failing action changes nothing.
        assert!(run.execute("bad", false).is_err());
        assert_eq!(run.evaluate("RAM.step", false).unwrap(), 2.0.into());
        assert_eq!(run.execute("clear", false).unwrap(), ActionOutcome::Reset);
        assert_eq!(run.evaluate("RAM.step", false).unwrap(), 0.0.into());
        assert!(run.set_input("missing", "1").is_err());
    }

    #[test]
    fn bindings_compute_locus_fields_and_activity() {
        let rows = vec![
            Row::value("Inputs", "n", "3"),
            Row::value("Data", "word", "Haus"),
        ];
        let run = Run::new(rows).unwrap();
        let bindings = vec![
            Binding {
                field: BindingField::Item,
                expression: "IF(View.reveal, Data.word, \"?\")".into(),
            },
            Binding {
                field: BindingField::Place,
                expression: "\"Ort \" + $n".into(),
            },
            Binding {
                field: BindingField::X,
                expression: "0.25".into(),
            },
            Binding {
                field: BindingField::Active,
                expression: "Integer($n) > 2".into(),
            },
        ];
        let locus = LocusInput {
            place: "a",
            item: "b",
            x: 0.5,
            y: 0.5,
            bindings: &bindings,
        };
        assert!(run.active(&bindings).unwrap());
        let hidden = run.resolve(&locus, false).unwrap();
        assert_eq!(
            (hidden.place.as_str(), hidden.item.as_str(), hidden.x),
            ("Ort 3", "?", 0.25)
        );
        assert_eq!(run.resolve(&locus, true).unwrap().item, "Haus");

        let bad = vec![Binding {
            field: BindingField::Y,
            expression: "2".into(),
        }];
        let locus = LocusInput {
            bindings: &bad,
            ..locus
        };
        assert!(run.resolve(&locus, false).is_err());
        let list = vec![Binding {
            field: BindingField::Item,
            expression: "[1,2]".into(),
        }];
        assert!(
            run.resolve(
                &LocusInput {
                    bindings: &list,
                    ..locus
                },
                true
            )
            .unwrap_err()
            .0
            .contains("Text(")
        );
        assert!(BindingField::parse("color").is_err());
    }
}
