//! Sound clips of a locus: MP3, M4A (AAC or Opus), WAV and Ogg (Opus or Vorbis), stored without
//! their tags and other metadata and never re-encoded. WebM sound is refused for now: its tags
//! sit in an EBML tree that a recorder may leave unsized, so it is not cleaned safely here.

use crate::{MediaError, Result, isobmff, mp3, ogg, wav};

/// The media types Locora stores sound as.
pub const TYPES: [&str; 4] = ["audio/mpeg", "audio/mp4", "audio/wav", "audio/ogg"];

/// The stored media type for a sound type a device or browser may report, if it is supported.
pub fn canonical(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "audio/mpeg" | "audio/mp3" => "audio/mpeg",
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" => "audio/mp4",
        "audio/wav" | "audio/x-wav" | "audio/wave" | "audio/vnd.wave" => "audio/wav",
        "audio/ogg" | "audio/opus" => "audio/ogg",
        _ => return None,
    })
}

/// The sound format the bytes really are, from their first bytes (WebM included, so it can be
/// refused with a clear message).
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"ID3") || mp3::is_frame(bytes) {
        Some("audio/mpeg")
    } else if bytes.get(4..8) == Some(b"ftyp") {
        Some("audio/mp4")
    } else if bytes.len() >= 12 && &bytes[8..12] == b"WAVE" && &bytes[..4] == b"RIFF" {
        Some("audio/wav")
    } else if bytes.starts_with(b"OggS") {
        Some("audio/ogg")
    } else if bytes.starts_with(b"\x1a\x45\xdf\xa3") {
        Some("audio/webm")
    } else {
        None
    }
}

/// The sound without metadata, and its stored media type.
pub fn clean(bytes: Vec<u8>, mime: &str) -> Result<(Vec<u8>, &'static str)> {
    if mime == "audio/webm" {
        return Err(MediaError::Refused(
            "WebM-Ton wird noch nicht unterstützt. Bitte die Aufnahme als MP3, M4A, WAV oder Ogg speichern."
                .into(),
        ));
    }
    let mime = canonical(mime).ok_or_else(|| MediaError::Unsupported(mime.to_owned()))?;
    if sniff(&bytes) != Some(mime) {
        return Err(MediaError::Malformed("Inhalt passt nicht zum Tonformat"));
    }
    let cleaned = match mime {
        "audio/mpeg" => mp3::clean(&bytes)?,
        "audio/wav" => wav::clean(&bytes)?,
        "audio/ogg" => ogg::clean(&bytes)?,
        _ => {
            let mut bytes = bytes;
            isobmff::clean_audio(&mut bytes)?;
            bytes
        }
    };
    Ok((cleaned, mime))
}
