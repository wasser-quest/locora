//! The format of a stored video or sound clip, named for a person ("HEVC, AAC"), for the message
//! shown when this device cannot play it. Read from the first and last bytes of its blob: the
//! sample entries of an MP4 (the movie box sits at either end) and the codec ids of a WebM.

use wasm_bindgen_futures::JsFuture;

/// How many bytes are read from each end of the file.
const WINDOW: f64 = 4.0 * 1024.0 * 1024.0;

/// The codecs of the media behind the blob URL `url` (made by `tauri::object_url`), or its
/// container, or "?".
pub async fn describe(url: &str) -> String {
    match read_ends(url).await {
        Some(bytes) => name(&bytes),
        None => "?".into(),
    }
}

async fn read_ends(url: &str) -> Option<Vec<u8>> {
    let blob = crate::tauri::blob_of(url)?;
    let size = blob.size();
    let mut bytes = Vec::new();
    for (start, end) in [
        (0.0, size.min(WINDOW)),
        ((size - WINDOW).max(WINDOW.min(size)), size),
    ] {
        if start >= end {
            continue;
        }
        let part = blob.slice_with_f64_and_f64(start, end).ok()?;
        let buffer = JsFuture::from(part.array_buffer()).await.ok()?;
        bytes.extend(js_sys::Uint8Array::new(&buffer).to_vec());
    }
    Some(bytes)
}

/// The codec names found in `bytes`, joined, or the container's name.
pub fn name(bytes: &[u8]) -> String {
    let mut found: Vec<String> = Vec::new();
    let mut add = |codec: String| {
        if !found.contains(&codec) {
            found.push(codec);
        }
    };
    // MP4: `stsd`, version and flags, entry count, then the first entry's size and type.
    for at in positions(bytes, b"stsd") {
        if let Some(kind) = bytes.get(at + 16..at + 20) {
            add(mp4_codec(kind));
        }
    }
    // WebM: a CodecID element (0x86) with a one-byte size and a `V_`/`A_` name.
    for at in positions(bytes, &[0x86]) {
        let Some(&size) = bytes.get(at + 1) else {
            continue;
        };
        let length = usize::from(size & 0x7F);
        if size & 0x80 == 0 || !(3..=32).contains(&length) {
            continue;
        }
        let Some(id) = bytes.get(at + 2..at + 2 + length) else {
            continue;
        };
        let id = String::from_utf8_lossy(id);
        if (id.starts_with("V_") || id.starts_with("A_"))
            && id.chars().all(|c| c.is_ascii_graphic())
        {
            add(webm_codec(&id));
        }
    }
    if !found.is_empty() {
        return found.join(", ");
    }
    container(bytes).into()
}

fn positions<'a>(bytes: &'a [u8], needle: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    bytes
        .windows(needle.len())
        .enumerate()
        .filter(move |(_, window)| *window == needle)
        .map(|(at, _)| at)
}

fn mp4_codec(kind: &[u8]) -> String {
    match kind {
        b"avc1" | b"avc3" => "H.264".into(),
        b"hvc1" | b"hev1" => "HEVC".into(),
        b"dvh1" | b"dvhe" => "Dolby Vision".into(),
        b"av01" => "AV1".into(),
        b"vp09" => "VP9".into(),
        b"vp08" => "VP8".into(),
        b"mp4a" => "AAC".into(),
        b"Opus" => "Opus".into(),
        b".mp3" => "MP3".into(),
        b"alac" => "ALAC".into(),
        b"fLaC" => "FLAC".into(),
        b"ac-3" => "AC-3".into(),
        b"ec-3" => "E-AC-3".into(),
        other => String::from_utf8_lossy(other).trim().to_owned(),
    }
}

fn webm_codec(id: &str) -> String {
    match id {
        "V_VP8" => "VP8".into(),
        "V_VP9" => "VP9".into(),
        "V_AV1" => "AV1".into(),
        "V_MPEG4/ISO/AVC" => "H.264".into(),
        "V_MPEGH/ISO/HEVC" => "HEVC".into(),
        "A_OPUS" => "Opus".into(),
        "A_VORBIS" => "Vorbis".into(),
        "A_FLAC" => "FLAC".into(),
        aac if aac.starts_with("A_AAC") => "AAC".into(),
        other => other
            .trim_start_matches("V_")
            .trim_start_matches("A_")
            .into(),
    }
}

fn container(bytes: &[u8]) -> &'static str {
    if bytes.get(4..8) == Some(b"ftyp") {
        "MP4"
    } else if bytes.starts_with(b"\x1a\x45\xdf\xa3") {
        "WebM"
    } else if bytes.starts_with(b"OggS") {
        "Ogg"
    } else if bytes.starts_with(b"fLaC") {
        "FLAC"
    } else if bytes.starts_with(b"RIFF") {
        "WAV"
    } else if bytes.starts_with(b"ID3") || bytes.first() == Some(&0xFF) {
        "MP3/AAC"
    } else {
        "?"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codecs_are_named_from_mp4_and_webm_headers() {
        let mut mp4 = b"\0\0\0\x18ftypisom".to_vec();
        for kind in [b"hvc1", b"mp4a"] {
            mp4.extend_from_slice(b"\0\0\0\x20stsd\0\0\0\0\0\0\0\x01\0\0\0\x10");
            mp4.extend_from_slice(kind);
        }
        assert_eq!(name(&mp4), "HEVC, AAC");
        let mut webm = b"\x1a\x45\xdf\xa3".to_vec();
        webm.extend_from_slice(b"\x86\x85V_VP9\x86\x86A_OPUS");
        assert_eq!(name(&webm), "VP9, Opus");
        assert_eq!(name(b"OggS\0"), "Ogg");
        assert_eq!(name(b"nothing"), "?");
    }
}
