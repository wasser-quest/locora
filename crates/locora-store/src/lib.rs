//! Offline-first local storage for Locora.
//!
//! One SQLite database per profile holds palaces, rooms, loci, media
//! metadata and plugin key-value data. Media files are stored next to it,
//! named by their SHA-256 (deduplicated, immutable). Every change is also
//! appended to an operation log stamped with a hybrid logical clock, so a
//! later sync service can replay changes between devices.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io};

use locora_formula::Row;
use locora_formula::workbook::{Binding, BindingField, effective_rows};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use locora_api::journey::{Answer, AnswerKind, QuizOption};

mod schema;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("Datenbank: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Datei: {0}")]
    Io(#[from] io::Error),
    #[error("{0} nicht gefunden")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Palace {
    pub id: String,
    pub name: String,
    /// Seconds since the Unix epoch.
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomKind {
    /// No picture: loci are placed on an empty canvas.
    Blank,
    Image,
    Video,
}

impl RoomKind {
    fn as_str(self) -> &'static str {
        match self {
            RoomKind::Blank => "blank",
            RoomKind::Image => "image",
            RoomKind::Video => "video",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "blank" => Ok(RoomKind::Blank),
            "image" => Ok(RoomKind::Image),
            "video" => Ok(RoomKind::Video),
            other => Err(StoreError::Invalid(format!(
                "unbekannte Raum-Art `{other}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub palace_id: String,
    pub position: u32,
    pub name: String,
    pub kind: RoomKind,
    /// SHA-256 of the image or video, if any.
    pub media: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Locus {
    pub id: String,
    pub room_id: String,
    pub position: u32,
    /// The place, e.g. "Herd".
    pub place: String,
    /// What to remember there, e.g. "Merkur".
    pub item: String,
    /// Position on the room, 0..1 from the left / top.
    pub x: f64,
    pub y: f64,
    /// For video rooms: seconds into the video.
    pub time: Option<f64>,
    #[serde(default)]
    pub answer: Answer,
    /// Formulas that compute fields of this locus from the palace's tables.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<Binding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomTree {
    pub room: Room,
    pub loci: Vec<Locus>,
}

/// A palace with all rooms and loci in route order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PalaceTree {
    pub palace: Palace,
    pub rooms: Vec<RoomTree>,
    /// Tables that belong to this palace (shared tables are separate).
    pub tables: Vec<Row>,
}

impl PalaceTree {
    /// All loci in route order (room by room).
    pub fn route(&self) -> impl Iterator<Item = &Locus> {
        self.rooms.iter().flat_map(|s| &s.loci)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PalaceSummary {
    pub id: String,
    pub name: String,
    pub rooms: u32,
    pub loci: u32,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewLocus {
    pub place: String,
    pub item: String,
    pub x: f64,
    pub y: f64,
    pub time: Option<f64>,
    #[serde(default)]
    pub answer: Answer,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LocusPatch {
    pub place: Option<String>,
    pub item: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub time: Option<Option<f64>>,
    pub answer: Option<Answer>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaRef {
    pub hash: String,
    pub mime: String,
    pub size: u64,
}

/// One logged change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Op {
    pub seq: i64,
    /// Hybrid logical clock: `<millis>-<counter>-<node>`, sortable as text.
    pub hlc: String,
    pub entity: String,
    pub entity_id: String,
    pub op: String,
    pub data: Option<serde_json::Value>,
}

pub struct Store {
    conn: Mutex<Connection>,
    media_dir: Option<PathBuf>,
    node: String,
    clock: Mutex<(u64, u32)>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn new_id() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}

fn check_position(x: f64, y: f64) -> Result<()> {
    if (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) {
        Ok(())
    } else {
        Err(StoreError::Invalid(format!(
            "Position ({x}, {y}) liegt außerhalb des Raums (0..1)"
        )))
    }
}

impl Store {
    /// Open (or create) the profile in `dir`: `dir/locora.db` and `dir/media/`.
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)?;
        let conn = Connection::open(dir.join("locora.db"))?;
        Self::init(conn, Some(dir.join("media")))
    }

    /// A store that lives only in memory (tests, previews). Media cannot be stored.
    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, None)
    }

    fn init(conn: Connection, media_dir: Option<PathBuf>) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        schema::migrate(&conn)?;
        let node = match conn
            .query_row("SELECT value FROM meta WHERE key = 'node'", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
        {
            Some(node) => node,
            None => {
                let node = new_id()[20..].to_owned();
                conn.execute("INSERT INTO meta(key, value) VALUES ('node', ?1)", [&node])?;
                node
            }
        };
        Ok(Self {
            conn: Mutex::new(conn),
            media_dir,
            node,
            clock: Mutex::new((0, 0)),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn tick(&self) -> String {
        let mut clock = self.clock.lock().unwrap_or_else(|p| p.into_inner());
        let wall = now_millis();
        *clock = if wall > clock.0 {
            (wall, 0)
        } else {
            (clock.0, clock.1 + 1)
        };
        format!("{:013}-{:05}-{}", clock.0, clock.1, self.node)
    }

    fn log(
        &self,
        conn: &Connection,
        entity: &str,
        id: &str,
        op: &str,
        data: Option<serde_json::Value>,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO ops(hlc, entity, entity_id, op, data) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![self.tick(), entity, id, op, data.map(|d| d.to_string())],
        )?;
        Ok(())
    }

    fn touch(&self, conn: &Connection, palace_id: &str) -> Result<()> {
        conn.execute(
            "UPDATE palaces SET updated_at = ?1 WHERE id = ?2",
            params![now_secs(), palace_id],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------ palaces

    pub fn create_palace(&self, name: &str) -> Result<Palace> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::Invalid("Der Palast braucht einen Namen".into()));
        }
        let conn = self.conn();
        let taken: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM palaces WHERE name = ?1 COLLATE NOCASE)",
            [name],
            |r| r.get(0),
        )?;
        if taken {
            return Err(StoreError::Invalid(format!(
                "Es gibt schon einen Palast „{name}“"
            )));
        }
        let now = now_secs();
        let palace = Palace {
            id: new_id(),
            name: name.to_owned(),
            created_at: now,
            updated_at: now,
        };
        conn.execute(
            "INSERT INTO palaces(id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![palace.id, palace.name, now, now],
        )?;
        self.log(
            &conn,
            "palace",
            &palace.id,
            "create",
            Some(serde_json::json!({ "name": palace.name })),
        )?;
        Ok(palace)
    }

    /// Find a palace by id, by name (case-insensitive) or by a unique id prefix.
    pub fn find_palace(&self, query: &str) -> Result<Option<Palace>> {
        let conn = self.conn();
        let row = |r: &rusqlite::Row<'_>| {
            Ok(Palace {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: r.get(2)?,
                updated_at: r.get(3)?,
            })
        };
        let cols = "SELECT id, name, created_at, updated_at FROM palaces";
        if let Some(p) = conn
            .query_row(
                &format!("{cols} WHERE id = ?1 OR name = ?1 COLLATE NOCASE"),
                [query],
                row,
            )
            .optional()?
        {
            return Ok(Some(p));
        }
        if query.len() >= 4 {
            let mut stmt = conn.prepare(&format!("{cols} WHERE id LIKE ?1 || '%' LIMIT 2"))?;
            let found: Vec<Palace> = stmt.query_map([query], row)?.collect::<Result<_, _>>()?;
            if found.len() == 1 {
                return Ok(found.into_iter().next());
            }
        }
        Ok(None)
    }

    /// Like [`Store::find_palace`], but an unknown palace is an error.
    pub fn palace(&self, query: &str) -> Result<PalaceTree> {
        let palace = self
            .find_palace(query)?
            .ok_or_else(|| StoreError::NotFound(format!("Palast „{query}“")))?;
        let conn = self.conn();
        let mut rooms = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id, palace_id, position, name, kind, media FROM rooms WHERE palace_id = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map([&palace.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        for row in rows {
            let (id, palace_id, position, name, kind, media) = row?;
            rooms.push(Room {
                id,
                palace_id,
                position,
                name,
                kind: RoomKind::parse(&kind)?,
                media,
            });
        }
        let mut loci_stmt = conn.prepare(
            "SELECT id, room_id, position, place, item, x, y, time, answer FROM loci WHERE room_id = ?1 ORDER BY position",
        )?;
        let mut tree = Vec::new();
        for room in rooms {
            let loci = loci_stmt
                .query_map([&room.id], |r| {
                    Ok(Locus {
                        id: r.get(0)?,
                        room_id: r.get(1)?,
                        position: r.get(2)?,
                        place: r.get(3)?,
                        item: r.get(4)?,
                        x: r.get(5)?,
                        y: r.get(6)?,
                        time: r.get(7)?,
                        answer: read_answer(r, 8)?,
                        bindings: Vec::new(),
                    })
                })?
                .collect::<Result<Vec<Locus>, _>>()?;
            tree.push(RoomTree { room, loci });
        }
        let mut bind_stmt = conn
            .prepare("SELECT field, expression FROM bindings WHERE locus_id = ?1 ORDER BY field")?;
        for room in &mut tree {
            for locus in &mut room.loci {
                let rows = bind_stmt.query_map([&locus.id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?;
                for row in rows {
                    let (field, expression) = row?;
                    let field =
                        BindingField::parse(&field).map_err(|e| StoreError::Invalid(e.0))?;
                    locus.bindings.push(Binding { field, expression });
                }
            }
        }
        drop(bind_stmt);
        drop(loci_stmt);
        drop(stmt);
        let tables = rows_in(&conn, Some(&palace.id))?;
        Ok(PalaceTree {
            palace,
            rooms: tree,
            tables,
        })
    }

    /// All palaces, most recently changed first.
    pub fn palaces(&self) -> Result<Vec<PalaceSummary>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT p.id, p.name, p.updated_at,
                    (SELECT COUNT(*) FROM rooms s WHERE s.palace_id = p.id),
                    (SELECT COUNT(*) FROM loci l JOIN rooms s ON l.room_id = s.id WHERE s.palace_id = p.id)
             FROM palaces p ORDER BY p.updated_at DESC, p.name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(PalaceSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                updated_at: r.get(2)?,
                rooms: r.get(3)?,
                loci: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn rename_palace(&self, id: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::Invalid("Der Palast braucht einen Namen".into()));
        }
        let conn = self.conn();
        let changed = conn.execute(
            "UPDATE palaces SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name, now_secs(), id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!("Palast {id}")));
        }
        self.log(
            &conn,
            "palace",
            id,
            "update",
            Some(serde_json::json!({ "name": name })),
        )
    }

    /// Delete a palace with all rooms and loci. Returns false if it did not exist.
    pub fn delete_palace(&self, id: &str) -> Result<bool> {
        let conn = self.conn();
        let deleted = conn.execute("DELETE FROM palaces WHERE id = ?1", [id])? > 0;
        if deleted {
            self.log(&conn, "palace", id, "delete", None)?;
        }
        Ok(deleted)
    }

    // ------------------------------------------------------------ rooms

    pub fn add_room(
        &self,
        palace_id: &str,
        name: &str,
        kind: RoomKind,
        media: Option<&str>,
    ) -> Result<Room> {
        if kind != RoomKind::Blank && media.is_none() {
            return Err(StoreError::Invalid(
                "Räume mit Bild oder Video brauchen eine Mediendatei".into(),
            ));
        }
        let conn = self.conn();
        let position: u32 = conn.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM rooms WHERE palace_id = ?1",
            [palace_id],
            |r| r.get(0),
        )?;
        let room = Room {
            id: new_id(),
            palace_id: palace_id.to_owned(),
            position,
            name: name.trim().to_owned(),
            kind,
            media: media.map(str::to_owned),
        };
        conn.execute(
            "INSERT INTO rooms(id, palace_id, position, name, kind, media) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![room.id, room.palace_id, position, room.name, kind.as_str(), room.media],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                StoreError::NotFound(format!("Palast {palace_id} oder Medium"))
            }
            e => e.into(),
        })?;
        self.touch(&conn, palace_id)?;
        self.log(
            &conn,
            "room",
            &room.id,
            "create",
            Some(serde_json::to_value(&room).expect("serializable")),
        )?;
        Ok(room)
    }

    // ------------------------------------------------------------ loci

    pub fn add_locus(&self, room_id: &str, locus: NewLocus) -> Result<Locus> {
        check_position(locus.x, locus.y)?;
        locus
            .answer
            .validate()
            .map_err(|e| StoreError::Invalid(e.into()))?;
        let conn = self.conn();
        let palace_id: String = conn
            .query_row(
                "SELECT palace_id FROM rooms WHERE id = ?1",
                [room_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("Raum {room_id}")))?;
        let position: u32 = conn.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM loci WHERE room_id = ?1",
            [room_id],
            |r| r.get(0),
        )?;
        let locus = Locus {
            id: new_id(),
            room_id: room_id.to_owned(),
            position,
            place: locus.place.trim().to_owned(),
            item: locus.item.trim().to_owned(),
            x: locus.x,
            y: locus.y,
            time: locus.time,
            answer: locus.answer,
            bindings: Vec::new(),
        };
        conn.execute(
            "INSERT INTO loci(id, room_id, position, place, item, x, y, time, answer) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![locus.id, room_id, position, locus.place, locus.item, locus.x, locus.y, locus.time, serde_json::to_string(&locus.answer).expect("serializable")],
        )?;
        self.touch(&conn, &palace_id)?;
        self.log(
            &conn,
            "locus",
            &locus.id,
            "create",
            Some(serde_json::to_value(&locus).expect("serializable")),
        )?;
        Ok(locus)
    }

    pub fn update_locus(&self, id: &str, patch: &LocusPatch) -> Result<Locus> {
        let conn = self.conn();
        let (palace_id, mut locus) = conn
            .query_row(
                "SELECT s.palace_id, l.id, l.room_id, l.position, l.place, l.item, l.x, l.y, l.time, l.answer
                 FROM loci l JOIN rooms s ON l.room_id = s.id WHERE l.id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        Locus {
                            id: r.get(1)?,
                            room_id: r.get(2)?,
                            position: r.get(3)?,
                            place: r.get(4)?,
                            item: r.get(5)?,
                            x: r.get(6)?,
                            y: r.get(7)?,
                            time: r.get(8)?,
                            answer: read_answer(r, 9)?,
                            bindings: Vec::new(),
                        },
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("Locus {id}")))?;
        if let Some(place) = &patch.place {
            locus.place = place.trim().to_owned();
        }
        if let Some(item) = &patch.item {
            locus.item = item.trim().to_owned();
        }
        if let Some(answer) = &patch.answer {
            answer
                .validate()
                .map_err(|e| StoreError::Invalid(e.into()))?;
            locus.answer = answer.clone();
        }
        locus.x = patch.x.unwrap_or(locus.x);
        locus.y = patch.y.unwrap_or(locus.y);
        if let Some(time) = patch.time {
            locus.time = time;
        }
        check_position(locus.x, locus.y)?;
        conn.execute(
            "UPDATE loci SET place = ?1, item = ?2, x = ?3, y = ?4, time = ?5, answer = ?6 WHERE id = ?7",
            params![locus.place, locus.item, locus.x, locus.y, locus.time, serde_json::to_string(&locus.answer).expect("serializable"), id],
        )?;
        self.touch(&conn, &palace_id)?;
        self.log(
            &conn,
            "locus",
            id,
            "update",
            Some(serde_json::to_value(patch).expect("serializable")),
        )?;
        Ok(locus)
    }

    /// Delete a locus and return its palace id, or `None` if it did not exist.
    pub fn delete_locus(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn();
        let palace_id: Option<String> = conn
            .query_row(
                "SELECT s.palace_id FROM loci l JOIN rooms s ON l.room_id = s.id WHERE l.id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(palace_id) = &palace_id {
            conn.execute("DELETE FROM loci WHERE id = ?1", [id])?;
            self.touch(&conn, palace_id)?;
            self.log(&conn, "locus", id, "delete", None)?;
        }
        Ok(palace_id)
    }

    // ------------------------------------------------------------ tables

    /// Insert or replace a table row of a palace (`Some(id)`) or a shared row (`None`).
    pub fn set_row(&self, palace_id: Option<&str>, row: &Row) -> Result<()> {
        row.validate().map_err(|e| StoreError::Invalid(e.0))?;
        let conn = self.conn();
        if let Some(id) = palace_id {
            let exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM palaces WHERE id = ?1)",
                [id],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(StoreError::NotFound(format!("Palast {id}")));
            }
        }
        let count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM table_rows WHERE COALESCE(palace_id, '') = COALESCE(?1, '')",
            [palace_id],
            |r| r.get(0),
        )?;
        let cells = serde_json::to_string(&row.cells).expect("serializable");
        let changed = conn.execute(
            "UPDATE table_rows SET cells = ?1 WHERE COALESCE(palace_id, '') = COALESCE(?2, '') AND table_name = ?3 AND key = ?4",
            params![cells, palace_id, row.table, row.key],
        )?;
        if changed == 0 {
            if count >= 1000 {
                return Err(StoreError::Invalid("Höchstens 1000 Tabellenzeilen".into()));
            }
            conn.execute(
                "INSERT INTO table_rows(id, palace_id, table_name, key, cells) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![new_id(), palace_id, row.table, row.key, cells],
            )?;
        }
        if let Some(id) = palace_id {
            self.touch(&conn, id)?;
        }
        let entity = format!(
            "{}/{}.{}",
            palace_id.unwrap_or("shared"),
            row.table,
            row.key
        );
        self.log(
            &conn,
            "row",
            &entity,
            "set",
            Some(serde_json::to_value(row).expect("serializable")),
        )
    }

    /// Delete a table row. Returns false if it did not exist.
    pub fn delete_row(&self, palace_id: Option<&str>, table: &str, key: &str) -> Result<bool> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM table_rows WHERE COALESCE(palace_id, '') = COALESCE(?1, '') AND table_name = ?2 AND key = ?3",
            params![palace_id, table, key],
        )? > 0;
        if deleted {
            let entity = format!("{}/{table}.{key}", palace_id.unwrap_or("shared"));
            self.log(&conn, "row", &entity, "delete", None)?;
        }
        Ok(deleted)
    }

    /// Rows of exactly one scope (a palace, or shared with `None`), in creation order.
    pub fn rows(&self, palace_id: Option<&str>) -> Result<Vec<Row>> {
        rows_in(&self.conn(), palace_id)
    }

    /// Shared rows replaced by the palace's own rows: what formulas of the palace see.
    pub fn effective_rows(&self, palace_id: &str) -> Result<Vec<Row>> {
        Ok(effective_rows(
            &self.rows(None)?,
            &self.rows(Some(palace_id))?,
        ))
    }

    // ------------------------------------------------------------ bindings

    /// Compute a locus field with a formula (replaces an existing binding).
    pub fn set_binding(&self, locus_id: &str, binding: &Binding) -> Result<()> {
        binding
            .validate()
            .map_err(|e| StoreError::Invalid(format!("Formel: {}", e.0)))?;
        let conn = self.conn();
        let palace_id: String = conn
            .query_row(
                "SELECT s.palace_id FROM loci l JOIN rooms s ON l.room_id = s.id WHERE l.id = ?1",
                [locus_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("Locus {locus_id}")))?;
        conn.execute(
            "INSERT INTO bindings(locus_id, field, expression) VALUES (?1, ?2, ?3)
             ON CONFLICT(locus_id, field) DO UPDATE SET expression = excluded.expression",
            params![locus_id, binding.field.name(), binding.expression],
        )?;
        self.touch(&conn, &palace_id)?;
        self.log(
            &conn,
            "binding",
            &format!("{locus_id}/{}", binding.field.name()),
            "set",
            Some(serde_json::json!({ "expression": binding.expression })),
        )
    }

    pub fn remove_binding(&self, locus_id: &str, field: BindingField) -> Result<bool> {
        let conn = self.conn();
        let removed = conn.execute(
            "DELETE FROM bindings WHERE locus_id = ?1 AND field = ?2",
            params![locus_id, field.name()],
        )? > 0;
        if removed {
            self.log(
                &conn,
                "binding",
                &format!("{locus_id}/{}", field.name()),
                "delete",
                None,
            )?;
        }
        Ok(removed)
    }

    // ------------------------------------------------------------ media

    /// Store media bytes under their SHA-256. Storing the same bytes twice is free.
    pub fn put_media(&self, bytes: &[u8], mime: &str) -> Result<MediaRef> {
        let dir = self.media_dir.as_ref().ok_or_else(|| {
            StoreError::Invalid("Dieser Speicher kann keine Medien ablegen".into())
        })?;
        let hash: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let path = media_path(dir, &hash);
        if !path.exists() {
            fs::create_dir_all(path.parent().expect("media path has a parent"))?;
            let tmp = path.with_extension("tmp");
            fs::write(&tmp, bytes)?;
            fs::rename(&tmp, &path)?;
        }
        let media = MediaRef {
            hash,
            mime: mime.to_owned(),
            size: bytes.len() as u64,
        };
        let conn = self.conn();
        conn.execute(
            "INSERT OR IGNORE INTO media(hash, mime, size) VALUES (?1, ?2, ?3)",
            params![media.hash, media.mime, media.size],
        )?;
        Ok(media)
    }

    /// File holding the media with `hash`, if this store keeps media on disk.
    pub fn media_file(&self, hash: &str) -> Option<PathBuf> {
        self.media_dir.as_ref().map(|dir| media_path(dir, hash))
    }

    pub fn media(&self, hash: &str) -> Result<Option<MediaRef>> {
        let conn = self.conn();
        Ok(conn
            .query_row(
                "SELECT hash, mime, size FROM media WHERE hash = ?1",
                [hash],
                |r| {
                    Ok(MediaRef {
                        hash: r.get(0)?,
                        mime: r.get(1)?,
                        size: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    // ------------------------------------------------------------ plugin data

    pub fn kv_get(&self, plugin: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.conn();
        Ok(conn
            .query_row(
                "SELECT value FROM kv WHERE plugin = ?1 AND key = ?2",
                params![plugin, key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn kv_set(&self, plugin: &str, key: &str, value: &[u8]) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO kv(plugin, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(plugin, key) DO UPDATE SET value = excluded.value",
            params![plugin, key, value],
        )?;
        self.log(&conn, "kv", &format!("{plugin}/{key}"), "set", None)
    }

    // ------------------------------------------------------------ sync

    /// Logged changes after `seq`, oldest first.
    pub fn ops_since(&self, seq: i64) -> Result<Vec<Op>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT seq, hlc, entity, entity_id, op, data FROM ops WHERE seq > ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map([seq], |r| {
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
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

fn rows_in(conn: &Connection, palace_id: Option<&str>) -> Result<Vec<Row>> {
    let mut stmt = conn.prepare(
        "SELECT table_name, key, cells FROM table_rows WHERE COALESCE(palace_id, '') = COALESCE(?1, '') ORDER BY rowid",
    )?;
    let rows = stmt.query_map([palace_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (table, key, cells) = row?;
        let cells = serde_json::from_str(&cells)
            .map_err(|e| StoreError::Invalid(format!("Tabellenzeile {table}.{key}: {e}")))?;
        out.push(Row { table, key, cells });
    }
    Ok(out)
}

fn media_path(dir: &Path, hash: &str) -> PathBuf {
    dir.join(&hash[..2]).join(&hash[2..])
}

#[cfg(test)]
mod tests;

fn read_answer(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Answer> {
    let text: String = row.get(index)?;
    let answer: Answer = serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    })?;
    answer.validate().map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::other(e)),
        )
    })?;
    Ok(answer)
}
