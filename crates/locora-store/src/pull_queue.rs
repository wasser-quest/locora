//! Durable cloud pull inbox. A page and its cursor are committed together;
//! each entry is then applied, retried, or retained in quarantine independently.

use rusqlite::{Connection, params};
use serde_json::Value;

use crate::sync::{Outcome, apply_one};
use crate::{ApplyReport, RemoteOp, Result, Store, StoreError};

/// Includes the initial attempt; each subsequent pull retries pending entries once.
pub const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug)]
pub struct PullRetry {
    /// Local inbox identity, independent of an untrusted cloud sequence number.
    pub id: i64,
    /// Preserve raw JSON so malformed wire operations can be quarantined too.
    pub operation: String,
    pub attempts: u32,
}

#[derive(Debug, Default)]
pub struct PullAttempt {
    pub applied: ApplyReport,
    pub quarantined: Option<String>,
}

impl Store {
    /// Stores every operation before acknowledging receipt of this page.
    pub fn queue_pull_page(&self, cursor: i64, ops: &[Value]) -> Result<Vec<PullRetry>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut entries = Vec::with_capacity(ops.len());
        for op in ops {
            let operation = op.to_string();
            tx.execute(
                "INSERT INTO sync_pull_retry(operation) VALUES (?1)",
                [&operation],
            )?;
            entries.push(PullRetry {
                id: tx.last_insert_rowid(),
                operation,
                attempts: 0,
            });
        }
        tx.execute(
            "INSERT INTO meta(key, value) VALUES ('sync.pull', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [cursor.to_string()],
        )?;
        tx.commit()?;
        Ok(entries)
    }

    pub fn pending_pull(&self) -> Result<Vec<PullRetry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, operation, attempts FROM sync_pull_retry WHERE quarantined = 0 ORDER BY id",
        )?;
        Ok(stmt
            .query_map([], |row| {
                Ok(PullRetry {
                    id: row.get(0)?,
                    operation: row.get(1)?,
                    attempts: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Application and retry bookkeeping share a transaction. Failed operations
    /// roll back their savepoint before updating their attempt count.
    pub fn apply_pull(&self, id: i64, op: &RemoteOp) -> Result<PullAttempt> {
        self.observe(&op.hlc);
        let mut conn = self.conn();
        let mut tx = conn.transaction()?;
        let mut report = PullAttempt::default();
        let outcome = {
            let mut savepoint = tx.savepoint()?;
            match apply_one(&savepoint, op) {
                Ok(outcome) => {
                    savepoint.commit()?;
                    Ok(outcome)
                }
                Err(error) => {
                    savepoint.rollback()?;
                    Err(error)
                }
            }
        };
        match outcome {
            Ok(outcome @ (Outcome::Applied | Outcome::Stale)) => {
                if matches!(outcome, Outcome::Applied) {
                    report.applied.applied = 1;
                } else {
                    report.applied.stale = 1;
                }
                tx.execute("DELETE FROM sync_pull_retry WHERE id = ?1", [id])?;
            }
            Ok(Outcome::Missing) => {
                report.applied.missing = 1;
                let reason = format!(
                    "{} {}: übergeordneter Eintrag fehlt",
                    op.entity, op.entity_id
                );
                report.quarantined = fail(&tx, id, &reason, false)?;
            }
            Err(error) => {
                let permanent = permanent(&error);
                let reason = format!("{} {}: {error}", op.entity, op.entity_id);
                report.quarantined = fail(&tx, id, &reason, permanent)?;
                report.applied.failed.push(reason);
            }
        }
        tx.commit()?;
        Ok(report)
    }

    /// Records failures outside application, such as decoding or fetching media.
    pub fn fail_pull(&self, id: i64, reason: &str, permanent: bool) -> Result<Option<String>> {
        fail(&self.conn(), id, reason, permanent)
    }
}

fn permanent(error: &StoreError) -> bool {
    match error {
        StoreError::Invalid(_) => true,
        StoreError::Sqlite(rusqlite::Error::SqliteFailure(code, _)) => matches!(
            code.extended_code,
            rusqlite::ffi::SQLITE_CONSTRAINT_CHECK
                | rusqlite::ffi::SQLITE_CONSTRAINT_NOTNULL
                | rusqlite::ffi::SQLITE_CONSTRAINT_DATATYPE
        ),
        _ => false,
    }
}

fn fail(conn: &Connection, id: i64, reason: &str, permanent: bool) -> Result<Option<String>> {
    let attempts: u32 = conn.query_row(
        "UPDATE sync_pull_retry SET attempts = attempts + 1, error = ?2,
         quarantined = (?3 OR attempts + 1 >= ?4) WHERE id = ?1 RETURNING attempts",
        params![id, reason, permanent, MAX_ATTEMPTS],
        |row| row.get(0),
    )?;
    Ok((permanent || attempts >= MAX_ATTEMPTS)
        .then(|| format!("Eintrag {id} nach {attempts} Versuch(en): {reason}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn queued(store: &Store) -> i64 {
        store
            .queue_pull_page(8, &[json!({"raw": "operation"})])
            .unwrap()[0]
            .id
    }

    fn retained(store: &Store, id: i64) -> (u32, bool, String, String) {
        store
            .conn()
            .query_row(
                "SELECT attempts, quarantined, error, operation FROM sync_pull_retry WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap()
    }

    #[test]
    fn page_and_cursor_are_atomic_and_pending_entries_survive_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store.set_sync_cursor("pull", 7).unwrap();
        store
            .conn()
            .execute_batch(
                "CREATE TRIGGER reject_cursor BEFORE UPDATE ON meta WHEN NEW.key = 'sync.pull'
             BEGIN SELECT RAISE(ABORT, 'test failure'); END;",
            )
            .unwrap();
        assert!(
            store
                .queue_pull_page(8, &[json!({"raw": "operation"})])
                .is_err()
        );
        assert!(store.pending_pull().unwrap().is_empty());
        assert_eq!(store.sync_cursor("pull").unwrap(), 7);
        store
            .conn()
            .execute_batch("DROP TRIGGER reject_cursor")
            .unwrap();
        let id = queued(&store);
        assert!(
            store
                .fail_pull(id, "temporary failure", false)
                .unwrap()
                .is_none()
        );
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        let pending = store.pending_pull().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].attempts, 1);
        assert_eq!(
            pending[0].operation,
            json!({"raw": "operation"}).to_string()
        );
        assert_eq!(store.sync_cursor("pull").unwrap(), 8);
        assert!(
            store
                .fail_pull(id, "still unavailable", false)
                .unwrap()
                .is_none()
        );
        assert!(store.fail_pull(id, "exhausted", false).unwrap().is_some());
        assert!(store.pending_pull().unwrap().is_empty());
        assert_eq!(
            retained(&store, id),
            (
                3,
                true,
                "exhausted".into(),
                json!({"raw": "operation"}).to_string()
            )
        );
    }

    #[test]
    fn database_validation_is_permanent_and_transient_apply_failure_can_recover() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let palace = store.create_palace("Palace").unwrap();
        let room = store
            .add_room(&palace.id, "Room", crate::RoomKind::Blank, None)
            .unwrap();
        let op = RemoteOp {
            hlc: "0000000000000001-000000-remote".into(),
            entity: "locus".into(),
            entity_id: "locus".into(),
            op: "create".into(),
            data: Some(
                json!({"id": "locus", "room_id": room.id, "position": 0, "place": "Door", "item": "Test", "x": 2.0, "y": 0.5, "time": null}),
            ),
        };
        let id = queued(&store);
        let attempt = store.apply_pull(id, &op).unwrap();
        assert_eq!(attempt.applied.failed.len(), 1);
        assert!(attempt.quarantined.is_some());
        assert_eq!(retained(&store, id).0, 1);
        assert!(store.pending_pull().unwrap().is_empty());

        let mut valid = op.clone();
        valid.data.as_mut().unwrap()["x"] = json!(0.5);
        // This fails after the row is inserted; the savepoint must undo it.
        store
            .conn()
            .execute_batch(
                "CREATE TRIGGER temporary_failure AFTER INSERT ON loci
             BEGIN SELECT RAISE(FAIL, 'temporarily unavailable'); END;",
            )
            .unwrap();
        let id = queued(&store);
        let attempt = store.apply_pull(id, &valid).unwrap();
        assert!(attempt.quarantined.is_none());
        assert_eq!(attempt.applied.failed.len(), 1);
        assert_eq!(store.pending_pull().unwrap()[0].attempts, 1);
        assert!(store.palace("Palace").unwrap().rooms[0].loci.is_empty());
        store
            .conn()
            .execute_batch("DROP TRIGGER temporary_failure")
            .unwrap();
        assert_eq!(store.apply_pull(id, &valid).unwrap().applied.applied, 1);
        assert!(store.pending_pull().unwrap().is_empty());
        let id = queued(&store);
        assert_eq!(store.apply_pull(id, &valid).unwrap().applied.stale, 1);
        assert!(store.pending_pull().unwrap().is_empty());
    }
}
