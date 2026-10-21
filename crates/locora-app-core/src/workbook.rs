//! The formulas of a palace in the editor: its workbook tables, the shared tables, and the
//! formulas bound to its loci (the same as `locora table`, `bind`, `unbind` and `calc`).

use anyhow::{Context, Result, bail};
use locora_app_api::{
    WorkbookBinding, WorkbookCell, WorkbookChange, WorkbookLocus, WorkbookRow, WorkbookView,
};
use locora_formula::workbook::{Binding, BindingField, Run};
use locora_formula::{Cell, CellType, Row};
use locora_store::Store;

/// Longest formula or cell value the editor accepts.
const MAX_TEXT: usize = 10_000;

pub fn view(store: &Store, palace_id: &str) -> Result<WorkbookView> {
    let tree = store.palace(palace_id)?;
    let run = Run::new(store.effective_rows(&tree.palace.id)?);
    let loci = tree
        .route()
        .enumerate()
        .map(|(index, locus)| WorkbookLocus {
            id: locus.id.clone(),
            number: u32::try_from(index + 1).unwrap_or(u32::MAX),
            place: locus.place.clone(),
            bindings: locus
                .bindings
                .iter()
                .map(|binding| {
                    let result = match &run {
                        Ok(run) => run
                            .evaluate(&binding.expression, false)
                            .map(|v| v.to_string())
                            .map_err(|error| error.to_string()),
                        Err(error) => Err(error.to_string()),
                    };
                    WorkbookBinding {
                        field: binding.field.name().to_owned(),
                        expression: binding.expression.clone(),
                        value: result.as_ref().ok().cloned(),
                        error: result.err(),
                    }
                })
                .collect(),
        })
        .collect();
    Ok(WorkbookView {
        palace_id: tree.palace.id.clone(),
        rows: rows(store.rows(Some(&tree.palace.id))?)?,
        shared: rows(store.rows(None)?)?,
        loci,
    })
}

fn rows(rows: Vec<Row>) -> Result<Vec<WorkbookRow>> {
    rows.into_iter()
        .map(|row| {
            Ok(WorkbookRow {
                table: row.table,
                key: row.key,
                cells: row
                    .cells
                    .into_iter()
                    .map(|cell| {
                        Ok(WorkbookCell {
                            column: cell.column,
                            kind: kind_name(cell.kind)?,
                            value: cell.value,
                        })
                    })
                    .collect::<Result<_>>()?,
            })
        })
        .collect()
}

