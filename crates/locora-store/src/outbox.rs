//! The outbox of the sync: local operations after the push cursor, what of them still needs to
//! be sent, and what they change, in words a person understands.
//!
//! An operation is left out of a push only when later operations in the same unsent range
//! replace everything it does on every device, whatever order the devices receive them in (see
//! `superseded`). The rules follow the merge in `sync.rs`: fields are last-writer-wins by HLC, and
//! a delete removes the entity (a palace with its rooms and loci) and outlasts older writes.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Op, RemoteOp, Result, Store};

/// One logged operation without its payload: what compaction and the summary need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpHead {
    pub seq: i64,
    pub hlc: String,
    pub entity: String,
    pub entity_id: String,
    pub op: String,
    /// The fields an update really sets (see `effective_fields`); empty for other operations.
    pub fields: Vec<String>,
    /// The parent named in the data: the palace of a room or a table row, the room of a locus.
    pub parent: Option<String>,
}

/// What a group of operations does to one palace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    New,
    Changed,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PalaceChange {
    pub id: String,
    /// `None` when this device never knew the name.
    pub name: Option<String>,
    pub kind: ChangeKind,
    /// Operations on the palace, its rooms and loci.
    pub changes: usize,
}

/// Changes grouped by palace; `other` counts the rest (reviews, journeys, settings of plugins).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSummary {
    pub total: usize,
    pub palaces: Vec<PalaceChange>,
    pub other: usize,
}

impl ChangeSummary {
    /// Counts one operation. A palace created and deleted again within the summary is left out:
    /// for everyone else it never existed.
    pub fn add(&mut self, palace: Option<&str>, kind: ChangeKind) {
        self.total += 1;
        let Some(id) = palace else {
            self.other += 1;
            return;
        };
        match self.palaces.iter().position(|entry| entry.id == id) {
            Some(index) => {
                let entry = &mut self.palaces[index];
                entry.changes += 1;
                match (entry.kind, kind) {
                    (ChangeKind::New, ChangeKind::Deleted) => {
                        self.palaces.remove(index);
                    }
                    (_, ChangeKind::Deleted) | (ChangeKind::Changed, ChangeKind::New) => {
                        entry.kind = kind
                    }
                    _ => {}
                }
            }
            None => self.palaces.push(PalaceChange {
                id: id.to_owned(),
                name: None,
                kind,
                changes: 1,
            }),
        }
    }

    /// The palaces this summary deletes.
    pub fn deleted(&self) -> impl Iterator<Item = &PalaceChange> {
        self.palaces
            .iter()
            .filter(|change| change.kind == ChangeKind::Deleted)
    }
}

/// The kind of change an operation makes to its palace.
pub fn change_kind(entity: &str, op: &str) -> ChangeKind {
    match (entity, op) {
        ("palace", "create") => ChangeKind::New,
        ("palace", "delete") => ChangeKind::Deleted,
        _ => ChangeKind::Changed,
    }
}

/// The fields of an update that change something when applied: a key with `null` changes
/// nothing, except where `null` removes a value (a locus's time, picture or sound; a marker).
pub fn effective_fields(entity: &str, op: &str, data: Option<&Value>) -> Vec<String> {
    if op != "update" {
        return Vec::new();
    }
    let Some(fields) = data.and_then(Value::as_object) else {
        return Vec::new();
    };
    let (known, nullable): (&[&str], &[&str]) = match entity {
        "palace" => (&["name", "marker"], &["marker"]),
        "room" => (&["name", "position"], &[]),
        "locus" => (
            &[
                "image_size",
                "zoom",
                "place",
                "item",
                "x",
                "y",
                "time",
                "answer",
                "image",
                "audio",
                "note",
            ],
            &["time", "image", "audio"],
        ),
        _ => return Vec::new(),
    };
    fields
        .iter()
        .filter(|(key, value)| {
            known.contains(&key.as_str()) && (!value.is_null() || nullable.contains(&key.as_str()))
        })
        .map(|(key, _)| key.clone())
        .collect()
}

/// What later operations of the same range do to one entity: the newest HLC of each kind.
#[derive(Default)]
struct Later<'a> {
    set: Option<&'a str>,
    delete: Option<&'a str>,
    update: Option<&'a str>,
    fields: HashMap<&'a str, &'a str>,
}

