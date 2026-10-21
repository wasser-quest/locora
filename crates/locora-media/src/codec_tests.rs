//! Pass-through of phone and browser codecs: HEVC and other MP4 tracks, metadata tracks, WebM,
//! FLAC and AAC (ADTS).

use super::*;
use crate::fixtures::{ebml, movie, mp4_box, track, webm};

fn ready(cleaned: Cleaned) -> (Vec<u8>, &'static str) {
    match cleaned {
        Cleaned::Ready { bytes, mime } => (bytes, mime),
        other => panic!("expected ready media, got {other:?}"),
    }
}

fn converts(result: Result<Cleaned>) -> String {
    match result {
        Ok(Cleaned::NeedsConversion { reason }) => reason,
        other => panic!("expected a conversion, got {other:?}"),
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

const GPS: &[u8] = b"GPS +48.2100+009.0200";

/// A track of `handler` with sample tables: one chunk at `offset` with one sample of `size` bytes.
fn track_with_sample(handler: &[u8; 4], codec: &[u8; 4], offset: u32, size: u32) -> Vec<u8> {
    let mut hdlr = vec![0; 8];
    hdlr.extend_from_slice(handler);
    hdlr.extend_from_slice(&[0; 12]);
    let entry = mp4_box(codec, &[vec![0; 8]]);
    let stbl = mp4_box(
        b"stbl",
        &[
            mp4_box(b"stsd", &[vec![0, 0, 0, 0, 0, 0, 0, 1], entry]),
            mp4_box(
                b"stsc",
                &[vec![
                    0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
                ]],
            ),
            mp4_box(
                b"stsz",
                &[[0u32, 0, 1, size]
                    .iter()
                    .flat_map(|v| v.to_be_bytes())
                    .collect()],
            ),
            mp4_box(
                b"stco",
                &[[0u32, 1, offset]
                    .iter()
                    .flat_map(|v| v.to_be_bytes())
                    .collect()],
            ),
        ],
    );
    let mdia = mp4_box(
        b"mdia",
        &[
            mp4_box(b"mdhd", &[vec![0; 16]]),
            mp4_box(b"hdlr", &[hdlr]),
            mp4_box(b"minf", &[stbl]),
        ],
    );
    mp4_box(b"trak", &[mp4_box(b"tkhd", &[vec![0; 16]]), mdia])
}

/// An iPhone-like HEVC movie whose metadata track (`mebx`) holds a location sample in `mdat`.
fn movie_with_location(brand: &[u8; 4]) -> Vec<u8> {
    let build = |offset: u32| {
        let tracks = vec![
            track(b"vide", b"hvc1", [0; 4]),
            track(b"soun", b"mp4a", [0; 4]),
            track_with_sample(b"meta", b"mebx", offset, GPS.len() as u32),
        ];
        let mut file = movie(brand, tracks, Vec::new());
        // movie() ends with an mdat holding "frames"; the location follows the frames.
        let mdat = file.len() - 14;
        file.extend_from_slice(GPS);
        let size = (file.len() - mdat) as u32;
        file[mdat..mdat + 4].copy_from_slice(&size.to_be_bytes());
        file
    };
    let draft = build(0);
    build((draft.len() - GPS.len()) as u32)
}

#[test]
fn a_metadata_track_is_removed_with_its_samples() {
    let file = movie_with_location(b"qt  ");
    assert!(contains(&file, GPS));
    let length = file.len();
    let (out, mime) = ready(clean(file, "video/quicktime").unwrap());
    assert_eq!(mime, "video/mp4");
    assert_eq!(out.len(), length, "nothing moves");
    assert!(!contains(&out, b"GPS") && !contains(&out, b"+48.21"));
    assert!(!contains(&out, b"mebx"), "the track is a free box now");
    assert!(contains(&out, b"hvc1") && contains(&out, b"mp4a") && contains(&out, b"frames"));
    // Cleaning again changes nothing more.
    assert_eq!(ready(clean(out.clone(), "video/mp4").unwrap()).0, out);
}

#[test]
fn a_metadata_track_outside_the_file_is_an_error() {
    let mut file = movie(
        b"isom",
        vec![
            track(b"vide", b"avc1", [0; 4]),
            track_with_sample(b"meta", b"gpmd", 0x7FFF_FFF0, 16),
        ],
        Vec::new(),
    );
    assert!(clean(file.clone(), "video/mp4").is_err());
    // Fragmented: the samples would sit in the fragments, so it is converted instead.
    file.extend(mp4_box(b"moof", &[vec![0; 8]]));
    assert!(converts(clean(file, "video/mp4")).contains("Fragment"));
}

#[test]
fn a_fragmented_video_without_extra_tracks_is_kept() {
    let mut file = crate::fixtures::mp4();
    file.extend(mp4_box(b"moof", &[vec![0; 8]]));
    file.extend(mp4_box(b"mdat", &[b"more".to_vec()]));
    assert_eq!(ready(clean(file.clone(), "video/mp4").unwrap()).0, file);
}

#[test]
fn an_m4a_loses_its_chapter_track() {
    let tracks = vec![
        track(b"soun", b"mp4a", [0; 4]),
        track_with_sample(b"text", b"text", 0, 0),
    ];
    let file = movie(b"M4A ", tracks, Vec::new());
    let (out, mime) = ready(clean(file.clone(), "audio/mp4").unwrap());
    assert_eq!(mime, "audio/mp4");
    assert_eq!(out.len(), file.len());
    assert!(!contains(&out, b"text") && contains(&out, b"mp4a"));
}

/// The plain WebM fixture with a title, a date, track names, tags, chapters and an attachment;
/// with `unknown`, the segment and its clusters have no size, as MediaRecorder writes them.
fn tagged_webm(tracks: &[(u8, &str)], unknown: bool) -> Vec<u8> {
    let info = ebml(
        0x1549_A966,
        &[
            ebml(0x2A_D7B1, &[0x0F, 0x42, 0x40]),
            ebml(0x4D80, b"Chrome"),
            ebml(0x5741, b"Chrome"),
            ebml(0x4461, &[1, 2, 3, 4, 5, 6, 7, 8]),
            ebml(0x7BA9, b"Urlaub in Albstadt"),
            ebml(0xBF, &[9, 9, 9, 9]),
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
                    ebml(0x536E, b"Kamera von Erika"),
                ]
                .concat(),
            )
        })
        .collect();
    let block = ebml(0xA3, b"\x81\0\0\x80frame-bytes");
    let cluster = |time: u8| {
        let body = [ebml(0xE7, &[time]), block.clone()].concat();
        if unknown {
            [
                vec![
                    0x1F, 0x43, 0xB6, 0x75, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                ],
                body,
            ]
            .concat()
        } else {
            ebml(0x1F43_B675, &body)
        }
    };
    let tags = ebml(
        0x1254_C367,
        &ebml(
            0x7373,
            &ebml(
                0x67C8,
                &[ebml(0x45A3, b"LOCATION"), ebml(0x4487, b"+48.21+009.02")].concat(),
            ),
        ),
    );
    let chapters = ebml(0x1043_A770, &ebml(0x45B9, &ebml(0x85, b"Kapitel Albstadt")));
    let attachments = ebml(
        0x1941_A469,
        &ebml(0x61A7, &ebml(0x466E, b"cover-Albstadt.jpg")),
    );
    let segment = [
        info,
        ebml(0x1654_AE6B, &entries),
        cluster(0),
        cluster(1),
        tags,
        chapters,
        attachments,
    ]
    .concat();
    let header = ebml(0x1A45_DFA3, &ebml(0x4282, b"webm"));
    let segment = if unknown {
        [
            vec![
                0x18, 0x53, 0x80, 0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            ],
            segment,
        ]
        .concat()
    } else {
        ebml(0x1853_8067, &segment)
    };
    [header, segment].concat()
}

