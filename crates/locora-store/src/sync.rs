//! Operation sync: replaying changes from other devices and the per-field
//! clocks that decide which write wins.
//!
//! Local writes call [`record`] inside their transaction, so every field has a
//! clock and every delete leaves a tombstone. Remote operations go through
//! [`Store::apply_remote`]: they write directly and never log again, so applied
//! changes are not sent back. Concurrent writes to one field resolve by HLC
//! (last writer wins); a delete wins over every edit of the same entity.

use locora_formula::workbook::{Binding, BindingField};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value};

use crate::{Locus, LocusPatch, Result, RoomKind, Store, StoreError, new_id, now_secs};

/// One operation received from the cloud.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteOp {
    pub hlc: String,
    pub entity: String,
    pub entity_id: String,
    pub op: String,
    pub data: Option<Value>,
}

/// What [`Store::apply_remote`] did with a batch.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    /// Changed the local data.
    pub applied: usize,
    /// Already known, or older than what is stored (last writer wins, or deleted).
    pub stale: usize,
    /// Refers to a palace, room or locus this device does not have (yet).
    pub missing: usize,
    /// Could not be applied; the batch carries on without it.
    pub failed: Vec<String>,
}

pub(crate) enum Outcome {
    Applied,
    Stale,
    Missing,
}

impl Store {
    /// Replays `ops` (in the order given) and returns what changed.
    pub fn apply_remote(&self, ops: &[RemoteOp]) -> Result<ApplyReport> {
        let mut report = ApplyReport::default();
        let mut conn = self.conn();
        let mut tx = conn.transaction()?;
        for op in ops {
            self.observe(&op.hlc);
            let mut savepoint = tx.savepoint()?;
            match apply_one(&savepoint, op) {
                Ok(outcome) => {
                    savepoint.commit()?;
                    match outcome {
                        Outcome::Applied => report.applied += 1,
                        Outcome::Stale => report.stale += 1,
                        Outcome::Missing => report.missing += 1,
                    }
                }
                Err(error) => {
                    savepoint.rollback()?;
                    report
                        .failed
                        .push(format!("{} {}: {error}", op.entity, op.entity_id));
                }
            }
        }
        tx.commit()?;
        Ok(report)
    }
}