/// Keeps the newer of two HLCs (they sort as text).
fn newer<'a>(slot: &mut Option<&'a str>, hlc: &'a str) {
    if slot.is_none_or(|current| current < hlc) {
        *slot = Some(hlc);
    }
}

/// The sequence numbers of operations whose every effect later operations of `heads` replace
/// on every device, whatever else the devices receive and in whichever order. `palace_of` names
/// the palace an operation belongs to (`None` when unknown). Leaving these out of a push changes
/// nothing that any device ends up with:
///
/// - a `set` (plugin data, table row, formula) followed by another `set` or a `delete` of the
///   same entity: the newest write wins by HLC, and a delete removes the row whatever its clock;
/// - an update of an entity that is deleted later: the delete removes it, and its tombstone
///   outlasts every older write;
/// - an update whose every field a later update of the same entity sets again (per-field
///   last-writer-wins: the older value can never be the one that stays);
/// - an update or `set` on a room, locus, formula or table row of a palace that is deleted
///   later: the palace delete removes them with it on every device.
///
/// Creates and deletes are always sent, so no device ever receives a change for something it
/// cannot place.
pub fn superseded(heads: &[OpHead], palace_of: impl Fn(&OpHead) -> Option<String>) -> HashSet<i64> {
    let mut later: HashMap<(&str, &str), Later> = HashMap::new();
    let mut palace_deleted: HashMap<&str, &str> = HashMap::new();
    let mut dropped = HashSet::new();
    for head in heads.iter().rev() {
        let hlc = head.hlc.as_str();
        let seen = later
            .entry((head.entity.as_str(), head.entity_id.as_str()))
            .or_default();
        let after = |slot: Option<&str>| slot.is_some_and(|newest| newest > hlc);
        let replaced = match head.op.as_str() {
            "set" => after(seen.set) || after(seen.delete),
            "update" => {
                after(seen.delete)
                    || (after(seen.update)
                        && head.fields.iter().all(|field| {
                            seen.fields
                                .get(field.as_str())
                                .is_some_and(|newest| *newest > hlc)
                        }))
            }
            _ => false,
        };
        let parent_deleted = matches!(head.op.as_str(), "set" | "update")
            && head.entity != "palace"
            && palace_of(head)
                .and_then(|palace| palace_deleted.get(palace.as_str()).copied())
                .is_some_and(|deleted| deleted > hlc);
        if replaced || parent_deleted {
            dropped.insert(head.seq);
        }
        match head.op.as_str() {
            "delete" => newer(&mut seen.delete, hlc),
            "set" => newer(&mut seen.set, hlc),
            "update" => {
                newer(&mut seen.update, hlc);
                for field in &head.fields {
                    let slot = seen.fields.entry(field.as_str()).or_insert(hlc);
                    if *slot < hlc {
                        *slot = hlc;
                    }
                }
            }
            _ => {}
        }
        if head.entity == "palace" && head.op == "delete" {
            let slot = palace_deleted.entry(head.entity_id.as_str()).or_insert(hlc);
            if *slot < hlc {
                *slot = hlc;
            }
        }
    }
    dropped
}

/// The palace of rooms and loci, from the tables and, for deleted ones, from the log.
struct Resolver<'a> {
    conn: &'a Connection,
    rooms: HashMap<String, Option<String>>,
    loci: HashMap<String, Option<String>>,
    logged: Option<(HashMap<String, String>, HashMap<String, String>)>,
}

