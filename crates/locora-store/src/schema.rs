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
    // 2: workbook tables (palace-local or shared) and formula bindings on loci
    "CREATE TABLE table_rows(
        id TEXT PRIMARY KEY,
        palace_id TEXT REFERENCES palaces(id) ON DELETE CASCADE,
        sheet TEXT NOT NULL,
        key TEXT NOT NULL,
        cells TEXT NOT NULL);
     CREATE UNIQUE INDEX table_rows_key ON table_rows(COALESCE(palace_id, ''), sheet, key);
     CREATE TABLE bindings(
        locus_id TEXT NOT NULL REFERENCES loci(id) ON DELETE CASCADE,
        field TEXT NOT NULL,
        expression TEXT NOT NULL,
        PRIMARY KEY(locus_id, field));",
    // 3: clearer names: stages are rooms; loci have a place, an item and a video time
    "ALTER TABLE stages RENAME TO rooms;
     ALTER TABLE loci RENAME COLUMN stage_id TO room_id;
     ALTER TABLE loci RENAME COLUMN label TO place;
     ALTER TABLE loci RENAME COLUMN memory TO item;
     ALTER TABLE loci RENAME COLUMN at TO time;
     ALTER TABLE table_rows RENAME COLUMN sheet TO table_name;
     UPDATE bindings SET field = 'place' WHERE field = 'label';
     UPDATE bindings SET field = 'item' WHERE field = 'memory';",
    // 4: answer configuration; existing loci remain Recall.
    "ALTER TABLE loci ADD COLUMN answer TEXT NOT NULL DEFAULT '{\"type\":\"recall\",\"options\":[]}';",
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upgrade_v3_preserves_route_and_defaults_to_recall() {
        let conn = Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..3] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 3).unwrap();
        conn.execute_batch(
            "INSERT INTO palaces VALUES ('p','P',1,2);
            INSERT INTO rooms VALUES ('r','p',0,'R','blank',NULL);
            INSERT INTO loci VALUES ('l','r',0,'Tür','Merkur',0.4,0.5,NULL);
            INSERT INTO kv VALUES ('review','cards/p',X'0102');",
        )
        .unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let locus = conn
            .query_row("SELECT place,item,answer FROM loci WHERE id='l'", [], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .unwrap();
        assert_eq!((locus.0.as_str(), locus.1.as_str()), ("Tür", "Merkur"));
        assert!(
            serde_json::from_str::<locora_api::journey::Answer>(&locus.2)
                .unwrap()
                .is_recall()
        );
        assert_eq!(
            conn.query_row("SELECT value FROM kv WHERE plugin='review'", [], |r| r
                .get::<_, Vec<u8>>(0))
                .unwrap(),
            [1, 2]
        );
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            4
        );
    }
}