/// Media hashes that an operation refers to. They must exist before it is applied.
pub fn media_refs(op: &RemoteOp) -> Vec<String> {
    let Some(data) = op.data.as_ref().and_then(Value::as_object) else {
        return Vec::new();
    };
    let hashes = match (op.entity.as_str(), op.op.as_str()) {
        ("room", "create") => vec![data.get("media")],
        ("palace", "update") => vec![data.get("marker")],
        ("locus", "create") | ("locus", "update") => vec![data.get("image")],
        _ => Vec::new(),
    };
    hashes
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// Records the clocks of a local write (called from `Store::log`).
pub(crate) fn record(
    conn: &Connection,
    entity: &str,
    entity_id: &str,
    op: &str,
    hlc: &str,
    data: Option<&Value>,
) -> Result<()> {
    if op == "delete" {
        return tombstone(conn, entity, entity_id, hlc);
    }
    if let Some(Value::Object(fields)) = data {
        for field in fields.keys() {
            stamp(conn, entity, entity_id, field, hlc)?;
        }
    }
    Ok(())
}

pub(crate) fn apply_one(conn: &Connection, op: &RemoteOp) -> Result<Outcome> {
    if tombstoned(conn, &op.entity, &op.entity_id, &op.hlc)? {
        return Ok(Outcome::Stale);
    }
    match (op.entity.as_str(), op.op.as_str()) {
        ("palace", "create") => palace_create(conn, op),
        ("palace", "update") => palace_update(conn, op),
        ("palace", "delete") => delete_entity(conn, op, "palaces", &op.entity_id),
        ("room", "create") => room_create(conn, op),
        ("locus", "create") => locus_create(conn, op),
        ("locus", "update") => locus_update(conn, op),
        ("locus", "delete") => delete_entity(conn, op, "loci", &op.entity_id),
        ("row", "set") => row_set(conn, op),
        ("row", "delete") => row_delete(conn, op),
        ("binding", "set") => binding_set(conn, op),
        ("binding", "delete") => binding_delete(conn, op),
        ("kv", "set") => kv_set(conn, op),
        ("journey", "create") => journey_create(conn, op),
        (entity, action) => Err(StoreError::Invalid(format!(
            "unbekannte Operation {entity}/{action}"
        ))),
    }
}

fn palace_create(conn: &Connection, op: &RemoteOp) -> Result<Outcome> {
    if exists(conn, "palaces", &op.entity_id)? {
        return Ok(Outcome::Stale);
    }
    let name = unique_palace_name(conn, text_field(op, "name")?, &op.entity_id)?;
    let now = now_secs() as i64;
    conn.execute(
        "INSERT INTO palaces(id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![op.entity_id, name, now],
    )?;
    stamp(conn, "palace", &op.entity_id, "name", &op.hlc)?;
    Ok(Outcome::Applied)
}

fn palace_update(conn: &Connection, op: &RemoteOp) -> Result<Outcome> {
    if !exists(conn, "palaces", &op.entity_id)? {
        return Ok(Outcome::Missing);
    }
    let fields = object(op)?;
    let mut applied = false;
    for (field, value) in fields {
        if !wins(conn, "palace", &op.entity_id, field, &op.hlc)? {
            continue;
        }
        match field.as_str() {
            "name" => {
                let name = unique_palace_name(conn, as_text(value)?, &op.entity_id)?;
                conn.execute(
                    "UPDATE palaces SET name = ?1 WHERE id = ?2",
                    params![name, op.entity_id],
                )?;
            }
            "marker" => {
                let marker = optional_text(value)?;
                if let Some(hash) = &marker {
                    require_media(conn, hash)?;
                }
                conn.execute(
                    "UPDATE palaces SET marker = ?1 WHERE id = ?2",
                    params![marker, op.entity_id],
                )?;
            }
            _ => continue,
        }
        stamp(conn, "palace", &op.entity_id, field, &op.hlc)?;
        applied = true;
    }
    Ok(if applied {
        Outcome::Applied
    } else {
        Outcome::Stale
    })
}

fn room_create(conn: &Connection, op: &RemoteOp) -> Result<Outcome> {
    let fields = object(op)?;
    let palace_id = text_field(op, "palace_id")?;
    let position = number_field(op, "position")?;
    let name = text_field(op, "name")?;
    let kind = RoomKind::parse(text_field(op, "kind")?)?;
    let media = optional_text(fields.get("media").unwrap_or(&Value::Null))?;
    if !exists(conn, "palaces", palace_id)? {
        return Ok(Outcome::Missing);
    }
    if exists(conn, "rooms", &op.entity_id)? {
        return Ok(Outcome::Stale);
    }
    if let Some(hash) = &media {
        require_media(conn, hash)?;
    }
    conn.execute(
        "INSERT INTO rooms(id, palace_id, position, name, kind, media) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            op.entity_id,
            palace_id,
            position,
            name,
            kind.as_str(),
            media,
        ],
    )?;
    stamp_all(conn, "room", op)?;
    Ok(Outcome::Applied)
}

fn locus_create(conn: &Connection, op: &RemoteOp) -> Result<Outcome> {
    let locus: Locus = serde_json::from_value(op.data.clone().unwrap_or(Value::Null))
        .map_err(|e| StoreError::Invalid(format!("Ort ist unvollständig: {e}")))?;
    if locus.id != op.entity_id {
        return Err(StoreError::Invalid(
            "Ort-Kennung passt nicht zur Operation".into(),
        ));
    }
    if !exists(conn, "rooms", &locus.room_id)? {
        return Ok(Outcome::Missing);
    }
    if exists(conn, "loci", &locus.id)? {
        return Ok(Outcome::Stale);
    }
    if let Some(hash) = &locus.image {
        require_media(conn, hash)?;
    }
    conn.execute(
        "INSERT INTO loci(id, room_id, position, place, item, x, y, time, answer, image, note, image_size, zoom)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            locus.id,
            locus.room_id,
            locus.position,
            locus.place,
            locus.item,
            locus.x,
            locus.y,
            locus.time,
            serde_json::to_string(&locus.answer).map_err(|_| StoreError::Invalid("Antwort".into()))?,
            locus.image,
            locus.note,
            locus.image_size,
            locus.zoom,
        ],
    )?;
    for binding in &locus.bindings {
        binding.validate().map_err(|e| StoreError::Invalid(e.0))?;
        upsert_binding(conn, &locus.id, binding)?;
    }
    stamp_all(conn, "locus", op)?;
    Ok(Outcome::Applied)
}

