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

mod history;
mod sync;
pub use sync::{ApplyReport, RemoteOp, media_refs};

/// One graded answer of a finished journey. `at_ms` counts from the start of
/// the journey and must not go backwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JourneyAnswer {
    pub correct: bool,
    pub at_ms: u64,
}

/// A finished journey as stored on this device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Journey {
    pub id: String,
    pub palace_name: String,
    pub answers: Vec<JourneyAnswer>,
    /// Seconds since the Unix epoch.
    pub recorded_at: u64,
}
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

/// Default locus picture width as a fraction of the room width.
pub const fn default_image_size() -> f64 {
    0.25
}

/// Default zoom on a locus while asking: the whole room.
pub const fn default_zoom() -> f64 {
    1.0
}

/// Zoom is finite and from 1 (whole room) to 4.
pub fn check_zoom(zoom: f64) -> Result<()> {
    if (1.0..=4.0).contains(&zoom) {
        Ok(())
    } else {
        Err(StoreError::Invalid(
            "Der Zoom muss zwischen 1 (ganzer Raum) und 4 liegen".into(),
        ))
    }
}

/// Picture widths must be finite and between 5% and 100% of the room width.
pub fn check_image_size(size: f64) -> Result<()> {
    if (0.05..=1.0).contains(&size) {
        Ok(())
    } else {
        Err(StoreError::Invalid(
            "Die Bildgröße muss zwischen 0,05 und 1 liegen (Anteil der Raumbreite)".into(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Palace {
    pub id: String,
    pub name: String,
    /// Seconds since the Unix epoch.
    pub created_at: u64,
    pub updated_at: u64,
    /// SHA-256 of a custom locus marker (`.sply`); `None` = the default marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
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
    /// Picture width as a fraction of the room width.
    #[serde(default = "default_image_size")]
    pub image_size: f64,
    /// Zoom on the locus while asking, 1 (whole room) to 4.
    #[serde(default = "default_zoom")]
    pub zoom: f64,
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
    /// SHA-256 of a picture shown at this locus (image or `.sply` animation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Memory aid: the vivid image in words, shown after the answer.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
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
    /// Picture width as a fraction of the room width.
    #[serde(default = "default_image_size")]
    pub image_size: f64,
    #[serde(default = "default_zoom")]
    pub zoom: f64,
    pub place: String,
    pub item: String,
    pub x: f64,
    pub y: f64,
    pub time: Option<f64>,
    #[serde(default)]
    pub answer: Answer,
    /// SHA-256 of a stored picture (image or `.sply` animation).
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LocusPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// `None` = keep, `Some(None)` = remove the time, `Some(Some(t))` = set it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "double_option"
    )]
    pub time: Option<Option<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Answer>,
    /// `None` = keep, `Some(None)` = remove, `Some(Some(hash))` = set.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "double_option"
    )]
    pub image: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Keeps an explicit `null` as `Some(None)`, so "remove" is not read as "keep".
fn double_option<'de, T, D>(deserializer: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Media types a locus picture may have.
pub fn is_picture(media_type: &str) -> bool {
    media_type.starts_with("image/") || media_type == SPLINELY
}

/// Media type of Splinely animations (`.sply`).
pub const SPLINELY: &str = "application/vnd.splinely";

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

    /// Moves the clock past a timestamp from another device, so later local
    /// writes always sort after everything this device has seen.
    fn observe(&self, hlc: &str) {
        let mut parts = hlc.splitn(3, '-');
        let millis = parts.next().and_then(|p| p.parse::<u64>().ok());
        let counter = parts.next().and_then(|c| c.parse::<u32>().ok());
        let (Some(millis), Some(counter)) = (millis, counter) else {
            return;
        };
        let mut clock = self.clock.lock().unwrap_or_else(|p| p.into_inner());
        if (millis, counter) > *clock {
            *clock = (millis, counter);
        }
    }

    /// This device's id inside the operation log (`hlc` suffix).
    pub fn node_id(&self) -> &str {
        &self.node
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
        let hlc = self.tick();
        conn.execute(
            "INSERT INTO ops(hlc, entity, entity_id, op, data) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![hlc, entity, id, op, data.as_ref().map(|d| d.to_string())],
        )?;
        sync::record(conn, entity, id, op, &hlc, data.as_ref())
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
            marker: None,
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

    /// Import validated media as a new palace and its first room. All database
    /// changes (including media metadata and ops) commit together. Immutable
    /// content files may remain unreferenced after a database failure.
    pub fn create_media_palace(
        &self,
        name: &str,
        kind: RoomKind,
        bytes: &[u8],
        mime: &str,
    ) -> Result<PalaceTree> {
        let base = name.trim();
        if base.is_empty()
            || bytes.is_empty()
            || !match kind {
                RoomKind::Image => is_picture(mime),
                RoomKind::Video => matches!(mime, "video/mp4" | "video/webm" | "video/quicktime"),
                RoomKind::Blank => false,
            }
        {
            return Err(StoreError::Invalid("Ungültiger Name oder Medientyp".into()));
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut name = base.to_owned();
        let mut suffix = 2;
        while tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM palaces WHERE name = ?1 COLLATE NOCASE)",
            [&name],
            |r| r.get::<_, bool>(0),
        )? {
            name = format!("{base} ({suffix})");
            suffix += 1;
        }
        let hash: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let existing: Option<String> = tx
            .query_row("SELECT mime FROM media WHERE hash = ?1", [&hash], |r| {
                r.get(0)
            })
            .optional()?;
        if existing.is_some_and(|m| m != mime) {
            return Err(StoreError::Invalid(
                "Gespeicherter Medientyp passt nicht zum Dateiformat".into(),
            ));
        }
        let dir = self.media_dir.as_ref().ok_or_else(|| {
            StoreError::Invalid("Dieser Speicher kann keine Medien ablegen".into())
        })?;
        let path = media_path(dir, &hash);
        if !path.exists() {
            fs::create_dir_all(path.parent().expect("media path has a parent"))?;
            let tmp = path.with_extension(new_id());
            fs::write(&tmp, bytes)?;
            if let Err(error) = fs::rename(&tmp, &path) {
                let _ = fs::remove_file(&tmp);
                return Err(error.into());
            }
        }
        tx.execute(
            "INSERT OR IGNORE INTO media(hash, mime, size) VALUES (?1, ?2, ?3)",
            params![hash, mime, bytes.len() as u64],
        )?;
        let now = now_secs();
        let palace = Palace {
            id: new_id(),
            name,
            created_at: now,
            updated_at: now,
            marker: None,
        };
        let room = Room {
            id: new_id(),
            palace_id: palace.id.clone(),
            position: 0,
            name: base.to_owned(),
            kind,
            media: Some(hash),
        };
        tx.execute(
            "INSERT INTO palaces(id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![palace.id, palace.name, now],
        )?;
        tx.execute("INSERT INTO rooms(id, palace_id, position, name, kind, media) VALUES (?1, ?2, 0, ?3, ?4, ?5)",
            params![room.id, room.palace_id, room.name, kind.as_str(), room.media])?;
        self.log(
            &tx,
            "palace",
            &palace.id,
            "create",
            Some(serde_json::json!({"name": palace.name})),
        )?;
        self.log(
            &tx,
            "room",
            &room.id,
            "create",
            Some(serde_json::to_value(&room).expect("serializable")),
        )?;
        tx.commit()?;
        Ok(PalaceTree {
            palace,
            rooms: vec![RoomTree { room, loci: vec![] }],
            tables: vec![],
        })
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
                marker: r.get(4)?,
            })
        };
        let cols = "SELECT id, name, created_at, updated_at, marker FROM palaces";
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
            "SELECT id, room_id, position, place, item, x, y, time, answer, image, note, image_size, zoom FROM loci WHERE room_id = ?1 ORDER BY position",
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
                        image: r.get(9)?,
                        note: r.get(10)?,
                        image_size: r.get(11)?,
                        zoom: r.get(12)?,
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

    /// Use a custom locus marker (a stored `.sply`) for a palace, or the default with `None`.
    pub fn set_marker(&self, id: &str, marker: Option<&str>) -> Result<()> {
        let conn = self.conn();
        if let Some(hash) = marker {
            let mime: Option<String> = conn
                .query_row("SELECT mime FROM media WHERE hash = ?1", [hash], |r| {
                    r.get(0)
                })
                .optional()?;
            match mime.as_deref() {
                None => return Err(StoreError::NotFound(format!("Medium {hash}"))),
                Some(SPLINELY) => {}
                Some(other) => {
                    return Err(StoreError::Invalid(format!(
                        "Ein Marker muss eine .sply-Animation sein, nicht {other}"
                    )));
                }
            }
        }
        let changed = conn.execute(
            "UPDATE palaces SET marker = ?1, updated_at = ?2 WHERE id = ?3",
            params![marker, now_secs(), id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!("Palast {id}")));
        }
        self.log(
            &conn,
            "palace",
            id,
            "update",
            Some(serde_json::json!({ "marker": marker })),
        )
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
        Ok(self.add_loci(room_id, vec![locus])?.remove(0))
    }

    /// Append a batch, including palace timestamp and operation log, atomically.
    pub fn add_loci(&self, room_id: &str, loci: Vec<NewLocus>) -> Result<Vec<Locus>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut added = Vec::with_capacity(loci.len());
        for locus in loci {
            added.push(self.insert_locus(&tx, room_id, locus)?);
        }
        tx.commit()?;
        Ok(added)
    }

    fn insert_locus(&self, conn: &Connection, room_id: &str, locus: NewLocus) -> Result<Locus> {
        check_image_size(locus.image_size)?;
        check_zoom(locus.zoom)?;
        check_position(locus.x, locus.y)?;
        locus
            .answer
            .validate()
            .map_err(|e| StoreError::Invalid(e.into()))?;
        if let Some(hash) = &locus.image {
            check_picture(conn, hash)?;
        }
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
            image: locus.image,
            image_size: locus.image_size,
            zoom: locus.zoom,
            note: locus.note.trim().to_owned(),
            bindings: Vec::new(),
        };
        conn.execute(
            "INSERT INTO loci(id, room_id, position, place, item, x, y, time, answer, image, note, image_size, zoom) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![locus.id, room_id, position, locus.place, locus.item, locus.x, locus.y, locus.time, serde_json::to_string(&locus.answer).expect("serializable"), locus.image, locus.note, locus.image_size, locus.zoom],
        )?;
        self.touch(conn, &palace_id)?;
        self.log(
            conn,
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
                "SELECT s.palace_id, l.id, l.room_id, l.position, l.place, l.item, l.x, l.y, l.time, l.answer, l.image, l.note, l.image_size, l.zoom
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
                            image: r.get(10)?,
                            note: r.get(11)?,
                            image_size: r.get(12)?,
                            zoom: r.get(13)?,
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
        if let Some(zoom) = patch.zoom {
            check_zoom(zoom)?;
            locus.zoom = zoom;
        }
        if let Some(size) = patch.image_size {
            check_image_size(size)?;
            locus.image_size = size;
        }
        if let Some(note) = &patch.note {
            locus.note = note.trim().to_owned();
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
        if let Some(image) = &patch.image {
            if let Some(hash) = image {
                check_picture(&conn, hash)?;
            }
            locus.image = image.clone();
        }
        check_position(locus.x, locus.y)?;
        conn.execute(
            "UPDATE loci SET place = ?1, item = ?2, x = ?3, y = ?4, time = ?5, answer = ?6, image = ?7, note = ?8, image_size = ?9, zoom = ?10 WHERE id = ?11",
            params![locus.place, locus.item, locus.x, locus.y, locus.time, serde_json::to_string(&locus.answer).expect("serializable"), locus.image, locus.note, locus.image_size, locus.zoom, id],
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
        let mut data = serde_json::to_value(row).expect("serializable");
        if let serde_json::Value::Object(map) = &mut data {
            map.insert("palace".into(), serde_json::json!(palace_id));
        }
        self.log(&conn, "row", &entity, "set", Some(data))
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
            let data = serde_json::json!({ "palace": palace_id, "table": table, "key": key });
            self.log(&conn, "row", &entity, "delete", Some(data))?;
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
        // Writing the same bytes again is not a change, so it is not logged or synced.
        let current: Option<Vec<u8>> = conn
            .query_row(
                "SELECT value FROM kv WHERE plugin = ?1 AND key = ?2",
                params![plugin, key],
                |r| r.get(0),
            )
            .optional()?;
        if current.as_deref() == Some(value) {
            return Ok(());
        }
        conn.execute(
            "INSERT INTO kv(plugin, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(plugin, key) DO UPDATE SET value = excluded.value",
            params![plugin, key, value],
        )?;
        self.log(
            &conn,
            "kv",
            &format!("{plugin}/{key}"),
            "set",
            Some(serde_json::json!({ "value": sync::hex(value) })),
        )
    }

    // ------------------------------------------------------------ sync

    /// Cursor of the sync client: `push` = last local operation sent,
    /// `pull` = last cloud operation received. Zero when never synced.
    pub fn sync_cursor(&self, name: &str) -> Result<i64> {
        let conn = self.conn();
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                [format!("sync.{name}")],
                |r| r.get(0),
            )
            .optional()?;
        Ok(value.and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    pub fn set_sync_cursor(&self, name: &str, value: i64) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![format!("sync.{name}"), value.to_string()],
        )?;
        Ok(())
    }

    /// Stores a finished journey and logs it, so it syncs to Locora Cloud.
    pub fn record_journey(&self, palace_id: &str, answers: &[JourneyAnswer]) -> Result<Journey> {
        if answers.is_empty() {
            return Err(StoreError::Invalid(
                "Die Journey hat keine Antworten".into(),
            ));
        }
        let palace_name: String = {
            let conn = self.conn();
            conn.query_row("SELECT name FROM palaces WHERE id = ?1", [palace_id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("Palast {palace_id}")))?
        };
        let journey = Journey {
            id: new_id(),
            palace_name,
            answers: answers.to_vec(),
            recorded_at: now_secs(),
        };
        let answers_json = serde_json::to_string(&journey.answers).expect("serializable");
        let conn = self.conn();
        conn.execute(
            "INSERT INTO journeys(id, palace_name, answers, recorded_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                journey.id,
                journey.palace_name,
                answers_json,
                journey.recorded_at
            ],
        )?;
        self.log(
            &conn,
            "journey",
            &journey.id,
            "create",
            Some(serde_json::json!({
                "palace_name": journey.palace_name,
                "answers": journey.answers,
                "recorded_at": journey.recorded_at,
            })),
        )?;
        Ok(journey)
    }

    /// Finished journeys, newest first.
    pub fn journeys(&self, limit: u32) -> Result<Vec<Journey>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, palace_name, answers, recorded_at FROM journeys ORDER BY recorded_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            let answers: String = r.get(2)?;
            Ok(Journey {
                id: r.get(0)?,
                palace_name: r.get(1)?,
                answers: serde_json::from_str(&answers).unwrap_or_default(),
                recorded_at: r.get::<_, i64>(3)? as u64,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

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

/// A locus picture must be stored media of an image or Splinely type.
fn check_picture(conn: &Connection, hash: &str) -> Result<()> {
    let mime: Option<String> = conn
        .query_row("SELECT mime FROM media WHERE hash = ?1", [hash], |r| {
            r.get(0)
        })
        .optional()?;
    match mime {
        None => Err(StoreError::NotFound(format!("Medium {hash}"))),
        Some(m) if is_picture(&m) => Ok(()),
        Some(m) => Err(StoreError::Invalid(format!(
            "Ein Locus-Bild muss ein Bild oder eine .sply-Animation sein, nicht {m}"
        ))),
    }
}

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
