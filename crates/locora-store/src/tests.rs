use super::*;

fn locus(place: &str, item: &str, x: f64) -> NewLocus {
    NewLocus {
        image_size: default_image_size(),
        zoom: default_zoom(),
        place: place.into(),
        item: item.into(),
        x,
        y: 0.5,
        time: None,
        answer: Default::default(),
        image: None,
        note: String::new(),
    }
}

#[test]
fn media_palace_is_atomic_including_metadata_and_ops() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let before = store.ops_since(0).unwrap();
    store.conn().execute_batch("CREATE TRIGGER reject_import BEFORE INSERT ON rooms BEGIN SELECT RAISE(ABORT, 'test import failure'); END;").unwrap();
    assert!(
        store
            .create_media_palace("Test", RoomKind::Image, b"picture", "image/png")
            .is_err()
    );
    assert!(store.palaces().unwrap().is_empty());
    assert_eq!(store.ops_since(0).unwrap(), before);
    let count: u64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM media", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    store
        .conn()
        .execute_batch("DROP TRIGGER reject_import")
        .unwrap();
    store.put_media(b"picture", "video/mp4").unwrap();
    assert!(
        store
            .create_media_palace("Test", RoomKind::Image, b"picture", "image/png")
            .is_err()
    );
    assert!(store.palaces().unwrap().is_empty());
    assert_eq!(store.ops_since(0).unwrap(), before);
    assert!(
        store
            .create_media_palace(" ", RoomKind::Image, b"x", "image/png")
            .is_err()
    );
    assert!(
        store
            .create_media_palace("Test", RoomKind::Video, b"x", "image/png")
            .is_err()
    );
}

#[test]
fn locus_batch_rolls_back_validation_and_database_failures() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let palace = store.create_palace("Batch").unwrap();
    let room = store
        .add_room(&palace.id, "Raum", RoomKind::Blank, None)
        .unwrap();
    store
        .add_locus(&room.id, locus("Bestehend", "alt", 0.5))
        .unwrap();
    let before = store.palace(&palace.id).unwrap();
    let ops = store.ops_since(0).unwrap();
    assert!(
        store
            .add_loci(
                &room.id,
                vec![locus("Gut", "gut", 0.1), locus("Ungültig", "x", f64::NAN)]
            )
            .is_err()
    );
    assert_eq!(store.palace(&palace.id).unwrap(), before);
    assert_eq!(store.ops_since(0).unwrap(), ops);
    store.conn().execute_batch("CREATE TRIGGER reject_batch BEFORE INSERT ON loci WHEN NEW.item = 'fail' BEGIN SELECT RAISE(ABORT, 'test batch failure'); END;").unwrap();
    assert!(
        store
            .add_loci(
                &room.id,
                vec![locus("Gut", "gut", 0.1), locus("Fehler", "fail", 0.2)]
            )
            .is_err()
    );
    assert_eq!(store.palace(&palace.id).unwrap(), before);
    assert_eq!(store.ops_since(0).unwrap(), ops);
    let added = store
        .add_loci(
            &room.id,
            vec![locus("Zwei", "neu", 0.1), locus("Drei", "auch neu", 0.2)],
        )
        .unwrap();
    assert_eq!((added[0].position, added[1].position), (1, 2));
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let tree = store.palace(&palace.id).unwrap();
    assert_eq!(tree.rooms[0].loci.len(), 3);
    assert_eq!(tree.rooms[0].loci[2].item, "auch neu");
    assert_eq!(store.ops_since(0).unwrap().len(), ops.len() + 2);
}

#[test]
fn palaces_stages_and_loci_keep_route_order() {
    let store = Store::in_memory().unwrap();
    let palace = store.create_palace("Planeten").unwrap();
    let room = store
        .add_room(&palace.id, "Wohnung", RoomKind::Blank, None)
        .unwrap();
    for (i, name) in ["Merkur", "Venus", "Erde"].iter().enumerate() {
        store
            .add_locus(&room.id, locus(&format!("Ort {i}"), name, 0.1 * i as f64))
            .unwrap();
    }
    let second = store
        .add_room(&palace.id, "Garten", RoomKind::Blank, None)
        .unwrap();
    store
        .add_locus(&second.id, locus("Baum", "Mars", 0.5))
        .unwrap();

    let tree = store.palace("planeten").unwrap();
    let route: Vec<&str> = tree.route().map(|l| l.item.as_str()).collect();
    assert_eq!(route, ["Merkur", "Venus", "Erde", "Mars"]);
    assert_eq!(tree.rooms[1].room.position, 1);

    let list = store.palaces().unwrap();
    assert_eq!((list[0].rooms, list[0].loci), (2, 4));
}

