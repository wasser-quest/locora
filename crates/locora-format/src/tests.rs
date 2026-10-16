use std::io::Cursor;

use super::*;

fn sample(dir: &std::path::Path) -> Store {
    let store = Store::open(dir).unwrap();
    let image = store.put_media(b"\x89PNG fake", "image/png").unwrap();
    let video = store.put_media(b"fake mp4", "video/mp4").unwrap();
    let p = store.create_palace("Planeten").unwrap();
    let s = store
        .add_stage(&p.id, "Wohnung", StageKind::Image, Some(&image.hash))
        .unwrap();
    for (i, m) in ["Merkur", "Venus"].iter().enumerate() {
        store
            .add_locus(
                &s.id,
                NewLocus {
                    label: format!("Ort {i}"),
                    memory: m.to_string(),
                    x: 0.2 + 0.3 * i as f64,
                    y: 0.5,
                    at: None,
                },
            )
            .unwrap();
    }
    let v = store.create_palace("Vortrag").unwrap();
    let vs = store
        .add_stage(&v.id, "Talk", StageKind::Video, Some(&video.hash))
        .unwrap();
    store
        .add_locus(
            &vs.id,
            NewLocus {
                label: "Tür".into(),
                memory: "These".into(),
                x: 0.5,
                y: 0.5,
                at: Some(83.0),
            },
        )
        .unwrap();
    // A second stage reusing the same image is exported once.
    store
        .add_stage(&p.id, "Flur", StageKind::Image, Some(&image.hash))
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
        out.start_file(name.as_str(), SimpleFileOptions::default())
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
    assert_eq!(PalaceDoc::from(&copy).stages.len(), 2);
    let memories: Vec<_> = copy
        .route()
        .map(|l| (l.label.clone(), l.memory.clone(), l.x))
        .collect();
    let expected: Vec<_> = original
        .route()
        .map(|l| (l.label.clone(), l.memory.clone(), l.x))
        .collect();
    assert_eq!(memories, expected);
    let hash = copy.stages[0].stage.media.clone().unwrap();
    assert_eq!(
        std::fs::read(target.media_file(&hash).unwrap()).unwrap(),
        b"\x89PNG fake"
    );
    assert_eq!(
        target.palace("Vortrag").unwrap().route().next().unwrap().at,
        Some(83.0)
    );

    // Importing again keeps both: „Planeten (2)“.
    let again = Package::open(Cursor::new(bytes))
        .unwrap()
        .import(&target)
        .unwrap();
    assert_eq!(again[0].name, "Planeten (2)");
    assert_eq!(again[0].original_name, "Planeten");
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

    let err = open_err(rewrite(&good, |e| {
        e.into_iter()
            .filter(|(n, _)| !n.ends_with(".mp4"))
            .collect()
    }));
    assert!(err.contains(".mp4 fehlt"), "{err}");

    let err = open_err(rewrite(&good, |mut e| {
        edit_json(&mut e, "palace", |p| {
            p["stages"][0]["loci"][0]["x"] = 1.5.into()
        });
        e
    }));
    assert!(err.contains("außerhalb der Stage"), "{err}");

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
    assert!(err.contains("passt nicht zum Hash"), "{err}");
    assert!(target.palaces().unwrap().is_empty());
}

#[test]
fn video_loci_need_times_and_blank_stages_no_media() {
    let a = tempfile::tempdir().unwrap();
    let store = sample(a.path());
    let good = exported(&store, &["Vortrag"]);
    let err = open_err(rewrite(&good, |mut e| {
        edit_json(&mut e, "palace", |p| {
            p["stages"][0]["loci"][0]
                .as_object_mut()
                .unwrap()
                .remove("at");
        });
        e
    }));
    assert!(err.contains("hat keine Zeit"), "{err}");
    let err = open_err(rewrite(&good, |mut e| {
        edit_json(&mut e, "palace", |p| {
            p["stages"][0]["kind"] = "blank".into()
        });
        e
    }));
    assert!(err.contains("ohne Bild hat ein Medium"), "{err}");
}
