//! MP4, MOV and AVIF (ISO base media files), changed in place so no offset moves: metadata boxes
//! become `free` boxes with zeroed content, creation times are zeroed, and an image's EXIF and
//! XMP items are overwritten with zeros. A video is checked for H.264 and, at most, AAC or Opus
//! sound; a QuickTime file of those is marked as MP4.

use crate::{MediaError, Result, be16, be32, be64};

/// The XMP box of Adobe, a `uuid` box.
const XMP_UUID: [u8; 16] = [
    0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8, 0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
];

#[derive(Clone, Copy)]
struct Box {
    kind: [u8; 4],
    /// Where the box starts, where its content starts, and where it ends.
    start: usize,
    body: usize,
    end: usize,
}

/// The boxes in `data[from..to]`.
fn boxes(data: &[u8], from: usize, to: usize) -> Result<Vec<Box>> {
    let mut found = Vec::new();
    let mut at = from;
    while at + 8 <= to {
        let size = u64::from(be32(data, at)?);
        let kind: [u8; 4] = data[at + 4..at + 8].try_into().expect("four bytes");
        let (body, size) = match size {
            0 => (at + 8, (to - at) as u64),
            1 => (at + 16, be64(data, at + 8)?),
            size => (at + 8, size),
        };
        let end = usize::try_from(size)
            .ok()
            .and_then(|size| at.checked_add(size))
            .filter(|&end| end <= to && end >= body);
        let end = end.ok_or(MediaError::Malformed("Box-Größe passt nicht"))?;
        found.push(Box {
            kind,
            start: at,
            body,
            end,
        });
        at = end;
    }
    Ok(found)
}

fn child(data: &[u8], parent: &Box, kind: &[u8; 4], skip: usize) -> Result<Option<Box>> {
    Ok(boxes(data, parent.body + skip, parent.end)?
        .into_iter()
        .find(|b| &b.kind == kind))
}

/// Turns a box into a `free` box of the same size, its content zeroed.
fn erase(data: &mut [u8], found: &Box) {
    data[found.start + 4..found.start + 8].copy_from_slice(b"free");
    data[found.body..found.end].fill(0);
}

/// Zeroes the creation and modification time of an `mvhd`, `tkhd` or `mdhd` box.
fn zero_times(data: &mut [u8], found: &Box) {
    let width = if data.get(found.body) == Some(&1) {
        8
    } else {
        4
    };
    let times = found.body + 4..found.body + 4 + 2 * width;
    if times.end <= found.end {
        data[times].fill(0);
    }
}

fn is_metadata(found: &Box, data: &[u8]) -> bool {
    match &found.kind {
        b"udta" | b"meta" | b"XMP_" => true,
        b"uuid" => data.get(found.body..found.body + 16) == Some(&XMP_UUID),
        _ => false,
    }
}

/// Cleans a video in place. `Some(reason)` when it needs a conversion instead.
pub fn clean_video(data: &mut [u8]) -> Result<Option<String>> {
    let top = boxes(data, 0, data.len())?;
    let ftyp = top.first().filter(|b| &b.kind == b"ftyp").copied();
    let ftyp = ftyp.ok_or(MediaError::Malformed("kein ftyp-Block"))?;
    let moov = top.iter().find(|b| &b.kind == b"moov").copied();
    let moov = moov.ok_or(MediaError::Malformed("kein moov-Block"))?;
    if top.iter().any(|b| &b.kind == b"moof") {
        return Ok(Some(
            "Fragmentierte Videos werden neu als MP4 gespeichert".into(),
        ));
    }
    let mut picture = false;
    for track in boxes(data, moov.body, moov.end)?
        .iter()
        .filter(|b| &b.kind == b"trak")
    {
        let (handler, codec) = track_codec(data, track)?;
        match (&handler, codec.as_ref().map(|c| &c[..])) {
            (b"vide", Some(b"avc1" | b"avc3")) => picture = true,
            (b"vide", other) => return Ok(Some(needs("Video", other))),
            (b"soun", Some(b"mp4a" | b"Opus")) => {}
            (b"soun", other) => return Ok(Some(needs("Ton", other))),
            // Timed metadata (GPS tracks of action cameras, camera motion) and other tracks.
            _ => {
                return Ok(Some(
                    "Das Video hat Zusatzspuren (z. B. GPS); es wird neu als MP4 gespeichert"
                        .into(),
                ));
            }
        }
    }
    if !picture {
        return Err(MediaError::Malformed("keine Videospur"));
    }
    for found in &top {
        if is_metadata(found, data) {
            erase(data, found);
        }
    }
    clean_movie(data, &moov)?;
    mark_as_mp4(data, &ftyp)?;
    Ok(None)
}