#[test]
fn palaces_are_found_by_name_id_or_prefix() {
    let store = Store::in_memory().unwrap();
    let p = store.create_palace("Vokabeln").unwrap();
    assert_eq!(store.find_palace("VOKABELN").unwrap().unwrap().id, p.id);
    assert_eq!(store.find_palace(&p.id).unwrap().unwrap().name, "Vokabeln");
    assert_eq!(store.find_palace(&p.id[..12]).unwrap().unwrap().id, p.id);
    assert!(store.find_palace("nix").unwrap().is_none());
    assert!(matches!(store.palace("nix"), Err(StoreError::NotFound(_))));
    assert!(matches!(
        store.create_palace("vokabeln"),
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        store.create_palace("  "),
        Err(StoreError::Invalid(_))
    ));
}

#[test]
fn updates_validate_positions_and_deletes_cascade() {
    let store = Store::in_memory().unwrap();
    let p = store.create_palace("P").unwrap();
    let s = store.add_room(&p.id, "S", RoomKind::Blank, None).unwrap();
    let l = store.add_locus(&s.id, locus("a", "b", 0.2)).unwrap();
    assert!(store.add_locus(&s.id, locus("x", "y", 1.5)).is_err());

    let patch = LocusPatch {
        item: Some("neu".into()),
        x: Some(0.9),
        ..Default::default()
    };
    let updated = store.update_locus(&l.id, &patch).unwrap();
    assert_eq!(
        (updated.item.as_str(), updated.x, updated.place.as_str()),
        ("neu", 0.9, "a")
    );
    assert!(
        store
            .update_locus(
                &l.id,
                &LocusPatch {
                    y: Some(-1.0),
                    ..Default::default()
                }
            )
            .is_err()
    );

    assert_eq!(
        store.delete_locus(&l.id).unwrap().as_deref(),
        Some(p.id.as_str())
    );
    assert_eq!(store.delete_locus(&l.id).unwrap(), None);
    store.add_locus(&s.id, locus("c", "d", 0.3)).unwrap();
    assert!(store.delete_palace(&p.id).unwrap());
    assert!(store.palaces().unwrap().is_empty());
    let conn = store.conn();
    let orphans: i64 = conn
        .query_row("SELECT COUNT(*) FROM loci", [], |r| r.get(0))
        .unwrap();
    assert_eq!(orphans, 0);
}

#[test]
fn media_is_content_addressed_and_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let hash;
    {
        let store = Store::open(dir.path()).unwrap();
        let a = store.put_media(b"bild", "image/png").unwrap();
        let b = store.put_media(b"bild", "image/png").unwrap();
        assert_eq!(a, b);
        hash = a.hash.clone();
        assert_eq!(
            std::fs::read(store.media_file(&hash).unwrap()).unwrap(),
            b"bild"
        );
        let p = store.create_palace("Mit Bild").unwrap();
        let s = store
            .add_room(&p.id, "Küche", RoomKind::Image, Some(&hash))
            .unwrap();
        assert_eq!(s.media.as_deref(), Some(hash.as_str()));
        assert!(store.add_room(&p.id, "x", RoomKind::Image, None).is_err());
        store.kv_set("review", "k", b"v").unwrap();
    }
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.media(&hash).unwrap().unwrap().size, 4);
    assert_eq!(
        store.palace("Mit Bild").unwrap().rooms[0].room.kind,
        RoomKind::Image
    );
    assert_eq!(
        store.kv_get("review", "k").unwrap().as_deref(),
        Some(&b"v"[..])
    );
    assert!(
        Store::in_memory()
            .unwrap()
            .put_media(b"x", "image/png")
            .is_err()
    );
}

