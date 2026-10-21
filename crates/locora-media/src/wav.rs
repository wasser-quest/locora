//! WAV: only the chunks needed to play the sound are kept (`fmt `, `fact`, `data`), so LIST/INFO
//! texts, `id3 ` tags, broadcast extensions (`bext`, with times and places), cue points and the
//! like are dropped. The samples stay as they are; the RIFF size is written anew.

use crate::{MediaError, Result, le32};

pub fn clean(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 12 || &data[8..12] != b"WAVE" {
        return Err(MediaError::Malformed("kein WAVE-Block"));
    }
    if &data[..4] != b"RIFF" {
        return Err(MediaError::Refused(
            "Dieses WAV (RF64 oder RIFX) wird nicht unterstützt. Bitte als normales WAV, MP3 oder M4A speichern."
                .into(),
        ));
    }
    let riff_end = (le32(data, 4)? as usize)
        .checked_add(8)
        .ok_or(MediaError::Malformed("RIFF-Größe"))?
        .min(data.len());
    let mut kept: Vec<&[u8]> = Vec::new();
    let (mut format, mut sound) = (false, false);
    let mut at = 12;
    while at + 8 <= riff_end {
        let id = &data[at..at + 4];
        let size = le32(data, at + 4)? as usize;
        let body_end = (at + 8)
            .checked_add(size)
            .filter(|&end| end <= riff_end)
            .ok_or(MediaError::Malformed("WAV-Block zu lang"))?;
        // Chunks of odd size are followed by a pad byte (missing at the very end of some files).
        let next = (body_end + (size & 1)).min(riff_end);
        match id {
            b"fmt " => {
                if format || sound || size < 16 {
                    return Err(MediaError::Malformed("fmt-Block"));
                }
                let tag = u16::from_le_bytes([data[at + 8], data[at + 9]]);
                if !matches!(tag, 1 | 3 | 0xFFFE) {
                    return Err(MediaError::Refused(
                        "Dieses WAV ist komprimiert und spielt nicht überall. Bitte als PCM-WAV, MP3 oder M4A speichern."
                            .into(),
                    ));
                }
                format = true;
            }
            b"data" => {
                if !format || sound {
                    return Err(MediaError::Malformed("data-Block"));
                }
                sound = true;
            }
            b"fact" => {}
            _ => {
                at = next;
                continue;
            }
        }
        kept.push(&data[at..body_end]);
        at = next;
    }
    if !sound {
        return Err(MediaError::Malformed("kein data-Block"));
    }
    let mut out = Vec::with_capacity(riff_end);
    out.extend_from_slice(b"RIFF\0\0\0\0WAVE");
    for chunk in kept {
        out.extend_from_slice(chunk);
        if chunk.len() % 2 == 1 {
            out.push(0);
        }
    }
    let size = u32::try_from(out.len() - 8).map_err(|_| MediaError::Malformed("zu groß"))?;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    Ok(out)
}
