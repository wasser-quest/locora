use super::*;

fn ready(cleaned: Cleaned) -> (Vec<u8>, &'static str) {
    match cleaned {
        Cleaned::Ready { bytes, mime } => (bytes, mime),
        other => panic!("expected ready media, got {other:?}"),
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// An EXIF block (big-endian) with the orientation and one more tag (a GPS pointer).
fn exif_tiff(orientation: u16) -> Vec<u8> {
    let mut tiff = b"MM\0\x2a\0\0\0\x08".to_vec();
    tiff.extend_from_slice(&2u16.to_be_bytes());
    tiff.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1]);
    tiff.extend_from_slice(&orientation.to_be_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&[0x88, 0x25, 0, 4, 0, 0, 0, 1, 0, 0, 0, 0x26]);
    tiff.extend_from_slice(&[0, 0, 0, 0]);
    tiff.extend_from_slice(b"GPS 48.21N 9.02E");
    tiff
}

fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0xFF, marker];
    out.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
    out.extend_from_slice(payload);
    out
}

#[test]
fn jpeg_keeps_picture_colour_and_orientation_but_no_metadata() {
    let mut exif = b"Exif\0\0".to_vec();
    exif.extend_from_slice(&exif_tiff(6));
    let scan = [0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56];
    let mut file = vec![0xFF, 0xD8];
    file.extend(segment(0xE0, b"JFIF\0\x01\x02\0\0\x01\0\x01\0\0"));
    file.extend(segment(0xE1, &exif));
    file.extend(segment(
        0xE1,
        b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta>Albstadt</x:xmpmeta>",
    ));
    file.extend(segment(0xE2, b"ICC_PROFILE\0\x01\x01profile"));
    file.extend(segment(0xED, b"Photoshop 3.0\0IPTC city"));
    file.extend(segment(0xFE, b"taken by someone"));
    file.extend(segment(0xDB, &[0; 65]));
    file.extend(segment(0xDA, &[1, 1, 0, 0, 63, 0]));
    file.extend_from_slice(&scan);
    file.extend_from_slice(&[0xFF, 0xD9]);
    file.extend_from_slice(b"\xFF\xD8appended picture with Exif");

    let (out, mime) = ready(clean(file, "image/jpeg").unwrap());
    assert_eq!(mime, "image/jpeg");
    for gone in [&b"GPS"[..], b"Albstadt", b"IPTC", b"taken by", b"appended"] {
        assert!(!contains(&out, gone), "{}", String::from_utf8_lossy(gone));
    }
    assert!(contains(&out, b"ICC_PROFILE\0"));
    assert!(contains(&out, &scan), "the image data is unchanged");
    assert!(out.ends_with(&[0xFF, 0xD9]));
    // The JFIF block stays first; the orientation follows it.
    assert_eq!(&out[2..4], &[0xFF, 0xE0]);
    let app1 = 2 + 4 + 14;
    assert_eq!(&out[app1..app1 + 2], &[0xFF, 0xE1]);
    assert_eq!(exif::orientation(&out[app1 + 4..]), Some(6));
}

#[test]
fn jpeg_without_orientation_gets_no_exif() {
    let mut file = vec![0xFF, 0xD8];
    file.extend(segment(0xFE, b"comment"));
    file.extend(segment(0xDA, &[1, 1, 0, 0, 63, 0]));
    file.extend_from_slice(&[0x01, 0x02, 0xFF, 0xD9]);
    let (out, _) = ready(clean(file, "image/jpeg").unwrap());
    assert_eq!(
        out,
        [
            0xFF, 0xD8, 0xFF, 0xDA, 0, 8, 1, 1, 0, 0, 63, 0, 0x01, 0x02, 0xFF, 0xD9
        ]
    );
    assert!(clean(vec![0xFF, 0xD8, 0xFF], "image/jpeg").is_err());
}

fn png_chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = u32::try_from(body.len()).unwrap().to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let crc = png::crc32(&out[4..]);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

