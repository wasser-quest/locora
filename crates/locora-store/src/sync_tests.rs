//! Two devices exchanging operations, the way the cloud does.

use locora_formula::Row;
use locora_formula::workbook::{Binding, BindingField};
use tempfile::TempDir;

use super::{ApplyReport, RemoteOp, media_refs};
use crate::{LocusPatch, NewLocus, Op, RoomKind, Store};

fn device() -> (TempDir, Store) {
    let dir = TempDir::new().unwrap();
    let store = Store::open(&dir.path().join("profile")).unwrap();
    (dir, store)
}

/// Everything `from` logged after `since`, as the cloud would hand it over.
fn ship(from: &Store, since: i64) -> Vec<RemoteOp> {
    from.ops_since(since)
        .unwrap()
        .into_iter()
        .map(remote)
        .collect()
}

fn last_seq(store: &Store) -> i64 {
    store.ops_since(0).unwrap().last().map_or(0, |op| op.seq)
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

fn locus(place: &str) -> NewLocus {
    NewLocus {
        audio: None,
        image_size: 0.25,
        zoom: 1.0,
        place: place.into(),
        item: "Merkur".into(),
        x: 0.4,
        y: 0.6,
        time: None,
        answer: Default::default(),
        image: None,
        note: String::new(),
    }
}

/// `(place, item)` of every locus in the palace, in route order.
fn loci(store: &Store, palace: &str) -> Vec<(String, String)> {
    let tree = store.palace(palace).unwrap();
    tree.rooms
        .iter()
        .flat_map(|room| room.loci.iter())
        .map(|l| (l.place.clone(), l.item.clone()))
        .collect()
}

fn setup() -> (TempDir, Store, TempDir, Store, String, String) {
    let (a_dir, a) = device();
    let (b_dir, b) = device();
    let palace = a.create_palace("Planeten").unwrap();
    let room = a
        .add_room(&palace.id, "Küche", RoomKind::Blank, None)
        .unwrap();
    let tur = a.add_locus(&room.id, locus("Tür")).unwrap();
    b.apply_remote(&ship(&a, 0)).unwrap();
    (a_dir, a, b_dir, b, palace.id, tur.id)
}

#[test]
fn palaces_rooms_loci_bindings_tables_and_kv_replicate() {
    let (_a_dir, a) = device();
    let (_b_dir, b) = device();
    let palace = a.create_palace("Planeten").unwrap();
    let room = a
        .add_room(&palace.id, "Küche", RoomKind::Blank, None)
        .unwrap();
    let tur = a.add_locus(&room.id, locus("Tür")).unwrap();
    a.set_binding(
        &tur.id,
        &Binding {
            field: BindingField::Item,
            expression: "\"Venus\"".into(),
        },
    )
    .unwrap();
    a.set_row(Some(&palace.id), &Row::value("Planeten", "Merkur", "Hitze"))
        .unwrap();
    a.kv_set("tutorial", "stand", b"zwei").unwrap();

    let report = b.apply_remote(&ship(&a, 0)).unwrap();
    assert_eq!(report.failed, Vec::<String>::new());
    assert_eq!(report.missing, 0);
    assert_eq!(
        loci(&b, "Planeten"),
        vec![("Tür".to_owned(), "Merkur".to_owned())]
    );
    let tree = b.palace("Planeten").unwrap();
    assert_eq!(tree.rooms[0].loci[0].bindings.len(), 1);
    assert_eq!(tree.tables.len(), 1);
    assert_eq!(
        b.kv_get("tutorial", "stand").unwrap(),
        Some(b"zwei".to_vec())
    );
}

#[test]
fn replaying_the_same_operations_changes_nothing() {
    let (_a_dir, a, _b_dir, b, _palace, _tur) = setup();
    let ops = ship(&a, 0);
    let again = b.apply_remote(&ops).unwrap();
    assert_eq!(
        again,
        ApplyReport {
            applied: 0,
            stale: ops.len(),
            missing: 0,
            failed: vec![]
        }
    );
    assert_eq!(b.palaces().unwrap().len(), 1);
}

#[test]
fn concurrent_edits_to_one_field_keep_the_later_timestamp_everywhere() {
    let (_a_dir, a, _b_dir, b, palace, tur) = setup();
    let patch = |place: &str| LocusPatch {
        place: Some(place.into()),
        ..Default::default()
    };
    let (cursor_a, cursor_b) = (last_seq(&a), last_seq(&b));
    // Neither device has seen the other's edit when it makes its own.
    a.update_locus(&tur, &patch("Fenster")).unwrap();
    b.update_locus(&tur, &patch("Dach")).unwrap();
    let hlc_a = a.ops_since(cursor_a).unwrap().last().unwrap().hlc.clone();
    let hlc_b = b.ops_since(cursor_b).unwrap().last().unwrap().hlc.clone();
    let winner = if hlc_a > hlc_b { "Fenster" } else { "Dach" };

    b.apply_remote(&ship(&a, cursor_a)).unwrap();
    a.apply_remote(&ship(&b, cursor_b)).unwrap();
    assert_eq!(loci(&a, &palace)[0].0, winner);
    assert_eq!(loci(&b, &palace)[0].0, winner);
}

#[test]
fn a_delete_wins_over_an_edit_made_on_the_other_device() {
    let (_a_dir, a, _b_dir, b, palace, tur) = setup();
    let (cursor_a, cursor_b) = (last_seq(&a), last_seq(&b));
    a.delete_locus(&tur).unwrap();
    b.update_locus(
        &tur,
        &LocusPatch {
            item: Some("Venus".into()),
            ..Default::default()
        },
    )
    .unwrap();
    b.apply_remote(&ship(&a, cursor_a)).unwrap();
    a.apply_remote(&ship(&b, cursor_b)).unwrap();
    assert!(loci(&a, &palace).is_empty());
    assert!(loci(&b, &palace).is_empty());
}

#[test]
fn palace_names_stay_unique_on_the_receiving_device() {
    let (_a_dir, a) = device();
    let (_b_dir, b) = device();
    a.create_palace("Planeten").unwrap();
    b.create_palace("planeten").unwrap();
    let report = b.apply_remote(&ship(&a, 0)).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let mut names: Vec<String> = b.palaces().unwrap().into_iter().map(|p| p.name).collect();
    names.sort();
    assert_eq!(
        names,
        vec!["Planeten (2)".to_owned(), "planeten".to_owned()]
    );
}

#[test]
fn loci_whose_room_is_unknown_wait_as_missing() {
    let (_a_dir, a) = device();
    let (_b_dir, b) = device();
    let palace = a.create_palace("Planeten").unwrap();
    let room = a
        .add_room(&palace.id, "Küche", RoomKind::Blank, None)
        .unwrap();
    a.add_locus(&room.id, locus("Tür")).unwrap();
    let locus_only: Vec<RemoteOp> = ship(&a, 0)
        .into_iter()
        .filter(|op| op.entity == "locus")
        .collect();
    let report = b.apply_remote(&locus_only).unwrap();
    assert_eq!(report.missing, 1);
    assert!(report.failed.is_empty());
}

#[test]
fn removing_a_picture_on_one_device_removes_it_on_the_other() {
    let (_a_dir, a) = device();
    let (_b_dir, b) = device();
    let picture = a.put_media(b"fake-picture", "image/png").unwrap();
    b.put_media(b"fake-picture", "image/png").unwrap();
    let palace = a.create_palace("Planeten").unwrap();
    let room = a
        .add_room(&palace.id, "Küche", RoomKind::Blank, None)
        .unwrap();
    let mut with_picture = locus("Tür");
    with_picture.image = Some(picture.hash.clone());
    let tur = a.add_locus(&room.id, with_picture).unwrap();
    b.apply_remote(&ship(&a, 0)).unwrap();
    assert_eq!(
        b.palace("Planeten").unwrap().rooms[0].loci[0].image,
        Some(picture.hash.clone())
    );

    let cursor = last_seq(&a);
    a.update_locus(
        &tur.id,
        &LocusPatch {
            image: Some(None),
            ..Default::default()
        },
    )
    .unwrap();
    let report = b.apply_remote(&ship(&a, cursor)).unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(b.palace("Planeten").unwrap().rooms[0].loci[0].image, None);
}

#[test]
fn media_references_are_listed_per_operation() {
    let op = |entity: &str, action: &str, data: serde_json::Value| RemoteOp {
        hlc: "0001-00000-a".into(),
        entity: entity.into(),
        entity_id: "id".into(),
        op: action.into(),
        data: Some(data),
    };
    assert_eq!(
        media_refs(&op("room", "create", serde_json::json!({"media": "h1"}))),
        vec!["h1"]
    );
    assert_eq!(
        media_refs(&op("room", "create", serde_json::json!({"media": null}))),
        Vec::<String>::new()
    );
    assert_eq!(
        media_refs(&op("palace", "update", serde_json::json!({"marker": "h2"}))),
        vec!["h2"]
    );
    assert_eq!(
        media_refs(&op("locus", "update", serde_json::json!({"image": "h3"}))),
        vec!["h3"]
    );
    assert!(media_refs(&op("kv", "set", serde_json::json!({"value": "00"}))).is_empty());
}

#[test]
fn finished_journeys_replicate_once_with_their_answers() {
    use crate::JourneyAnswer;
    let (_a_dir, a, _b_dir, b, palace, _tur) = setup();
    let answers = [
        JourneyAnswer {
            correct: true,
            at_ms: 1_000,
        },
        JourneyAnswer {
            correct: false,
            at_ms: 2_500,
        },
        JourneyAnswer {
            correct: true,
            at_ms: 4_000,
        },
    ];
    let journey = a.record_journey(&palace, &answers).unwrap();
    assert_eq!(journey.palace_name, "Planeten");
    assert!(
        a.record_journey(&palace, &[]).is_err(),
        "empty journeys are refused"
    );

    let report = b.apply_remote(&ship(&a, 0)).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let stored = b.journeys(10).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].id, journey.id);
    assert_eq!(stored[0].answers, answers.to_vec());
    // A second delivery does not store it again.
    b.apply_remote(&ship(&a, 0)).unwrap();
    assert_eq!(b.journeys(10).unwrap().len(), 1);
}

