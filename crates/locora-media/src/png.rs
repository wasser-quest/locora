//! PNG: text chunks, the EXIF chunk and the modification time are dropped; data after the end
//! chunk too. A non-default orientation is written back as a minimal EXIF chunk.

use crate::exif;
use crate::{MediaError, Result, be32};

const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

pub fn clean(data: &[u8]) -> Result<Vec<u8>> {
    if !data.starts_with(SIGNATURE) {
        return Err(MediaError::Malformed("keine PNG-Signatur"));
    }
    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(SIGNATURE);
    let mut orientation = None;
    let mut after_header = None;
    let mut at = SIGNATURE.len();
    loop {
        let length =
            usize::try_from(be32(data, at)?).map_err(|_| MediaError::Malformed("Chunk zu groß"))?;
        let end = at + 12 + length;
        let chunk = data
            .get(at..end)
            .ok_or(MediaError::Malformed("Chunk zu kurz"))?;
        let kind = &chunk[4..8];
        match kind {
            b"eXIf" => orientation = orientation.or(exif::orientation(&chunk[8..8 + length])),
            b"tEXt" | b"zTXt" | b"iTXt" | b"tIME" => {}
            _ => out.extend_from_slice(chunk),
        }
        if kind == b"IHDR" {
            after_header = Some(out.len());
        }
        at = end;
        if kind == b"IEND" {
            break;
        }
    }
    if let (Some(orientation), Some(insert_at)) = (orientation, after_header) {
        out.splice(
            insert_at..insert_at,
            chunk(b"eXIf", &exif::minimal_tiff(orientation))?,
        );
    }
    Ok(out)
}

fn chunk(kind: &[u8; 4], body: &[u8]) -> Result<Vec<u8>> {
    let length: u32 = exif::fits(body.len())?;
    let mut out = Vec::with_capacity(12 + body.len());
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let crc = crc32(&out[4..]);
    out.extend_from_slice(&crc.to_be_bytes());
    Ok(out)
}

/// CRC-32 as PNG uses it (ISO 3309), bit by bit: chunks written here are tiny.
pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
