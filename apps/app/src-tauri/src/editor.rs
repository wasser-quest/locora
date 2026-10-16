//! Scoped editor mutations. All domain validation remains in the store.
use anyhow::{Context, Result, ensure};
use locora_app_api::{EditorAction, EditorChange};
use locora_store::{LocusPatch, NewLocus, PalaceTree, RoomKind, Store};

/// Check exact ownership before mutating; IPC must not target another room.
/// Return a snapshot assembled from the successful store write, without a
/// fallible post-commit reload that could invite replaying a completed mutation.
pub fn apply(store: &Store, change: &EditorChange) -> Result<(PalaceTree, Option<String>)> {
    let mut tree = store.palace(&change.palace_id)?;
    ensure!(
        tree.palace.id == change.palace_id,
        "Palast-ID muss vollständig sein"
    );
    let room = tree
        .rooms
        .iter_mut()
        .find(|r| r.room.id == change.room_id)
        .context("Raum gehört nicht zu diesem Palast")?;
    let id = match &change.action {
        EditorAction::Create { x, y } => {
            ensure!(
                room.room.kind != RoomKind::Video,
                "Neue Orte in Video-Räumen benötigen eine Zeit; bitte die CLI verwenden"
            );
            let locus = store.add_locus(
                &room.room.id,
                NewLocus {
                    place: "Neuer Ort".into(),
                    item: String::new(),
                    x: *x,
                    y: *y,
                    time: None,
                    answer: Default::default(),
                    image: None,
                    image_size: locora_store::default_image_size(),
                    note: String::new(),
                },
            )?;
            let id = locus.id.clone();
            room.loci.push(locus);
            return Ok((tree, Some(id)));
        }
        EditorAction::Paste { items } => {
            ensure!(
                room.room.kind != RoomKind::Video,
                "Listen sind in Video-Räumen noch nicht verfügbar"
            );
            locora_app_api::validate_paste_items(items).map_err(anyhow::Error::msg)?;
            let loci = items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let (x, y) = locora_app_api::paste_route_position(index, items.len());
                    NewLocus {
                        place: format!(
                            "Ort {}",
                            room.loci
                                .iter()
                                .map(|l| l.position)
                                .max()
                                .map_or(0, |p| p + 1) as usize
                                + index
                                + 1
                        ),
                        item: item.trim().to_owned(),
                        x,
                        y,
                        time: None,
                        answer: Default::default(),
                        image: None,
                        image_size: locora_store::default_image_size(),
                        note: String::new(),
                    }
                })
                .collect();
            let added = store.add_loci(&room.room.id, loci)?;
            let selected = added.first().map(|l| l.id.clone());
            room.loci.extend(added);
            return Ok((tree, selected));
        }
        EditorAction::Save { id, .. }
        | EditorAction::Move { id, .. }
        | EditorAction::Resize { id, .. }
        | EditorAction::Delete { id } => id,
    };
    let locus = room
        .loci
        .iter_mut()
        .find(|l| &l.id == id)
        .context("Ort gehört nicht zu diesem Raum")?;
    let patch = match &change.action {
        EditorAction::Save { content, .. } => {
            ensure!(
                locus.bindings.is_empty(),
                "Dieser Ort hat Workbook-Formeln; Inhalte bitte über die Quelldatei bearbeiten"
            );
            LocusPatch {
                place: Some(content.place.clone()),
                item: Some(content.item.clone()),
                note: Some(content.note.clone()),
                image: Some(content.image.clone()),
                image_size: Some(content.image_size),
                answer: Some(content.answer.clone()),
                ..Default::default()
            }
        }
        EditorAction::Move { x, y, .. } => LocusPatch {
            x: Some(*x),
            y: Some(*y),
            ..Default::default()
        },
        EditorAction::Resize { image_size, .. } => LocusPatch {
            image_size: Some(*image_size),
            ..Default::default()
        },
        EditorAction::Delete { .. } => {
            store.delete_locus(id)?;
            room.loci.retain(|l| &l.id != id);
            return Ok((tree, None));
        }
        EditorAction::Create { .. } | EditorAction::Paste { .. } => unreachable!(),
    };
    let mut updated = store.update_locus(id, &patch)?;
    updated.bindings = locus.bindings.clone();
    *locus = updated;
    Ok((tree, Some(id.clone())))
}

