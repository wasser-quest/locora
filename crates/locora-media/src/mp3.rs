//! MP3: the tags around the frames are cut off, the frames stay as they are. ID3v2 tags at the
//! start (title, artist, cover, comments, sometimes a recording date or place), and ID3v1, APEv2,
//! Lyrics3 and appended ID3v2 tags at the end. A valid frame must follow.

use crate::{MediaError, Result, le32};

/// The size of an ID3v2 tag body: four bytes of seven bits each.
fn syncsafe(bytes: &[u8]) -> Result<usize> {
    if bytes.len() != 4 || bytes.iter().any(|b| b & 0x80 != 0) {
        return Err(MediaError::Malformed("ID3-Größe"));
    }
    Ok(bytes
        .iter()
        .fold(0usize, |size, &b| (size << 7) | usize::from(b)))
}

pub fn clean(data: &[u8]) -> Result<Vec<u8>> {
    let mut start = 0;
    // One or more ID3v2 tags, possibly followed by zero padding.
    while data.len() >= start + 10 && &data[start..start + 3] == b"ID3" {
        let footer = if data[start + 5] & 0x10 != 0 { 10 } else { 0 };
        let size = syncsafe(&data[start + 6..start + 10])?;
        start = (start + 10 + footer)
            .checked_add(size)
            .filter(|&end| end <= data.len())
            .ok_or(MediaError::Malformed("ID3-Block zu lang"))?;
        while data.get(start) == Some(&0) {
            start += 1;
        }
    }
    let mut end = data.len();
    loop {
        let rest = end - start;
        if rest >= 128 && &data[end - 128..end - 125] == b"TAG" {
            end -= 128;
        } else if rest >= 32 && &data[end - 32..end - 24] == b"APETAGEX" {
            let size = le32(data, end - 20)? as usize;
            let header = if le32(data, end - 12)? & 0x8000_0000 != 0 {
                32
            } else {
                0
            };
            let total = size
                .checked_add(header)
                .filter(|&total| total >= 32 && total <= rest)
                .ok_or(MediaError::Malformed("APE-Block"))?;
            end -= total;
        } else if rest >= 15 && &data[end - 9..end] == b"LYRICS200" {
            let digits = std::str::from_utf8(&data[end - 15..end - 9])
                .ok()
                .and_then(|text| text.parse::<usize>().ok())
                .ok_or(MediaError::Malformed("Lyrics3-Block"))?;
            let total = digits
                .checked_add(15)
                .filter(|&total| total <= rest)
                .ok_or(MediaError::Malformed("Lyrics3-Block"))?;
            end -= total;
        } else if rest >= 20 && &data[end - 10..end - 7] == b"3DI" {
            let total = syncsafe(&data[end - 4..end])?
                .checked_add(20)
                .filter(|&total| total <= rest)
                .ok_or(MediaError::Malformed("ID3-Block am Ende"))?;
            end -= total;
        } else {
            break;
        }
    }
    let frames = &data[start..end];
    let first = frame_length(frames).ok_or(MediaError::Malformed("kein MP3-Frame"))?;
    // A second frame must follow the first one, unless the file holds a single frame.
    if frames.len() > first && frame_length(&frames[first..]).is_none() {
        return Err(MediaError::Malformed("MP3-Frames passen nicht"));
    }
    Ok(frames.to_vec())
}

/// Whether `data` starts with an MPEG audio layer III frame header.
pub fn is_frame(data: &[u8]) -> bool {
    frame_length(data).is_some()
}

/// The length of the layer III frame at the start of `data`, from its header.
fn frame_length(data: &[u8]) -> Option<usize> {
    let header = data.get(..4)?;
    if header[0] != 0xFF || header[1] & 0xE0 != 0xE0 {
        return None;
    }
    let version = (header[1] >> 3) & 3; // 0: MPEG 2.5, 2: MPEG 2, 3: MPEG 1
    let layer = (header[1] >> 1) & 3; // 1: layer III
    if version == 1 || layer != 1 {
        return None;
    }
    let bitrate = usize::from(header[2] >> 4);
    let rate = usize::from((header[2] >> 2) & 3);
    if bitrate == 0 || bitrate == 15 || rate == 3 {
        return None;
    }
    const MPEG1: [usize; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const MPEG2: [usize; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let (kbps, hz, factor) = match version {
        3 => (MPEG1[bitrate], [44_100, 48_000, 32_000][rate], 144),
        2 => (MPEG2[bitrate], [22_050, 24_000, 16_000][rate], 72),
        _ => (MPEG2[bitrate], [11_025, 12_000, 8_000][rate], 72),
    };
    let padding = usize::from((header[2] >> 1) & 1);
    Some(factor * kbps * 1000 / hz + padding)
}
