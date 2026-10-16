use super::*;

fn locus(label: &str, memory: &str, x: f64) -> NewLocus {
    NewLocus {
        label: label.into(),
        memory: memory.into(),
        x,
        y: 0.5,
        at: None,
    }
}

#[test]
fn palaces_stages_and_loci_keep_route_order() {
    let store = Store::in_memory().unwrap();
    let palace = store.create_palace("Planeten").unwrap();
    let stage = store
        .add_stage(&palace.id, "Wohnung", StageKind::Blank, None)
        .unwrap();
    for (i, name) in ["Merkur", "Venus", "Erde"].iter().enumerate() {
        store
            .add_locus(&stage.id, locus(&format!("Ort {i}"), name, 0.1 * i as f64))
            .unwrap();
    }
    let second = store
        .add_stage(&palace.id, "Garten", StageKind::Blank, None)
        .unwrap();
    store
        .add_locus(&second.id, locus("Baum", "Mars", 0.5))
        .unwrap();

    let tree = store.palace("planeten").unwrap();
    let route: Vec<&str> = tree.route().map(|l| l.memory.as_str()).collect();
    assert_eq!(route, ["Merkur", "Venus", "Erde", "Mars"]);
    assert_eq!(tree.stages[1].stage.position, 1);

    let list = store.palaces().unwrap();
    assert_eq!((list[0].stages, list[0].loci), (2, 4));
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
    let s = store.add_stage(&p.id, "S", StageKind::Blank, None).unwrap();
    let l = store.add_locus(&s.id, locus("a", "b", 0.2)).unwrap();
    assert!(store.add_locus(&s.id, locus("x", "y", 1.5)).is_err());

    let patch = LocusPatch {
        memory: Some("neu".into()),
        x: Some(0.9),
        ..Default::default()
    };
    let updated = store.update_locus(&l.id, &patch).unwrap();
    assert_eq!(
        (updated.memory.as_str(), updated.x, updated.label.as_str()),
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
            .add_stage(&p.id, "Küche", StageKind::Image, Some(&hash))
            .unwrap();
        assert_eq!(s.media.as_deref(), Some(hash.as_str()));
        assert!(store.add_stage(&p.id, "x", StageKind::Image, None).is_err());
        store.kv_set("review", "k", b"v").unwrap();
    }
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.media(&hash).unwrap().unwrap().size, 4);
    assert_eq!(
        store.palace("Mit Bild").unwrap().stages[0].stage.kind,
        StageKind::Image
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
    let s = store.add_stage(&p.id, "S", StageKind::Blank, None).unwrap();
    let l = store.add_locus(&s.id, locus("a", "b", 0.5)).unwrap();
    store
        .update_locus(
            &l.id,
            &LocusPatch {
                memory: Some("c".into()),
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
            ("stage", "create"),
            ("locus", "create"),
            ("locus", "update"),
            ("locus", "delete")
        ]
    );
    assert!(ops.windows(2).all(|w| w[0].hlc < w[1].hlc));
    assert_eq!(store.ops_since(ops[2].seq).unwrap().len(), 2);
}
