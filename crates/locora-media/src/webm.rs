//! WebM (and Matroska with the `webm` document type), changed in place so no offset moves, like
//! the `free` boxes of an MP4: `Tags`, `Chapters` and `Attachments` and the `Info` entries that say
//! when and with what a file was made (`DateUTC`, `Title`, `MuxingApp`, `WritingApp`, file names)
//! become `Void` elements of exactly the same size, as do track names. Checksums (`CRC-32`) of
//! changed parts are voided too. The frames are never touched.
//!
//! Recorders in browsers (MediaRecorder) leave the `Segment` and its `Cluster`s without a size:
//! such a cluster ends where the next element of the segment's own level begins.

use crate::{MediaError, Result};

const EBML: u32 = 0x1A45_DFA3;
const DOC_TYPE: u32 = 0x4282;
const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114D_9B74;
const INFO: u32 = 0x1549_A966;
const TRACKS: u32 = 0x1654_AE6B;
const CUES: u32 = 0x1C53_BB6B;
const CLUSTER: u32 = 0x1F43_B675;
const TAGS: u32 = 0x1254_C367;
const CHAPTERS: u32 = 0x1043_A770;
const ATTACHMENTS: u32 = 0x1941_A469;
const VOID: u32 = 0xEC;
const CRC32: u32 = 0xBF;
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;

/// `Info` entries that say when, where from or with what the file was made.
const INFO_METADATA: [u32; 7] = [
    0x4461,    // DateUTC
    0x7BA9,    // Title
    0x4D80,    // MuxingApp
    0x5741,    // WritingApp
    0x7384,    // SegmentFilename
    0x3C_B923, // PrevFilename
    0x3E_B923, // NextFilename
];
/// `TrackEntry` entries with free text: Name.
const TRACK_METADATA: [u32; 1] = [0x536E];

/// The elements a segment holds; an unsized cluster ends where one of them begins.
const SEGMENT_LEVEL: [u32; 10] = [
    SEEK_HEAD,
    INFO,
    TRACKS,
    CUES,
    CLUSTER,
    TAGS,
    CHAPTERS,
    ATTACHMENTS,
    EBML,
    SEGMENT,
];

/// Video codecs that are kept as they are.
const VIDEO: [&str; 5] = [
    "V_VP8",
    "V_VP9",
    "V_AV1",
    "V_MPEG4/ISO/AVC",
    "V_MPEGH/ISO/HEVC",
];
/// Sound codecs that are kept as they are (AAC with or without a profile suffix).
const SOUND: [&str; 4] = ["A_OPUS", "A_VORBIS", "A_AAC", "A_FLAC"];

/// An element's header: its id, where its content starts, and its size (`None`: unknown).
fn header(data: &[u8], at: usize) -> Result<(u32, usize, Option<usize>)> {
    let (id, length) = id(data, at)?;
    let first = *data
        .get(at + length)
        .ok_or(MediaError::Malformed("WebM-Element zu kurz"))?;
    let size_length = first.leading_zeros() as usize + 1;
    if size_length > 8 {
        return Err(MediaError::Malformed("WebM-Größe ungültig"));
    }
    let bytes = data
        .get(at + length..at + length + size_length)
        .ok_or(MediaError::Malformed("WebM-Element zu kurz"))?;
    let mask = if size_length == 8 {
        0
    } else {
        0xFFu8 >> size_length
    };
    let mut value = u64::from(first & mask);
    for &b in &bytes[1..] {
        value = value << 8 | u64::from(b);
    }
    let unknown = value == (1u64 << (7 * size_length)) - 1;
    let size = if unknown {
        None
    } else {
        Some(usize::try_from(value).map_err(|_| MediaError::Malformed("WebM-Größe"))?)
    };
    Ok((id, at + length + size_length, size))
}

