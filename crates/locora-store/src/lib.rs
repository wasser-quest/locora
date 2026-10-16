//! Offline-first local storage for Locora.
//!
//! One SQLite database per profile holds palaces, stages, loci, media
//! metadata and plugin key-value data. Media files are stored next to it,
//! named by their SHA-256 (deduplicated, immutable). Every change is also
//! appended to an operation log stamped with a hybrid logical clock, so a
//! later sync service can replay changes between devices.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
pub enum StageKind {
    /// No picture: loci are placed on an empty canvas.
    Blank,
    Image,
    Video,
}

impl StageKind {
    fn as_str(self) -> &'static str {
        match self {
            StageKind::Blank => "blank",
            StageKind::Image => "image",
            StageKind::Video => "video",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "blank" => Ok(StageKind::Blank),
            "image" => Ok(StageKind::Image),
            "video" => Ok(StageKind::Video),
            other => Err(StoreError::Invalid(format!(
                "unbekannte Stage-Art `{other}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub id: String,
    pub palace_id: String,
    pub position: u32,
    pub name: String,
    pub kind: StageKind,
    /// SHA-256 of the image or video, if any.
    pub media: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Locus {
    pub id: String,
    pub stage_id: String,
    pub position: u32,
    /// The place, e.g. "Herd".
    pub label: String,
    /// What to remember there, e.g. "Merkur".
    pub memory: String,
    /// Position on the stage, 0..1 from the left / top.
    pub x: f64,
    pub y: f64,
    /// For video stages: seconds into the video.
    pub at: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageTree {
    pub stage: Stage,
    pub loci: Vec<Locus>,
}

/// A palace with all stages and loci in route order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PalaceTree {
    pub palace: Palace,
    pub stages: Vec<StageTree>,
}

impl PalaceTree {
    /// All loci in route order (stage by stage).
    pub fn route(&self) -> impl Iterator<Item = &Locus> {
        self.stages.iter().flat_map(|s| &s.loci)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PalaceSummary {
    pub id: String,
    pub name: String,
    pub stages: u32,
    pub loci: u32,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewLocus {
    pub label: String,
    pub memory: String,
    pub x: f64,
    pub y: f64,
    pub at: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LocusPatch {
    pub label: Option<String>,
    pub memory: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub at: Option<Option<f64>>,
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
            "Position ({x}, {y}) liegt außerhalb der Stage (0..1)"
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
        let mut stages = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id, palace_id, position, name, kind, media FROM stages WHERE palace_id = ?1 ORDER BY position",
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
            stages.push(Stage {
                id,
                palace_id,
                position,
                name,
                kind: StageKind::parse(&kind)?,
                media,
            });
        }
        let mut loci_stmt = conn.prepare(
            "SELECT id, stage_id, position, label, memory, x, y, at FROM loci WHERE stage_id = ?1 ORDER BY position",
        )?;
        let mut tree = Vec::new();
        for stage in stages {
            let loci = loci_stmt
                .query_map([&stage.id], |r| {
                    Ok(Locus {
                        id: r.get(0)?,
                        stage_id: r.get(1)?,
                        position: r.get(2)?,
                        label: r.get(3)?,
                        memory: r.get(4)?,
                        x: r.get(5)?,
                        y: r.get(6)?,
                        at: r.get(7)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
            tree.push(StageTree { stage, loci });
        }
        Ok(PalaceTree {
            palace,
            stages: tree,
        })
    }

    /// All palaces, most recently changed first.
    pub fn palaces(&self) -> Result<Vec<PalaceSummary>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT p.id, p.name, p.updated_at,
                    (SELECT COUNT(*) FROM stages s WHERE s.palace_id = p.id),
                    (SELECT COUNT(*) FROM loci l JOIN stages s ON l.stage_id = s.id WHERE s.palace_id = p.id)
             FROM palaces p ORDER BY p.updated_at DESC, p.name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(PalaceSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                updated_at: r.get(2)?,
                stages: r.get(3)?,
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

    /// Delete a palace with all stages and loci. Returns false if it did not exist.
    pub fn delete_palace(&self, id: &str) -> Result<bool> {
        let conn = self.conn();
        let deleted = conn.execute("DELETE FROM palaces WHERE id = ?1", [id])? > 0;
        if deleted {
            self.log(&conn, "palace", id, "delete", None)?;
        }
        Ok(deleted)
    }

    // ------------------------------------------------------------ stages

    pub fn add_stage(
        &self,
        palace_id: &str,
        name: &str,
        kind: StageKind,
        media: Option<&str>,
    ) -> Result<Stage> {
        if kind != StageKind::Blank && media.is_none() {
            return Err(StoreError::Invalid(
                "Bild- und Video-Stages brauchen eine Mediendatei".into(),
            ));
        }
        let conn = self.conn();
        let position: u32 = conn.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM stages WHERE palace_id = ?1",
            [palace_id],
            |r| r.get(0),
        )?;
        let stage = Stage {
            id: new_id(),
            palace_id: palace_id.to_owned(),
            position,
            name: name.trim().to_owned(),
            kind,
            media: media.map(str::to_owned),
        };
        conn.execute(
            "INSERT INTO stages(id, palace_id, position, name, kind, media) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![stage.id, stage.palace_id, position, stage.name, kind.as_str(), stage.media],
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
            "stage",
            &stage.id,
            "create",
            Some(serde_json::to_value(&stage).expect("serializable")),
        )?;
        Ok(stage)
    }

    // ------------------------------------------------------------ loci

    pub fn add_locus(&self, stage_id: &str, locus: NewLocus) -> Result<Locus> {
        check_position(locus.x, locus.y)?;
        let conn = self.conn();
        let palace_id: String = conn
            .query_row(
                "SELECT palace_id FROM stages WHERE id = ?1",
                [stage_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("Stage {stage_id}")))?;
        let position: u32 = conn.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM loci WHERE stage_id = ?1",
            [stage_id],
            |r| r.get(0),
        )?;
        let locus = Locus {
            id: new_id(),
            stage_id: stage_id.to_owned(),
            position,
            label: locus.label.trim().to_owned(),
            memory: locus.memory.trim().to_owned(),
            x: locus.x,
            y: locus.y,
            at: locus.at,
        };
        conn.execute(
            "INSERT INTO loci(id, stage_id, position, label, memory, x, y, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![locus.id, stage_id, position, locus.label, locus.memory, locus.x, locus.y, locus.at],
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
                "SELECT s.palace_id, l.id, l.stage_id, l.position, l.label, l.memory, l.x, l.y, l.at
                 FROM loci l JOIN stages s ON l.stage_id = s.id WHERE l.id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        Locus {
                            id: r.get(1)?,
                            stage_id: r.get(2)?,
                            position: r.get(3)?,
                            label: r.get(4)?,
                            memory: r.get(5)?,
                            x: r.get(6)?,
                            y: r.get(7)?,
                            at: r.get(8)?,
                        },
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("Locus {id}")))?;
        if let Some(label) = &patch.label {
            locus.label = label.trim().to_owned();
        }
        if let Some(memory) = &patch.memory {
            locus.memory = memory.trim().to_owned();
        }
        locus.x = patch.x.unwrap_or(locus.x);
        locus.y = patch.y.unwrap_or(locus.y);
        if let Some(at) = patch.at {
            locus.at = at;
        }
        check_position(locus.x, locus.y)?;
        conn.execute(
            "UPDATE loci SET label = ?1, memory = ?2, x = ?3, y = ?4, at = ?5 WHERE id = ?6",
            params![locus.label, locus.memory, locus.x, locus.y, locus.at, id],
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
                "SELECT s.palace_id FROM loci l JOIN stages s ON l.stage_id = s.id WHERE l.id = ?1",
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

fn media_path(dir: &Path, hash: &str) -> PathBuf {
    dir.join(&hash[..2]).join(&hash[2..])
}

#[cfg(test)]
mod tests;
