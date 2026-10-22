//! The outbox: which operations a push may leave out, and the summary of what it sends.

use std::collections::HashSet;

use locora_formula::Row;
use locora_formula::workbook::{Binding, BindingField};
use serde_json::json;
use tempfile::TempDir;

use super::*;
use crate::{LocusPatch, NewLocus, RoomKind};

fn head(seq: i64, entity: &str, id: &str, op: &str, data: Option<Value>) -> OpHead {
    OpHead {
        seq,
        hlc: format!("{seq:013}-00000-node"),
        entity: entity.into(),
        entity_id: id.into(),
        op: op.into(),
        fields: effective_fields(entity, op, data.as_ref()),
        parent: parent_of(entity, data.as_ref()),
    }
}

fn dropped(heads: &[OpHead]) -> Vec<i64> {
    let mut seqs: Vec<i64> = superseded(heads, |head| match head.entity.as_str() {
        "palace" => Some(head.entity_id.clone()),
        "room" => Some("p".into()),
        "locus" => Some("p".into()),
        "row" => head.parent.clone(),
        _ => None,
    })
    .into_iter()
    .collect();
    seqs.sort();
    seqs
}

#[test]
fn a_value_set_again_is_sent_once() {
    let heads = [
        head(1, "kv", "review/cards", "set", None),
        head(2, "kv", "review/log", "set", None),
        head(3, "kv", "review/cards", "set", None),
        head(4, "kv", "review/cards", "set", None),
    ];
    assert_eq!(dropped(&heads), [1, 3]);
}

#[test]
fn an_update_is_left_out_only_when_every_field_is_set_again() {
    let heads = [
        head(
            1,
            "locus",
            "l",
            "update",
            Some(json!({"place": "a", "x": 0.5})),
        ),
        head(2, "locus", "l", "update", Some(json!({"place": "b"}))),
        head(
            3,
            "locus",
            "l",
            "update",
            Some(json!({"x": 0.1, "item": "c"})),
        ),
        // `null` keeps a text field, so it does not replace one.
        head(4, "locus", "l", "update", Some(json!({"item": null}))),
        // `null` removes a time, so it does.
        head(5, "locus", "l", "update", Some(json!({"time": 3.0}))),
        head(6, "locus", "l", "update", Some(json!({"time": null}))),
        head(7, "locus", "other", "update", Some(json!({"place": "z"}))),
    ];
    // 1: place by 2, x by 3. 2 stays (nothing later sets place). 3: item is not set again by 4.
    // 4 sets nothing at all (a null text keeps the value), so it is left out as well.
    assert_eq!(dropped(&heads), [1, 4, 5]);
}

#[test]
fn changes_before_a_delete_are_left_out_but_creates_and_deletes_are_sent() {
    let heads = [
        head(1, "palace", "p", "create", Some(json!({"name": "P"}))),
        head(2, "room", "r", "create", Some(json!({"palace_id": "p"}))),
        head(3, "room", "r", "update", Some(json!({"name": "R"}))),
        head(4, "locus", "l", "update", Some(json!({"place": "a"}))),
        head(5, "row", "p/t.k", "set", Some(json!({"palace": "p"}))),
        head(6, "binding", "l/item", "set", None),
        head(7, "binding", "l/item", "delete", None),
        head(8, "palace", "p", "update", Some(json!({"name": "Q"}))),
        head(9, "palace", "p", "delete", None),
        head(10, "palace", "p", "create", Some(json!({"name": "P"}))),
        head(11, "room", "r", "update", Some(json!({"name": "S"}))),
    ];
    // 3, 4, 5: their palace is deleted later; 6: deleted later; 8: the palace itself is deleted.
    // 11 comes after the delete (the palace was made again) and stays.
    assert_eq!(dropped(&heads), [3, 4, 5, 6, 8]);
}

fn device() -> (TempDir, Store) {
    let dir = TempDir::new().unwrap();
    let store = Store::open(&dir.path().join("profile")).unwrap();
    (dir, store)
}

fn new_locus(place: &str) -> NewLocus {
    NewLocus {
        audio: None,
        image_size: 0.25,
        zoom: 1.0,
        place: place.into(),
        item: "Item".into(),
        x: 0.4,
        y: 0.6,
        time: None,
        answer: Default::default(),
        image: None,
        note: String::new(),
    }
}