fn needs(what: &str, codec: Option<&[u8]>) -> String {
    let codec = codec
        .map(|c| String::from_utf8_lossy(c).trim().to_owned())
        .unwrap_or_else(|| "?".into());
    format!("{what} im Format {codec} wird als MP4 (H.264) gespeichert")
}

/// The handler type and the first sample entry of a track.
fn track_codec(data: &[u8], track: &Box) -> Result<([u8; 4], Option<[u8; 4]>)> {
    let Some(mdia) = child(data, track, b"mdia", 0)? else {
        return Ok((*b"none", None));
    };
    let handler = match child(data, &mdia, b"hdlr", 0)? {
        Some(hdlr) => data
            .get(hdlr.body + 8..hdlr.body + 12)
            .and_then(|b| b.try_into().ok())
            .unwrap_or(*b"none"),
        None => *b"none",
    };
    let stsd = match child(data, &mdia, b"minf", 0)? {
        Some(minf) => match child(data, &minf, b"stbl", 0)? {
            Some(stbl) => child(data, &stbl, b"stsd", 0)?,
            None => None,
        },
        None => None,
    };
    // stsd: version and flags, an entry count, then the sample entries as boxes.
    let codec = match stsd {
        Some(stsd) => boxes(data, stsd.body + 8, stsd.end)?
            .first()
            .map(|entry| entry.kind),
        None => None,
    };
    Ok((handler, codec))
}

/// Erases the metadata of the movie and its tracks, and zeroes their times.
fn clean_movie(data: &mut [u8], moov: &Box) -> Result<()> {
    for found in boxes(data, moov.body, moov.end)? {
        match &found.kind {
            b"mvhd" => zero_times(data, &found),
            b"trak" => {
                for part in boxes(data, found.body, found.end)? {
                    match &part.kind {
                        b"tkhd" => zero_times(data, &part),
                        b"mdia" => {
                            if let Some(mdhd) = child(data, &part, b"mdhd", 0)? {
                                zero_times(data, &mdhd);
                            }
                        }
                        _ if is_metadata(&part, data) => erase(data, &part),
                        _ => {}
                    }
                }
            }
            _ if is_metadata(&found, data) => erase(data, &found),
            _ => {}
        }
    }
    Ok(())
}

/// A QuickTime file with MP4 codecs: its brands are set to MP4's, in place.
fn mark_as_mp4(data: &mut [u8], ftyp: &Box) -> Result<()> {
    let brands = data
        .get_mut(ftyp.body..ftyp.end)
        .ok_or(MediaError::Malformed("ftyp zu kurz"))?;
    if brands.len() < 8 || &brands[..4] != b"qt  " {
        return Ok(());
    }
    brands[..4].copy_from_slice(b"isom");
    let compatible = [*b"isom", *b"mp42", *b"avc1"];
    for (index, slot) in brands[8..].chunks_exact_mut(4).enumerate() {
        slot.copy_from_slice(&compatible[index.min(2)]);
    }
    Ok(())
}

