//! A sound clip for a locus, chosen in the editor. The file's type comes from its name and must
//! match its content; it is stored without its tags (see `locora_media`).

use anyhow::{Context, Result, ensure};
use locora_app_api::PictureTarget;
use locora_store::{LocusPatch, PalaceTree, Store};

use crate::editor::{picture_target, read_bounded, set_locus_media, store_media};

/// File extensions the sound pickers offer (AMR, 3GP and CAF are converted where ffmpeg is
/// installed, otherwise refused with a hint).
pub const EXTENSIONS: [&str; 14] = [
    "mp3", "m4a", "wav", "ogg", "oga", "opus", "weba", "webm", "flac", "aac", "3gp", "3ga", "amr",
    "caf",
];

/// Backend-only path obtained from the native dialog or the browser's inbox, never an IPC argument.
pub fn import_audio(
    store: &Store,
    target: &PictureTarget,
    path: &std::path::Path,
) -> Result<(PalaceTree, Option<String>)> {
    picture_target(store, target)?;
    let (mime, bytes) = read_selected_audio(path)?;
    let hash = store_media(store, &bytes, mime)?;
    set_locus_media(
        store,
        target,
        LocusPatch {
            audio: Some(Some(hash)),
            ..Default::default()
        },
    )
}

/// The cleaned sound and its stored media type. The content must be the format the name says
/// (`locora_media` checks); a format outside the list is converted by an installed ffmpeg.
fn read_selected_audio(path: &std::path::Path) -> Result<(&'static str, Vec<u8>)> {
    let mime = locora_source::audio_type(path)
        .context("Unbekanntes Tonformat (mp3, m4a, wav, ogg, opus, weba, flac, aac)")?;
    let bytes = read_bounded(path)?;
    ensure!(!bytes.is_empty(), "Die Tondatei ist leer");
    let (bytes, mime) = crate::convert::prepare(path, bytes, mime)?;
    Ok((mime, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_store::RoomKind;

    fn palace(store: &Store) -> PictureTarget {
        let p = store.create_palace("Klänge").unwrap();
        let r = store
            .add_room(&p.id, "Raum", RoomKind::Blank, None)
            .unwrap();
        let l = store
            .add_locus(
                &r.id,
                locora_store::NewLocus {
                    audio: None,
                    image_size: locora_store::default_image_size(),
                    zoom: locora_store::default_zoom(),
                    place: "Tür".into(),
                    item: "Merkur".into(),
                    x: 0.5,
                    y: 0.5,
                    time: None,
                    answer: Default::default(),
                    image: None,
                    note: String::new(),
                },
            )
            .unwrap();
        PictureTarget {
            palace_id: p.id,
            room_id: r.id,
            locus_id: l.id,
        }
    }

    #[test]
    fn a_chosen_sound_file_is_cleaned_and_set() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("profil")).unwrap();
        let target = palace(&store);
        let mut tagged = locora_media::fixtures::wav();
        tagged.extend_from_slice(b"LIST\x0c\0\0\0INFOINAM\0\0\0\0");
        let size = (tagged.len() - 8) as u32;
        tagged[4..8].copy_from_slice(&size.to_le_bytes());
        let file = dir.path().join("Glocke.WAV");
        std::fs::write(&file, &tagged).unwrap();

        let (tree, selected) = import_audio(&store, &target, &file).unwrap();
        assert_eq!(selected.as_deref(), Some(target.locus_id.as_str()));
        let hash = tree.route().next().unwrap().audio.clone().unwrap();
        let media = store.media(&hash).unwrap().unwrap();
        assert_eq!(media.mime, "audio/wav");
        assert_eq!(
            std::fs::read(store.media_file(&hash).unwrap()).unwrap(),
            locora_media::fixtures::wav()
        );
        let view = crate::views::media(&store, &hash).unwrap();
        assert_eq!(view.kind, locora_app_api::MediaKind::Audio);
    }

    #[test]
    fn a_browser_recording_is_kept_as_webm() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("profil")).unwrap();
        let target = palace(&store);
        let file = dir.path().join("Aufnahme.webm");
        let recording = locora_media::fixtures::webm("webm", &[(2, "A_OPUS")]);
        std::fs::write(&file, &recording).unwrap();
        let (tree, _) = import_audio(&store, &target, &file).unwrap();
        let hash = tree.route().next().unwrap().audio.clone().unwrap();
        assert_eq!(store.media(&hash).unwrap().unwrap().mime, "audio/webm");
        let stored = std::fs::read(store.media_file(&hash).unwrap()).unwrap();
        assert_eq!(
            stored.len(),
            recording.len(),
            "not re-encoded, nothing moved"
        );
        assert!(!stored.windows(6).any(|w| w == b"Chrome"));
    }

    #[test]
    fn wrong_or_foreign_sound_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("profil")).unwrap();
        let target = palace(&store);
        let cases = [
            ("klang.mp3", locora_media::fixtures::wav(), "passt nicht"),
            ("klang.flac", b"fLaC".to_vec(), "beschädigt"),
            ("klang.weba", b"\x1a\x45\xdf\xa3".to_vec(), "beschädigt"),
            (
                "film.weba",
                locora_media::fixtures::webm("webm", &[(1, "V_VP8"), (2, "A_OPUS")]),
                "Videospur",
            ),
            (
                "klang.wma",
                locora_media::fixtures::wav(),
                "Unbekanntes Tonformat",
            ),
            (
                "klang.png",
                locora_media::fixtures::png(),
                "Unbekanntes Tonformat",
            ),
        ];
        for (name, bytes, message) in cases {
            let file = dir.path().join(name);
            std::fs::write(&file, bytes).unwrap();
            let err = format!("{:#}", import_audio(&store, &target, &file).unwrap_err());
            assert!(err.contains(message), "{name}: {err}");
        }
        let other = PictureTarget {
            locus_id: "fremd".into(),
            ..target.clone()
        };
        let file = dir.path().join("ok.wav");
        std::fs::write(&file, locora_media::fixtures::wav()).unwrap();
        assert!(import_audio(&store, &other, &file).is_err());
        let tree = store.palace(&target.palace_id).unwrap();
        assert_eq!(tree.route().next().unwrap().audio, None);
    }
}
