//! Transactional structural editor history. Only opaque receipts cross IPC.
//! Plugin KV (including schedule/review logs) is deliberately never deleted.
use super::*;

fn scope(conn: &Connection, palace: &str, room: &str) -> Result<()> {
    let valid: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM rooms WHERE id=?1 AND palace_id=?2)",
        params![room, palace],
        |r| r.get(0),
    )?;
    if !valid {
        return Err(StoreError::Invalid(
            "Raum gehört nicht zu diesem Palast".into(),
        ));
    }
    Ok(())
}

fn snapshot(conn: &Connection, id: &str) -> Result<Option<Locus>> {
    let mut locus = conn.query_row(
        "SELECT id, room_id, position, place, item, x, y, time, answer, image, note, image_size, zoom FROM loci WHERE id=?1",
        [id], |r| Ok(Locus {
            id: r.get(0)?, room_id: r.get(1)?, position: r.get(2)?, place: r.get(3)?,
            item: r.get(4)?, x: r.get(5)?, y: r.get(6)?, time: r.get(7)?,
            answer: read_answer(r, 8)?, image: r.get(9)?, note: r.get(10)?,
            image_size: r.get(11)?, zoom: r.get(12)?, bindings: Vec::new(),
        }),
    ).optional()?;
    if let Some(locus) = &mut locus {
        let mut stmt = conn
            .prepare("SELECT field, expression FROM bindings WHERE locus_id=?1 ORDER BY field")?;
        for row in stmt.query_map([id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (field, expression) = row?;
            locus.bindings.push(Binding {
                field: BindingField::parse(&field)
                    .map_err(|e| StoreError::Invalid(e.to_string()))?,
                expression,
            });
        }
    }
    Ok(locus)
}

fn receipt(
    conn: &Connection,
    palace: &str,
    room: &str,
    before: &[Locus],
    after: &[Locus],
) -> Result<String> {
    let id = new_id();
    conn.execute(
        "INSERT INTO editor_receipts VALUES (?1,?2,?3,?4,?5)",
        params![
            id,
            palace,
            room,
            serde_json::to_string(before).expect("serializable"),
            serde_json::to_string(after).expect("serializable")
        ],
    )?;
    // The UI keeps at most 100 steps per stack; older receipts are unreachable.
    conn.execute(
        "DELETE FROM editor_receipts WHERE rowid <= (SELECT MAX(rowid) FROM editor_receipts) - ?1",
        [MAX_RECEIPTS],
    )?;
    Ok(id)
}

const MAX_RECEIPTS: i64 = 500;

impl Store {
    /// Append a whole list and issue its undo capability in the same transaction.
    pub fn editor_add_loci(
        &self,
        palace: &str,
        room: &str,
        loci: Vec<NewLocus>,
    ) -> Result<(Vec<Locus>, String)> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        scope(&tx, palace, room)?;
        if loci.is_empty() {
            return Err(StoreError::Invalid("Leere Liste".into()));
        }
        let mut added = Vec::with_capacity(loci.len());
        for locus in loci {
            added.push(self.insert_locus(&tx, room, locus, None)?);
        }
        let token = receipt(&tx, palace, room, &[], &added)?;
        tx.commit()?;
        Ok((added, token))
    }

    /// Preserve the raw stored locus, not workbook-evaluated UI content.
    pub fn editor_delete_locus(&self, palace: &str, room: &str, id: &str) -> Result<String> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        scope(&tx, palace, room)?;
        let locus =
            snapshot(&tx, id)?.ok_or_else(|| StoreError::NotFound(format!("Locus {id}")))?;
        if locus.room_id != room {
            return Err(StoreError::Invalid(
                "Ort gehört nicht zu diesem Raum".into(),
            ));
        }
        tx.execute("DELETE FROM loci WHERE id=?1", [id])?;
        self.touch(&tx, palace)?;
        self.log(&tx, "locus", id, "delete", None)?;
        let token = receipt(&tx, palace, room, &[locus], &[])?;
        tx.commit()?;
        Ok(token)
    }

    /// Consume a scoped receipt and return a reciprocal receipt for redo/undo.
    /// A changed affected locus or occupied route position is a conflict, not an overwrite.
    pub fn editor_restore(
        &self,
        palace: &str,
        room: &str,
        token: &str,
    ) -> Result<(Vec<Locus>, String)> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        scope(&tx, palace, room)?;
        let (before, after): (String, String) = tx.query_row(
            "SELECT before_json, after_json FROM editor_receipts WHERE id=?1 AND palace_id=?2 AND room_id=?3",
            params![token, palace, room], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?.ok_or_else(|| StoreError::Invalid("Ungültiger oder verbrauchter Undo-Beleg".into()))?;
        let decode = |json: &str| {
            serde_json::from_str::<Vec<Locus>>(json).map_err(|e| StoreError::Invalid(e.to_string()))
        };
        let before = decode(&before)?;
        let after = decode(&after)?;
        for locus in &after {
            if snapshot(&tx, &locus.id)?.as_ref() != Some(locus) {
                return Err(StoreError::Invalid(
                    "Undo-Konflikt: Ort wurde geändert".into(),
                ));
            }
        }
        for locus in &before {
            let occupied: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM loci WHERE id=?1 OR (room_id=?2 AND position=?3))",
                params![locus.id, room, locus.position],
                |r| r.get(0),
            )?;
            if occupied {
                return Err(StoreError::Invalid(
                    "Undo-Konflikt: Route wurde geändert".into(),
                ));
            }
        }
        for locus in &after {
            tx.execute("DELETE FROM loci WHERE id=?1", [&locus.id])?;
            self.log(&tx, "locus", &locus.id, "delete", None)?;
        }
        for locus in &before {
            tx.execute(
                "INSERT INTO loci(id,room_id,position,place,item,x,y,time,answer,image,note,image_size,zoom) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![locus.id, locus.room_id, locus.position, locus.place, locus.item, locus.x, locus.y, locus.time,
                    serde_json::to_string(&locus.answer).expect("serializable"), locus.image, locus.note, locus.image_size, locus.zoom],
            )?;
            for binding in &locus.bindings {
                tx.execute(
                    "INSERT INTO bindings VALUES (?1,?2,?3)",
                    params![locus.id, binding.field.name(), binding.expression],
                )?;
            }
            self.log(
                &tx,
                "locus",
                &locus.id,
                "create",
                Some(serde_json::to_value(locus).expect("serializable")),
            )?;
        }
        self.touch(&tx, palace)?;
        tx.execute("DELETE FROM editor_receipts WHERE id=?1", [token])?;
        let reciprocal = receipt(&tx, palace, room, &after, &before)?;
        let ids = tx
            .prepare("SELECT id FROM loci WHERE room_id=?1 ORDER BY position, id")?
            .query_map([room], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let loci = ids
            .iter()
            .map(|id| snapshot(&tx, id).map(|l| l.expect("selected in transaction")))
            .collect::<Result<Vec<_>>>()?;
        tx.commit()?;
        Ok((loci, reciprocal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locus(item: &str) -> NewLocus {
        NewLocus {
            place: "Tür".into(),
            item: item.into(),
            x: 0.2,
            y: 0.7,
            time: Some(3.5),
            answer: Default::default(),
            image: None,
            note: "Notiz".into(),
            image_size: 0.3,
            zoom: 2.0,
        }
    }

    #[test]
    fn delete_restore_keeps_raw_bindings_media_route_and_review_kv() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let palace = store.create_palace("History").unwrap();
        let room = store
            .add_room(&palace.id, "Room", RoomKind::Blank, None)
            .unwrap();
        let hash = store.put_media(b"picture", "image/png").unwrap();
        let mut input = locus("raw item");
        input.image = Some(hash.hash.clone());
        let first = store.add_locus(&room.id, input).unwrap();
        store.add_locus(&room.id, locus("second")).unwrap();
        store
            .set_binding(
                &first.id,
                &Binding {
                    field: BindingField::Item,
                    expression: "\"computed\"".into(),
                },
            )
            .unwrap();
        store
            .kv_set("review", "cards/p", b"schedule and review log")
            .unwrap();
        let original = snapshot(&store.conn(), &first.id).unwrap().unwrap();
        let token = store
            .editor_delete_locus(&palace.id, &room.id, &first.id)
            .unwrap();
        assert!(snapshot(&store.conn(), &first.id).unwrap().is_none());
        assert_eq!(
            store.kv_get("review", "cards/p").unwrap().unwrap(),
            b"schedule and review log"
        );
        // Receipt and media survive reopening; no UI-supplied snapshots or IDs.
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        let (restored, redo) = store.editor_restore(&palace.id, &room.id, &token).unwrap();
        assert_eq!(restored[0], original);
        assert_eq!(restored[1].item, "second");
        assert!(store.editor_restore(&palace.id, &room.id, &token).is_err());
        let (_, undo) = store.editor_restore(&palace.id, &room.id, &redo).unwrap();
        store.editor_restore(&palace.id, &room.id, &undo).unwrap();
        assert_eq!(
            snapshot(&store.conn(), &first.id).unwrap().unwrap(),
            original
        );
        assert_eq!(
            store.kv_get("review", "cards/p").unwrap().unwrap(),
            b"schedule and review log"
        );
        assert_eq!(store.media(&hash.hash).unwrap().unwrap().mime, "image/png");
    }

    #[test]
    fn old_receipts_are_pruned() {
        let store = Store::in_memory().unwrap();
        let palace = store.create_palace("History").unwrap();
        let room = store
            .add_room(&palace.id, "Room", RoomKind::Blank, None)
            .unwrap();
        let (_, first) = store
            .editor_add_loci(&palace.id, &room.id, vec![locus("first")])
            .unwrap();
        let mut last = String::new();
        for _ in 0..MAX_RECEIPTS {
            last = store
                .editor_add_loci(&palace.id, &room.id, vec![locus("more")])
                .unwrap()
                .1;
        }
        let count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM editor_receipts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, MAX_RECEIPTS);
        assert!(store.editor_restore(&palace.id, &room.id, &first).is_err());
        store.editor_restore(&palace.id, &room.id, &last).unwrap();
    }

    #[test]
    fn list_receipts_are_scoped_atomic_and_conflict_checked() {
        let store = Store::in_memory().unwrap();
        let palace = store.create_palace("History").unwrap();
        let room = store
            .add_room(&palace.id, "Room", RoomKind::Blank, None)
            .unwrap();
        let other = store
            .add_room(&palace.id, "Other", RoomKind::Blank, None)
            .unwrap();
        let foreign = store.create_palace("Foreign").unwrap();
        let (added, token) = store
            .editor_add_loci(&palace.id, &room.id, vec![locus("one"), locus("two")])
            .unwrap();
        assert!(store.editor_restore(&palace.id, &other.id, &token).is_err());
        assert!(store.editor_restore(&foreign.id, &room.id, &token).is_err());
        assert!(
            store
                .editor_restore(&palace.id, &room.id, "forged")
                .is_err()
        );
        store
            .update_locus(
                &added[0].id,
                &LocusPatch {
                    item: Some("changed".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(store.editor_restore(&palace.id, &room.id, &token).is_err());
        store
            .update_locus(
                &added[0].id,
                &LocusPatch {
                    item: Some("one".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let (_, redo) = store.editor_restore(&palace.id, &room.id, &token).unwrap();
        let ops = store.ops_since(0).unwrap();
        store.conn().execute_batch("CREATE TRIGGER reject_restore BEFORE INSERT ON loci WHEN NEW.item='two' BEGIN SELECT RAISE(ABORT, 'test restore failure'); END;").unwrap();
        assert!(store.editor_restore(&palace.id, &room.id, &redo).is_err());
        assert_eq!(store.ops_since(0).unwrap(), ops);
        assert!(store.palace(&palace.id).unwrap().rooms[0].loci.is_empty());
        store
            .conn()
            .execute_batch("DROP TRIGGER reject_restore")
            .unwrap();
        assert_eq!(
            store.editor_restore(&palace.id, &room.id, &redo).unwrap().0,
            added
        );
        let ops = store.ops_since(0).unwrap();
        assert!(
            store
                .editor_add_loci(
                    &palace.id,
                    &room.id,
                    vec![
                        locus("valid"),
                        NewLocus {
                            x: f64::NAN,
                            ..locus("bad")
                        }
                    ]
                )
                .is_err()
        );
        assert_eq!(store.ops_since(0).unwrap(), ops);
        // Receipt insertion failure rolls back deletion, binding cascade and log.
        store.conn().execute_batch("CREATE TRIGGER reject_receipt BEFORE INSERT ON editor_receipts BEGIN SELECT RAISE(ABORT, 'test receipt failure'); END;").unwrap();
        assert!(
            store
                .editor_delete_locus(&palace.id, &room.id, &added[0].id)
                .is_err()
        );
        assert_eq!(store.ops_since(0).unwrap(), ops);
        assert_eq!(
            snapshot(&store.conn(), &added[0].id).unwrap().unwrap(),
            added[0]
        );
    }
}
