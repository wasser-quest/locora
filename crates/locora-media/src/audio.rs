//! Sound clips of a locus: MP3, M4A (AAC, Opus, ALAC, FLAC or MP3), WAV, Ogg (Opus or Vorbis),
//! WebM (Opus, Vorbis, AAC or FLAC; what browsers record), FLAC and AAC in ADTS, stored without
//! their tags and other metadata and never re-encoded. Sound in other formats (AMR of old phones,
//! 3GP, Apple's CAF) needs a conversion, which the platform does (see `prepare`).

use crate::{Cleaned, MediaError, Result, aac, flac, isobmff, mp3, ogg, wav, webm};

/// The media types Locora stores sound as.
pub const TYPES: [&str; 7] = [
    "audio/mpeg",
    "audio/mp4",
    "audio/wav",
    "audio/ogg",
    "audio/webm",
    "audio/flac",
    "audio/aac",
];

/// The stored media type for a sound type a device or browser may report, if it is supported.
pub fn canonical(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "audio/mpeg" | "audio/mp3" => "audio/mpeg",
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" => "audio/mp4",
        "audio/wav" | "audio/x-wav" | "audio/wave" | "audio/vnd.wave" => "audio/wav",
        "audio/ogg" | "audio/opus" => "audio/ogg",
        "audio/webm" => "audio/webm",
        "audio/flac" | "audio/x-flac" => "audio/flac",
        "audio/aac" | "audio/x-aac" | "audio/aacp" => "audio/aac",
        _ => return None,
    })
}

/// Sound types that are taken but only after a conversion (unless a 3GP holds AAC): the
/// container `sniff` reports for them.
fn converted(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "audio/3gpp" | "audio/3gpp2" => "audio/mp4",
        "audio/amr" | "audio/amr-wb" => "audio/amr",
        "audio/x-caf" => "audio/x-caf",
        _ => return None,
    })
}

/// The sound format the bytes really are, from their first bytes.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    // ID3 tags are meant for MP3, but some programs put them in front of FLAC and AAC files too.
    let after_tags = mp3::untagged(bytes).map_or(0, |(start, _)| start);
    let sound = &bytes[after_tags.min(bytes.len())..];
    if flac::is_flac(sound) {
        Some("audio/flac")
    } else if aac::is_frame(sound) {
        Some("audio/aac")
    } else if bytes.starts_with(b"ID3") || mp3::is_frame(bytes) {
        Some("audio/mpeg")
    } else if bytes.get(4..8) == Some(b"ftyp") {
        Some("audio/mp4")
    } else if bytes.len() >= 12 && &bytes[8..12] == b"WAVE" && &bytes[..4] == b"RIFF" {
        Some("audio/wav")
    } else if bytes.starts_with(b"OggS") {
        Some("audio/ogg")
    } else if bytes.starts_with(b"\x1a\x45\xdf\xa3") {
        Some("audio/webm")
    } else if bytes.starts_with(b"#!AMR") {
        Some("audio/amr")
    } else if bytes.starts_with(b"caff") {
        Some("audio/x-caf")
    } else {
        None
    }
}

/// The sound without metadata and its stored media type, or the reason it needs a conversion.
pub fn clean(bytes: Vec<u8>, mime: &str) -> Result<Cleaned> {
    let container = canonical(mime)
        .or_else(|| converted(mime))
        .ok_or_else(|| MediaError::Unsupported(mime.to_owned()))?;
    if sniff(&bytes) != Some(container) {
        return Err(MediaError::Malformed("Inhalt passt nicht zum Tonformat"));
    }
    let ready = |bytes, mime| Ok(Cleaned::Ready { bytes, mime });
    let converting = |reason: Option<String>, bytes, mime| match reason {
        None => ready(bytes, mime),
        Some(reason) => Ok(Cleaned::NeedsConversion { reason }),
    };
    match container {
        "audio/mpeg" => ready(mp3::clean(&bytes)?, "audio/mpeg"),
        "audio/wav" => ready(wav::clean(&bytes)?, "audio/wav"),
        "audio/ogg" => ready(ogg::clean(&bytes)?, "audio/ogg"),
        "audio/flac" => ready(flac::clean(&bytes)?, "audio/flac"),
        "audio/aac" => ready(aac::clean(&bytes)?, "audio/aac"),
        "audio/webm" => {
            let mut bytes = bytes;
            let reason = webm::clean(&mut bytes, true)?;
            converting(reason, bytes, "audio/webm")
        }
        "audio/mp4" => {
            let mut bytes = bytes;
            let reason = isobmff::clean_audio(&mut bytes)?;
            converting(reason, bytes, "audio/mp4")
        }
        other => Ok(Cleaned::NeedsConversion {
            reason: format!(
                "Ton im Format {} wird umgewandelt",
                if other == "audio/amr" { "AMR" } else { "CAF" }
            ),
        }),
    }
}