#[test]
fn the_summary_names_new_changed_and_deleted_palaces() {
    let (_dir, store) = device();
    let kept = store.create_palace("Behalten").unwrap();
    let gone = store.create_palace("Weg").unwrap();
    let room = store
        .add_room(&kept.id, "Küche", RoomKind::Blank, None)
        .unwrap();
    store.add_locus(&room.id, new_locus("Tür")).unwrap();
    // Everything so far reached the cloud.
    store
        .set_sync_cursor("push", store.last_op_seq().unwrap())
        .unwrap();
    assert_eq!(
        store
            .pending_changes(store.sync_cursor("push").unwrap())
            .unwrap(),
        ChangeSummary::default()
    );

    let fresh = store.create_palace("Neu").unwrap();
    store
        .add_room(&fresh.id, "Flur", RoomKind::Blank, None)
        .unwrap();
    store.rename_room(&room.id, "Bad").unwrap();
    store.rename_room(&room.id, "Keller").unwrap();
    store.rename_palace(&gone.id, "Bald weg").unwrap();
    store.delete_palace(&gone.id).unwrap();
    let brief = store.create_palace("Kurz").unwrap();
    store.delete_palace(&brief.id).unwrap();
    store.kv_set("review", "cards", b"1").unwrap();
    store.kv_set("review", "cards", b"2").unwrap();

    let summary = store
        .pending_changes(store.sync_cursor("push").unwrap())
        .unwrap();
    let mut palaces: Vec<(Option<&str>, ChangeKind, usize)> = summary
        .palaces
        .iter()
        .map(|p| (p.name.as_deref(), p.kind, p.changes))
        .collect();
    palaces.sort_by_key(|(name, ..)| *name);
    assert_eq!(
        palaces,
        [
            (Some("Bald weg"), ChangeKind::Deleted, 1),
            (Some("Behalten"), ChangeKind::Changed, 1),
            (Some("Neu"), ChangeKind::New, 2),
        ],
        "{summary:?}"
    );
    // The first rename and the first value are replaced; "Kurz" sends its create and delete.
    assert_eq!(summary.other, 1);
    assert_eq!(summary.total, 7);
    assert_eq!(
        summary.deleted().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        [gone.id.as_str()]
    );
}

#[test]
fn a_received_change_is_placed_in_its_palace() {
    let (_dir, store) = device();
    let palace = store.create_palace("Haus").unwrap();
    let room = store
        .add_room(&palace.id, "Küche", RoomKind::Blank, None)
        .unwrap();
    let locus = store.add_locus(&room.id, new_locus("Tür")).unwrap();
    let remote = |entity: &str, id: &str, op: &str, data: Option<Value>| RemoteOp {
        hlc: "9999999999999-00000-other".into(),
        entity: entity.into(),
        entity_id: id.into(),
        op: op.into(),
        data,
    };
    let of = |op: RemoteOp| store.palace_of(&op).unwrap();
    assert_eq!(
        of(remote("locus", &locus.id, "update", None)),
        Some(palace.id.clone())
    );
    assert_eq!(
        of(remote(
            "binding",
            &format!("{}/item", locus.id),
            "set",
            None
        )),
        Some(palace.id.clone())
    );
    assert_eq!(
        of(remote(
            "locus",
            "new",
            "create",
            Some(json!({"room_id": room.id}))
        )),
        Some(palace.id.clone())
    );
    assert_eq!(of(remote("kv", "review/cards", "set", None)), None);
    store.delete_palace(&palace.id).unwrap();
    // Deleted here: the log still knows where the locus was, and the palace's name.
    assert_eq!(
        of(remote("locus", &locus.id, "update", None)),
        Some(palace.id.clone())
    );
    assert_eq!(
        store.palace_name(&palace.id).unwrap().as_deref(),
        Some("Haus")
    );
}

/// A small deterministic generator, so a failing case can be run again.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }

    fn pick(&mut self, items: &[String]) -> Option<String> {
        (!items.is_empty()).then(|| items[self.below(items.len() as u64) as usize].clone())
    }

    fn unit(&mut self) -> f64 {
        self.below(101) as f64 / 100.0
    }
}