#[test]
fn every_change_is_logged_with_increasing_clocks() {
    let store = Store::in_memory().unwrap();
    let p = store.create_palace("Log").unwrap();
    let s = store.add_room(&p.id, "S", RoomKind::Blank, None).unwrap();
    let l = store.add_locus(&s.id, locus("a", "b", 0.5)).unwrap();
    store
        .update_locus(
            &l.id,
            &LocusPatch {
                item: Some("c".into()),
                ..Default::default()
            },
        )
        .unwrap();
    store.delete_locus(&l.id).unwrap();
    let ops = store.ops_since(0).unwrap();
    let kinds: Vec<(&str, &str)> = ops
        .iter()
        .map(|o| (o.entity.as_str(), o.op.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("palace", "create"),
            ("room", "create"),
            ("locus", "create"),
            ("locus", "update"),
            ("locus", "delete")
        ]
    );
    assert!(ops.windows(2).all(|w| w[0].hlc < w[1].hlc));
    assert_eq!(store.ops_since(ops[2].seq).unwrap().len(), 2);
}

#[test]
fn tables_are_local_or_shared_and_local_rows_win() {
    use locora_formula::{CellType, Row};
    let store = Store::in_memory().unwrap();
    let p = store.create_palace("Major").unwrap();
    store
        .set_row(None, &Row::value("Major", "1", "Tee"))
        .unwrap();
    store
        .set_row(None, &Row::value("Major", "2", "Noah"))
        .unwrap();
    store
        .set_row(Some(&p.id), &Row::value("Major", "2", "Neo"))
        .unwrap();
    // Replacing keeps one row per key.
    store
        .set_row(
            Some(&p.id),
            &Row::typed("Major", "2", CellType::Text, "Nil"),
        )
        .unwrap();
    assert_eq!(store.rows(Some(&p.id)).unwrap().len(), 1);
    let effective = store.effective_rows(&p.id).unwrap();
    assert_eq!(effective.len(), 2);
    assert_eq!(
        effective.iter().find(|r| r.key == "2").unwrap().cells[0].value,
        "Nil"
    );
    assert_eq!(store.palace("Major").unwrap().tables.len(), 1);

    assert!(
        store
            .set_row(None, &Row::typed("S", "k", CellType::Number, "abc"))
            .is_err()
    );
    assert!(
        store
            .set_row(Some("nope"), &Row::value("S", "k", "v"))
            .is_err()
    );
    assert!(store.delete_row(None, "Major", "1").unwrap());
    assert!(!store.delete_row(None, "Major", "1").unwrap());
    store.delete_palace(&p.id).unwrap();
    assert_eq!(
        store.rows(None).unwrap().len(),
        1,
        "shared rows outlive palaces"
    );
}