fn locus_update(conn: &Connection, op: &RemoteOp) -> Result<Outcome> {
    let patch: LocusPatch = serde_json::from_value(op.data.clone().unwrap_or(Value::Null))
        .map_err(|e| StoreError::Invalid(format!("Änderung ist ungültig: {e}")))?;
    if !exists(conn, "loci", &op.entity_id)? {
        return Ok(Outcome::Missing);
    }
    let mut changes: Vec<(&str, SqlValue)> = Vec::new();
    if let Some(v) = patch.image_size {
        changes.push(("image_size", SqlValue::Real(v)));
    }
    if let Some(v) = patch.zoom {
        changes.push(("zoom", SqlValue::Real(v)));
    }
    if let Some(v) = &patch.place {
        changes.push(("place", SqlValue::Text(v.clone())));
    }
    if let Some(v) = &patch.item {
        changes.push(("item", SqlValue::Text(v.clone())));
    }
    if let Some(v) = patch.x {
        changes.push(("x", SqlValue::Real(v)));
    }
    if let Some(v) = patch.y {
        changes.push(("y", SqlValue::Real(v)));
    }
    if let Some(time) = patch.time {
        changes.push(("time", time.map_or(SqlValue::Null, SqlValue::Real)));
    }
    if let Some(answer) = &patch.answer {
        let json =
            serde_json::to_string(answer).map_err(|_| StoreError::Invalid("Antwort".into()))?;
        changes.push(("answer", SqlValue::Text(json)));
    }
    if let Some(image) = &patch.image {
        if let Some(hash) = image {
            require_media(conn, hash)?;
        }
        changes.push((
            "image",
            image.clone().map_or(SqlValue::Null, SqlValue::Text),
        ));
    }
    if let Some(v) = &patch.note {
        changes.push(("note", SqlValue::Text(v.clone())));
    }
    let mut applied = false;
    for (column, value) in changes {
        if !wins(conn, "locus", &op.entity_id, column, &op.hlc)? {
            continue;
        }
        // `column` comes from the fixed list above, never from the network.
        conn.execute(
            &format!("UPDATE loci SET {column} = ?1 WHERE id = ?2"),
            params![value, op.entity_id],
        )?;
        stamp(conn, "locus", &op.entity_id, column, &op.hlc)?;
        applied = true;
    }
    Ok(if applied {
        Outcome::Applied
    } else {
        Outcome::Stale
    })
}

fn row_set(conn: &Connection, op: &RemoteOp) -> Result<Outcome> {
    let fields = object(op)?;
    let palace = optional_text(fields.get("palace").unwrap_or(&Value::Null))?;
    if let Some(palace_id) = &palace
        && !exists(conn, "palaces", palace_id)?
    {
        return Ok(Outcome::Missing);
    }
    if !wins(conn, "row", &op.entity_id, "cells", &op.hlc)? {
        return Ok(Outcome::Stale);
    }
    let cells = serde_json::to_string(fields.get("cells").unwrap_or(&Value::Null))
        .map_err(|_| StoreError::Invalid("Zellen".into()))?;
    let table = text_field(op, "table")?;
    let key = text_field(op, "key")?;
    let changed = conn.execute(
        "UPDATE table_rows SET cells = ?1
         WHERE COALESCE(palace_id, '') = COALESCE(?2, '') AND table_name = ?3 AND key = ?4",
        params![cells, palace, table, key],
    )?;
    if changed == 0 {
        conn.execute(
            "INSERT INTO table_rows(id, palace_id, table_name, key, cells) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![new_id(), palace, table, key, cells],
        )?;
    }
    stamp_all(conn, "row", op)?;
    Ok(Outcome::Applied)
}

fn row_delete(conn: &Connection, op: &RemoteOp) -> Result<Outcome> {
    let fields = object(op)?;
    let palace = optional_text(fields.get("palace").unwrap_or(&Value::Null))?;
    let deleted = conn.execute(
        "DELETE FROM table_rows
         WHERE COALESCE(palace_id, '') = COALESCE(?1, '') AND table_name = ?2 AND key = ?3",
        params![palace, text_field(op, "table")?, text_field(op, "key")?],
    )?;
    tombstone(conn, "row", &op.entity_id, &op.hlc)?;
    Ok(if deleted > 0 {
        Outcome::Applied
    } else {
        Outcome::Stale
    })
}