fn ids(store: &Store, table: &str) -> Vec<String> {
    let conn = store.conn();
    let mut stmt = conn
        .prepare(&format!("SELECT id FROM {table} ORDER BY id"))
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .map(|id| id.unwrap())
        .collect()
}

fn random_patch(rng: &mut Rng) -> LocusPatch {
    let mut patch = LocusPatch::default();
    for _ in 0..1 + rng.below(3) {
        match rng.below(8) {
            0 => patch.place = Some(format!("place {}", rng.below(5))),
            1 => patch.item = Some(format!("item {}", rng.below(5))),
            2 => patch.x = Some(rng.unit()),
            3 => patch.y = Some(rng.unit()),
            4 => patch.time = Some((rng.below(2) == 0).then(|| rng.below(60) as f64)),
            5 => patch.note = Some(format!("note {}", rng.below(5))),
            6 => patch.zoom = Some(1.0 + rng.below(3) as f64),
            _ => patch.image_size = Some(0.1 + rng.below(9) as f64 / 10.0),
        }
    }
    patch
}

/// One random local change; failures (a name taken twice, nothing to change) are ignored.
fn act(rng: &mut Rng, store: &Store, counter: &mut u32, deletes: bool) {
    *counter += 1;
    let n = *counter;
    let palaces = ids(store, "palaces");
    let rooms = ids(store, "rooms");
    let loci = ids(store, "loci");
    let _ = match rng.below(20) {
        0 => store.create_palace(&format!("P{n}")).map(drop),
        1 | 2 => match rng.pick(&palaces) {
            Some(p) => store
                .add_room(&p, &format!("R{n}"), RoomKind::Blank, None)
                .map(drop),
            None => Ok(()),
        },
        3 | 4 => match rng.pick(&rooms) {
            Some(r) => store.add_locus(&r, new_locus(&format!("L{n}"))).map(drop),
            None => Ok(()),
        },
        5..=8 => match rng.pick(&loci) {
            Some(l) => store.update_locus(&l, &random_patch(rng)).map(drop),
            None => Ok(()),
        },
        9 => match rng.pick(&palaces) {
            Some(p) => store.rename_palace(&p, &format!("P{n}")),
            None => Ok(()),
        },
        10 => match rng.pick(&rooms) {
            Some(r) if rng.below(2) == 0 => store.rename_room(&r, &format!("R{n}")),
            Some(r) => store.move_room(&r, if rng.below(2) == 0 { 1 } else { -1 }),
            None => Ok(()),
        },
        11..=13 => store.kv_set(
            "review",
            &format!("k{}", rng.below(3)),
            format!("v{n}").as_bytes(),
        ),
        14 => match rng.pick(&palaces) {
            Some(p) if rng.below(3) == 0 => store
                .delete_row(Some(&p), "T", &format!("{}", rng.below(2)))
                .map(drop),
            Some(p) => store.set_row(
                Some(&p),
                &Row::value("T", &format!("{}", rng.below(2)), &format!("c{n}")),
            ),
            None => Ok(()),
        },
        15 => match rng.pick(&loci) {
            Some(l) if rng.below(3) == 0 => store.remove_binding(&l, BindingField::Item).map(drop),
            Some(l) => store.set_binding(
                &l,
                &Binding {
                    field: BindingField::Item,
                    expression: format!("\"b{n}\""),
                },
            ),
            None => Ok(()),
        },
        16 if deletes => match rng.pick(&loci) {
            Some(l) => store.delete_locus(&l).map(drop),
            None => Ok(()),
        },
        17 if deletes => match rng.pick(&rooms) {
            Some(r) => store.delete_room(&r).map(drop),
            None => Ok(()),
        },
        18 if deletes && rng.below(3) == 0 => match rng.pick(&palaces) {
            Some(p) => store.delete_palace(&p).map(drop),
            None => Ok(()),
        },
        _ => Ok(()),
    };
}