impl<'a> Resolver<'a> {
    fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            rooms: HashMap::new(),
            loci: HashMap::new(),
            logged: None,
        }
    }

    /// Rooms and loci as they were created on this device: (room → palace, locus → room).
    fn logged(&mut self) -> Result<&(HashMap<String, String>, HashMap<String, String>)> {
        if self.logged.is_none() {
            let mut rooms = HashMap::new();
            let mut loci = HashMap::new();
            let mut stmt = self.conn.prepare(
                "SELECT entity, entity_id,
                        CASE entity WHEN 'room' THEN json_extract(data, '$.palace_id')
                                    ELSE json_extract(data, '$.room_id') END
                 FROM ops WHERE op = 'create' AND entity IN ('room', 'locus') AND json_valid(data)",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?;
            for row in rows {
                let (entity, id, parent) = row?;
                if let Some(parent) = parent {
                    if entity == "room" {
                        rooms.insert(id, parent);
                    } else {
                        loci.insert(id, parent);
                    }
                }
            }
            self.logged = Some((rooms, loci));
        }
        Ok(self.logged.as_ref().expect("filled above"))
    }

    fn room(&mut self, id: &str, hint: Option<&str>) -> Result<Option<String>> {
        if let Some(known) = self.rooms.get(id) {
            return Ok(known.clone());
        }
        let mut palace: Option<String> = self
            .conn
            .query_row("SELECT palace_id FROM rooms WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        if palace.is_none() {
            palace = match hint {
                Some(hint) => Some(hint.to_owned()),
                None => self.logged()?.0.get(id).cloned(),
            };
        }
        self.rooms.insert(id.to_owned(), palace.clone());
        Ok(palace)
    }

    fn locus(&mut self, id: &str, hint: Option<&str>) -> Result<Option<String>> {
        if let Some(known) = self.loci.get(id) {
            return Ok(known.clone());
        }
        let mut room: Option<String> = self
            .conn
            .query_row("SELECT room_id FROM loci WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        if room.is_none() {
            room = match hint {
                Some(hint) => Some(hint.to_owned()),
                None => self.logged()?.1.get(id).cloned(),
            };
        }
        let palace = match room {
            Some(room) => self.room(&room, None)?,
            None => None,
        };
        self.loci.insert(id.to_owned(), palace.clone());
        Ok(palace)
    }

    /// The palace an operation belongs to; `None` for plugin data, journeys and shared rows.
    fn palace(
        &mut self,
        entity: &str,
        entity_id: &str,
        parent: Option<&str>,
    ) -> Result<Option<String>> {
        match entity {
            "palace" => Ok(Some(entity_id.to_owned())),
            "room" => self.room(entity_id, parent),
            "locus" => self.locus(entity_id, parent),
            "binding" => match entity_id.rsplit_once('/') {
                Some((locus, _)) => self.locus(locus, None),
                None => Ok(None),
            },
            "row" => Ok(parent.map(str::to_owned)),
            _ => Ok(None),
        }
    }
}

/// The parent an operation's data names (see `OpHead::parent`).
fn parent_of(entity: &str, data: Option<&Value>) -> Option<String> {
    let key = match entity {
        "room" => "palace_id",
        "locus" => "room_id",
        "row" => "palace",
        _ => return None,
    };
    data?.get(key)?.as_str().map(str::to_owned)
}

/// The last name this device logged for a palace (for one that is deleted now).
fn logged_name(conn: &Connection, id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT json_extract(data, '$.name') FROM ops
             WHERE entity = 'palace' AND entity_id = ?1 AND json_valid(data)
               AND json_type(data, '$.name') = 'text'
             ORDER BY seq DESC LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .optional()?)
}

fn current_name(conn: &Connection, id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT name FROM palaces WHERE id = ?1", [id], |r| r.get(0))
        .optional()?)
}

impl Store {
    /// Heads of the logged operations after `seq`, oldest first. Plugin data and journeys are read
    /// without their payload, which can be large.
    pub fn outbox_heads(&self, seq: i64) -> Result<Vec<OpHead>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT seq, hlc, entity, entity_id, op,
                    CASE WHEN entity IN ('palace', 'room', 'locus', 'row') THEN data END
             FROM ops WHERE seq > ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map([seq], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        let mut heads = Vec::new();
        for row in rows {
            let (seq, hlc, entity, entity_id, op, data) = row?;
            let data: Option<Value> = data.and_then(|text| serde_json::from_str(&text).ok());
            heads.push(OpHead {
                fields: effective_fields(&entity, &op, data.as_ref()),
                parent: parent_of(&entity, data.as_ref()),
                seq,
                hlc,
                entity,
                entity_id,
                op,
            });
        }
        Ok(heads)
    }