#[test]
fn webm_loses_tags_dates_and_names_in_place() {
    for unknown in [false, true] {
        let file = tagged_webm(&[(1, "V_VP9"), (2, "A_OPUS")], unknown);
        let length = file.len();
        let (out, mime) = ready(clean(file, "video/webm").unwrap());
        assert_eq!(mime, "video/webm");
        assert_eq!(out.len(), length, "nothing moves");
        for gone in [
            &b"Albstadt"[..],
            b"Chrome",
            b"Erika",
            b"LOCATION",
            b"+48.21",
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[9, 9, 9, 9],
        ] {
            assert!(!contains(&out, gone), "{unknown}: {gone:?}");
        }
        assert!(contains(&out, b"V_VP9") && contains(&out, b"A_OPUS"));
        assert_eq!(out.windows(11).filter(|w| w == b"frame-bytes").count(), 2);
        // The voided parts are valid Void elements: a second pass reads the same file.
        assert_eq!(ready(clean(out.clone(), "video/webm").unwrap()).0, out);
    }
}

#[test]
fn webm_codecs_are_checked() {
    for codecs in [
        &[(1, "V_VP8"), (2, "A_VORBIS")][..],
        &[(1, "V_AV1")],
        &[(1, "V_MPEG4/ISO/AVC"), (2, "A_AAC")],
        &[(1, "V_MPEGH/ISO/HEVC"), (2, "A_AAC/MPEG4/LC")],
        &[(1, "V_VP9"), (2, "A_FLAC")],
    ] {
        let file = webm("webm", codecs);
        assert_eq!(
            ready(clean(file.clone(), "video/webm").unwrap()).0.len(),
            file.len()
        );
        assert!(!contains(
            &ready(clean(file, "video/x-matroska").unwrap()).0,
            b"Chrome"
        ));
    }
    for (codecs, reason) in [
        (&[(1, "V_MPEG4/ISO/ASP")][..], "V_MPEG4/ISO/ASP"),
        (&[(1, "V_VP9"), (2, "A_DTS")], "A_DTS"),
        (&[(1, "V_VP9"), (17, "S_TEXT/UTF8")], "Zusatzspuren"),
    ] {
        assert!(converts(clean(webm("webm", codecs), "video/webm")).contains(reason));
    }
    let mkv = webm("matroska", &[(1, "V_MPEG4/ISO/AVC")]);
    assert!(converts(clean(mkv, "video/x-matroska")).contains("Matroska"));
    assert!(matches!(
        clean(webm("webm", &[(2, "A_OPUS")]), "video/webm"),
        Err(MediaError::Malformed("keine Videospur"))
    ));
}