#[test]
fn bindings_belong_to_loci() {
    use locora_formula::workbook::{Binding, BindingField};
    let store = Store::in_memory().unwrap();
    let p = store.create_palace("P").unwrap();
    let s = store.add_room(&p.id, "S", RoomKind::Blank, None).unwrap();
    let l = store.add_locus(&s.id, locus("a", "b", 0.5)).unwrap();
    let memory = Binding {
        field: BindingField::Item,
        expression: "Data.word".into(),
    };
    store.set_binding(&l.id, &memory).unwrap();
    store
        .set_binding(
            &l.id,
            &Binding {
                field: BindingField::Item,
                expression: "\"neu\"".into(),
            },
        )
        .unwrap();
    store
        .set_binding(
            &l.id,
            &Binding {
                field: BindingField::Active,
                expression: "true".into(),
            },
        )
        .unwrap();
    let tree = store.palace("P").unwrap();
    let bindings = &tree.route().next().unwrap().bindings;
    assert_eq!(bindings.len(), 2);
    assert!(
        bindings
            .iter()
            .any(|b| b.field == BindingField::Item && b.expression == "\"neu\"")
    );
    assert!(
        store
            .set_binding(
                &l.id,
                &Binding {
                    field: BindingField::X,
                    expression: "1 +".into()
                }
            )
            .is_err()
    );
    assert!(store.remove_binding(&l.id, BindingField::Active).unwrap());
    assert!(!store.remove_binding(&l.id, BindingField::Active).unwrap());
    store.delete_locus(&l.id).unwrap();
    let conn = store.conn();
    let left: i64 = conn
        .query_row("SELECT COUNT(*) FROM bindings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn answers_persist_patch_and_validate_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let p = store.create_palace("P").unwrap();
    let r = store.add_room(&p.id, "R", RoomKind::Blank, None).unwrap();
    let l = store.add_locus(&r.id, locus("Tür", "Merkur", 0.5)).unwrap();
    assert!(l.answer.is_recall());
    let quiz = Answer {
        kind: AnswerKind::Quiz,
        options: vec![
            QuizOption {
                text: "Merkur".into(),
                correct: true,
            },
            QuizOption {
                text: "Venus".into(),
                correct: false,
            },
        ],
        ..Answer::default()
    };
    store
        .update_locus(
            &l.id,
            &LocusPatch {
                answer: Some(quiz.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    let before: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM ops", [], |r| r.get(0))
        .unwrap();
    let invalid = Answer {
        kind: AnswerKind::Quiz,
        options: vec![],
        ..Answer::default()
    };
    assert!(
        store
            .update_locus(
                &l.id,
                &LocusPatch {
                    item: Some("bad".into()),
                    answer: Some(invalid.clone()),
                    ..Default::default()
                }
            )
            .is_err()
    );
    let mut new = locus("X", "Y", 0.2);
    new.answer = invalid;
    assert!(store.add_locus(&r.id, new).is_err());
    assert_eq!(
        before,
        store
            .conn()
            .query_row("SELECT COUNT(*) FROM ops", [], |r| r.get::<_, i64>(0))
            .unwrap()
    );
    store
        .update_locus(
            &l.id,
            &LocusPatch {
                place: Some("Tor".into()),
                ..Default::default()
            },
        )
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let tree = store.palace("P").unwrap();
    assert_eq!(tree.rooms[0].loci[0].answer, quiz);
    assert_eq!(tree.rooms[0].loci[0].item, "Merkur");
    store
        .update_locus(
            &l.id,
            &LocusPatch {
                answer: Some(Answer::default()),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        store.palace("P").unwrap().rooms[0].loci[0]
            .answer
            .is_recall()
    );
}

#[test]
fn loci_can_show_pictures_but_not_videos() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let sply = store.put_media(b"\x89SPLY\r\n\x1a fake", SPLINELY).unwrap();
    let png = store.put_media(b"\x89PNG fake", "image/png").unwrap();
    let mp4 = store.put_media(b"fake mp4", "video/mp4").unwrap();
    let p = store.create_palace("Bilder").unwrap();
    let r = store
        .add_room(&p.id, "Raum", RoomKind::Blank, None)
        .unwrap();
    let l = store
        .add_locus(
            &r.id,
            NewLocus {
                image: Some(sply.hash.clone()),
                ..locus("a", "b", 0.5)
            },
        )
        .unwrap();
    assert_eq!(l.image.as_deref(), Some(sply.hash.as_str()));
    assert!(
        store
            .add_locus(
                &r.id,
                NewLocus {
                    image: Some(mp4.hash.clone()),
                    ..locus("c", "d", 0.5)
                }
            )
            .is_err()
    );
    assert!(
        store
            .add_locus(
                &r.id,
                NewLocus {
                    image: Some("0".repeat(64)),
                    ..locus("c", "d", 0.5)
                }
            )
            .is_err()
    );
    let changed = store
        .update_locus(
            &l.id,
            &LocusPatch {
                image: Some(Some(png.hash.clone())),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(changed.image.as_deref(), Some(png.hash.as_str()));
    assert_eq!(
        store
            .palace("Bilder")
            .unwrap()
            .route()
            .next()
            .unwrap()
            .image
            .as_deref(),
        Some(png.hash.as_str())
    );
    let cleared = store
        .update_locus(
            &l.id,
            &LocusPatch {
                image: Some(None),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(cleared.image, None);
    assert!(is_picture(SPLINELY) && is_picture("image/webp") && !is_picture("video/mp4"));
}

#[test]
fn palaces_can_have_a_custom_marker() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let marker = store
        .put_media(b"\x89SPLY\r\n\x1a marker", SPLINELY)
        .unwrap();
    let png = store.put_media(b"\x89PNG", "image/png").unwrap();
    let p = store.create_palace("Mit Marker").unwrap();
    assert_eq!(p.marker, None);
    store.set_marker(&p.id, Some(&marker.hash)).unwrap();
    assert_eq!(
        store.palace("Mit Marker").unwrap().palace.marker.as_deref(),
        Some(marker.hash.as_str())
    );
    assert!(
        store.set_marker(&p.id, Some(&png.hash)).is_err(),
        "markers are .sply only"
    );
    assert!(store.set_marker("nope", None).is_err());
    store.set_marker(&p.id, None).unwrap();
    assert_eq!(
        store.find_palace("Mit Marker").unwrap().unwrap().marker,
        None
    );
}

#[test]
fn image_size_defaults_persists_and_rejects_invalid_changes() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let p = store.create_palace("Größen").unwrap();
    let r = store
        .add_room(&p.id, "Raum", RoomKind::Blank, None)
        .unwrap();
    let old = serde_json::to_value(locus("Tor", "Inhalt", 0.5)).unwrap();
    let mut old = old.as_object().unwrap().clone();
    old.remove("image_size");
    let new: NewLocus = serde_json::from_value(old.into()).unwrap();
    assert_eq!(new.image_size, 0.25);
    let l = store.add_locus(&r.id, new).unwrap();
    assert_eq!(l.image_size, 0.25);
    for size in [0.05, 0.4, 1.0] {
        let changed = store
            .update_locus(
                &l.id,
                &LocusPatch {
                    image_size: Some(size),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(changed.image_size, size);
    }
    let before = store.ops_since(0).unwrap().len();
    for size in [
        0.0,
        -0.1,
        0.049,
        1.01,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        let mut new = locus("Tor", "Inhalt", 0.5);
        new.image_size = size;
        assert!(store.add_locus(&r.id, new).is_err());
        assert!(
            store
                .update_locus(
                    &l.id,
                    &LocusPatch {
                        image_size: Some(size),
                        item: Some("Nicht speichern".into()),
                        ..Default::default()
                    }
                )
                .is_err()
        );
    }
    assert_eq!(store.ops_since(0).unwrap().len(), before);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let tree = store.palace(&p.id).unwrap();
    let copy = tree.route().next().unwrap();
    assert_eq!(copy.image_size, 1.0);
    assert_eq!(copy.item, "Inhalt");
}

#[test]
fn zoom_defaults_persists_and_rejects_invalid_changes() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let p = store.create_palace("Zoom").unwrap();
    let r = store
        .add_room(&p.id, "Raum", RoomKind::Blank, None)
        .unwrap();
    let l = store.add_locus(&r.id, locus("Tor", "Inhalt", 0.5)).unwrap();
    assert_eq!(l.zoom, 1.0);
    let patch = |zoom| LocusPatch {
        zoom: Some(zoom),
        ..Default::default()
    };
    assert_eq!(store.update_locus(&l.id, &patch(2.5)).unwrap().zoom, 2.5);
    for zoom in [0.5, 4.01, f64::NAN, f64::INFINITY] {
        assert!(store.update_locus(&l.id, &patch(zoom)).is_err(), "{zoom}");
    }
    assert_eq!(
        store.palace(&p.id).unwrap().route().next().unwrap().zoom,
        2.5
    );
}

#[test]
fn rooms_and_palaces_can_be_renamed_and_rooms_reordered() {
    let store = Store::in_memory().unwrap();
    let palace = store.create_palace("Burg").unwrap();
    let first = store
        .add_room(&palace.id, "Eingang", RoomKind::Blank, None)
        .unwrap();
    let second = store
        .add_room(&palace.id, "Hof", RoomKind::Blank, None)
        .unwrap();
    let names = |store: &Store| -> Vec<String> {
        store
            .palace(&palace.id)
            .unwrap()
            .rooms
            .iter()
            .map(|r| r.room.name.clone())
            .collect()
    };

    store.move_room(&second.id, -1).unwrap();
    assert_eq!(names(&store), ["Hof", "Eingang"]);
    // Past the first room nothing moves; a jump of more than one place is refused.
    store.move_room(&second.id, -1).unwrap();
    assert_eq!(names(&store), ["Hof", "Eingang"]);
    assert!(store.move_room(&second.id, 2).is_err());

    store.rename_room(&first.id, "  Halle  ").unwrap();
    assert_eq!(names(&store), ["Hof", "Halle"]);
    assert!(store.rename_room(&first.id, "  ").is_err());
    assert!(store.rename_room("unbekannt", "x").is_err());

    // Palace names stay unique, case-insensitively, and a clash says so.
    store.create_palace("Schloss").unwrap();
    let clash = store.rename_palace(&palace.id, "schloss").unwrap_err();
    assert!(clash.to_string().contains("schon einen Palast"));
    store.rename_palace(&palace.id, "Festung").unwrap();
    assert_eq!(store.palace(&palace.id).unwrap().palace.name, "Festung");
}
