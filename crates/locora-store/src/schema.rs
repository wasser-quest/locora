//! Schema migrations. Each entry upgrades the database by one version.
use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    // 1: palaces, stages, loci, media, plugin data, operation log
    "CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
     CREATE TABLE palaces(
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL);
     CREATE UNIQUE INDEX palaces_name ON palaces(name COLLATE NOCASE);
     CREATE TABLE media(hash TEXT PRIMARY KEY, mime TEXT NOT NULL, size INTEGER NOT NULL);
     CREATE TABLE stages(
        id TEXT PRIMARY KEY,
        palace_id TEXT NOT NULL REFERENCES palaces(id) ON DELETE CASCADE,
        position INTEGER NOT NULL,
        name TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind IN ('blank', 'image', 'video')),
        media TEXT REFERENCES media(hash));
     CREATE INDEX stages_palace ON stages(palace_id, position);
     CREATE TABLE loci(
        id TEXT PRIMARY KEY,
        stage_id TEXT NOT NULL REFERENCES stages(id) ON DELETE CASCADE,
        position INTEGER NOT NULL,
        label TEXT NOT NULL,
        memory TEXT NOT NULL,
        x REAL NOT NULL CHECK(x BETWEEN 0 AND 1),
        y REAL NOT NULL CHECK(y BETWEEN 0 AND 1),
        at REAL);
     CREATE INDEX loci_stage ON loci(stage_id, position);
     CREATE TABLE kv(plugin TEXT NOT NULL, key TEXT NOT NULL, value BLOB NOT NULL, PRIMARY KEY(plugin, key));
     CREATE TABLE ops(
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        hlc TEXT NOT NULL,
        entity TEXT NOT NULL,
        entity_id TEXT NOT NULL,
        op TEXT NOT NULL,
        data TEXT);",
];

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > MIGRATIONS.len() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
    }
    Ok(())
}
