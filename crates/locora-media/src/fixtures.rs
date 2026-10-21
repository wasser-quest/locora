//! Small valid files for tests (feature `fixtures`): a 1×1 PNG, MP4s and WebMs with chosen tracks.

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
            mp4_box(b"mdhd", std::slice::from_ref(&header)),
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

/// A short silent WAV (8 kHz, 8 bit, mono) without metadata: `clean` leaves it as it is.
pub fn wav() -> Vec<u8> {
    let samples = [0x80u8; 16];
    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(4 + 24 + 8 + samples.len() as u32).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    // PCM, one channel, 8000 Hz, 8000 bytes per second, blocks of one byte, 8 bits.
    file.extend_from_slice(&[1, 0, 1, 0, 0x40, 0x1F, 0, 0, 0x40, 0x1F, 0, 0, 1, 0, 8, 0]);
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    file.extend_from_slice(&samples);
    file
}

/// An M4A with one AAC sound track and no metadata.
pub fn m4a() -> Vec<u8> {
    movie(b"M4A ", vec![track(b"soun", b"mp4a", [0; 4])], Vec::new())
}

/// An MP4 with one H.264 track and no metadata: `clean` leaves it as it is.
pub fn mp4() -> Vec<u8> {
    movie(b"isom", vec![track(b"vide", b"avc1", [0; 4])], Vec::new())
}

/// An EBML element with a known size (a minimal size field).
pub fn ebml(id: u32, body: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = id
        .to_be_bytes()
        .into_iter()
        .skip_while(|&b| b == 0)
        .collect();
    let size = body.len() as u64;
    let length = (1..=8u32)
        .find(|&length| size < (1u64 << (7 * length)) - 1)
        .expect("small element");
    let field = size | 1u64 << (7 * length);
    out.extend_from_slice(&field.to_be_bytes()[8 - length as usize..]);
    out.extend_from_slice(body);
    out
}

/// A WebM file of document type `doc_type` with one track per (track type, codec id): 1 is a
/// video, 2 a sound track. Its info names the writing app (`Chrome`); two clusters hold frames.
pub fn webm(doc_type: &str, tracks: &[(u8, &str)]) -> Vec<u8> {
    let header = ebml(0x1A45_DFA3, &ebml(0x4282, doc_type.as_bytes()));
    let info = ebml(
        0x1549_A966,
        &[
            ebml(0x2A_D7B1, &[0x0F, 0x42, 0x40]),
            ebml(0x5741, b"Chrome"),
        ]
        .concat(),
    );
    let entries: Vec<u8> = tracks
        .iter()
        .enumerate()
        .flat_map(|(index, (kind, codec))| {
            ebml(
                0xAE,
                &[
                    ebml(0xD7, &[index as u8 + 1]),
                    ebml(0x83, &[*kind]),
                    ebml(0x86, codec.as_bytes()),
                ]
                .concat(),
            )
        })
        .collect();
    let cluster = |time: u8| {
        ebml(
            0x1F43_B675,
            &[ebml(0xE7, &[time]), ebml(0xA3, b"\x81\0\0\x80frame-bytes")].concat(),
        )
    };
    let segment = [info, ebml(0x1654_AE6B, &entries), cluster(0), cluster(1)].concat();
    [header, ebml(0x1853_8067, &segment)].concat()
}