/// Validate ownership before opening a dialog or reading any selected file.
pub fn picture_target(store: &Store, target: &locora_app_api::PictureTarget) -> Result<()> {
    let tree = store.palace(&target.palace_id)?;
    ensure!(
        tree.palace.id == target.palace_id,
        "Palast-ID muss vollständig sein"
    );
    let room = tree
        .rooms
        .iter()
        .find(|r| r.room.id == target.room_id)
        .context("Raum gehört nicht zu diesem Palast")?;
    let locus = room
        .loci
        .iter()
        .find(|l| l.id == target.locus_id)
        .context("Ort gehört nicht zu diesem Raum")?;
    ensure!(
        locus.bindings.is_empty(),
        "Workbook-Ort: Bild bitte in der Quelldatei ändern"
    );
    Ok(())
}

/// Backend-only path obtained from the native dialog, never an IPC argument.
pub fn import_picture(
    store: &Store,
    target: &locora_app_api::PictureTarget,
    path: &std::path::Path,
) -> Result<(PalaceTree, Option<String>)> {
    picture_target(store, target)?;
    let (_, mime, bytes) = read_selected_media(path, true)?;
    // Existing content-addressed bytes may have been registered under another MIME.
    let media = store.put_media(&bytes, mime)?;
    ensure!(
        store.media(&media.hash)?.is_some_and(|m| m.mime == mime),
        "Gespeicherter Medientyp passt nicht zum Dateiformat"
    );
    let mut tree = store.palace(&target.palace_id)?;
    let room = tree
        .rooms
        .iter_mut()
        .find(|r| r.room.id == target.room_id)
        .context("Raum gehört nicht zu diesem Palast")?;
    let locus = room
        .loci
        .iter_mut()
        .find(|l| l.id == target.locus_id)
        .context("Ort gehört nicht zu diesem Raum")?;
    let mut updated = store.update_locus(
        &target.locus_id,
        &LocusPatch {
            image: Some(Some(media.hash)),
            ..Default::default()
        },
    )?;
    updated.bindings = locus.bindings.clone();
    *locus = updated;
    Ok((tree, Some(target.locus_id.clone())))
}

/// Both native pickers share bounded regular-file and signature validation.
fn read_selected_media(
    path: &std::path::Path,
    picture_only: bool,
) -> Result<(RoomKind, &'static str, Vec<u8>)> {
    let (kind, mime) = locora_source::media_type(path).context(
        "Unbekanntes Bildformat/Videoformat (png, jpg, webp, gif, avif, sply, mp4, m4v, webm, mov)",
    )?;
    ensure!(
        !picture_only || kind == RoomKind::Image,
        "Ein Locus-Bild kann kein Video sein"
    );
    // Bound reads as well as allocations, including files that grow during reading.
    use std::io::Read;
    const LIMIT: u64 = 64 * 1024 * 1024;
    // Reject special files before opening: opening a named pipe can block even
    // before the handle metadata check below runs.
    ensure!(
        std::fs::metadata(path)?.is_file(),
        "Bitte eine reguläre Bilddatei wählen"
    );
    let file = std::fs::File::open(path).context("Bilddatei öffnen")?;
    ensure!(
        file.metadata()?.is_file(),
        "Bitte eine reguläre Bilddatei wählen"
    );
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .context("Bilddatei lesen")?;
    ensure!(
        bytes.len() as u64 <= LIMIT,
        "Bilddatei ist größer als 64 MiB"
    );
    if mime == locora_store::SPLINELY {
        ensure!(
            bytes.starts_with(b"\x89SPLY\r\n\x1a"),
            "Keine binäre .sply-Datei"
        );
        crate::sply::to_svg(&bytes).context("Ungültige .sply-Datei")?;
    } else {
        let detected = infer::get(&bytes).map(|t| t.mime_type());
        ensure!(
            detected == Some(mime) || (mime == "video/mp4" && detected == Some("video/x-m4v")),
            "Bildinhalt passt nicht zum Dateiformat ({mime})"
        );
    }

    Ok((kind, mime, bytes))
}

