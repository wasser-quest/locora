//! The one piece of EXIF that is kept: the orientation, written as a minimal TIFF block.

use crate::{MediaError, Result};

const ORIENTATION: u16 = 0x0112;

/// The orientation (1–8) of an EXIF block, given as TIFF data with or without the `Exif\0\0`
/// prefix. `None` when there is none or it is the default (1).
pub fn orientation(data: &[u8]) -> Option<u16> {
    let tiff = data.strip_prefix(b"Exif\0\0").unwrap_or(data);
    let little = match tiff.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |at: usize| -> Option<u16> {
        let b = tiff.get(at..at + 2)?;
        Some(if little {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let b = tiff.get(at..at + 4)?;
        let b = [b[0], b[1], b[2], b[3]];
        Some(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    if u16_at(2)? != 42 {
        return None;
    }
    let ifd = usize::try_from(u32_at(4)?).ok()?;
    let count = usize::from(u16_at(ifd)?);
    (0..count).find_map(|index| {
        let entry = ifd + 2 + index * 12;
        if u16_at(entry)? != ORIENTATION || u16_at(entry + 2)? != 3 {
            return None;
        }
        let value = u16_at(entry + 8)?;
        ((2..=8).contains(&value)).then_some(value)
    })
}

/// A TIFF block with only the orientation (big-endian, one IFD entry).
pub fn minimal_tiff(orientation: u16) -> Vec<u8> {
    let mut tiff = Vec::with_capacity(26);
    tiff.extend_from_slice(b"MM\0\x2a\0\0\0\x08");
    tiff.extend_from_slice(&1u16.to_be_bytes());
    tiff.extend_from_slice(&ORIENTATION.to_be_bytes());
    tiff.extend_from_slice(&3u16.to_be_bytes());
    tiff.extend_from_slice(&1u32.to_be_bytes());
    tiff.extend_from_slice(&orientation.to_be_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_be_bytes());
    tiff
}

/// A segment length or size that must fit its field.
pub fn fits<T: TryFrom<usize>>(length: usize) -> Result<T> {
    T::try_from(length).map_err(|_| MediaError::Malformed("Block zu groß"))
}
