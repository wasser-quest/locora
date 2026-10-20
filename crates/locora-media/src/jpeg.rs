//! JPEG: the segments before the image data are copied except metadata (EXIF, XMP, IPTC,
//! comments, other application blocks); the colour profile and the Adobe colour block stay.
//! Data after the end of the image (extra pictures or a motion clip some phones append, each
//! with its own metadata) is dropped.

use crate::exif;
use crate::{MediaError, Result, be16};

const SOI: u8 = 0xD8;
const EOI: u8 = 0xD9;
const SOS: u8 = 0xDA;
const APP0: u8 = 0xE0;
const APP1: u8 = 0xE1;
const APP2: u8 = 0xE2;
const APP14: u8 = 0xEE;

pub fn clean(data: &[u8]) -> Result<Vec<u8>> {
    if data.get(..2) != Some(&[0xFF, SOI]) {
        return Err(MediaError::Malformed("kein JPEG-Anfang"));
    }
    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(&[0xFF, SOI]);
    let mut orientation = None;
    // Where the orientation block goes: after the JFIF block if there is one, else after SOI.
    let mut insert_at = out.len();
    let mut first = true;
    let mut at = 2;
    loop {
        if data.get(at) != Some(&0xFF) {
            return Err(MediaError::Malformed("Segment erwartet"));
        }
        while data.get(at + 1) == Some(&0xFF) {
            at += 1;
        }
        let marker = *data
            .get(at + 1)
            .ok_or(MediaError::Malformed("Ende ohne EOI"))?;
        if marker == EOI {
            out.extend_from_slice(&[0xFF, EOI]);
            break;
        }
        if matches!(marker, 0x01 | 0xD0..=0xD7) {
            out.extend_from_slice(&[0xFF, marker]);
            at += 2;
            continue;
        }
        let length = usize::from(be16(data, at + 2)?);
        let end = at + 2 + length;
        let segment = data
            .get(at..end)
            .ok_or(MediaError::Malformed("Segment zu kurz"))?;
        let payload = &segment[4..];
        let keep = match marker {
            APP0 => true,
            APP1 => {
                if payload.starts_with(b"Exif\0\0") {
                    orientation = orientation.or(exif::orientation(payload));
                }
                false
            }
            APP2 => payload.starts_with(b"ICC_PROFILE\0"),
            APP14 => true,
            0xE3..=0xEF | 0xFE => false,
            _ => true,
        };
        if keep {
            out.extend_from_slice(segment);
            if first && marker == APP0 {
                insert_at = out.len();
            }
        }
        first = false;
        at = end;
        if marker == SOS {
            // The entropy-coded data runs to the next marker that is not a stuffed byte or restart.
            let start = at;
            while at + 1 < data.len()
                && !(data[at] == 0xFF && !matches!(data[at + 1], 0x00 | 0xD0..=0xD7 | 0xFF))
            {
                at += 1;
            }
            if at + 1 >= data.len() {
                return Err(MediaError::Malformed("Bilddaten ohne Ende"));
            }
            out.extend_from_slice(&data[start..at]);
        }
    }
    if let Some(orientation) = orientation {
        let tiff = exif::minimal_tiff(orientation);
        let length: u16 = exif::fits(2 + 6 + tiff.len())?;
        let mut segment = vec![0xFF, APP1];
        segment.extend_from_slice(&length.to_be_bytes());
        segment.extend_from_slice(b"Exif\0\0");
        segment.extend_from_slice(&tiff);
        out.splice(insert_at..insert_at, segment);
    }
    Ok(out)
}