fn binding_set(conn: &Connection, op: &RemoteOp) -> Result<Outcome> {
    let (locus_id, field) = split_entity(&op.entity_id)?;
    let field = BindingField::parse(field).map_err(|e| StoreError::Invalid(e.0))?;
    let binding = Binding {
        field,
        expression: text_field(op, "expression")?.to_owned(),
    };
    binding.validate().map_err(|e| StoreError::Invalid(e.0))?;
    if !exists(conn, "loci", locus_id)? {
        return Ok(Outcome::Missing);
    }
    if !wins(conn, "binding", &op.entity_id, "expression", &op.hlc)? {
        return Ok(Outcome::Stale);
    }
    upsert_binding(conn, locus_id, &binding)?;
    stamp(conn, "binding", &op.entity_id, "expression", &op.hlc)?;
    Ok(Outcome::Applied)
}

fn binding_delete(conn: &Connection, op: &RemoteOp) -> Result<Outcome> {
    let (locus_id, field) = split_entity(&op.entity_id)?;
    let removed = conn.execute(
        "DELETE FROM bindings WHERE locus_id = ?1 AND field = ?2",
        params![locus_id, field],
    )?;
    tombstone(conn, "binding", &op.entity_id, &op.hlc)?;
    Ok(if removed > 0 {
        Outcome::Applied
    } else {
        Outcome::Stale
    })
}

fn kv_set(conn: &Connection, op: &RemoteOp) -> Result<Outcome> {
    let (plugin, key) = op
        .entity_id
        .split_once('/')
        .ok_or_else(|| StoreError::Invalid("Plugin-Schlüssel ist ungültig".into()))?;
    if !wins(conn, "kv", &op.entity_id, "value", &op.hlc)? {
        return Ok(Outcome::Stale);
    }
    let value = unhex(text_field(op, "value")?)?;
    conn.execute(
        "INSERT INTO kv(plugin, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(plugin, key) DO UPDATE SET value = excluded.value",
        params![plugin, key, value],
    )?;
    stamp(conn, "kv", &op.entity_id, "value", &op.hlc)?;
    Ok(Outcome::Applied)
}

/// Deletes one row by id and leaves a tombstone, so older writes stay deleted.
fn delete_entity(conn: &Connection, op: &RemoteOp, table: &str, id: &str) -> Result<Outcome> {
    let deleted = conn.execute(&format!("DELETE FROM {table} WHERE id = ?1"), [id])?;
    tombstone(conn, &op.entity, id, &op.hlc)?;
    Ok(if deleted > 0 {
        Outcome::Applied
    } else {
        Outcome::Stale
    })
}

/// Finished journeys are immutable: a journey is stored once, on every device.
fn journey_create(conn: &Connection, op: &RemoteOp) -> Result<Outcome> {
    if exists(conn, "journeys", &op.entity_id)? {
        return Ok(Outcome::Stale);
    }
    let answers = object(op)?.get("answers").cloned().unwrap_or(Value::Null);
    conn.execute(
        "INSERT INTO journeys(id, palace_name, answers, recorded_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            op.entity_id,
            text_field(op, "palace_name")?,
            serde_json::to_string(&answers).map_err(|_| StoreError::Invalid("Antworten".into()))?,
            number_field(op, "recorded_at").unwrap_or(0) as i64,
        ],
    )?;
    Ok(Outcome::Applied)
}

fn upsert_binding(conn: &Connection, locus_id: &str, binding: &Binding) -> Result<()> {
    conn.execute(
        "INSERT INTO bindings(locus_id, field, expression) VALUES (?1, ?2, ?3)
         ON CONFLICT(locus_id, field) DO UPDATE SET expression = excluded.expression",
        params![locus_id, binding.field.name(), binding.expression],
    )?;
    Ok(())
}