    /// The operations after `seq` that a push leaves out (see `superseded`).
    pub fn superseded_after(&self, seq: i64) -> Result<HashSet<i64>> {
        let heads = self.outbox_heads(seq)?;
        self.superseded_in(&heads)
    }

    fn superseded_in(&self, heads: &[OpHead]) -> Result<HashSet<i64>> {
        let conn = self.conn();
        let mut resolver = Resolver::new(&conn);
        let mut palaces = HashMap::new();
        for head in heads {
            let palace = resolver.palace(&head.entity, &head.entity_id, head.parent.as_deref())?;
            palaces.insert(head.seq, palace);
        }
        Ok(superseded(heads, |head| {
            palaces.get(&head.seq).cloned().flatten()
        }))
    }

    /// Up to `limit` logged operations after `seq`, oldest first.
    pub fn ops_after(&self, seq: i64, limit: usize) -> Result<Vec<Op>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT seq, hlc, entity, entity_id, op, data FROM ops WHERE seq > ?1 ORDER BY seq LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![seq, limit as i64], |r| {
            Ok(Op {
                seq: r.get(0)?,
                hlc: r.get(1)?,
                entity: r.get(2)?,
                entity_id: r.get(3)?,
                op: r.get(4)?,
                data: r
                    .get::<_, Option<String>>(5)?
                    .and_then(|d| serde_json::from_str(&d).ok()),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The sequence number of the newest logged operation (0 for none).
    pub fn last_op_seq(&self) -> Result<i64> {
        let conn = self.conn();
        Ok(conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM ops", [], |r| r.get(0))?)
    }

    /// What the next push sends after `seq`, grouped by palace. A palace that was created and
    /// deleted again since then is left out: it never reached the cloud.
    pub fn pending_changes(&self, seq: i64) -> Result<ChangeSummary> {
        let heads = self.outbox_heads(seq)?;
        let dropped = self.superseded_in(&heads)?;
        let created: HashSet<&str> = heads
            .iter()
            .filter(|head| head.entity == "palace" && head.op == "create")
            .map(|head| head.entity_id.as_str())
            .collect();
        let conn = self.conn();
        let mut resolver = Resolver::new(&conn);
        let mut summary = ChangeSummary::default();
        for head in heads.iter().filter(|head| !dropped.contains(&head.seq)) {
            let palace = resolver.palace(&head.entity, &head.entity_id, head.parent.as_deref())?;
            let mut kind = change_kind(&head.entity, &head.op);
            // A palace created in this range is new, whichever of its operations is left.
            if kind == ChangeKind::Changed && palace.as_deref().is_some_and(|p| created.contains(p))
            {
                kind = ChangeKind::New;
            }
            summary.add(palace.as_deref(), kind);
        }
        drop(resolver);
        name_palaces(&conn, &mut summary, &HashMap::new())?;
        Ok(summary)
    }

    /// The palace a received operation belongs to, as this device knows it before applying it.
    pub fn palace_of(&self, op: &RemoteOp) -> Result<Option<String>> {
        let conn = self.conn();
        let parent = parent_of(&op.entity, op.data.as_ref());
        Resolver::new(&conn).palace(&op.entity, &op.entity_id, parent.as_deref())
    }

    /// The name of a palace on this device, or the last one it logged.
    pub fn palace_name(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn();
        match current_name(&conn, id)? {
            Some(name) => Ok(Some(name)),
            None => logged_name(&conn, id),
        }
    }

    /// Fills in the names of a summary's palaces: the current name, else one from `known` (taken
    /// before a received delete), else the last one logged here.
    pub fn name_palaces(
        &self,
        summary: &mut ChangeSummary,
        known: &HashMap<String, String>,
    ) -> Result<()> {
        let conn = self.conn();
        name_palaces(&conn, summary, known)
    }
}

fn name_palaces(
    conn: &Connection,
    summary: &mut ChangeSummary,
    known: &HashMap<String, String>,
) -> Result<()> {
    for entry in &mut summary.palaces {
        if entry.name.is_some() {
            continue;
        }
        entry.name = match current_name(conn, &entry.id)? {
            Some(name) => Some(name),
            None => match known.get(&entry.id) {
                Some(name) => Some(name.clone()),
                None => logged_name(conn, &entry.id)?,
            },
        };
    }
    Ok(())
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