#[test]
fn room_order_and_names_replicate_field_by_field() {
    let (_a_dir, a, _b_dir, b, palace, _tur) = setup();
    let names = |store: &Store| -> Vec<String> {
        store
            .palace(&palace)
            .unwrap()
            .rooms
            .iter()
            .map(|r| r.room.name.clone())
            .collect()
    };
    let cursor = last_seq(&a);
    let second = a.add_room(&palace, "Hof", RoomKind::Blank, None).unwrap();
    b.apply_remote(&ship(&a, cursor)).unwrap();

    let (cursor_a, cursor_b) = (last_seq(&a), last_seq(&b));
    // One device moves a room while the other renames it: both changes must survive.
    a.move_room(&second.id, -1).unwrap();
    b.rename_room(&second.id, "Innenhof").unwrap();
    b.apply_remote(&ship(&a, cursor_a)).unwrap();
    a.apply_remote(&ship(&b, cursor_b)).unwrap();
    assert_eq!(names(&a), ["Innenhof", "Küche"]);
    assert_eq!(names(&b), ["Innenhof", "Küche"]);
}

#[test]
fn a_deleted_room_is_deleted_on_the_other_device() {
    let (_a_dir, a, _b_dir, b, palace, tur) = setup();
    let cursor = last_seq(&a);
    let room = a.palace(&palace).unwrap().rooms[0].room.id.clone();
    a.delete_room(&room).unwrap();
    b.apply_remote(&ship(&a, cursor)).unwrap();
    assert!(b.palace(&palace).unwrap().rooms.is_empty());
    assert!(
        loci(&b, &palace).is_empty(),
        "loci of the room go with it: {tur}"
    );
}