/// The element id at `at` (with its length marker, as the specification writes ids).
fn id(data: &[u8], at: usize) -> Result<(u32, usize)> {
    let first = *data
        .get(at)
        .ok_or(MediaError::Malformed("WebM-Element zu kurz"))?;
    let length = first.leading_zeros() as usize + 1;
    if length > 4 {
        return Err(MediaError::Malformed("WebM-Kennung ungültig"));
    }
    let bytes = data
        .get(at..at + length)
        .ok_or(MediaError::Malformed("WebM-Element zu kurz"))?;
    Ok((
        bytes.iter().fold(0, |id, &b| id << 8 | u32::from(b)),
        length,
    ))
}

/// The end of an element whose content runs from `body` with `size` bytes, inside `to`.
fn end_of(body: usize, size: usize, to: usize) -> Result<usize> {
    body.checked_add(size)
        .filter(|&end| end <= to)
        .ok_or(MediaError::Malformed("WebM-Element zu lang"))
}

/// Overwrites `data[start..end]` with a `Void` element of the same length.
fn void(data: &mut [u8], start: usize, end: usize) {
    let total = end - start;
    // Pick the shortest size field that can hold the rest (a valid element has at least two bytes).
    let Some(length) = (1..=8usize).find(|&length| {
        total > length && ((total - 1 - length) as u64) < (1u64 << (7 * length)) - 1
    }) else {
        return;
    };
    let size = (total - 1 - length) as u64 | 1u64 << (7 * length);
    data[start] = VOID as u8;
    data[start + 1..start + 1 + length].copy_from_slice(&size.to_be_bytes()[8 - length..]);
    data[start + 1 + length..end].fill(0);
}

/// The elements inside `data[from..to]` as (id, start, body, end); all must have a size.
fn children(data: &[u8], from: usize, to: usize) -> Result<Vec<(u32, usize, usize, usize)>> {
    let mut found = Vec::new();
    let mut at = from;
    while at < to {
        let (id, body, size) = header(data, at)?;
        let size = size.ok_or(MediaError::Malformed("WebM-Element ohne Größe"))?;
        let end = end_of(body, size, to)?;
        found.push((id, at, body, end));
        at = end;
    }
    Ok(found)
}

/// Voids the children of `data[from..to]` with the given ids, and the checksum if any changed.
fn void_children(data: &mut [u8], from: usize, to: usize, ids: &[u32]) -> Result<()> {
    let parts = children(data, from, to)?;
    if !parts.iter().any(|(id, ..)| ids.contains(id)) {
        return Ok(());
    }
    for (id, start, _, end) in parts {
        if ids.contains(&id) || id == CRC32 {
            void(data, start, end);
        }
    }
    Ok(())
}

/// Where an unsized cluster starting its content at `body` ends: at the next element of the
/// segment's level, or at `to`. A cut-off last block counts as the end.
fn cluster_end(data: &[u8], body: usize, to: usize) -> Result<usize> {
    let mut at = body;
    while at < to {
        let (id, _) = id(data, at)?;
        if SEGMENT_LEVEL.contains(&id) {
            return Ok(at);
        }
        let (_, inner, size) = header(data, at)?;
        let size = size.ok_or(MediaError::Malformed("WebM-Element ohne Größe"))?;
        match inner.checked_add(size) {
            Some(end) if end <= to => at = end,
            _ => return Ok(to),
        }
    }
    Ok(to)
}

#[derive(Default)]
struct Found {
    doc_type: Option<String>,
    /// (track type, codec id) of every track.
    tracks: Vec<(u64, String)>,
    segments: usize,
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_owned()
}

fn read_tracks(data: &mut [u8], from: usize, to: usize, found: &mut Found) -> Result<()> {
    let mut changed = false;
    for (id, _, body, end) in children(data, from, to)? {
        if id != TRACK_ENTRY {
            continue;
        }
        let mut kind = 0;
        let mut codec = String::new();
        for (part, _, inner, part_end) in children(data, body, end)? {
            match part {
                TRACK_TYPE => {
                    kind = data[inner..part_end]
                        .iter()
                        .take(8)
                        .fold(0u64, |value, &b| value << 8 | u64::from(b));
                }
                CODEC_ID => codec = text(&data[inner..part_end]),
                part if TRACK_METADATA.contains(&part) => changed = true,
                _ => {}
            }
        }
        void_children(data, body, end, &TRACK_METADATA)?;
        found.tracks.push((kind, codec));
    }
    if changed {
        for (id, start, _, end) in children(data, from, to)? {
            if id == CRC32 {
                void(data, start, end);
            }
        }
    }
    Ok(())
}

