//! AAC in ADTS frames (`.aac`, as some recorders and radios write it): the file is kept from its
//! first to its last whole frame. Tags in front of it (ID3) and anything after the frames (ID3v1,
//! APE, a cut-off frame) are dropped. ADTS frames carry no metadata of their own.

use crate::{MediaError, Result, mp3};

/// The length of the ADTS frame at the start of `data`, from its header.
fn frame_length(data: &[u8]) -> Option<usize> {
    let header = data.get(..7)?;
    // Twelve sync bits, then the MPEG version (either), and layer 0.
    if header[0] != 0xFF || header[1] & 0xF6 != 0xF0 {
        return None;
    }
    // Sampling frequency index 13 to 15 is reserved.
    if (header[2] >> 2) & 0xF > 12 {
        return None;
    }
    let length = usize::from(header[3] & 3) << 11
        | usize::from(header[4]) << 3
        | usize::from(header[5] >> 5);
    let header_length = if header[1] & 1 == 0 { 9 } else { 7 };
    (length >= header_length).then_some(length)
}

/// Whether `data` starts with an ADTS frame header.
pub fn is_frame(data: &[u8]) -> bool {
    frame_length(data).is_some()
}

pub fn clean(data: &[u8]) -> Result<Vec<u8>> {
    let (start, _) = mp3::untagged(data)?;
    let mut at = start;
    while let Some(length) = frame_length(&data[at..]) {
        if at + length > data.len() {
            break;
        }
        at += length;
    }
    if at == start {
        return Err(MediaError::Malformed("kein AAC-Frame"));
    }
    Ok(data[start..at].to_vec())
}
