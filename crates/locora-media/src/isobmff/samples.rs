//! Where the samples of a track lie in the file, from its sample tables: used to zero the samples
//! of a track that is removed (GPS and other timed metadata).

use super::{Box, boxes, child};
use crate::{MediaError, Result, be32, be64};

/// Where the samples of a track lie in the file, chunk by chunk: (start, length) pairs, from its
/// chunk offsets (`stco`/`co64`), chunks (`stsc`) and sample sizes (`stsz`/`stz2`). A track
/// without chunk offsets has no samples in the file.
pub fn samples(data: &[u8], track: &Box) -> Result<Vec<(usize, usize)>> {
    let Some(stbl) = sample_table(data, track)? else {
        return Ok(Vec::new());
    };
    let parts = boxes(data, stbl.body, stbl.end)?;
    let find = |kind: &[u8; 4]| parts.iter().find(|b| &b.kind == kind).copied();
    // Full boxes: version and flags, an entry count, then the entries.
    let entries = |table: &Box, width: usize| -> Result<std::ops::Range<usize>> {
        let count = be32(data, table.body + 4)? as usize;
        Ok(0..count.min((table.end - table.body).saturating_sub(8) / width))
    };
    let offsets: Vec<u64> = match (find(b"stco"), find(b"co64")) {
        (Some(stco), _) => entries(&stco, 4)?
            .map(|i| be32(data, stco.body + 8 + 4 * i).map(u64::from))
            .collect::<Result<_>>()?,
        (None, Some(co64)) => entries(&co64, 8)?
            .map(|i| be64(data, co64.body + 8 + 8 * i))
            .collect::<Result<_>>()?,
        (None, None) => return Ok(Vec::new()),
    };
    let sizes = sample_sizes(data, find(b"stsz"), find(b"stz2"))?;
    // stsc: runs of (first chunk, samples per chunk, sample description).
    let stsc = find(b"stsc").ok_or(MediaError::Malformed("stsc fehlt"))?;
    let runs: Vec<(u64, u64)> = entries(&stsc, 12)?
        .map(|i| {
            let at = stsc.body + 8 + 12 * i;
            Ok((u64::from(be32(data, at)?), u64::from(be32(data, at + 4)?)))
        })
        .collect::<Result<_>>()?;
    let mut extents = Vec::new();
    let mut sample = 0usize;
    for (index, &offset) in offsets.iter().enumerate() {
        let chunk = index as u64 + 1;
        let per_chunk = runs
            .iter()
            .rev()
            .find(|(first, _)| *first <= chunk)
            .map_or(0, |(_, per)| *per);
        let mut length = 0u64;
        for _ in 0..per_chunk {
            let Some(size) = sizes.get(sample) else {
                break;
            };
            length += size;
            sample += 1;
        }
        let start = usize::try_from(offset).map_err(|_| MediaError::Malformed("Spurdaten"))?;
        let length = usize::try_from(length).map_err(|_| MediaError::Malformed("Spurdaten"))?;
        extents.push((start, length));
    }
    Ok(extents)
}

fn sample_table(data: &[u8], track: &Box) -> Result<Option<Box>> {
    let Some(mdia) = child(data, track, b"mdia", 0)? else {
        return Ok(None);
    };
    let Some(minf) = child(data, &mdia, b"minf", 0)? else {
        return Ok(None);
    };
    child(data, &minf, b"stbl", 0)
}

/// The size of every sample, from `stsz` or the compact `stz2`.
fn sample_sizes(data: &[u8], stsz: Option<Box>, stz2: Option<Box>) -> Result<Vec<u64>> {
    if let Some(stsz) = stsz {
        let uniform = be32(data, stsz.body + 4)?;
        let count = be32(data, stsz.body + 8)? as usize;
        if uniform != 0 {
            // More samples than bytes in the file cannot lie in it.
            return Ok(vec![u64::from(uniform); count.min(data.len())]);
        }
        let table = (stsz.end - stsz.body).saturating_sub(12) / 4;
        return (0..count.min(table))
            .map(|i| be32(data, stsz.body + 12 + 4 * i).map(u64::from))
            .collect();
    }
    let stz2 = stz2.ok_or(MediaError::Malformed("stsz fehlt"))?;
    let field = *data
        .get(stz2.body + 7)
        .ok_or(MediaError::Malformed("stz2 zu kurz"))?;
    let count = be32(data, stz2.body + 8)? as usize;
    let table = data
        .get(stz2.body + 12..stz2.end)
        .ok_or(MediaError::Malformed("stz2 zu kurz"))?;
    Ok(match field {
        4 => table
            .iter()
            .flat_map(|b| [u64::from(b >> 4), u64::from(b & 0xF)])
            .take(count)
            .collect(),
        8 => table.iter().take(count).map(|&b| u64::from(b)).collect(),
        16 => table
            .as_chunks::<2>()
            .0
            .iter()
            .take(count)
            .map(|b| u64::from(u16::from_be_bytes(*b)))
            .collect(),
        _ => return Err(MediaError::Malformed("stz2-Feldbreite")),
    })
}
