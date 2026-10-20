//! Pictures and videos as Locora stores them. Whatever a device adds goes through `clean`
//! first: metadata that can say where, when or with what a picture or video was taken (EXIF with
//! GPS, XMP, IPTC, comments, creation times) is removed without re-encoding anything, and a video
//! must be MP4 with H.264, which every browser and app plays.
//!
//! Only the orientation of a photo is kept (as a minimal EXIF block), so it is not shown sideways.
//! A video in another format or codec is not changed here: `clean` says it needs a conversion,
//! which the platform does (Android's encoder, WebCodecs in the browser, ffmpeg on a desktop).

mod convert;
mod exif;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
mod gif;
mod isobmff;
mod jpeg;
mod png;
mod webp;

#[cfg(test)]
mod tests;

/// The result of `clean`.
#[derive(Debug, PartialEq, Eq)]
pub enum Cleaned {
    /// Ready to store: the bytes without metadata, and their media type.
    Ready { bytes: Vec<u8>, mime: &'static str },
    /// A video that is not MP4 with H.264 (or carries tracks beyond picture and sound): it must
    /// be converted first. The reason is for the person, in German like the app's other messages.
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
}

pub use convert::prepare;

pub(crate) type Result<T> = std::result::Result<T, MediaError>;

/// Removes the metadata of a picture or video of type `mime` and checks a video's format.
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
        "video/mp4" | "video/x-m4v" | "video/quicktime" => {
            let mut bytes = bytes;
            match isobmff::clean_video(&mut bytes)? {
                None => ready(bytes, "video/mp4"),
                Some(reason) => Ok(Cleaned::NeedsConversion { reason }),
            }
        }
        "video/webm" => Ok(Cleaned::NeedsConversion {
            reason: "WebM wird als MP4 (H.264) gespeichert".into(),
        }),
        other => Err(MediaError::Unsupported(other.to_owned())),
    }
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
