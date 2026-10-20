//! Small valid files for tests (feature `fixtures`): a 1×1 PNG and MP4s with chosen tracks.

/// A 1×1 transparent PNG without metadata.
pub fn png() -> Vec<u8> {
    let mut file = b"\x89PNG\r\n\x1a\n".to_vec();
    file.extend(png_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]));
    // zlib, one stored block: the filter byte and four zero bytes, then their Adler-32.
    file.extend(png_chunk(
        b"IDAT",
        &[
            0x78, 0x01, 0x01, 0x05, 0x00, 0xFA, 0xFF, 0, 0, 0, 0, 0, 0x00, 0x05, 0x00, 0x01,
        ],
    ));
    file.extend(png_chunk(b"IEND", b""));
    file
}

pub fn png_chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = u32::try_from(body.len())
        .expect("small chunk")
        .to_be_bytes()
        .to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let crc = crate::png::crc32(&out[4..]);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

/// An ISO box with its parts as content.
pub fn mp4_box(kind: &[u8; 4], parts: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = parts.concat();
    let mut out = u32::try_from(body.len() + 8)
        .expect("small box")
        .to_be_bytes()
        .to_vec();
    out.extend_from_slice(kind);
    out.extend(body);
    out
}

/// A track of `handler` (`vide`, `soun` …) whose first sample entry is `codec`, its times set.
pub fn track(handler: &[u8; 4], codec: &[u8; 4], times: [u8; 4]) -> Vec<u8> {
    let header = [vec![0; 4], times.to_vec(), times.to_vec(), vec![0, 0, 0, 1]].concat();
    let mut hdlr = vec![0; 8];
    hdlr.extend_from_slice(handler);
    hdlr.extend_from_slice(&[0; 12]);
    let entry = mp4_box(codec, &[vec![0; 8]]);
    let stsd = mp4_box(b"stsd", &[vec![0, 0, 0, 0, 0, 0, 0, 1], entry]);
    let minf = mp4_box(b"minf", &[mp4_box(b"stbl", &[stsd])]);
    let mdia = mp4_box(
        b"mdia",
        &[
            mp4_box(b"mdhd", &[header.clone()]),
            mp4_box(b"hdlr", &[hdlr]),
            minf,
        ],
    );
    mp4_box(b"trak", &[mp4_box(b"tkhd", &[header]), mdia])
}

/// A movie of brand `brand` with the given tracks and the extra boxes in its `moov`.
pub fn movie(brand: &[u8; 4], tracks: Vec<Vec<u8>>, extra: Vec<Vec<u8>>) -> Vec<u8> {
    let ftyp = mp4_box(b"ftyp", &[brand.to_vec(), vec![0, 0, 2, 0], brand.to_vec()]);
    let mvhd = mp4_box(b"mvhd", &[vec![0; 12], vec![0, 0, 0, 1]]);
    let mut parts = vec![mvhd];
    parts.extend(tracks);
    parts.extend(extra);
    [
        ftyp,
        mp4_box(b"moov", &parts),
        mp4_box(b"mdat", &[b"frames".to_vec()]),
    ]
    .concat()
}

/// An MP4 with one H.264 track and no metadata: `clean` leaves it as it is.
pub fn mp4() -> Vec<u8> {
    movie(b"isom", vec![track(b"vide", b"avc1", [0; 4])], Vec::new())
}
