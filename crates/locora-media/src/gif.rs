//! GIF: comment extensions and embedded XMP are dropped; data after the trailer too.

use crate::{MediaError, Result};

pub fn clean(data: &[u8]) -> Result<Vec<u8>> {
    if !(data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
        return Err(MediaError::Malformed("keine GIF-Signatur"));
    }
    let screen = data.get(6..13).ok_or(MediaError::Malformed("zu kurz"))?;
    let mut at = 13 + color_table(screen[4]);
    let mut out = data
        .get(..at)
        .ok_or(MediaError::Malformed("Farbtabelle zu kurz"))?
        .to_vec();
    loop {
        match *data
            .get(at)
            .ok_or(MediaError::Malformed("Ende ohne Abschluss"))?
        {
            0x3B => {
                out.push(0x3B);
                return Ok(out);
            }
            0x21 => {
                let label = *data
                    .get(at + 1)
                    .ok_or(MediaError::Malformed("Erweiterung zu kurz"))?;
                let end = sub_blocks(data, at + 2)?;
                let xmp = label == 0xFF && data.get(at + 3..at + 14) == Some(b"XMP DataXMP");
                if label != 0xFE && !xmp {
                    out.extend_from_slice(&data[at..end]);
                }
                at = end;
            }
            0x2C => {
                let descriptor = data
                    .get(at..at + 10)
                    .ok_or(MediaError::Malformed("Bildblock zu kurz"))?;
                // Descriptor, local colour table, LZW code size, then the image data blocks.
                let start = at + 10 + color_table(descriptor[9]) + 1;
                let end = sub_blocks(data, start)?;
                out.extend_from_slice(&data[at..end]);
                at = end;
            }
            _ => return Err(MediaError::Malformed("unbekannter GIF-Block")),
        }
    }
}

fn color_table(flags: u8) -> usize {
    if flags & 0x80 == 0 {
        0
    } else {
        3 << ((flags & 0x07) + 1)
    }
}

/// The end of a chain of data sub-blocks that starts at `at` (after its terminating zero).
fn sub_blocks(data: &[u8], mut at: usize) -> Result<usize> {
    loop {
        let size = usize::from(
            *data
                .get(at)
                .ok_or(MediaError::Malformed("Datenblock zu kurz"))?,
        );
        at += 1 + size;
        if size == 0 {
            return Ok(at);
        }
        if at > data.len() {
            return Err(MediaError::Malformed("Datenblock zu kurz"));
        }
    }
}
