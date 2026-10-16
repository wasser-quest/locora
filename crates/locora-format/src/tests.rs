use std::io::Cursor;

use super::*;

fn sample(dir: &std::path::Path) -> Store {
    let store = Store::open(dir).unwrap();
    let image = store.put_media(b"\x89PNG fake", "image/png").unwrap();
    let video = store.put_media(b"fake mp4", "video/mp4").unwrap();
    let p = store.create_palace("Planeten").unwrap();
    let r = store
        .add_room(&p.id, "Wohnung", RoomKind::Image, Some(&image.hash))
        .unwrap();
    for (i, item) in ["Merkur", "Venus"].iter().enumerate() {
        store
            .add_locus(
                &r.id,
                NewLocus {
                    place: format!("Ort {i}"),
                    item: item.to_string(),
                    x: 0.2 + 0.3 * i as f64,
                    y: 0.5,
                    time: None,
                },
            )
            .unwrap();
    }
    let v = store.create_palace("Vortrag").unwrap();
    let vr = store
        .add_room(&v.id, "Talk", RoomKind::Video, Some(&video.hash))
        .unwrap();
    store
        .add_locus(
            &vr.id,
            NewLocus {
                place: "Tür".into(),
                item: "These".into(),
                x: 0.5,
                y: 0.5,
                time: Some(83.0),
            },
        )
        .unwrap();
    // A second room reusing the same image is exported once.
    store
        .add_room(&p.id, "Flur", RoomKind::Image, Some(&image.hash))
        .unwrap();
    store
}

fn exported(store: &Store, palaces: &[&str]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    let names: Vec<String> = palaces.iter().map(|s| s.to_string()).collect();
    export(store, &names, &mut out).unwrap();
    out.into_inner()
}

