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
                    image_size: locora_store::default_image_size(),
                    zoom: locora_store::default_zoom(),
                    place: format!("Ort {i}"),
                    item: item.to_string(),
                    x: 0.2 + 0.3 * i as f64,
                    y: 0.5,
                    time: None,
                    answer: Default::default(),
                    image: None,
                    note: String::new(),
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
                image_size: locora_store::default_image_size(),
                zoom: locora_store::default_zoom(),
                place: "Tür".into(),
                item: "These".into(),
                x: 0.5,
                y: 0.5,
                time: Some(83.0),
                answer: Default::default(),
                image: None,
                note: String::new(),
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
        edit_json(&mut e, "manifest.json", |m| {
            m["version"] = (VERSION + 1).into()
        });
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
                image_size: locora_store::default_image_size(),
                zoom: locora_store::default_zoom(),
                place: "Tür".into(),
                item: "?".into(),
                x: 0.5,
                y: 0.5,
                time: None,
                answer: Default::default(),
                image: None,
                note: String::new(),
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

#[test]
fn answer_versions_round_trip_and_fail_closed() {
    use locora_store::{Answer, AnswerKind, LocusPatch, QuizOption};
    let dir = tempfile::tempdir().unwrap();
    let store = sample(dir.path());
    let v1 = exported(&store, &["Planeten"]);
    let legacy = Package::open(Cursor::new(v1)).unwrap();
    assert_eq!(legacy.manifest.version, 1);
    assert!(legacy.palaces[0].rooms[0].loci[0].answer.is_recall());
    let tree = store.palace("Planeten").unwrap();
    let answers = [
        Answer {
            kind: AnswerKind::Guess,
            accept: vec!["Hermes".into()],
            ..Answer::default()
        },
        Answer {
            kind: AnswerKind::Quiz,
            options: vec![
                QuizOption {
                    text: "a".into(),
                    correct: true,
                },
                QuizOption {
                    text: "b".into(),
                    correct: false,
                },
                QuizOption {
                    text: "c".into(),
                    correct: true,
                },
            ],
            ..Answer::default()
        },
    ];
    for (l, answer) in tree.route().zip(&answers) {
        store
            .update_locus(
                &l.id,
                &LocusPatch {
                    answer: Some(answer.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let bytes = exported(&store, &["Planeten"]);
    assert_eq!(bytes, exported(&store, &["Planeten"]));
    let package = Package::open(Cursor::new(&bytes)).unwrap();
    assert_eq!(package.manifest.version, 2);
    let target_dir = tempfile::tempdir().unwrap();
    let target = Store::open(target_dir.path()).unwrap();
    package.import(&target).unwrap();
    let imported = target.palace("Planeten").unwrap();
    assert_eq!(
        imported
            .route()
            .map(|l| l.answer.clone())
            .collect::<Vec<_>>(),
        answers
    );
    let wrong_version = rewrite(&bytes, |mut entries| {
        edit_json(&mut entries, "manifest.json", |m| m["version"] = 1.into());
        entries
    });
    assert!(open_err(wrong_version).contains("Format 2"));
    let invalid = rewrite(&bytes, |mut entries| {
        edit_json(&mut entries, "palace", |p| {
            p["rooms"][0]["loci"][1]["answer"]["options"][1]["text"] = " A ".into()
        });
        entries
    });
    assert!(open_err(invalid).contains("eindeutig"));
}

#[test]
fn plugin_files_travel_and_paths_are_checked() {
    let a = tempfile::tempdir().unwrap();
    let store = sample(a.path());
    let entry = PluginEntry {
        id: "review".into(),
        version: "0.1.0".into(),
        required: false,
    };
    let file = PluginFile {
        plugin: entry.clone(),
        path: "progress.json".into(),
        bytes: b"{\"version\":1}".to_vec(),
    };
    let names = vec!["Planeten".to_string()];
    let write = |files: &[PluginFile]| {
        let mut out = Cursor::new(Vec::new());
        export_with(&store, &names, files, &mut out).map(|_| out.into_inner())
    };
    let bytes = write(std::slice::from_ref(&file)).unwrap();
    assert_eq!(
        bytes,
        write(std::slice::from_ref(&file)).unwrap(),
        "still reproducible"
    );
    let mut package = Package::open(Cursor::new(bytes)).unwrap();
    assert_eq!(package.manifest.plugins, vec![entry.clone()]);
    assert_eq!(
        package
            .plugin_file("review", "progress.json")
            .unwrap()
            .unwrap(),
        b"{\"version\":1}"
    );
    assert_eq!(package.plugin_file("review", "missing.json").unwrap(), None);
    assert!(package.plugin_file("review", "../manifest.json").is_err());

    for path in ["../x", "a//b", "", "x\\y", "./x"] {
        let bad = PluginFile {
            path: path.into(),
            ..file.clone()
        };
        assert!(write(&[bad]).is_err(), "{path:?}");
    }
    let bad_id = PluginFile {
        plugin: PluginEntry {
            id: "../x".into(),
            ..entry
        },
        ..file
    };
    assert!(write(&[bad_id]).is_err());
}

#[test]
fn splinely_animations_as_backdrops_and_locus_pictures() {
    let a = tempfile::tempdir().unwrap();
    let store = Store::open(a.path()).unwrap();
    let scene = store
        .put_media(b"\x89SPLY\r\n\x1a scene", locora_store::SPLINELY)
        .unwrap();
    let simo = store
        .put_media(b"\x89SPLY\r\n\x1a simo", locora_store::SPLINELY)
        .unwrap();
    let p = store.create_palace("Tutorial").unwrap();
    let r = store
        .add_room(&p.id, "Eingang", RoomKind::Image, Some(&scene.hash))
        .unwrap();
    store
        .add_locus(
            &r.id,
            NewLocus {
                image_size: locora_store::default_image_size(),
                zoom: locora_store::default_zoom(),
                place: "Tor".into(),
                item: "Ein Palast ist eine Route".into(),
                x: 0.5,
                y: 0.5,
                time: None,
                answer: Default::default(),
                image: Some(simo.hash.clone()),
                note: String::new(),
            },
        )
        .unwrap();
    let bytes = exported(&store, &["Tutorial"]);
    let package = Package::open(Cursor::new(bytes.clone())).unwrap();
    assert_eq!(package.manifest.version, 2, "locus pictures need format 2");
    assert!(
        package
            .manifest
            .media
            .iter()
            .all(|m| m.media_type == locora_store::SPLINELY)
    );
    let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    assert!(
        archive
            .by_name(&format!("media/{}.sply", simo.hash))
            .is_ok()
    );

    let b = tempfile::tempdir().unwrap();
    let target = Store::open(b.path()).unwrap();
    package.import(&target).unwrap();
    let copy = target.palace("Tutorial").unwrap();
    assert_eq!(copy.rooms[0].room.kind, RoomKind::Image);
    assert_eq!(
        copy.route().next().unwrap().image.as_deref(),
        Some(simo.hash.as_str())
    );

    let err = open_err(rewrite(&bytes, |mut e| {
        edit_json(&mut e, "manifest.json", |m| m["version"] = 1.into());
        e
    }));
    assert!(err.contains("Format 2"), "{err}");
    let err = open_err(rewrite(&bytes, |mut e| {
        edit_json(&mut e, "palace", |p| {
            p["rooms"][0]["loci"][0]["image"] = "f".repeat(64).into()
        });
        e
    }));
    assert!(err.contains("Bild") && err.contains("fehlt"), "{err}");
}

#[test]
fn custom_markers_travel_and_must_be_splinely() {
    let a = tempfile::tempdir().unwrap();
    let store = Store::open(a.path()).unwrap();
    let marker = store
        .put_media(b"\x89SPLY\r\n\x1a marker", locora_store::SPLINELY)
        .unwrap();
    let p = store.create_palace("Markiert").unwrap();
    store
        .add_room(&p.id, "Raum", RoomKind::Blank, None)
        .unwrap();
    store.set_marker(&p.id, Some(&marker.hash)).unwrap();
    let bytes = exported(&store, &["Markiert"]);
    let package = Package::open(Cursor::new(bytes.clone())).unwrap();
    assert_eq!(package.manifest.version, 2);
    let b = tempfile::tempdir().unwrap();
    let target = Store::open(b.path()).unwrap();
    package.import(&target).unwrap();
    assert_eq!(
        target.palace("Markiert").unwrap().palace.marker.as_deref(),
        Some(marker.hash.as_str())
    );

    let err = open_err(rewrite(&bytes, |mut e| {
        edit_json(&mut e, "manifest.json", |m| {
            m["media"][0]["mediaType"] = "image/png".into()
        });
        e.into_iter()
            .map(|(n, d)| (n.replace(".sply", ".png"), d))
            .collect()
    }));
    assert!(err.contains("muss eine .sply-Animation sein"), "{err}");
}

#[test]
fn image_size_roundtrip_and_version_compatibility() {
    let dir = tempfile::tempdir().unwrap();
    let store = sample(dir.path());
    let tree = store.palace("Planeten").unwrap();
    let id = tree.route().next().unwrap().id.clone();
    let legacy = exported(&store, &["Planeten"]);
    let legacy_package = Package::open(Cursor::new(&legacy)).unwrap();
    assert_eq!(legacy_package.manifest.version, 1);
    assert_eq!(legacy_package.palaces[0].rooms[0].loci[0].image_size, 0.25);
    store
        .update_locus(
            &id,
            &locora_store::LocusPatch {
                image_size: Some(0.4),
                ..Default::default()
            },
        )
        .unwrap();
    let bytes = exported(&store, &["Planeten"]);
    let package = Package::open(Cursor::new(&bytes)).unwrap();
    assert_eq!(package.manifest.version, 3);
    assert_eq!(package.palaces[0].rooms[0].loci[0].image_size, 0.4);
    let copy_dir = tempfile::tempdir().unwrap();
    let copy = Store::open(copy_dir.path()).unwrap();
    package.import(&copy).unwrap();
    assert_eq!(
        copy.palace("Planeten")
            .unwrap()
            .route()
            .next()
            .unwrap()
            .image_size,
        0.4
    );
    for version in [1, 2] {
        let err = open_err(rewrite(&bytes, |mut entries| {
            edit_json(&mut entries, "manifest.json", |m| {
                m["version"] = version.into()
            });
            entries
        }));
        assert!(err.contains("benötigen Format 3"), "{err}");
    }
    let palace_file = format!("palaces/{}.json", tree.palace.id);
    for size in [0.0, 0.049, 1.01, -0.1] {
        let err = open_err(rewrite(&bytes, |mut entries| {
            edit_json(&mut entries, &palace_file, |p| {
                p["rooms"][0]["loci"][0]["imageSize"] = size.into();
            });
            entries
        }));
        assert!(err.contains("Bildgröße"), "{err}");
    }
}

#[test]
fn zoom_needs_format_3_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let store = sample(dir.path());
    let tree = store.palace("Planeten").unwrap();
    let id = tree.route().next().unwrap().id.clone();
    store
        .update_locus(
            &id,
            &locora_store::LocusPatch {
                zoom: Some(2.0),
                ..Default::default()
            },
        )
        .unwrap();
    let bytes = exported(&store, &["Planeten"]);
    let package = Package::open(Cursor::new(&bytes)).unwrap();
    assert_eq!(package.manifest.version, 3);
    assert_eq!(package.palaces[0].rooms[0].loci[0].zoom, 2.0);
    let err = open_err(rewrite(&bytes, |mut entries| {
        edit_json(&mut entries, "manifest.json", |m| m["version"] = 2.into());
        entries
    }));
    assert!(err.contains("Format 3"), "{err}");
    let palace_file = format!("palaces/{}.json", tree.palace.id);
    let err = open_err(rewrite(&bytes, |mut entries| {
        edit_json(&mut entries, &palace_file, |p| {
            p["rooms"][0]["loci"][0]["zoom"] = 9.0.into();
        });
        entries
    }));
    assert!(err.contains("Zoom"), "{err}");
}