/// Cancellation is handled before validation or persistence; paths are backend-only.
pub fn import_palace(store: &Store, path: Option<&std::path::Path>) -> Result<Option<PalaceTree>> {
    let Some(path) = path else { return Ok(None) };
    let (kind, mime, bytes) = read_selected_media(path, false)?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .trim();
    let name = if name.is_empty() {
        "Neuer Palast"
    } else {
        name
    };
    Ok(Some(store.create_media_palace(name, kind, &bytes, mime)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_import_cancel_validation_persistence_and_names() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert!(import_palace(&store, None).unwrap().is_none());
        for (ext, bytes) in [
            ("png", b"invalid".as_slice()),
            ("mp4", b"invalid"),
            ("sply", b"invalid"),
        ] {
            let path = dir.path().join(format!("bad.{ext}"));
            std::fs::write(&path, bytes).unwrap();
            assert!(import_palace(&store, Some(&path)).is_err());
        }
        assert!(store.palaces().unwrap().is_empty());
        let mut webm = vec![0u8; 257];
        webm[..4].copy_from_slice(b"\x1a\x45\xdf\xa3");
        webm[4..11].copy_from_slice(b"\x42\x82\x84webm");
        for (ext, bytes, kind) in [
            ("png", b"\x89PNG\r\n\x1a\n".as_slice(), RoomKind::Image),
            ("mp4", b"\0\0\0\x18ftypisom\0\0\0\0", RoomKind::Video),
            ("m4v", b"\0\0\0\x18ftypM4V \0\0\0\0", RoomKind::Video),
            ("mov", b"\0\0\0\x18ftypqt  \0\0\0\0", RoomKind::Video),
            ("webm", webm.as_slice(), RoomKind::Video),
            (
                "sply",
                locora_runtime::tutorial::DEFAULT_MARKER,
                RoomKind::Image,
            ),
        ] {
            let path = dir.path().join(format!("Raum.{ext}"));
            std::fs::write(&path, bytes).unwrap();
            let tree = import_palace(&store, Some(&path)).unwrap().unwrap();
            assert_eq!(tree.rooms.len(), 1);
            assert_eq!(tree.rooms[0].room.kind, kind);
            assert!(tree.rooms[0].loci.is_empty());
            let hash = tree.rooms[0].room.media.as_ref().unwrap();
            assert_eq!(
                std::fs::read(store.media_file(hash).unwrap()).unwrap(),
                bytes
            );
            assert_eq!(
                Store::open(dir.path())
                    .unwrap()
                    .palace(&tree.palace.id)
                    .unwrap(),
                tree
            );
        }
        let palaces = store.palaces().unwrap();
        assert_eq!(palaces.len(), 6);
        assert!(palaces.iter().any(|p| p.name == "Raum (2)"));
        let oversized = dir.path().join("large.png");
        std::fs::File::create(&oversized)
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        assert!(import_palace(&store, Some(&oversized)).is_err());
        assert!(import_palace(&store, Some(dir.path())).is_err());
        assert_eq!(store.palaces().unwrap().len(), 6);
    }
    use locora_app_api::{EditorAnswer, EditorAnswerKind, LocusContent, QuizOption};

    #[test]
    fn selected_picture_validates_scope_content_and_preserves_assignment_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let palace = store.create_palace("Bilder").unwrap();
        let room = store
            .add_room(&palace.id, "Raum", RoomKind::Blank, None)
            .unwrap();
        let locus_id = apply(
            &store,
            &EditorChange {
                palace_id: palace.id.clone(),
                room_id: room.id.clone(),
                action: EditorAction::Create { x: 0.5, y: 0.5 },
            },
        )
        .unwrap()
        .1
        .unwrap();
        let target = locora_app_api::PictureTarget {
            palace_id: palace.id.clone(),
            room_id: room.id.clone(),
            locus_id,
        };
        let png = dir.path().join("bild.PNG");
        let bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
        std::fs::write(&png, bytes).unwrap();
        let (tree, selected) = import_picture(&store, &target, &png).unwrap();
        assert_eq!(selected.as_deref(), Some(target.locus_id.as_str()));
        let hash = tree.rooms[0].loci[0].image.as_ref().unwrap();
        assert_eq!(store.media(hash).unwrap().unwrap().mime, "image/png");
        assert_eq!(
            std::fs::read(store.media_file(hash).unwrap()).unwrap(),
            bytes
        );
        let before = store.palace(&palace.id).unwrap();
        let ops = store.ops_since(0).unwrap().len();
        for (name, data, error) in [
            ("bild.jpg", bytes.as_slice(), "Bildinhalt passt nicht"),
            (
                "bild.png",
                b"<svg></svg>".as_slice(),
                "Bildinhalt passt nicht",
            ),
            ("bild.sply", b"not sply".as_slice(), "Keine binäre"),
            (
                "broken.sply",
                b"\x89SPLY\r\n\x1a".as_slice(),
                "Ungültige .sply",
            ),
            ("film.mp4", bytes.as_slice(), "kein Video"),
            ("bild.svg", bytes.as_slice(), "Unbekanntes Bildformat"),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, data).unwrap();
            let message = format!("{:#}", import_picture(&store, &target, &path).unwrap_err());
            assert!(message.contains(error), "{message}");
            assert_eq!(store.palace(&palace.id).unwrap(), before);
            assert_eq!(store.ops_since(0).unwrap().len(), ops);
        }
        assert!(import_picture(&store, &target, &dir.path().join("missing.png")).is_err());
        let directory = dir.path().join("directory.png");
        std::fs::create_dir(&directory).unwrap();
        assert!(
            format!(
                "{:#}",
                import_picture(&store, &target, &directory).unwrap_err()
            )
            .contains("reguläre Bilddatei")
        );
        assert_eq!(store.palace(&palace.id).unwrap(), before);
        assert_eq!(store.ops_since(0).unwrap().len(), ops);
        let other_room = store
            .add_room(&palace.id, "Fremd", RoomKind::Blank, None)
            .unwrap();
        let other_palace = store.create_palace("Fremd").unwrap();
        for wrong in [
            locora_app_api::PictureTarget {
                room_id: other_room.id,
                ..target.clone()
            },
            locora_app_api::PictureTarget {
                palace_id: other_palace.id,
                ..target.clone()
            },
            locora_app_api::PictureTarget {
                locus_id: "missing".into(),
                ..target.clone()
            },
        ] {
            assert!(import_picture(&store, &wrong, &png).is_err());
        }
        assert_eq!(
            store.palace(&palace.id).unwrap().rooms[0].loci,
            before.rooms[0].loci
        );
        let sply = dir.path().join("bild.sply");
        std::fs::write(&sply, locora_runtime::tutorial::DEFAULT_MARKER).unwrap();
        let (tree, _) = import_picture(&store, &target, &sply).unwrap();
        let hash = tree.rooms[0].loci[0].image.as_ref().unwrap();
        assert_eq!(
            store.media(hash).unwrap().unwrap().mime,
            locora_store::SPLINELY
        );
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.palace(&palace.id).unwrap(), tree);
    }

    #[test]
    fn stored_mime_conflicts_do_not_assign_a_picture() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let palace = store.create_palace("MIME").unwrap();
        let room = store
            .add_room(&palace.id, "Raum", RoomKind::Blank, None)
            .unwrap();
        let id = apply(
            &store,
            &EditorChange {
                palace_id: palace.id.clone(),
                room_id: room.id.clone(),
                action: EditorAction::Create { x: 0.5, y: 0.5 },
            },
        )
        .unwrap()
        .1
        .unwrap();
        let bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
        store.put_media(bytes, "video/mp4").unwrap();
        let path = dir.path().join("bild.png");
        std::fs::write(&path, bytes).unwrap();
        let before = store.palace(&palace.id).unwrap();
        let error = import_picture(
            &store,
            &locora_app_api::PictureTarget {
                palace_id: palace.id.clone(),
                room_id: room.id,
                locus_id: id,
            },
            &path,
        )
        .unwrap_err();
        assert!(error.to_string().contains("Medientyp"));
        assert_eq!(store.palace(&palace.id).unwrap(), before);
    }

    #[test]
    fn crud_persists_and_invalid_commands_leave_no_writes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let palace = store.create_palace("Editor").unwrap();
        let room = store
            .add_room(&palace.id, "Raum", RoomKind::Blank, None)
            .unwrap();
        let other = store
            .add_room(&palace.id, "Anderer Raum", RoomKind::Blank, None)
            .unwrap();
        let change = |action| EditorChange {
            palace_id: palace.id.clone(),
            room_id: room.id.clone(),
            action,
        };
        let id = apply(&store, &change(EditorAction::Create { x: 0.2, y: 0.8 }))
            .unwrap()
            .1
            .unwrap();
        let image = store.put_media(b"picture", "image/png").unwrap().hash;
        let content = LocusContent {
            place: " Herd ".into(),
            item: "Merkur".into(),
            note: "Heiß".into(),
            image: Some(image.clone()),
            image_size: 0.4,
            answer: EditorAnswer {
                kind: EditorAnswerKind::Guess,
                accept: vec!["Mercury".into()],
                options: vec![],
            },
        };
        apply(
            &store,
            &change(EditorAction::Save {
                id: id.clone(),
                content: content.clone(),
            }),
        )
        .unwrap();
        apply(
            &store,
            &change(EditorAction::Move {
                id: id.clone(),
                x: 1.0,
                y: 0.0,
            }),
        )
        .unwrap();
        apply(
            &store,
            &change(EditorAction::Resize {
                id: id.clone(),
                image_size: 0.7,
            }),
        )
        .unwrap();
        let locus = store.palace(&palace.id).unwrap().rooms[0].loci[0].clone();
        assert_eq!(
            (&*locus.place, &*locus.item, &*locus.note),
            ("Herd", "Merkur", "Heiß")
        );
        assert_eq!((locus.x, locus.y, locus.image_size), (1.0, 0.0, 0.7));
        assert_eq!(locus.image.as_deref(), Some(image.as_str()));
        assert_eq!(locus.answer, content.answer);

        let before = store.palace(&palace.id).unwrap();
        let ops = store.ops_since(0).unwrap().len();
        let video = store.put_media(b"video", "video/mp4").unwrap().hash;
        let mut bad_content = content.clone();
        bad_content.image = Some(video);
        let mut bad_quiz = content.clone();
        bad_quiz.answer = EditorAnswer {
            kind: EditorAnswerKind::Quiz,
            accept: vec![],
            options: vec![QuizOption {
                text: "Nur richtig".into(),
                correct: true,
            }],
        };
        let mut bad_guess = content.clone();
        bad_guess.answer.accept = vec!["Erde!".into(), "erde".into()];
        let mut unknown_image = content.clone();
        unknown_image.image = Some("missing".into());
        for action in [
            EditorAction::Move {
                id: id.clone(),
                x: f64::NAN,
                y: 0.2,
            },
            EditorAction::Move {
                id: id.clone(),
                x: 0.2,
                y: 1.1,
            },
            EditorAction::Resize {
                id: id.clone(),
                image_size: f64::INFINITY,
            },
            EditorAction::Resize {
                id: id.clone(),
                image_size: 0.01,
            },
            EditorAction::Create { x: -0.1, y: 0.0 },
            EditorAction::Save {
                id: id.clone(),
                content: bad_content,
            },
            EditorAction::Save {
                id: id.clone(),
                content: bad_quiz,
            },
            EditorAction::Save {
                id: id.clone(),
                content: bad_guess,
            },
            EditorAction::Save {
                id: id.clone(),
                content: unknown_image,
            },
            EditorAction::Delete {
                id: "unknown".into(),
            },
        ] {
            assert!(apply(&store, &change(action)).is_err());
            assert_eq!(store.palace(&palace.id).unwrap(), before);
            assert_eq!(store.ops_since(0).unwrap().len(), ops);
        }
        let wrong_room = EditorChange {
            room_id: other.id,
            ..change(EditorAction::Delete { id: id.clone() })
        };
        assert!(apply(&store, &wrong_room).is_err());
        let another = store.create_palace("Fremd").unwrap();
        let wrong_palace = EditorChange {
            palace_id: another.id,
            ..change(EditorAction::Move {
                id: id.clone(),
                x: 0.0,
                y: 0.0,
            })
        };
        assert!(apply(&store, &wrong_palace).is_err());
        // Full content uses explicit null as clear, not the nested Option patch trap.
        let mut clear = content.clone();
        clear.image = None;
        let wire = serde_json::to_value(change(EditorAction::Save {
            id: id.clone(),
            content: clear,
        }))
        .unwrap();
        assert!(wire["action"]["content"]["image"].is_null());
        apply(&store, &serde_json::from_value(wire).unwrap()).unwrap();
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        assert!(
            store.palace(&palace.id).unwrap().rooms[0].loci[0]
                .image
                .is_none()
        );
        apply(&store, &change(EditorAction::Delete { id: id.clone() })).unwrap();
        assert!(store.palace(&palace.id).unwrap().rooms[0].loci.is_empty());
        assert!(apply(&store, &change(EditorAction::Delete { id })).is_err());
    }

    #[test]
    fn list_paste_is_scoped_validated_and_persistent() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let palace = store.create_palace("Liste").unwrap();
        let room = store
            .add_room(&palace.id, "Raum", RoomKind::Blank, None)
            .unwrap();
        let mut change = EditorChange {
            palace_id: palace.id.clone(),
            room_id: room.id.clone(),
            action: EditorAction::Paste {
                items: vec![" Merkur ".into(), "Venus".into(), "Erde".into()],
            },
        };
        let (tree, selected) = apply(&store, &change).unwrap();
        assert_eq!(selected.as_deref(), Some(tree.rooms[0].loci[0].id.as_str()));
        assert_eq!(tree.rooms[0].loci[0].item, "Merkur");
        let before = store.palace(&palace.id).unwrap();
        let ops = store.ops_since(0).unwrap().len();
        for items in [
            vec![],
            vec!["ok".into(), " ".into()],
            vec!["x".repeat(501)],
            vec!["x".into(); 101],
            vec!["a\nb".into()],
        ] {
            change.action = EditorAction::Paste { items };
            assert!(apply(&store, &change).is_err());
            assert_eq!(store.palace(&palace.id).unwrap(), before);
            assert_eq!(store.ops_since(0).unwrap().len(), ops);
        }
        change.action = EditorAction::Paste {
            items: vec!["Fremd".into()],
        };
        let other = store.create_palace("Fremd").unwrap();
        change.palace_id = other.id;
        assert!(apply(&store, &change).is_err());
        assert_eq!(store.palace(&palace.id).unwrap(), before);
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.palace(&palace.id).unwrap(), before);
    }

    #[test]
    fn video_creation_requires_timeline_editor() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let media = store.put_media(b"video", "video/mp4").unwrap();
        let palace = store.create_palace("Video").unwrap();
        let room = store
            .add_room(&palace.id, "Video", RoomKind::Video, Some(&media.hash))
            .unwrap();
        assert!(
            apply(
                &store,
                &EditorChange {
                    palace_id: palace.id.clone(),
                    room_id: room.id.clone(),
                    action: EditorAction::Paste {
                        items: vec!["Video".into()]
                    },
                }
            )
            .is_err()
        );
        assert!(
            apply(
                &store,
                &EditorChange {
                    palace_id: palace.id,
                    room_id: room.id,
                    action: EditorAction::Create { x: 0.5, y: 0.5 }
                }
            )
            .is_err()
        );
    }
}
