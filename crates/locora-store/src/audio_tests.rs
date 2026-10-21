//! A sound clip per locus: stored, changed, removed, checked and synced.
use super::*;

fn locus(place: &str) -> NewLocus {
    NewLocus {
        audio: None,
        image_size: default_image_size(),
        zoom: default_zoom(),
        place: place.into(),
        item: "Merkur".into(),
        x: 0.5,
        y: 0.5,
        time: None,
        answer: Default::default(),
        image: None,
        note: String::new(),
    }
}

fn first(store: &Store) -> Locus {
    store
        .palace("Klänge")
        .unwrap()
        .route()
        .next()
        .unwrap()
        .clone()
}

#[test]
fn loci_keep_a_sound_clip_across_reopen_patch_and_removal() {
    let dir = tempfile::tempdir().unwrap();
    let (mp3, wav, id) = {
        let store = Store::open(dir.path()).unwrap();
        let mp3 = store.put_media(b"ID3 fake", "audio/mpeg").unwrap();
        let wav = store.put_media(b"RIFF fake", "audio/wav").unwrap();
        let p = store.create_palace("Klänge").unwrap();
        let r = store
            .add_room(&p.id, "Raum", RoomKind::Blank, None)
            .unwrap();
        let added = store
            .add_locus(
                &r.id,
                NewLocus {
                    audio: Some(mp3.hash.clone()),
                    ..locus("Tür")
                },
            )
            .unwrap();
        assert_eq!(added.audio.as_deref(), Some(mp3.hash.as_str()));
        (mp3.hash, wav.hash, added.id)
    };
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(first(&store).audio.as_deref(), Some(mp3.as_str()));
    // A patch without `audio` keeps it; the picture is independent of it.
    store
        .update_locus(
            &id,
            &LocusPatch {
                item: Some("Venus".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(first(&store).audio.as_deref(), Some(mp3.as_str()));
    let changed = store
        .update_locus(
            &id,
            &LocusPatch {
                audio: Some(Some(wav.clone())),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(changed.audio.as_deref(), Some(wav.as_str()));
    assert_eq!(first(&store).audio.as_deref(), Some(wav.as_str()));
    let removed = store
        .update_locus(
            &id,
            &LocusPatch {
                audio: Some(None),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(removed.audio, None);
    assert_eq!(first(&store).audio, None);
    // `null` in a patch means "remove", not "keep".
    let patch: LocusPatch = serde_json::from_str(r#"{"audio":null}"#).unwrap();
    assert_eq!(patch.audio, Some(None));
}

#[test]
fn a_sound_clip_must_be_stored_sound() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let png = store.put_media(b"\x89PNG fake", "image/png").unwrap();
    let ogg = store.put_media(b"OggS fake", "audio/ogg").unwrap();
    let p = store.create_palace("Klänge").unwrap();
    let r = store
        .add_room(&p.id, "Raum", RoomKind::Blank, None)
        .unwrap();
    for wrong in [png.hash.clone(), "0".repeat(64)] {
        let added = store.add_locus(
            &r.id,
            NewLocus {
                audio: Some(wrong),
                ..locus("Tür")
            },
        );
        assert!(added.is_err());
    }
    let l = store.add_locus(&r.id, locus("Tür")).unwrap();
    let err = store
        .update_locus(
            &l.id,
            &LocusPatch {
                audio: Some(Some(png.hash.clone())),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("Ton"), "{err}");
    // A sound clip is not a picture either.
    assert!(
        store
            .update_locus(
                &l.id,
                &LocusPatch {
                    image: Some(Some(ogg.hash.clone())),
                    ..Default::default()
                },
            )
            .is_err()
    );
    assert!(is_audio("audio/mp4") && !is_audio("image/png") && !is_audio("audio/webm"));
}

#[test]
fn sound_clips_sync_with_their_media() {
    let (a_dir, b_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let a = Store::open(&a_dir.path().join("p")).unwrap();
    let b = Store::open(&b_dir.path().join("p")).unwrap();
    let clip = a.put_media(b"ID3 clip", "audio/mpeg").unwrap();
    let p = a.create_palace("Klänge").unwrap();
    let r = a.add_room(&p.id, "Raum", RoomKind::Blank, None).unwrap();
    let l = a
        .add_locus(
            &r.id,
            NewLocus {
                audio: Some(clip.hash.clone()),
                ..locus("Tür")
            },
        )
        .unwrap();
    let ops: Vec<RemoteOp> = a
        .ops_since(0)
        .unwrap()
        .into_iter()
        .map(|op| RemoteOp {
            hlc: op.hlc,
            entity: op.entity,
            entity_id: op.entity_id,
            op: op.op,
            data: op.data,
        })
        .collect();
    let refs: Vec<String> = ops.iter().flat_map(media_refs).collect();
    assert!(
        refs.contains(&clip.hash),
        "the clip travels before its locus"
    );
    // Without the media the locus cannot be applied; with it, it can.
    assert!(!b.apply_remote(&ops).unwrap().failed.is_empty());
    b.put_media(b"ID3 clip", "audio/mpeg").unwrap();
    b.apply_remote(&ops).unwrap();
    assert_eq!(first(&b).audio.as_deref(), Some(clip.hash.as_str()));

    let since = a.ops_since(0).unwrap().last().unwrap().seq;
    a.update_locus(
        &l.id,
        &LocusPatch {
            audio: Some(None),
            ..Default::default()
        },
    )
    .unwrap();
    let update: Vec<RemoteOp> = a
        .ops_since(since)
        .unwrap()
        .into_iter()
        .map(|op| RemoteOp {
            hlc: op.hlc,
            entity: op.entity,
            entity_id: op.entity_id,
            op: op.op,
            data: op.data,
        })
        .collect();
    assert_eq!(b.apply_remote(&update).unwrap().applied, 1);
    assert_eq!(first(&b).audio, None);
}