/// A palace name must stay unique (case-insensitive); a clash gets " (2)", " (3)", …
fn unique_palace_name(conn: &Connection, base: &str, palace_id: &str) -> Result<String> {
    let base = base.trim();
    if base.is_empty() {
        return Err(StoreError::Invalid("Der Palastname ist leer".into()));
    }
    let mut candidate = base.to_owned();
    let mut number = 2;
    loop {
        let taken: i64 = conn.query_row(
            "SELECT COUNT(*) FROM palaces WHERE name = ?1 COLLATE NOCASE AND id <> ?2",
            params![candidate, palace_id],
            |r| r.get(0),
        )?;
        if taken == 0 {
            return Ok(candidate);
        }
        candidate = format!("{base} ({number})");
        number += 1;
    }
}

fn require_media(conn: &Connection, hash: &str) -> Result<()> {
    let present: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM media WHERE hash = ?1)",
        [hash],
        |r| r.get(0),
    )?;
    if present {
        Ok(())
    } else {
        Err(StoreError::Invalid(format!(
            "Das Medium {hash} fehlt auf diesem Gerät"
        )))
    }
}

fn exists(conn: &Connection, table: &str, id: &str) -> Result<bool> {
    Ok(conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ?1)"),
        [id],
        |r| r.get(0),
    )?)
}

/// True if `value` (an operation of `entity_id`) is not newer than the last write to `field`.
fn wins(conn: &Connection, entity: &str, entity_id: &str, field: &str, hlc: &str) -> Result<bool> {
    let current: Option<String> = conn
        .query_row(
            "SELECT hlc FROM sync_clock WHERE entity = ?1 AND entity_id = ?2 AND field = ?3",
            params![entity, entity_id, field],
            |r| r.get(0),
        )
        .optional()?;
    Ok(current.is_none_or(|current| current.as_str() < hlc))
}

fn stamp(conn: &Connection, entity: &str, entity_id: &str, field: &str, hlc: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_clock(entity, entity_id, field, hlc) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(entity, entity_id, field) DO UPDATE SET hlc = excluded.hlc",
        params![entity, entity_id, field, hlc],
    )?;
    Ok(())
}

fn stamp_all(conn: &Connection, entity: &str, op: &RemoteOp) -> Result<()> {
    for field in object(op)?.keys() {
        stamp(conn, entity, &op.entity_id, field, &op.hlc)?;
    }
    Ok(())
}

fn tombstone(conn: &Connection, entity: &str, entity_id: &str, hlc: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_tombstone(entity, entity_id, hlc) VALUES (?1, ?2, ?3)
         ON CONFLICT(entity, entity_id) DO UPDATE SET
           hlc = CASE WHEN excluded.hlc > sync_tombstone.hlc THEN excluded.hlc ELSE sync_tombstone.hlc END",
        params![entity, entity_id, hlc],
    )?;
    Ok(())
}

/// True if a delete at least as new as `hlc` already removed this entity.
fn tombstoned(conn: &Connection, entity: &str, entity_id: &str, hlc: &str) -> Result<bool> {
    let deleted: Option<String> = conn
        .query_row(
            "SELECT hlc FROM sync_tombstone WHERE entity = ?1 AND entity_id = ?2",
            params![entity, entity_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(deleted.is_some_and(|deleted| deleted.as_str() >= hlc))
}

fn split_entity(entity_id: &str) -> Result<(&str, &str)> {
    entity_id
        .rsplit_once('/')
        .ok_or_else(|| StoreError::Invalid("Kennung ist ungültig".into()))
}

fn object(op: &RemoteOp) -> Result<&Map<String, Value>> {
    op.data
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| StoreError::Invalid(format!("{}/{} ohne Daten", op.entity, op.op)))
}

fn text_field<'a>(op: &'a RemoteOp, field: &str) -> Result<&'a str> {
    as_text(object(op)?.get(field).unwrap_or(&Value::Null))
        .map_err(|_| StoreError::Invalid(format!("Feld `{field}` fehlt")))
}

fn number_field(op: &RemoteOp, field: &str) -> Result<u32> {
    object(op)?
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| StoreError::Invalid(format!("Feld `{field}` ist keine Zahl")))
}

fn as_text(value: &Value) -> Result<&str> {
    value
        .as_str()
        .ok_or_else(|| StoreError::Invalid("Text erwartet".into()))
}

fn optional_text(value: &Value) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        other => Ok(Some(as_text(other)?.to_owned())),
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Result<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err(StoreError::Invalid("Wert ist kein Hex".into()));
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16))
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| StoreError::Invalid("Wert ist kein Hex".into()))
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