#[test]
fn png_drops_text_time_and_exif_and_keeps_the_orientation() {
    let mut file = b"\x89PNG\r\n\x1a\n".to_vec();
    file.extend(png_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
    file.extend(png_chunk(b"tEXt", b"Author\0someone"));
    file.extend(png_chunk(b"eXIf", &exif_tiff(3)));
    file.extend(png_chunk(b"IDAT", b"pixels"));
    file.extend(png_chunk(b"tIME", &[7, 234, 10, 20, 12, 0, 0]));
    file.extend(png_chunk(b"IEND", b""));
    file.extend_from_slice(b"trailing");
    let (out, _) = ready(clean(file, "image/png").unwrap());
    assert!(!contains(&out, b"someone") && !contains(&out, b"GPS") && !contains(&out, b"tIME"));
    assert!(!contains(&out, b"trailing"));
    assert!(contains(&out, b"pixels"));
    // Signature, IHDR (25 bytes), then the minimal EXIF chunk with a valid CRC.
    let exif_chunk = &out[8 + 25..];
    assert_eq!(&exif_chunk[4..8], b"eXIf");
    let length = u32::from_be_bytes(exif_chunk[..4].try_into().unwrap()) as usize;
    assert_eq!(exif::orientation(&exif_chunk[8..8 + length]), Some(3));
    let crc = u32::from_be_bytes(exif_chunk[8 + length..12 + length].try_into().unwrap());
    assert_eq!(crc, png::crc32(&exif_chunk[4..8 + length]));
}

fn riff_chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

#[test]
fn webp_drops_exif_and_xmp_and_fixes_its_header() {
    let mut body = b"WEBP".to_vec();
    body.extend(riff_chunk(b"VP8X", &[0x2C, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
    body.extend(riff_chunk(b"ICCP", b"profile"));
    body.extend(riff_chunk(b"VP8L", b"pixels!"));
    body.extend(riff_chunk(b"EXIF", &exif_tiff(8)));
    body.extend(riff_chunk(b"XMP ", b"<x:xmpmeta>Albstadt</x:xmpmeta>"));
    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
    file.extend(body);
    let (out, _) = ready(clean(file, "image/webp").unwrap());
    assert!(!contains(&out, b"GPS") && !contains(&out, b"Albstadt"));
    assert!(contains(&out, b"profile") && contains(&out, b"pixels!"));
    assert_eq!(
        u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize,
        out.len() - 8
    );
    // ICC flag (0x20) stays, XMP (0x04) is cleared, EXIF (0x08) stays for the orientation.
    assert_eq!(out[20], 0x28);
    let exif_at = out.windows(4).position(|w| w == b"EXIF").unwrap();
    assert_eq!(exif::orientation(&out[exif_at + 8..]), Some(8));
}

#[test]
fn gif_drops_comments_and_xmp() {
    let mut file = b"GIF89a\x01\0\x01\0\x80\0\0".to_vec();
    file.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
    file.extend_from_slice(b"\x21\xFE\x08a commnt\0");
    file.extend_from_slice(b"\x21\xFF\x0bXMP DataXMP\x08Albstadt\0");
    file.extend_from_slice(b"\x21\xF9\x04\0\0\0\0\0");
    file.extend_from_slice(b"\x2C\0\0\0\0\x01\0\x01\0\0\x02\x02\x44\x01\0");
    file.extend_from_slice(b"\x3Btrailing");
    let (out, _) = ready(clean(file, "image/gif").unwrap());
    assert!(
        !contains(&out, b"commnt") && !contains(&out, b"Albstadt") && !contains(&out, b"trailing")
    );
    assert!(contains(&out, b"\x21\xF9\x04") && contains(&out, b"\x02\x02\x44\x01\0"));
    assert_eq!(out.last(), Some(&0x3B));
}

fn mp4_box(kind: &[u8; 4], parts: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = parts.concat();
    let mut out = u32::try_from(body.len() + 8)
        .unwrap()
        .to_be_bytes()
        .to_vec();
    out.extend_from_slice(kind);
    out.extend(body);
    out
}

fn track(handler: &[u8; 4], codec: &[u8; 4]) -> Vec<u8> {
    let times = [
        0, 0, 0, 0, 0x11, 0x11, 0x11, 0x11, 0x22, 0x22, 0x22, 0x22, 0, 0, 0, 1,
    ]
    .to_vec();
    let mut hdlr = vec![0; 8];
    hdlr.extend_from_slice(handler);
    hdlr.extend_from_slice(&[0; 12]);
    let entry = mp4_box(codec, &[vec![0; 8]]);
    let stsd = mp4_box(b"stsd", &[vec![0, 0, 0, 0, 0, 0, 0, 1], entry]);
    let minf = mp4_box(b"minf", &[mp4_box(b"stbl", &[stsd])]);
    let mdia = mp4_box(
        b"mdia",
        &[
            mp4_box(b"mdhd", std::slice::from_ref(&times)),
            mp4_box(b"hdlr", &[hdlr]),
            minf,
        ],
    );
    mp4_box(
        b"trak",
        &[
            mp4_box(b"tkhd", &[times]),
            mdia,
            mp4_box(b"udta", &[b"track name".to_vec()]),
        ],
    )
}

fn movie(brand: &[u8; 4], tracks: Vec<Vec<u8>>) -> Vec<u8> {
    let ftyp = mp4_box(b"ftyp", &[brand.to_vec(), vec![0, 0, 2, 0], brand.to_vec()]);
    let mvhd = mp4_box(
        b"mvhd",
        &[vec![
            0, 0, 0, 0, 0x33, 0x33, 0x33, 0x33, 0x44, 0x44, 0x44, 0x44, 0, 0, 0, 1,
        ]],
    );
    let udta = mp4_box(
        b"udta",
        &[mp4_box(b"\xA9xyz", &[b"+48.2100+009.0200/".to_vec()])],
    );
    let meta = mp4_box(
        b"meta",
        &[b"com.apple.quicktime.location.ISO6709 +48.21".to_vec()],
    );
    let mut parts = vec![mvhd];
    parts.extend(tracks);
    parts.extend([udta, meta]);
    [
        ftyp,
        mp4_box(b"moov", &parts),
        mp4_box(b"mdat", &[b"frames".to_vec()]),
    ]
    .concat()
}

#[test]
fn an_h264_video_loses_location_names_and_times_in_place() {
    let file = movie(
        b"qt  ",
        vec![track(b"vide", b"avc1"), track(b"soun", b"mp4a")],
    );
    let length = file.len();
    let (out, mime) = ready(clean(file, "video/quicktime").unwrap());
    assert_eq!(mime, "video/mp4");
    assert_eq!(
        out.len(),
        length,
        "nothing moves, so chunk offsets stay right"
    );
    for gone in [
        &b"+48.21"[..],
        b"track name",
        b"udta",
        b"meta",
        &[0x11; 4],
        &[0x22; 4],
        &[0x33; 4],
        &[0x44; 4],
    ] {
        assert!(!contains(&out, gone), "{gone:?}");
    }
    assert!(contains(&out, b"frames") && contains(&out, b"avc1"));
    assert_eq!(&out[8..12], b"isom");
}

#[test]
fn other_videos_need_a_conversion() {
    for (tracks, mime) in [
        (vec![track(b"vide", b"hvc1")], "video/mp4"),
        (
            vec![track(b"vide", b"avc1"), track(b"soun", b"samr")],
            "video/mp4",
        ),
        (
            vec![track(b"vide", b"avc1"), track(b"meta", b"gpmd")],
            "video/mp4",
        ),
    ] {
        let cleaned = clean(movie(b"isom", tracks), mime).unwrap();
        assert!(
            matches!(cleaned, Cleaned::NeedsConversion { .. }),
            "{cleaned:?}"
        );
    }
    assert!(matches!(
        clean(b"\x1a\x45\xdf\xa3".to_vec(), "video/webm").unwrap(),
        Cleaned::NeedsConversion { .. }
    ));
    assert!(
        clean(movie(b"isom", Vec::new()), "video/mp4").is_err(),
        "no picture at all"
    );
}

#[test]
fn an_avif_keeps_its_picture_and_loses_its_exif_item() {
    let infe = |id: u8, kind: &[u8; 4]| {
        mp4_box(
            b"infe",
            &[vec![2, 0, 0, 0, 0, id, 0, 0], kind.to_vec(), vec![0]],
        )
    };
    let iinf = mp4_box(
        b"iinf",
        &[vec![0, 0, 0, 0, 0, 2], infe(1, b"av01"), infe(2, b"Exif")],
    );
    // iloc version 0: 4-byte offsets and lengths, no base offset; offsets are patched below.
    let iloc_body = |picture: u32, exif: u32| {
        let mut body = vec![0, 0, 0, 0, 0x44, 0x00, 0, 2];
        for (id, offset, length) in [(1u16, picture, 6u32), (2, exif, 23)] {
            body.extend_from_slice(&id.to_be_bytes());
            body.extend_from_slice(&[0, 0, 0, 1]);
            body.extend_from_slice(&offset.to_be_bytes());
            body.extend_from_slice(&length.to_be_bytes());
        }
        body
    };
    let build = |picture: u32, exif: u32| {
        let meta = mp4_box(
            b"meta",
            &[
                vec![0; 4],
                mp4_box(b"hdlr", &[vec![0; 20]]),
                iinf.clone(),
                mp4_box(b"iloc", &[iloc_body(picture, exif)]),
            ],
        );
        let ftyp = mp4_box(b"ftyp", &[b"avif".to_vec(), vec![0; 4], b"avif".to_vec()]);
        [
            ftyp,
            meta,
            mp4_box(
                b"mdat",
                &[b"pixels".to_vec(), b"\0\0\0\0GPS 48.21N 9.02E!!!".to_vec()],
            ),
        ]
        .concat()
    };
    let draft = build(0, 0);
    let mdat = draft.len() - 8 - 6 - 23;
    let file = build(
        u32::try_from(mdat + 8).unwrap(),
        u32::try_from(mdat + 14).unwrap(),
    );
    let (out, mime) = ready(clean(file, "image/avif").unwrap());
    assert_eq!(mime, "image/avif");
    assert!(contains(&out, b"pixels"));
    assert!(!contains(&out, b"GPS"));
}

/// Broken and hostile files give an error, never a panic: every prefix and many byte flips of
/// valid files of each format.
#[test]
fn damaged_files_never_panic() {
    let mut exif = b"Exif\0\0".to_vec();
    exif.extend_from_slice(&exif_tiff(6));
    let mut jpeg = vec![0xFF, 0xD8];
    jpeg.extend(segment(0xE1, &exif));
    jpeg.extend(segment(0xDA, &[1, 1, 0, 0, 63, 0]));
    jpeg.extend_from_slice(&[0x12, 0xFF, 0x00, 0xFF, 0xD9]);
    let samples = [
        (jpeg, "image/jpeg"),
        (fixtures::png(), "image/png"),
        (
            movie(
                b"qt  ",
                vec![track(b"vide", b"avc1"), track(b"soun", b"mp4a")],
            ),
            "video/mp4",
        ),
        (
            b"GIF89a\x01\0\x01\0\0\0\0\x2C\0\0\0\0\x01\0\x01\0\0\x02\x02\x44\x01\0\x3B".to_vec(),
            "image/gif",
        ),
        (
            [
                b"RIFF\x16\0\0\0WEBP".to_vec(),
                riff_chunk(b"VP8X", &[0x08; 10]),
            ]
            .concat(),
            "image/webp",
        ),
    ];
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for (file, mime) in samples {
        for end in 0..file.len() {
            let _ = clean(file[..end].to_vec(), mime);
        }
        for _ in 0..3000 {
            let mut damaged = file.clone();
            for _ in 0..1 + next() % 4 {
                let at = (next() as usize) % damaged.len();
                damaged[at] = next() as u8;
            }
            let _ = clean(damaged, mime);
        }
    }
    for size in [0u32, 1, 7, 8, 0xFFFF_FFFF] {
        let mut avif = mp4_box(b"ftyp", &[b"avif".to_vec()]);
        avif.extend_from_slice(&size.to_be_bytes());
        avif.extend_from_slice(b"meta\0\0\0\0\0\0\0\x08infe");
        let _ = clean(avif, "image/avif");
    }
}