fn clean_segment(data: &mut [u8], from: usize, to: usize, found: &mut Found) -> Result<()> {
    found.segments += 1;
    let mut at = from;
    while at < to {
        let (id, body, size) = header(data, at)?;
        let end = match size {
            Some(size) if id == CLUSTER => body.checked_add(size).map_or(to, |end| end.min(to)),
            Some(size) => end_of(body, size, to)?,
            None if id == CLUSTER => cluster_end(data, body, to)?,
            None => return Err(MediaError::Malformed("WebM-Element ohne Größe")),
        };
        match id {
            INFO => void_children(data, body, end, &INFO_METADATA)?,
            TRACKS => read_tracks(data, body, end, found)?,
            TAGS | CHAPTERS | ATTACHMENTS => void(data, at, end),
            _ => {}
        }
        at = end;
    }
    Ok(())
}

/// Cleans a WebM file in place. `Some(reason)` when it needs a conversion instead (another
/// document type, codec or kind of track). With `sound_only`, a video track is refused.
pub fn clean(data: &mut [u8], sound_only: bool) -> Result<Option<String>> {
    let mut found = Found::default();
    let mut at = 0;
    while at < data.len() {
        let (id, body, size) = header(data, at)?;
        if at == 0 && id != EBML {
            return Err(MediaError::Malformed("kein EBML-Kopf"));
        }
        let end = match size {
            Some(size) => end_of(body, size, data.len())?,
            None if id == SEGMENT => data.len(),
            None => return Err(MediaError::Malformed("WebM-Element ohne Größe")),
        };
        match id {
            EBML => {
                for (part, _, inner, part_end) in children(data, body, end)? {
                    if part == DOC_TYPE {
                        found.doc_type = Some(text(&data[inner..part_end]));
                    }
                }
            }
            SEGMENT => clean_segment(data, body, end, &mut found)?,
            _ => {}
        }
        at = end;
    }
    if found.segments != 1 {
        return Err(MediaError::Malformed("WebM braucht genau ein Segment"));
    }
    decide(&found, sound_only)
}

fn decide(found: &Found, sound_only: bool) -> Result<Option<String>> {
    let doc_type = found.doc_type.as_deref().unwrap_or("?");
    if doc_type != "webm" {
        return Ok(Some(format!(
            "Matroska-Dateien (Typ {doc_type}) werden umgewandelt"
        )));
    }
    let (mut picture, mut sound) = (false, false);
    for (kind, codec) in &found.tracks {
        let sound_codec = SOUND
            .iter()
            .any(|c| codec == c || (*c == "A_AAC" && codec.starts_with("A_AAC/")));
        match kind {
            1 if sound_only => {
                return Err(MediaError::Refused(
                    "Die Datei hat neben dem Ton eine Videospur. Bitte nur den Ton speichern (z. B. als WebM-Audio, M4A oder MP3)."
                        .into(),
                ));
            }
            1 if VIDEO.contains(&codec.as_str()) => picture = true,
            1 => return Ok(Some(format!("Video im Format {codec} wird umgewandelt"))),
            2 if sound_codec => sound = true,
            2 => return Ok(Some(format!("Ton im Format {codec} wird umgewandelt"))),
            _ => {
                return Ok(Some(
                    "Die Datei hat Zusatzspuren (z. B. Untertitel); sie wird umgewandelt".into(),
                ));
            }
        }
    }
    if sound_only && !sound {
        return Err(MediaError::Malformed("keine Tonspur"));
    }
    if !sound_only && !picture {
        return Err(MediaError::Malformed("keine Videospur"));
    }
    Ok(None)
}
