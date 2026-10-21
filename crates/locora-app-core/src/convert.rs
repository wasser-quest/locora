//! Pictures, videos and sound a person adds, as Locora stores them (see `locora_media::prepare`).

use anyhow::Result;

/// The bytes and media type to store for a picture, video or sound clip read from `path`; `.sply`
/// as it is.
pub fn prepare(
    path: &std::path::Path,
    bytes: Vec<u8>,
    mime: &'static str,
) -> Result<(Vec<u8>, &'static str)> {
    if mime == locora_store::SPLINELY {
        return Ok((bytes, mime));
    }
    Ok(locora_media::prepare(path, bytes, mime)?)
}

/// Whether the video at `path` must be converted before Locora can store it: its codec is not
/// one that devices play (H.264, HEVC, AV1, VP9, VP8 …). Android's encoder runs only then; a
/// video Locora takes as it is, and anything that is no video or cannot be read, gives `false`,
/// so the import itself reports the problem.
pub fn video_needs_conversion(path: &std::path::Path) -> bool {
    let Some((locora_store::RoomKind::Video, mime)) = locora_source::media_type(path) else {
        return false;
    };
    let Ok(bytes) = crate::editor::read_bounded(path) else {
        return false;
    };
    matches!(
        locora_media::clean(bytes, mime),
        Ok(locora_media::Cleaned::NeedsConversion { .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_media::fixtures;

    #[test]
    fn only_videos_outside_the_codec_list_are_converted() {
        let dir = tempfile::tempdir().unwrap();
        let movie = |codec| {
            fixtures::movie(
                b"qt  ",
                vec![fixtures::track(b"vide", codec, [0; 4])],
                Vec::new(),
            )
        };
        for (name, bytes, converted) in [
            ("iphone.mov", movie(b"hvc1"), false),
            ("pixel.mp4", movie(b"avc1"), false),
            (
                "chrome.webm",
                fixtures::webm("webm", &[(1, "V_VP8")]),
                false,
            ),
            ("prores.mov", movie(b"apcn"), true),
            ("divx.mp4", movie(b"mp4v"), true),
            ("bild.png", fixtures::png(), false),
            ("kaputt.mp4", b"broken".to_vec(), false),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(video_needs_conversion(&path), converted, "{name}");
        }
    }
}
