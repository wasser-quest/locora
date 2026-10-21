//! FLAC: only the metadata blocks a player needs stay (STREAMINFO and SEEKTABLE); Vorbis comments
//! (title, artist, sometimes dates and places), pictures, cue sheets, application blocks and
//! padding are dropped, and the last kept block is marked as the last. The frames stay as they
//! are. ID3 and APE tags that some programs put around a FLAC file are cut off too.

use crate::{MediaError, Result, mp3};

const STREAMINFO: u8 = 0;
const SEEKTABLE: u8 = 3;
const LAST: u8 = 0x80;

/// Whether `data` starts with the FLAC signature.
pub fn is_flac(data: &[u8]) -> bool {
    data.starts_with(b"fLaC")
}

pub fn clean(data: &[u8]) -> Result<Vec<u8>> {
    let (start, end) = mp3::untagged(data)?;
    let data = &data[start..end];
    if !is_flac(data) {
        return Err(MediaError::Malformed("keine FLAC-Signatur"));
    }
    let mut kept: Vec<&[u8]> = Vec::new();
    let mut at = 4;
    loop {
        let header = data
            .get(at..at + 4)
            .ok_or(MediaError::Malformed("FLAC-Block zu kurz"))?;
        let kind = header[0] & !LAST;
        let length =
            usize::from(header[1]) << 16 | usize::from(header[2]) << 8 | usize::from(header[3]);
        let block_end = (at + 4)
            .checked_add(length)
            .filter(|&block_end| block_end <= data.len())
            .ok_or(MediaError::Malformed("FLAC-Block zu lang"))?;
        if kind == 127 {
            return Err(MediaError::Malformed("ungültiger FLAC-Block"));
        }
        if at == 4 && (kind != STREAMINFO || length != 34) {
            return Err(MediaError::Malformed("FLAC ohne STREAMINFO"));
        }
        if kind == STREAMINFO && at != 4 {
            return Err(MediaError::Malformed("doppelte STREAMINFO"));
        }
        if matches!(kind, STREAMINFO | SEEKTABLE) {
            kept.push(&data[at..block_end]);
        }
        at = block_end;
        if header[0] & LAST != 0 {
            break;
        }
    }
    let frames = &data[at..];
    // Frames start with the sync code 0xFFF8 (fixed block size) or 0xFFF9 (variable).
    if !frames.is_empty() && (frames.len() < 2 || frames[0] != 0xFF || frames[1] & 0xFE != 0xF8) {
        return Err(MediaError::Malformed("kein FLAC-Frame"));
    }
    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(b"fLaC");
    let count = kept.len();
    for (index, block) in kept.into_iter().enumerate() {
        let flag = if index + 1 == count { LAST } else { 0 };
        out.push((block[0] & !LAST) | flag);
        out.extend_from_slice(&block[1..]);
    }
    out.extend_from_slice(frames);
    Ok(out)
}
