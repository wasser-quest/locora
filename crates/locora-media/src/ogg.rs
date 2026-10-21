//! Ogg with Opus or Vorbis: the comment header (title, artist, cover pictures, sometimes dates
//! and places) is written anew with its vendor string and no comments. Only the header pages are
//! built again; the audio pages keep their content and get new sequence numbers and checksums.
//! Files with more than one stream (chained or multiplexed) are refused.

use crate::{MediaError, Result, le32};

#[derive(Clone, Copy)]
struct Page {
    start: usize,
    flags: u8,
    serial: u32,
    /// Lacing values and body.
    lacing: (usize, usize),
    body: (usize, usize),
    end: usize,
}

const CONTINUED: u8 = 0x01;
const FIRST: u8 = 0x02;

fn pages(data: &[u8]) -> Result<Vec<Page>> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let header = data
            .get(at..at + 27)
            .ok_or(MediaError::Malformed("Ogg-Seite zu kurz"))?;
        if &header[..4] != b"OggS" || header[4] != 0 {
            return Err(MediaError::Malformed("keine Ogg-Seite"));
        }
        let count = usize::from(header[26]);
        let lacing = (at + 27, at + 27 + count);
        let table = data
            .get(lacing.0..lacing.1)
            .ok_or(MediaError::Malformed("Ogg-Seite zu kurz"))?;
        let length: usize = table.iter().map(|&b| usize::from(b)).sum();
        let end = lacing.1 + length;
        if end > data.len() {
            return Err(MediaError::Malformed("Ogg-Seite zu kurz"));
        }
        let stored = le32(data, at + 22)?;
        if crc(&data[at..end], true) != stored {
            return Err(MediaError::Malformed("Ogg-Prüfsumme"));
        }
        found.push(Page {
            start: at,
            flags: header[5],
            serial: le32(data, at + 14)?,
            lacing,
            body: (lacing.1, end),
            end,
        });
        at = end;
    }
    Ok(found)
}

/// Adds the packets of `page` to `out`, each with whether it ended (a last lacing value below 255).
fn add_packets(data: &[u8], page: &Page, out: &mut Vec<(Vec<u8>, bool)>) {
    let mut at = page.body.0;
    for &value in &data[page.lacing.0..page.lacing.1] {
        let value = usize::from(value);
        match out.last_mut() {
            Some((packet, false)) => packet.extend_from_slice(&data[at..at + value]),
            _ => out.push((data[at..at + value].to_vec(), false)),
        }
        if value < 255 {
            out.last_mut().expect("just pushed").1 = true;
        }
        at += value;
    }
}

fn packets(data: &[u8], pages: &[Page]) -> Vec<(Vec<u8>, bool)> {
    let mut out = Vec::new();
    for page in pages {
        add_packets(data, page, &mut out);
    }
    out
}

pub fn clean(data: &[u8]) -> Result<Vec<u8>> {
    let all = pages(data)?;
    let first = *all.first().ok_or(MediaError::Malformed("leer"))?;
    if first.flags & FIRST == 0 || all.iter().any(|p| p.serial != first.serial) {
        return Err(MediaError::Refused(
            "Diese Ogg-Datei hat mehrere Spuren. Bitte als einfache Ogg-, MP3- oder M4A-Datei speichern."
                .into(),
        ));
    }
    if all[1..].iter().any(|p| p.flags & FIRST != 0) {
        return Err(MediaError::Malformed("Ogg-Anfang doppelt"));
    }
    let head = packets(data, &all[..1]);
    let (opus, needed) = match head.as_slice() {
        [(packet, true)] if packet.starts_with(b"OpusHead") => (true, 1),
        [(packet, true)] if packet.starts_with(b"\x01vorbis") => (false, 2),
        _ => {
            return Err(MediaError::Refused(
                "Nur Ogg mit Opus oder Vorbis wird unterstützt. Bitte als MP3 oder M4A speichern."
                    .into(),
            ));
        }
    };
    // The header packets after the first page end on a page of their own (the codecs' rules).
    let mut headers = None;
    let mut found = Vec::new();
    for (last, page) in all.iter().enumerate().skip(1) {
        add_packets(data, page, &mut found);
        if found.len() >= needed && found[needed - 1].1 {
            if found.len() > needed {
                return Err(MediaError::Malformed("Kopfdaten und Ton auf einer Seite"));
            }
            headers = Some(last);
            break;
        }
    }
    let last = headers.ok_or(MediaError::Malformed("Kommentarkopf fehlt"))?;
    found[0].0 = without_comments(&found[0].0, opus)?;
    let mut out = data[..first.end].to_vec();
    let mut sequence = 1;
    for page in paginate(&found, first.serial) {
        out.extend(with_sequence(&page, &mut sequence));
    }
    for page in &all[last + 1..] {
        out.extend(with_sequence(&data[page.start..page.end], &mut sequence));
    }
    Ok(out)
}