/// Rewrite an archive: `edit` gets every (name, bytes) entry and returns the new list.
fn rewrite(
    bytes: &[u8],
    edit: impl FnOnce(Vec<(String, Vec<u8>)>) -> Vec<(String, Vec<u8>)>,
) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        entries.push((f.name().unwrap().into_owned(), data));
    }
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in edit(entries) {
        let method = if name == "mimetype" {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        out.start_file(
            name.as_str(),
            SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn edit_json(
    entries: &mut [(String, Vec<u8>)],
    name: &str,
    f: impl FnOnce(&mut serde_json::Value),
) {
    let entry = entries
        .iter_mut()
        .find(|(n, _)| n == name || (name == "palace" && n.starts_with("palaces/")))
        .unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&entry.1).unwrap();
    f(&mut value);
    entry.1 = serde_json::to_vec(&value).unwrap();
}

fn open_err(bytes: Vec<u8>) -> String {
    match Package::open(Cursor::new(bytes)) {
        Ok(_) => panic!("unexpectedly valid"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn round_trip_into_another_store() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let source = sample(a.path());
    let bytes = exported(&source, &["Planeten", "Vortrag"]);

    let package = Package::open(Cursor::new(bytes.clone())).unwrap();
    assert_eq!(package.palaces.len(), 2);
    assert_eq!(package.manifest.media.len(), 2, "shared image stored once");
    assert_eq!(package.loci(), 3);

    let target = Store::open(b.path()).unwrap();
    let imported = package.import(&target).unwrap();
    assert_eq!(imported.len(), 2);
    let original = source.palace("Planeten").unwrap();
    let copy = target.palace("Planeten").unwrap();
    assert_ne!(copy.palace.id, original.palace.id, "imports are new copies");
    assert_eq!(copy.rooms.len(), 2);
    assert_eq!(copy.rooms[0].room.kind, RoomKind::Image);
    let fields = |t: &PalaceTree| {
        t.route()
            .map(|l| (l.place.clone(), l.item.clone(), l.x))
            .collect::<Vec<_>>()
    };
    assert_eq!(fields(&copy), fields(&original));
    let hash = copy.rooms[0].room.media.clone().unwrap();
    assert_eq!(
        std::fs::read(target.media_file(&hash).unwrap()).unwrap(),
        b"\x89PNG fake"
    );
    let talk = target.palace("Vortrag").unwrap();
    assert_eq!(talk.rooms[0].room.kind, RoomKind::Video);
    assert_eq!(talk.route().next().unwrap().time, Some(83.0));

    // Importing again keeps both: „Planeten (2)“.
    let again = Package::open(Cursor::new(bytes))
        .unwrap()
        .import(&target)
        .unwrap();
    assert_eq!(again[0].name, "Planeten (2)");
    assert_eq!(again[0].original_name, "Planeten");
}

#[test]
fn exports_are_reproducible_and_self_describing() {
    let a = tempfile::tempdir().unwrap();
    let store = sample(a.path());
    let first = exported(&store, &["Planeten", "Vortrag"]);
    let second = exported(&store, &["Planeten", "Vortrag"]);
    assert_eq!(first, second, "same content, same bytes");
    // `file`-style detection: the archive starts with the stored mimetype.
    assert_eq!(&first[30..38], b"mimetype");
    assert_eq!(&first[38..38 + MEDIA_TYPE.len()], MEDIA_TYPE.as_bytes());
    let mut archive = ZipArchive::new(Cursor::new(&first)).unwrap();
    assert_eq!(
        archive.by_index(1).unwrap().name().unwrap(),
        "manifest.json"
    );
    let manifest: serde_json::Value = {
        let mut text = String::new();
        archive
            .by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        serde_json::from_str(&text).unwrap()
    };
    assert!(
        manifest["createdBy"]
            .as_str()
            .unwrap()
            .starts_with("locora ")
    );
    assert!(
        manifest["media"][0]["mediaType"].is_string() && manifest["media"][0]["sha256"].is_string()
    );
}

#[test]
fn broken_and_hostile_files_are_rejected() {
    let a = tempfile::tempdir().unwrap();
    let store = sample(a.path());
    let good = exported(&store, &["Planeten", "Vortrag"]);

    let err = open_err(rewrite(&good, |mut e| {
        edit_json(&mut e, "manifest.json", |m| m["version"] = 2.into());
        e
    }));
    assert!(err.contains("neuere Locora-Version"), "{err}");

    let err = open_err(rewrite(&good, |mut e| {
        edit_json(&mut e, "manifest.json", |m| m["surprise"] = true.into());
        e
    }));
    assert!(err.contains("unknown field"), "{err}");

    let err = open_err(rewrite(&good, |e| {
        e.into_iter().filter(|(n, _)| n != "mimetype").collect()
    }));
    assert!(err.contains("mimetype"), "{err}");

    let err = open_err(rewrite(&good, |mut e| {
        e.push(("notes.txt".into(), b"hi".to_vec()));
        e
    }));
    assert!(err.contains("unbekannte Datei"), "{err}");

    let err = open_err(rewrite(&good, |mut e| {
        e.push(("../evil".into(), b"x".to_vec()));
        e
    }));
    assert!(err.contains("unsicherer Pfad"), "{err}");

    let err = open_err(rewrite(&good, |mut e| {
        edit_json(&mut e, "manifest.json", |m| {
            m["palaces"][0]["id"] = "../x".into()
        });
        e
    }));
    assert!(err.contains("ungültige Palast-Id"), "{err}");

    let err = open_err(rewrite(&good, |e| {
        e.into_iter()
            .filter(|(n, _)| !n.ends_with(".mp4"))
            .collect()
    }));
    assert!(err.contains(".mp4 fehlt"), "{err}");

    let err = open_err(rewrite(&good, |mut e| {
        edit_json(&mut e, "palace", |p| {
            p["rooms"][0]["loci"][0]["x"] = 1.5.into()
        });
        e
    }));
    assert!(err.contains("außerhalb des Raums"), "{err}");

    let err = open_err(rewrite(&good, |mut e| {
        edit_json(&mut e, "palace", |p| {
            p["rooms"][0]["loci"][0]["time"] = 3.into()
        });
        e
    }));
    assert!(err.contains("kein Video"), "{err}");

    // Plugin data is reserved and ignored by version 1.
    let with_plugin = rewrite(&good, |mut e| {
        e.push(("plugins/review/cards.json".into(), b"{}".to_vec()));
        e
    });
    assert!(Package::open(Cursor::new(with_plugin)).is_ok());

    // Same size, different content: caught while importing, before any palace exists.
    let tampered = rewrite(&good, |e| {
        e.into_iter()
            .map(|(n, d)| {
                if n.ends_with(".png") {
                    (n, b"\x89PNG FAKE".to_vec())
                } else {
                    (n, d)
                }
            })
            .collect()
    });
    let target_dir = tempfile::tempdir().unwrap();
    let target = Store::open(target_dir.path()).unwrap();
    let err = Package::open(Cursor::new(tampered))
        .unwrap()
        .import(&target)
        .unwrap_err()
        .to_string();
    assert!(err.contains("passt nicht zum SHA-256"), "{err}");
    assert!(target.palaces().unwrap().is_empty());
}

#[test]
fn video_loci_need_times() {
    let a = tempfile::tempdir().unwrap();
    let store = sample(a.path());
    let good = exported(&store, &["Vortrag"]);
    let err = open_err(rewrite(&good, |mut e| {
        edit_json(&mut e, "palace", |p| {
            p["rooms"][0]["loci"][0]
                .as_object_mut()
                .unwrap()
                .remove("time");
        });
        e
    }));
    assert!(err.contains("hat keine Zeit"), "{err}");
}

#[test]
fn tables_and_bindings_travel_and_clashes_stay_local() {
    let a = tempfile::tempdir().unwrap();
    let source = Store::open(a.path()).unwrap();
    let p = source.create_palace("Zahlen").unwrap();
    let r = source
        .add_room(&p.id, "Raum", RoomKind::Blank, None)
        .unwrap();
    let l = source
        .add_locus(
            &r.id,
            NewLocus {
                place: "Tür".into(),
                item: "?".into(),
                x: 0.5,
                y: 0.5,
                time: None,
            },
        )
        .unwrap();
    source
        .set_binding(
            &l.id,
            &Binding {
                field: BindingField::Item,
                expression: "Major[\"7\"]".into(),
            },
        )
        .unwrap();
    source
        .set_row(None, &Row::value("Major", "7", "Kuh"))
        .unwrap();
    source
        .set_row(Some(&p.id), &Row::value("Inputs", "zahl", "47"))
        .unwrap();
    let bytes = exported(&source, &["Zahlen"]);

    // The target already has a different shared Major.7.
    let b = tempfile::tempdir().unwrap();
    let target = Store::open(b.path()).unwrap();
    target
        .set_row(None, &Row::value("Major", "7", "Kaffee"))
        .unwrap();
    let package = Package::open(Cursor::new(bytes.clone())).unwrap();
    assert_eq!(package.shared_tables.len(), 1);
    assert_eq!(
        package.palaces[0].rooms[0].loci[0].bindings.item.as_deref(),
        Some("Major[\"7\"]")
    );
    let imported = package.import(&target).unwrap();
    assert_eq!(imported[0].localized_rows, 1);
    let copy = target.palace("Zahlen").unwrap();
    assert_eq!(
        copy.route().next().unwrap().bindings[0].expression,
        "Major[\"7\"]"
    );
    let rows = target.effective_rows(&copy.palace.id).unwrap();
    let major = rows.iter().find(|r| r.table == "Major").unwrap();
    assert_eq!(
        major.cells[0].value, "Kuh",
        "the palace keeps its own value"
    );
    assert_eq!(
        target.rows(None).unwrap()[0].cells[0].value,
        "Kaffee",
        "existing shared rows are untouched"
    );
    assert!(rows.iter().any(|r| r.table == "Inputs"));

    let err = open_err(rewrite(&bytes, |mut e| {
        edit_json(&mut e, "palace", |p| {
            p["rooms"][0]["loci"][0]["bindings"]["item"] = "1 +".into()
        });
        e
    }));
    assert!(err.contains("Formel für item"), "{err}");
    let err = open_err(rewrite(&bytes, |mut e| {
        edit_json(&mut e, "palace", |p| {
            p["rooms"][0]["loci"][0]["bindings"]["colour"] = "1".into()
        });
        e
    }));
    assert!(err.contains("unknown field"), "{err}");
    let err = open_err(rewrite(&bytes, |mut e| {
        edit_json(&mut e, "palace", |p| {
            let row = p["tables"][0].clone();
            p["tables"].as_array_mut().unwrap().push(row);
        });
        e
    }));
    assert!(err.contains("doppelt"), "{err}");
}
