//! Workbook editing and evaluation for the CLI.

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use locora_formula::workbook::{Binding, BindingField, Run};
use locora_formula::{Cell, CellType, Row};

use crate::app::App;

#[derive(Args)]
#[group(required = true, multiple = false)]
pub struct Scope {
    /// Tabelle dieses Palasts bearbeiten
    #[arg(long, value_name = "PALAST")]
    palace: Option<String>,
    /// Gemeinsame Tabelle bearbeiten
    #[arg(long)]
    shared: bool,
}

#[derive(Subcommand)]
pub enum TableCommand {
    /// Effektive Tabellen eines Palasts oder gemeinsame Tabellen zeigen
    Show {
        #[arg(conflicts_with = "shared")]
        palace: Option<String>,
        #[arg(long)]
        shared: bool,
    },
    /// Zelle setzen (Standardtyp: text); andere Spalten bleiben erhalten
    Set {
        #[command(flatten)]
        scope: Scope,
        address: String,
        #[arg(allow_hyphen_values = true)]
        value: String,
        /// Zelltyp: text, number, boolean, json, formula, asset, palace, room
        #[arg(short = 't', long = "type", default_value = "text")]
        kind: String,
    },
    /// Zeile (Tabelle.key) oder einzelne Zelle (Tabelle.key.spalte) löschen
    Rm {
        #[command(flatten)]
        scope: Scope,
        address: String,
    },
}

fn address(text: &str) -> Result<(&str, &str, Option<&str>)> {
    let parts: Vec<_> = text.split('.').collect();
    match parts.as_slice() {
        [table, key] if !table.is_empty() && !key.is_empty() => Ok((table, key, None)),
        [table, key, column] if !table.is_empty() && !key.is_empty() && !column.is_empty() => {
            Ok((table, key, Some(column)))
        }
        _ => bail!("Adresse erwartet Tabelle.key[.spalte]"),
    }
}

fn kind(text: &str) -> Result<CellType> {
    serde_json::from_value(serde_json::Value::String(text.into()))
        .context("Unbekannter Zelltyp: text, number, boolean, json, formula, asset, palace, room")
}

pub fn table(app: &mut App, command: TableCommand) -> Result<()> {
    let (scope, addr, value) = match command {
        TableCommand::Show { palace, .. } => {
            let rows = match palace {
                Some(query) => {
                    let tree = app.store.palace(&query)?;
                    app.store.effective_rows(&tree.palace.id)?
                }
                None => app.store.rows(None)?,
            };
            if rows.is_empty() {
                println!("Keine Tabellen vorhanden.");
            }
            for row in rows {
                for cell in row.cells {
                    let kind = serde_json::to_value(cell.kind)?;
                    println!(
                        "{}.{}.{} [{}] = {}",
                        row.table,
                        row.key,
                        cell.column,
                        kind.as_str().unwrap_or_default(),
                        cell.value
                    );
                }
            }
            return Ok(());
        }
        TableCommand::Set {
            scope,
            address,
            value,
            kind: cell_type,
        } => (scope, address, Some((value, kind(&cell_type)?))),
        TableCommand::Rm { scope, address } => (scope, address, None),
    };
    let id = scope
        .palace
        .as_deref()
        .map(|q| app.store.palace(q).map(|t| t.palace.id))
        .transpose()?;
    let (table, key, column) = address(&addr)?;
    let mut row = app
        .store
        .rows(id.as_deref())?
        .into_iter()
        .find(|r| r.table == table && r.key == key);
    if let Some((value, kind)) = value {
        let row = row.get_or_insert_with(|| Row {
            table: table.into(),
            key: key.into(),
            cells: vec![],
        });
        let column = column.unwrap_or("value");
        row.cells.retain(|c| c.column != column);
        row.cells.push(Cell {
            column: column.into(),
            kind,
            value,
        });
        app.store
            .set_row(id.as_deref(), row)
            .context("Zelle konnte nicht gespeichert werden")?;
        println!("✓ Zelle {addr} gespeichert.");
    } else {
        let mut row = row.context("Zeile in diesem Bereich nicht gefunden")?;
        if let Some(column) = column {
            let before = row.cells.len();
            row.cells.retain(|c| c.column != column);
            if before == row.cells.len() {
                bail!("Spalte nicht gefunden");
            }
        } else {
            row.cells.clear();
        }
        if row.cells.is_empty() {
            app.store.delete_row(id.as_deref(), table, key)?;
        } else {
            app.store.set_row(id.as_deref(), &row)?;
        }
        println!("✓ {addr} gelöscht.");
    }
    Ok(())
}

pub fn field(text: &str) -> Result<BindingField> {
    BindingField::ALL
        .into_iter()
        .find(|f| f.name() == text)
        .context("Unbekanntes Feld: place, item, x, y, visible, active")
}

pub fn bind(
    app: &mut App,
    palace: &str,
    n: usize,
    name: &str,
    expression: Option<String>,
) -> Result<()> {
    let tree = app.store.palace(palace)?;
    let locus = tree
        .route()
        .nth(n.wrapping_sub(1))
        .context("Locus-Nummer nicht gefunden")?;
    let field = field(name)?;
    match expression {
        Some(expression) => app
            .store
            .set_binding(&locus.id, &Binding { field, expression })
            .context("Bindung konnte nicht gespeichert werden")?,
        None => {
            app.store.remove_binding(&locus.id, field)?;
        }
    }
    println!("✓ Bindung für Locus {n}, Feld {name} aktualisiert.");
    Ok(())
}

pub fn calc(app: &mut App, first: &str, expression: Option<&str>) -> Result<()> {
    let (rows, source) = match expression {
        Some(source) => {
            let tree = app.store.palace(first)?;
            (app.store.effective_rows(&tree.palace.id)?, source)
        }
        None => (app.store.rows(None)?, first),
    };
    let result = Run::new(rows)?
        .evaluate(source, false)
        .context("Formel konnte nicht berechnet werden")?;
    println!("{result}");
    Ok(())
}