fn kind_name(kind: CellType) -> Result<String> {
    Ok(serde_json::to_value(kind)?
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

fn kind_of(text: &str) -> Result<CellType> {
    serde_json::from_value(serde_json::Value::String(text.into()))
        .context("Unbekannter Zelltyp: text, number, boolean, json, formula, asset, palace, room")
}

fn field_of(name: &str) -> Result<BindingField> {
    BindingField::ALL
        .into_iter()
        .find(|field| field.name() == name)
        .context("Unbekanntes Feld: place, item, x, y, visible, active")
}

/// Applies one change; the reply is the workbook as it now reads.
pub fn change(store: &Store, palace_id: &str, change: WorkbookChange) -> Result<WorkbookView> {
    let tree = store.palace(palace_id)?;
    let palace = tree.palace.id.clone();
    let owns = |locus_id: &str| tree.route().any(|locus| locus.id == locus_id);
    match change {
        WorkbookChange::SetCell {
            shared,
            table,
            key,
            column,
            kind,
            value,
        } => {
            if value.chars().count() > MAX_TEXT {
                bail!("Der Wert ist zu lang");
            }
            let scope = (!shared).then_some(palace.as_str());
            let mut row = store
                .rows(scope)?
                .into_iter()
                .find(|row| row.table == table && row.key == key)
                .unwrap_or_else(|| Row {
                    table,
                    key,
                    cells: Vec::new(),
                });
            row.cells.retain(|cell| cell.column != column);
            row.cells.push(Cell {
                column,
                kind: kind_of(&kind)?,
                value,
            });
            store
                .set_row(scope, &row)
                .context("Zelle konnte nicht gespeichert werden")?;
        }
        WorkbookChange::RemoveCell {
            shared,
            table,
            key,
            column,
        } => {
            let scope = (!shared).then_some(palace.as_str());
            let mut row = store
                .rows(scope)?
                .into_iter()
                .find(|row| row.table == table && row.key == key)
                .context("Zeile nicht gefunden")?;
            row.cells.retain(|cell| cell.column != column);
            if row.cells.is_empty() {
                store.delete_row(scope, &table, &key)?;
            } else {
                store.set_row(scope, &row)?;
            }
        }
        WorkbookChange::Bind {
            locus_id,
            field,
            expression,
        } => {
            if !owns(&locus_id) {
                bail!("Der Ort gehört nicht zu diesem Palast");
            }
            if expression.trim().is_empty() || expression.chars().count() > MAX_TEXT {
                bail!("Die Formel ist leer oder zu lang");
            }
            let binding = Binding {
                field: field_of(&field)?,
                expression,
            };
            store
                .set_binding(&locus_id, &binding)
                .context("Bindung konnte nicht gespeichert werden")?;
        }
        WorkbookChange::Unbind { locus_id, field } => {
            if !owns(&locus_id) {
                bail!("Der Ort gehört nicht zu diesem Palast");
            }
            store.remove_binding(&locus_id, field_of(&field)?)?;
        }
    }
    view(store, &palace)
}

/// The value of a formula with the palace's tables; nothing is saved.
pub fn calc(store: &Store, palace_id: &str, expression: &str) -> Result<String> {
    if expression.chars().count() > MAX_TEXT {
        bail!("Die Formel ist zu lang");
    }
    let tree = store.palace(palace_id)?;
    let run = Run::new(store.effective_rows(&tree.palace.id)?)?;
    let value = run
        .evaluate(expression, false)
        .context("Formel konnte nicht berechnet werden")?;
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_store::RoomKind;

    #[test]
    fn cells_bindings_and_calc_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let palace = store.create_palace("Formeln").unwrap();
        let room = store
            .add_room(&palace.id, "Raum", RoomKind::Blank, None)
            .unwrap();
        let locus = store
            .add_locus(
                &room.id,
                locora_store::NewLocus {
                    audio: None,
                    image_size: 0.2,
                    zoom: 1.0,
                    place: "Tisch".into(),
                    item: "?".into(),
                    x: 0.5,
                    y: 0.5,
                    time: None,
                    answer: Default::default(),
                    image: None,
                    note: String::new(),
                },
            )
            .unwrap();
        let set = |table: &str, column: &str, kind: &str, value: &str| WorkbookChange::SetCell {
            shared: false,
            table: table.into(),
            key: "a".into(),
            column: column.into(),
            kind: kind.into(),
            value: value.into(),
        };
        change(&store, &palace.id, set("Zahlen", "value", "number", "20")).unwrap();
        let view = change(
            &store,
            &palace.id,
            set("Zahlen", "plus", "formula", "Zahlen.a.value + 1"),
        )
        .unwrap();
        assert_eq!(view.rows[0].cells.len(), 2);
        assert!(view.shared.is_empty());
        assert_eq!(
            calc(&store, &palace.id, "Zahlen.a.value * 2").unwrap(),
            "40"
        );

        let bind = WorkbookChange::Bind {
            locus_id: locus.id.clone(),
            field: "item".into(),
            expression: "Zahlen.a.value + 2".into(),
        };
        let view = change(&store, &palace.id, bind).unwrap();
        assert_eq!(view.loci[0].bindings[0].value.as_deref(), Some("22"));
        let unbind = WorkbookChange::Unbind {
            locus_id: locus.id.clone(),
            field: "item".into(),
        };
        assert!(
            change(&store, &palace.id, unbind).unwrap().loci[0]
                .bindings
                .is_empty()
        );

        let remove = |column: &str| WorkbookChange::RemoveCell {
            shared: false,
            table: "Zahlen".into(),
            key: "a".into(),
            column: column.into(),
        };
        change(&store, &palace.id, remove("plus")).unwrap();
        assert!(
            change(&store, &palace.id, remove("value"))
                .unwrap()
                .rows
                .is_empty(),
            "the empty row is gone"
        );

        let other = store.create_palace("Andere").unwrap();
        let foreign = WorkbookChange::Bind {
            locus_id: locus.id,
            field: "item".into(),
            expression: "1".into(),
        };
        assert!(
            change(&store, &other.id, foreign).is_err(),
            "a locus of another palace"
        );
    }
}
