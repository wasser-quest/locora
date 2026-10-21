//! Pictures, videos and sound clips as Locora stores them. Whatever a device adds goes through
//! `clean` first: metadata that can say where, when or with what a picture, video or recording was
//! made (EXIF with GPS, XMP, IPTC, comments, creation times, GPS tracks, tags) is removed without
//! re-encoding anything. The pictures and sound themselves stay bit for bit as they were.
//!
//! Only the orientation of a photo is kept (as a minimal EXIF block), so it is not shown sideways.
//! Videos keep the codec they were recorded in, and the device that plays them decodes it (Locora
//! bundles no codec): MP4/MOV with H.264, HEVC, AV1 or VP9 (sound AAC, Opus, MP3, ALAC or FLAC),
//! and WebM with VP8, VP9, AV1, H.264 or HEVC (sound Opus, Vorbis, AAC or FLAC). Only a video
//! outside that list (ProRes, MPEG-4 Part 2, AMR sound …) is not changed here: `clean` says it
//! needs a conversion, which the platform does as a fallback (Android's encoder, WebCodecs in the
//! browser, an installed ffmpeg on a desktop).
//!
//! Sound (MP3, M4A, WAV, Ogg, WebM, FLAC, AAC) loses its tags the same way; see `audio`.

mod aac;
mod audio;
mod convert;
mod exif;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
mod flac;
mod gif;
mod isobmff;
mod jpeg;
mod mp3;
mod ogg;
mod png;
mod wav;
mod webm;
mod webp;

#[cfg(test)]
mod audio_tests;
#[cfg(test)]
mod codec_tests;
#[cfg(test)]
mod tests;

/// The result of `clean`.
#[derive(Debug, PartialEq, Eq)]
pub enum Cleaned {
    /// Ready to store: the bytes without metadata, and their media type.
    Ready { bytes: Vec<u8>, mime: &'static str },
    /// A video or sound clip in a codec outside the list above (or with tracks that cannot be
    /// removed in place): it must be converted first. The reason is for the person, in German like the app's other messages.
    NeedsConversion { reason: String },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MediaError {
    #[error("Die Datei ist beschädigt oder unvollständig ({0})")]
    Malformed(&'static str),
    #[error("Unbekanntes Medienformat: {0}")]
    Unsupported(String),
    #[error("{0}")]
    Conversion(String),
    /// A file of a known kind that Locora does not take; the reason is for the person (German).
    #[error("{0}")]
    Refused(String),
}

pub use audio::{TYPES as AUDIO_TYPES, canonical as audio_canonical, sniff as sniff_audio};
pub use convert::prepare;

pub(crate) type Result<T> = std::result::Result<T, MediaError>;

/// Removes the metadata of a picture, video or sound clip of type `mime` and checks a video's format.
pub fn clean(bytes: Vec<u8>, mime: &str) -> Result<Cleaned> {
    let ready = |bytes, mime| Ok(Cleaned::Ready { bytes, mime });
    match mime {
        "image/jpeg" => ready(jpeg::clean(&bytes)?, "image/jpeg"),
        "image/png" => ready(png::clean(&bytes)?, "image/png"),
        "image/webp" => ready(webp::clean(&bytes)?, "image/webp"),
        "image/gif" => ready(gif::clean(&bytes)?, "image/gif"),
        "image/avif" => {
            let mut bytes = bytes;
            isobmff::clean_image(&mut bytes)?;
            ready(bytes, "image/avif")
        }
        "video/mp4" | "video/x-m4v" | "video/quicktime" | "video/3gpp" | "video/3gpp2" => {
            let mut bytes = bytes;
            match isobmff::clean_video(&mut bytes)? {
                None => ready(bytes, "video/mp4"),
                Some(reason) => Ok(Cleaned::NeedsConversion { reason }),
            }
        }
        "video/webm" | "video/x-matroska" => {
            let mut bytes = bytes;
            match webm::clean(&mut bytes, false)? {
                None => ready(bytes, "video/webm"),
                Some(reason) => Ok(Cleaned::NeedsConversion { reason }),
            }
        }
        audio if audio.starts_with("audio/") => audio::clean(bytes, audio),
        other => Err(MediaError::Unsupported(other.to_owned())),
    }
}

/// Whether a file of this type is a sound clip Locora stores (after `audio_canonical`).
pub fn is_audio(mime: &str) -> bool {
    AUDIO_TYPES.contains(&mime)
}

/// Whether a file of this type is a video (and so may need a conversion).
pub fn is_video(mime: &str) -> bool {
    mime.starts_with("video/")
}

/// Big-endian and little-endian readers that fail instead of panicking on short input.
pub(crate) fn be16(data: &[u8], at: usize) -> Result<u16> {
    data.get(at..at + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or(MediaError::Malformed("zu kurz"))
}

pub(crate) fn be32(data: &[u8], at: usize) -> Result<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(MediaError::Malformed("zu kurz"))
}

pub(crate) fn be64(data: &[u8], at: usize) -> Result<u64> {
    data.get(at..at + 8)
        .map(|b| u64::from_be_bytes(b.try_into().expect("eight bytes")))
        .ok_or(MediaError::Malformed("zu kurz"))
}

pub(crate) fn le32(data: &[u8], at: usize) -> Result<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(MediaError::Malformed("zu kurz"))
}