/// The comment header with its vendor string and no comments.
fn without_comments(packet: &[u8], opus: bool) -> Result<Vec<u8>> {
    let magic: &[u8] = if opus { b"OpusTags" } else { b"\x03vorbis" };
    if !packet.starts_with(magic) {
        return Err(MediaError::Malformed("Kommentarkopf"));
    }
    let length = le32(packet, magic.len())? as usize;
    let vendor_end = (magic.len() + 4)
        .checked_add(length)
        .filter(|&end| end <= packet.len())
        .ok_or(MediaError::Malformed("Kommentarkopf"))?;
    let mut out = packet[..vendor_end].to_vec();
    out.extend_from_slice(&0u32.to_le_bytes());
    if !opus {
        out.push(1); // framing bit
    }
    Ok(out)
}

/// Pages (sequence numbers and checksums still to be set) for whole header packets.
fn paginate(packets: &[(Vec<u8>, bool)], serial: u32) -> Vec<Vec<u8>> {
    let mut lacing: Vec<u8> = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    for (packet, _) in packets {
        let mut rest = packet.len();
        loop {
            let value = rest.min(255);
            lacing.push(value as u8);
            rest -= value;
            if value < 255 {
                break;
            }
        }
        body.extend_from_slice(packet);
    }
    let mut out = Vec::new();
    let mut offset = 0;
    let mut continued = false;
    for chunk in lacing.chunks(255) {
        let length: usize = chunk.iter().map(|&b| usize::from(b)).sum();
        let ends = chunk.last().is_some_and(|&v| v < 255);
        let mut page = b"OggS\0".to_vec();
        page.push(if continued { CONTINUED } else { 0 });
        // Header pages have granule position 0; a page on which no packet ends has -1.
        let granule: u64 = if chunk.iter().any(|&v| v < 255) {
            0
        } else {
            u64::MAX
        };
        page.extend_from_slice(&granule.to_le_bytes());
        page.extend_from_slice(&serial.to_le_bytes());
        page.extend_from_slice(&[0; 8]);
        page.push(chunk.len() as u8);
        page.extend_from_slice(chunk);
        page.extend_from_slice(&body[offset..offset + length]);
        offset += length;
        continued = !ends;
        out.push(page);
    }
    out
}

/// The page with sequence number `sequence` (then counted up) and its checksum.
fn with_sequence(page: &[u8], sequence: &mut u32) -> Vec<u8> {
    let mut page = page.to_vec();
    page[18..22].copy_from_slice(&sequence.to_le_bytes());
    *sequence += 1;
    let sum = crc(&page, true);
    page[22..26].copy_from_slice(&sum.to_le_bytes());
    page
}

/// The Ogg CRC-32 (polynomial 0x04C11DB7, not reflected) of a page, its checksum field as zero.
pub(crate) fn crc(page: &[u8], skip_field: bool) -> u32 {
    let mut sum = 0u32;
    for (index, &byte) in page.iter().enumerate() {
        let byte = if skip_field && (22..26).contains(&index) {
            0
        } else {
            byte
        };
        sum ^= u32::from(byte) << 24;
        for _ in 0..8 {
            sum = if sum & 0x8000_0000 != 0 {
                (sum << 1) ^ 0x04C1_1DB7
            } else {
                sum << 1
            };
        }
    }
    sum
}

/// An Ogg stream of the given packets for tests: the first packet on its own page, the header
/// packets (`headers` of them) on the next pages, and each further packet on a page of its own.
#[cfg(test)]
pub(crate) fn stream(packets: &[Vec<u8>], headers: usize) -> Vec<u8> {
    let serial = 0x1234_5678;
    let whole = |list: &[Vec<u8>]| list.iter().map(|p| (p.clone(), true)).collect::<Vec<_>>();
    let mut out = Vec::new();
    let mut sequence = 0;
    let mut first = paginate(&whole(&packets[..1]), serial);
    first[0][5] = FIRST;
    for page in first {
        out.extend(with_sequence(&page, &mut sequence));
    }
    for page in paginate(&whole(&packets[1..=headers]), serial) {
        out.extend(with_sequence(&page, &mut sequence));
    }
    for packet in &packets[headers + 1..] {
        for mut page in paginate(&whole(std::slice::from_ref(packet)), serial) {
            page[6..14].copy_from_slice(&960u64.to_le_bytes());
            out.extend(with_sequence(&page, &mut sequence));
        }
    }
    out
}

/// The packets of an Ogg stream, for tests.
#[cfg(test)]
pub(crate) fn read_packets(data: &[u8]) -> Result<Vec<Vec<u8>>> {
    let all = pages(data)?;
    Ok(packets(data, &all).into_iter().map(|(p, _)| p).collect())
}