/// Everything a person can see, without random row ids or clocks.
fn state(store: &Store) -> Vec<String> {
    let conn = store.conn();
    let queries = [
        "SELECT id, name, IFNULL(marker, '') FROM palaces",
        "SELECT id, palace_id, position, name, kind FROM rooms",
        "SELECT id, room_id, position, place, item, x, y, IFNULL(time, -1), answer, note, image_size, zoom FROM loci",
        "SELECT locus_id, field, expression FROM bindings",
        "SELECT IFNULL(palace_id, ''), table_name, key, cells FROM table_rows",
        "SELECT plugin, key, hex(value) FROM kv",
    ];
    let mut rows = Vec::new();
    for query in queries {
        let mut stmt = conn.prepare(query).unwrap();
        let columns = stmt.column_count();
        let found = stmt
            .query_map([], |r| {
                let mut row = Vec::new();
                for i in 0..columns {
                    let value: rusqlite::types::Value = r.get(i)?;
                    row.push(format!("{value:?}"));
                }
                Ok(row.join("|"))
            })
            .unwrap();
        for row in found {
            rows.push(format!("{query}: {}", row.unwrap()));
        }
    }
    rows.sort();
    rows
}

fn remote(op: Op) -> RemoteOp {
    RemoteOp {
        hlc: op.hlc,
        entity: op.entity,
        entity_id: op.entity_id,
        op: op.op,
        data: op.data,
    }
}

/// A fresh device that already has `base`, then receives `first` and `second`.
fn receive(base: &[RemoteOp], first: &[RemoteOp], second: &[RemoteOp]) -> Vec<String> {
    let store = Store::in_memory().unwrap();
    store.apply_remote(base).unwrap();
    store.apply_remote(first).unwrap();
    store.apply_remote(second).unwrap();
    state(&store)
}

#[test]
fn leaving_out_superseded_operations_changes_nothing_any_device_ends_up_with() {
    let mut compared = 0;
    let mut left_out = 0;
    for seed in 1..=40u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let a = Store::in_memory().unwrap();
        let mut counter = 0;
        for _ in 0..40 {
            act(&mut rng, &a, &mut counter, false);
        }
        // What the cloud already has; the rest is pending on this device.
        let cursor = a.last_op_seq().unwrap();
        let base: Vec<RemoteOp> = a.ops_since(0).unwrap().into_iter().map(remote).collect();

        // Another device starts from the same data and changes it at the same time.
        let c = Store::in_memory().unwrap();
        c.apply_remote(&base).unwrap();
        let c_start = c.last_op_seq().unwrap();
        for _ in 0..30 {
            act(&mut rng, &c, &mut counter, true);
            act(&mut rng, &a, &mut counter, true);
        }
        let concurrent: Vec<RemoteOp> = c
            .ops_since(c_start)
            .unwrap()
            .into_iter()
            .map(remote)
            .collect();
        let pending = a.ops_since(cursor).unwrap();
        let skip = a.superseded_after(cursor).unwrap();
        left_out += skip.len();
        let full: Vec<RemoteOp> = pending.iter().cloned().map(remote).collect();
        let compact: Vec<RemoteOp> = pending
            .into_iter()
            .filter(|op| !skip.contains(&op.seq))
            .map(remote)
            .collect();
        assert_eq!(
            receive(&base, &full, &concurrent),
            receive(&base, &compact, &concurrent),
            "seed {seed}: pending first"
        );
        assert_eq!(
            receive(&base, &concurrent, &full),
            receive(&base, &concurrent, &compact),
            "seed {seed}: concurrent first"
        );
        compared += 2;
    }
    assert_eq!(compared, 80);
    assert!(
        left_out > 100,
        "the generator must exercise compaction ({left_out})"
    );
}

#[test]
fn a_page_of_the_outbox_is_read_with_a_limit() {
    let (_dir, store) = device();
    for n in 0..5 {
        store.kv_set("p", "k", format!("{n}").as_bytes()).unwrap();
    }
    let page = store.ops_after(1, 2).unwrap();
    assert_eq!(page.iter().map(|op| op.seq).collect::<Vec<_>>(), [2, 3]);
    assert_eq!(store.last_op_seq().unwrap(), 5);
    let skip: HashSet<i64> = store.superseded_after(0).unwrap();
    assert_eq!(skip.len(), 4);
}
