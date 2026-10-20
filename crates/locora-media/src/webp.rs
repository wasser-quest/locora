//! WebP: the EXIF and XMP chunks are dropped and their flags cleared in the extended header. A
//! non-default orientation is written back as a minimal EXIF chunk.

use crate::exif;
use crate::{MediaError, Result, le32};

const EXIF_FLAG: u8 = 0x08;
const XMP_FLAG: u8 = 0x04;

pub fn clean(data: &[u8]) -> Result<Vec<u8>> {
    if data.get(..4) != Some(b"RIFF") || data.get(8..12) != Some(b"WEBP") {
        return Err(MediaError::Malformed("keine WebP-Datei"));
    }
    let riff_end = (le32(data, 4)? as usize).saturating_add(8).min(data.len());
    let mut chunks = Vec::new();
    let mut orientation = None;
    let mut extended = None;
    let mut at = 12;
    while at + 8 <= riff_end {
        let size = le32(data, at + 4)? as usize;
        let end = (at + 8)
            .checked_add(size)
            .and_then(|end| end.checked_add(size & 1))
            .ok_or(MediaError::Malformed("Chunk zu groß"))?;
        let chunk = data
            .get(at..end.min(riff_end))
            .ok_or(MediaError::Malformed("Chunk zu kurz"))?;
        let kind = &chunk[..4];
        match kind {
            b"EXIF" => orientation = orientation.or(exif::orientation(&chunk[8..])),
            b"XMP " => {}
            _ => {
                if kind == b"VP8X" {
                    extended = Some(chunks.len());
                }
                chunks.push(chunk.to_vec());
            }
        }
        at = end;
    }
    if let Some(index) = extended {
        let flags = chunks[index]
            .get_mut(8)
            .ok_or(MediaError::Malformed("VP8X zu kurz"))?;
        *flags &= !(EXIF_FLAG | XMP_FLAG);
        if orientation.is_some() {
            *flags |= EXIF_FLAG;
        }
    }
    // EXIF needs the extended header; a simple file never carried it.
    if let (Some(orientation), Some(_)) = (orientation, extended) {
        let tiff = exif::minimal_tiff(orientation);
        let size: u32 = exif::fits(tiff.len())?;
        let mut chunk = b"EXIF".to_vec();
        chunk.extend_from_slice(&size.to_le_bytes());
        chunk.extend_from_slice(&tiff);
        if tiff.len() % 2 == 1 {
            chunk.push(0);
        }
        chunks.push(chunk);
    }
    let body: usize = chunks.iter().map(Vec::len).sum();
    let riff_size: u32 = exif::fits(4 + body)?;
    let mut out = Vec::with_capacity(12 + body);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WEBP");
    for chunk in chunks {
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}