/// Cleans an AVIF picture in place: metadata boxes are erased, and the EXIF and XMP items are
/// overwritten with zeros (their place in the file stays, so no offset moves).
pub fn clean_image(data: &mut [u8]) -> Result<()> {
    let top = boxes(data, 0, data.len())?;
    let meta = top.iter().find(|b| &b.kind == b"meta").copied();
    let meta = meta.ok_or(MediaError::Malformed("kein meta-Block"))?;
    // meta is a full box: version and flags come first.
    let parts = boxes(data, meta.body + 4, meta.end)?;
    let metadata_items = match parts.iter().find(|b| &b.kind == b"iinf") {
        Some(iinf) => metadata_items(data, iinf)?,
        None => Vec::new(),
    };
    if let Some(iloc) = parts.iter().find(|b| &b.kind == b"iloc") {
        let idat = parts.iter().find(|b| &b.kind == b"idat").map(|b| b.body);
        for (start, length) in item_extents(data, iloc, &metadata_items, idat)? {
            let end = start.checked_add(length).filter(|&end| end <= data.len());
            let end = end.ok_or(MediaError::Malformed("Element außerhalb der Datei"))?;
            data[start..end].fill(0);
        }
    }
    for found in top.iter().filter(|b| &b.kind != b"meta") {
        if is_metadata(found, data) {
            erase(data, found);
        }
    }
    Ok(())
}

/// The ids of the EXIF and XMP items of an `iinf` box.
fn metadata_items(data: &[u8], iinf: &Box) -> Result<Vec<u32>> {
    let version = *data
        .get(iinf.body)
        .ok_or(MediaError::Malformed("iinf zu kurz"))?;
    let first = iinf.body + 4 + if version == 0 { 2 } else { 4 };
    let mut ids = Vec::new();
    for infe in boxes(data, first, iinf.end)?
        .iter()
        .filter(|b| &b.kind == b"infe")
    {
        let version = data[infe.body];
        if version < 2 {
            continue;
        }
        let (id, after) = if version == 2 {
            (u32::from(be16(data, infe.body + 4)?), infe.body + 6)
        } else {
            (be32(data, infe.body + 4)?, infe.body + 8)
        };
        let kind = data
            .get(after + 2..after + 6)
            .ok_or(MediaError::Malformed("infe zu kurz"))?;
        let xmp = kind == b"mime"
            && data
                .get(after + 6..infe.end)
                .is_some_and(|rest| rest.windows(7).any(|w| w == b"rdf+xml"));
        if kind == b"Exif" || xmp {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Where the given items lie in the file, from an `iloc` box: (start, length) pairs.
fn item_extents(
    data: &[u8],
    iloc: &Box,
    items: &[u32],
    idat: Option<usize>,
) -> Result<Vec<(usize, usize)>> {
    let version = data[iloc.body];
    let sizes = be16(data, iloc.body + 4)?;
    let (offset_size, length_size) = (usize::from(sizes >> 12), usize::from((sizes >> 8) & 0xF));
    let (base_size, index_size) = (usize::from((sizes >> 4) & 0xF), usize::from(sizes & 0xF));
    let read = |at: usize, width: usize| -> Result<u64> {
        match width {
            0 => Ok(0),
            4 => Ok(u64::from(be32(data, at)?)),
            8 => be64(data, at),
            _ => Err(MediaError::Malformed("iloc-Feldbreite")),
        }
    };
    let mut at = iloc.body + 6;
    let count = if version < 2 {
        at += 2;
        u32::from(be16(data, at - 2)?)
    } else {
        at += 4;
        be32(data, at - 4)?
    };
    let mut extents = Vec::new();
    for _ in 0..count {
        let id = if version < 2 {
            at += 2;
            u32::from(be16(data, at - 2)?)
        } else {
            at += 4;
            be32(data, at - 4)?
        };
        let method = if version >= 1 {
            at += 2;
            be16(data, at - 2)? & 0xF
        } else {
            0
        };
        at += 2; // data reference index
        let base = read(at, base_size)?;
        at += base_size;
        let extent_count = be16(data, at)?;
        at += 2;
        for _ in 0..extent_count {
            if version >= 1 {
                at += index_size;
            }
            let offset = read(at, offset_size)?;
            at += offset_size;
            let length = read(at, length_size)?;
            at += length_size;
            if !items.contains(&id) {
                continue;
            }
            let origin = match method {
                0 => 0,
                1 => idat.ok_or(MediaError::Malformed("idat fehlt"))? as u64,
                _ => return Err(MediaError::Malformed("iloc-Verfahren")),
            };
            let start = usize::try_from(origin + base + offset)
                .map_err(|_| MediaError::Malformed("iloc"))?;
            let length = usize::try_from(length).map_err(|_| MediaError::Malformed("iloc"))?;
            extents.push((start, length));
        }
    }
    Ok(extents)
}