#[test]
fn webm_sound_is_kept() {
    let file = tagged_webm(&[(2, "A_OPUS")], true);
    let (out, mime) = ready(clean(file.clone(), "audio/webm").unwrap());
    assert_eq!(mime, "audio/webm");
    assert_eq!(out.len(), file.len());
    assert!(!contains(&out, b"Albstadt"));
    let video = webm("webm", &[(1, "V_VP8"), (2, "A_OPUS")]);
    assert!(matches!(
        clean(video, "audio/webm"),
        Err(MediaError::Refused(_))
    ));
    assert!(converts(clean(webm("webm", &[(2, "A_MPEG/L3")]), "audio/webm")).contains("A_MPEG"));
}

fn flac_block(kind: u8, body: &[u8]) -> Vec<u8> {
    let length = body.len() as u32;
    let mut out = vec![kind];
    out.extend_from_slice(&length.to_be_bytes()[1..]);
    out.extend_from_slice(body);
    out
}

#[test]
fn flac_keeps_stream_info_seek_table_and_frames_only() {
    let streaminfo = flac_block(0, &[0x11; 34]);
    let seektable = flac_block(3, &[0x22; 18]);
    let frames = [0xFF, 0xF8, 0x69, 0x08, 0x00, 0x33];
    let file = [
        b"ID3\x04\0\0\0\0\0\x05TIT2x".to_vec(),
        b"fLaC".to_vec(),
        streaminfo.clone(),
        flac_block(4, b"\x08\0\0\0reference\x01\0\0\0\x0e\0\0\0TITLE=Albstadt"),
        seektable.clone(),
        flac_block(6, b"cover picture"),
        flac_block(1 | 0x80, &[0; 8]),
        frames.to_vec(),
    ]
    .concat();
    let (out, mime) = ready(clean(file, "audio/flac").unwrap());
    assert_eq!(mime, "audio/flac");
    let mut last = seektable;
    last[0] |= 0x80;
    assert_eq!(
        out,
        [b"fLaC".to_vec(), streaminfo, last, frames.to_vec()].concat()
    );
    let without = [
        b"fLaC".to_vec(),
        flac_block(4 | 0x80, b"x"),
        frames.to_vec(),
    ]
    .concat();
    assert!(
        clean(without, "audio/flac").is_err(),
        "STREAMINFO comes first"
    );
}

fn adts(payload: &[u8]) -> Vec<u8> {
    let length = payload.len() + 7;
    let mut frame = vec![
        0xFF,
        0xF1,
        0x50,
        0x80 | (length >> 11) as u8,
        (length >> 3) as u8,
        ((length & 7) << 5) as u8 | 0x1F,
        0xFC,
    ];
    frame.extend_from_slice(payload);
    frame
}

#[test]
fn aac_keeps_its_frames_without_tags() {
    let frames = [adts(b"first"), adts(b"second")].concat();
    let mut id3v1 = b"TAG".to_vec();
    id3v1.resize(128, b'A');
    let file = [
        b"ID3\x04\0\0\0\0\0\x05TIT2x".to_vec(),
        frames.clone(),
        id3v1,
    ]
    .concat();
    let (out, mime) = ready(clean(file, "audio/aac").unwrap());
    assert_eq!((out, mime), (frames, "audio/aac"));
    assert!(clean(b"\xFF\xF1\0\0".to_vec(), "audio/aac").is_err());
}

/// Broken WebM, FLAC and AAC files give an error, never a panic.
#[test]
fn damaged_new_formats_never_panic() {
    let samples = [
        (
            tagged_webm(&[(1, "V_VP9"), (2, "A_OPUS")], true),
            "video/webm",
        ),
        (tagged_webm(&[(2, "A_OPUS")], false), "audio/webm"),
        (movie_with_location(b"isom"), "video/mp4"),
        (
            [
                b"fLaC".to_vec(),
                flac_block(0x80, &[0; 34]),
                vec![0xFF, 0xF8, 1],
            ]
            .concat(),
            "audio/flac",
        ),
        ([adts(b"a"), adts(b"bb")].concat(), "audio/aac"),
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
}
