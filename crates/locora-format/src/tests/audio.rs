//! Locus sound clips: format 4, stored as media, checked on reading.
use super::*;

/// The sample palace with a sound clip at its first locus; returns the store and the clip's hash.
fn with_sound(dir: &std::path::Path) -> (Store, String) {
    with_sound_as(dir, b"RIFF fake wav", "audio/wav")
}

fn with_sound_as(dir: &std::path::Path, bytes: &[u8], mime: &str) -> (Store, String) {
    let store = sample(dir);
    let sound = store.put_media(bytes, mime).unwrap();
    let id = store
        .palace("Planeten")
        .unwrap()
        .route()
        .next()
        .unwrap()
        .id
        .clone();
    store
        .update_locus(
            &id,
            &locora_store::LocusPatch {
                audio: Some(Some(sound.hash.clone())),
                ..Default::default()
            },
        )
        .unwrap();
    (store, sound.hash)
}

#[test]
fn sound_clips_travel_with_format_4() {
    let a = tempfile::tempdir().unwrap();
    let (store, hash) = with_sound(a.path());
    let bytes = exported(&store, &["Planeten"]);
    let package = Package::open(Cursor::new(bytes.clone())).unwrap();
    assert_eq!(package.manifest.version, 4, "locus sound needs format 4");
    assert_eq!(
        package.palaces[0].rooms[0].loci[0].audio.as_deref(),
        Some(hash.as_str())
    );
    assert!(
        package
            .manifest
            .media
            .iter()
            .any(|m| m.sha256 == hash && m.media_type == "audio/wav")
    );
    let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    assert!(archive.by_name(&format!("media/{hash}.wav")).is_ok());

    let b = tempfile::tempdir().unwrap();
    let target = Store::open(b.path()).unwrap();
    package.import(&target).unwrap();
    let copy = target.palace("Planeten").unwrap();
    let locus = copy.route().next().unwrap();
    assert_eq!(locus.audio.as_deref(), Some(hash.as_str()));
    assert_eq!(target.media(&hash).unwrap().unwrap().mime, "audio/wav");
    assert_eq!(copy.route().nth(1).unwrap().audio, None);
}

#[test]
fn sound_clips_are_checked_on_reading() {
    let a = tempfile::tempdir().unwrap();
    let (store, _) = with_sound(a.path());
    let tree = store.palace("Planeten").unwrap();
    let bytes = exported(&store, &["Planeten"]);
    let palace_file = format!("palaces/{}.json", tree.palace.id);
    let backdrop = tree.rooms[0].room.media.clone().unwrap();

    let err = open_err(rewrite(&bytes, |mut entries| {
        edit_json(&mut entries, "manifest.json", |m| m["version"] = 3.into());
        entries
    }));
    assert!(err.contains("Format 4"), "{err}");
    let err = open_err(rewrite(&bytes, |mut entries| {
        edit_json(&mut entries, &palace_file, |p| {
            p["rooms"][0]["loci"][0]["audio"] = backdrop.clone().into();
        });
        entries
    }));
    assert!(err.contains("kein Ton"), "{err}");
    let err = open_err(rewrite(&bytes, |mut entries| {
        edit_json(&mut entries, &palace_file, |p| {
            p["rooms"][0]["loci"][0]["audio"] = "e".repeat(64).into();
        });
        entries
    }));
    assert!(err.contains("Ton") && err.contains("fehlt"), "{err}");
    let sound = store
        .palace("Planeten")
        .unwrap()
        .route()
        .next()
        .unwrap()
        .audio
        .clone();
    let err = open_err(rewrite(&bytes, |mut entries| {
        edit_json(&mut entries, &palace_file, |p| {
            p["rooms"][0]["backdrop"] = sound.clone().into();
        });
        entries
    }));
    assert!(err.contains("Hintergrund"), "{err}");
}

#[test]
fn webm_flac_and_aac_sound_need_format_5() {
    for (mime, extension) in [
        ("audio/webm", "weba"),
        ("audio/flac", "flac"),
        ("audio/aac", "aac"),
    ] {
        let a = tempfile::tempdir().unwrap();
        let (store, hash) = with_sound_as(a.path(), b"fake sound", mime);
        let bytes = exported(&store, &["Planeten"]);
        let package = Package::open(Cursor::new(bytes.clone())).unwrap();
        assert_eq!(package.manifest.version, 5, "{mime}");
        let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        assert!(
            archive
                .by_name(&format!("media/{hash}.{extension}"))
                .is_ok()
        );
        let b = tempfile::tempdir().unwrap();
        let target = Store::open(b.path()).unwrap();
        package.import(&target).unwrap();
        assert_eq!(target.media(&hash).unwrap().unwrap().mime, mime);

        let err = open_err(rewrite(&bytes, |mut entries| {
            edit_json(&mut entries, "manifest.json", |m| m["version"] = 4.into());
            entries
        }));
        assert!(err.contains(mime), "{err}");
    }
}
