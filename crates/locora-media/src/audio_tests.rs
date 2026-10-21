use super::*;
use crate::fixtures::{m4a, movie, mp4_box, track, wav};

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

fn refused(result: Result<Cleaned>) -> String {
    match result {
        Err(MediaError::Refused(reason)) => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Two MPEG-1 layer III frames (128 kbit/s, 44.1 kHz): 417 bytes each.
fn frames() -> Vec<u8> {
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
    frame.resize(417, 0x55);
    [frame.clone(), frame].concat()
}

#[test]
fn mp3_loses_its_tags_but_keeps_its_frames() {
    let mut file = b"ID3\x04\x00\x00\x00\x00\x00\x12".to_vec();
    file.extend_from_slice(b"TIT2\0\0\0\x08\0\0\x03Geheim");
    file.extend_from_slice(&[0; 4]);
    file.extend(frames());
    // APEv2 footer only: 32 bytes with a size of 32 + 8 items bytes.
    let mut ape = b"Albstadt".to_vec();
    ape.extend_from_slice(b"APETAGEX");
    ape.extend_from_slice(&2000u32.to_le_bytes());
    ape.extend_from_slice(&40u32.to_le_bytes());
    ape.extend_from_slice(&1u32.to_le_bytes());
    ape.extend_from_slice(&0u32.to_le_bytes());
    ape.extend_from_slice(&[0; 8]);
    file.extend(ape);
    let mut v1 = b"TAGTitel aus dem Urlaub".to_vec();
    v1.resize(128, 0);
    file.extend(v1);

    let (out, mime) = ready(clean(file, "audio/mpeg").unwrap());
    assert_eq!(mime, "audio/mpeg");
    assert_eq!(out, frames());
    // `audio/mp3` is a name some systems use for the same type.
    let (out, _) = ready(clean(frames(), "audio/mp3").unwrap());
    assert_eq!(out, frames());
}

#[test]
fn mp3_without_frames_is_refused() {
    let mut file = b"ID3\x04\x00\x00\x00\x00\x00\x02\0\0".to_vec();
    file.extend_from_slice(b"no frames here");
    assert!(matches!(
        clean(file, "audio/mpeg"),
        Err(MediaError::Malformed(_))
    ));
    let mut broken = frames();
    broken[417] = 0;
    assert!(clean(broken, "audio/mpeg").is_err());
    let mut tag = b"ID3\x04\x00\x00\x00\x00\x7F\x7F".to_vec();
    tag.extend(frames());
    assert!(
        clean(tag, "audio/mpeg").is_err(),
        "a tag longer than the file"
    );
}

fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

#[test]
fn wav_keeps_only_format_and_samples() {
    let clean_wav = wav();
    assert_eq!(
        ready(clean(clean_wav.clone(), "audio/wav").unwrap()).0,
        clean_wav
    );
    let fmt = &clean_wav[12..36];
    let data = &clean_wav[36..];
    let body = [
        b"WAVE".to_vec(),
        chunk(b"bext", b"Aufgenommen in Albstadt 2026-10-21"),
        fmt.to_vec(),
        chunk(b"LIST", b"INFOINAM\x07\0\0\0Geheim\0"),
        chunk(b"id3 ", b"ID3 Tag"),
        data.to_vec(),
        chunk(b"cue ", &[0; 4]),
    ]
    .concat();
    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend(body);
    file.extend_from_slice(b"trailing junk");

    let (out, mime) = ready(clean(file, "audio/x-wav").unwrap());
    assert_eq!(mime, "audio/wav");
    assert_eq!(out, clean_wav);
    for gone in [&b"Albstadt"[..], b"Geheim", b"ID3", b"junk"] {
        assert!(!contains(&out, gone));
    }
}

#[test]
fn wav_needs_pcm_format_and_samples() {
    let mut adpcm = wav();
    adpcm[20] = 2;
    assert!(refused(clean(adpcm, "audio/wav")).contains("PCM"));
    let mut no_data = wav();
    no_data[36..40].copy_from_slice(b"junk");
    assert!(matches!(
        clean(no_data, "audio/wav"),
        Err(MediaError::Malformed(_))
    ));
    let mut long = wav();
    long[40] = 0xFF;
    assert!(clean(long, "audio/wav").is_err());
}

fn comment(magic: &[u8], vendor: &str, comments: &[&[u8]], vorbis: bool) -> Vec<u8> {
    let mut out = magic.to_vec();
    out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    out.extend_from_slice(vendor.as_bytes());
    out.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for text in comments {
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(text);
    }
    if vorbis {
        out.push(1);
    }
    out
}

#[test]
fn ogg_opus_keeps_its_vendor_but_no_comments() {
    let head = b"OpusHead\x01\x01\x38\x01\x80\xBB\0\0\0\0\0".to_vec();
    // A cover picture larger than one page (more than 255 × 255 bytes).
    let picture = [b"METADATA_BLOCK_PICTURE=".to_vec(), vec![b'A'; 70_000]].concat();
    let tags = comment(
        b"OpusTags",
        "libopus 1.4",
        &[b"TITLE=Geheim", b"LOCATION=Albstadt", &picture],
        false,
    );
    let audio = vec![vec![0xFC, 1, 2, 3], vec![0xFC; 300], vec![0xFC, 9]];
    let packets = [vec![head.clone(), tags], audio.clone()].concat();
    let file = ogg::stream(&packets, 1);

    let (out, mime) = ready(clean(file.clone(), "audio/ogg").unwrap());
    assert_eq!(mime, "audio/ogg");
    assert!(out.len() < file.len() - 70_000);
    // Every page's checksum and sequence number is right again.
    let read = ogg::read_packets(&out).unwrap();
    assert_eq!(
        read,
        [
            vec![head, comment(b"OpusTags", "libopus 1.4", &[], false)],
            audio
        ]
        .concat()
    );
    let sequences: Vec<u32> = out
        .windows(4)
        .enumerate()
        .filter(|(_, w)| *w == b"OggS")
        .map(|(at, _)| u32::from_le_bytes(out[at + 18..at + 22].try_into().unwrap()))
        .collect();
    assert_eq!(sequences, (0..sequences.len() as u32).collect::<Vec<_>>());
    assert!(!contains(&out, b"Albstadt") && !contains(&out, b"Geheim"));
    // An already clean file comes out the same; `audio/opus` is the same type.
    assert_eq!(ready(clean(out.clone(), "audio/opus").unwrap()).0, out);
}

#[test]
fn ogg_vorbis_keeps_its_setup_header() {
    let id = b"\x01vorbis\0\0\0\0\x01\x44\xAC\0\0\0\0\0\0\0\x71\x02\0\0\0\0\0\xB8\x01".to_vec();
    let tags = comment(
        b"\x03vorbis",
        "Xiph.Org libVorbis",
        &[b"ARTIST=Jemand"],
        true,
    );
    let setup = [b"\x05vorbis".to_vec(), vec![7; 600]].concat();
    let audio = vec![vec![1, 2, 3]];
    let file = ogg::stream(
        &[vec![id.clone(), tags, setup.clone()], audio.clone()].concat(),
        2,
    );
    let (out, _) = ready(clean(file, "audio/ogg").unwrap());
    let empty = comment(b"\x03vorbis", "Xiph.Org libVorbis", &[], true);
    assert_eq!(
        ogg::read_packets(&out).unwrap(),
        [vec![id, empty, setup], audio].concat()
    );
    assert!(!contains(&out, b"Jemand"));
}

#[test]
fn ogg_with_other_codecs_streams_or_broken_pages_is_refused() {
    let flac = ogg::stream(&[b"\x7FFLAC\x01\0".to_vec(), vec![0; 4]], 1);
    assert!(refused(clean(flac, "audio/ogg")).contains("Opus"));
    let head = b"OpusHead\x01\x01\x38\x01\x80\xBB\0\0\0\0\0".to_vec();
    let tags = comment(b"OpusTags", "x", &[], false);
    let one = ogg::stream(&[head.clone(), tags.clone(), vec![1]], 1);
    let mut two = ogg::stream(&[head, tags, vec![1]], 1);
    // The second stream gets another serial number (and a right checksum).
    two[14] ^= 1;
    let sum = ogg::crc(&two[..47], true);
    two[22..26].copy_from_slice(&sum.to_le_bytes());
    let chained = [one.clone(), two].concat();
    assert!(clean(chained, "audio/ogg").is_err());
    let mut broken = one;
    let last = broken.len() - 1;
    broken[last] ^= 0xFF;
    assert!(matches!(
        clean(broken, "audio/ogg"),
        Err(MediaError::Malformed("Ogg-Prüfsumme"))
    ));
}

#[test]
fn m4a_loses_its_tags_and_times() {
    let file = m4a();
    assert_eq!(ready(clean(file.clone(), "audio/mp4").unwrap()).0, file);
    let ilst = mp4_box(b"ilst", &[b"\xA9nam Geheim, Albstadt".to_vec()]);
    let meta = mp4_box(b"meta", &[vec![0; 4], ilst]);
    let tagged = movie(
        b"M4A ",
        vec![track(b"soun", b"mp4a", [0x12, 0x34, 0x56, 0x78])],
        vec![mp4_box(b"udta", &[meta])],
    );
    let (out, mime) = ready(clean(tagged.clone(), "audio/x-m4a").unwrap());
    assert_eq!(mime, "audio/mp4");
    assert_eq!(out.len(), tagged.len(), "no offset moves");
    assert!(!contains(&out, b"Albstadt"));
    assert!(!contains(&out, &[0x12, 0x34, 0x56, 0x78]));
    assert!(contains(&out, b"frames"));
}

#[test]
fn m4a_with_pictures_is_refused_and_other_codecs_need_a_conversion() {
    let video = movie(
        b"M4A ",
        vec![
            track(b"soun", b"mp4a", [0; 4]),
            track(b"vide", b"avc1", [0; 4]),
        ],
        Vec::new(),
    );
    assert!(refused(clean(video, "audio/mp4")).contains("Spuren"));
    for codec in [b"alac", b"fLaC", b".mp3", b"Opus"] {
        let file = movie(b"M4A ", vec![track(b"soun", codec, [0; 4])], Vec::new());
        assert_eq!(ready(clean(file.clone(), "audio/mp4").unwrap()).0, file);
    }
    for (codec, mime) in [
        (b"samr", "audio/mp4"),
        (b"sawb", "audio/3gpp"),
        (b"ac-3", "audio/mp4"),
    ] {
        let file = movie(b"3gp4", vec![track(b"soun", codec, [0; 4])], Vec::new());
        assert!(
            matches!(clean(file, mime).unwrap(), Cleaned::NeedsConversion { .. }),
            "{mime}"
        );
    }
    // A 3GP recording with AAC is stored as M4A.
    let aac = movie(b"3gp4", vec![track(b"soun", b"mp4a", [0; 4])], Vec::new());
    assert_eq!(ready(clean(aac, "audio/3gpp").unwrap()).1, "audio/mp4");
    let silent = movie(b"M4A ", Vec::new(), Vec::new());
    assert!(matches!(
        clean(silent, "audio/mp4"),
        Err(MediaError::Malformed(_))
    ));
}

#[test]
fn sound_types_are_sniffed() {
    assert_eq!(sniff_audio(&frames()), Some("audio/mpeg"));
    assert_eq!(sniff_audio(&wav()), Some("audio/wav"));
    assert_eq!(sniff_audio(&m4a()), Some("audio/mp4"));
    assert_eq!(sniff_audio(b"OggS\0"), Some("audio/ogg"));
    assert_eq!(sniff_audio(b"\x1a\x45\xdf\xa3"), Some("audio/webm"));
    assert_eq!(sniff_audio(b"fLaC\0\0\0\x22"), Some("audio/flac"));
    assert_eq!(
        sniff_audio(&[0xFF, 0xF1, 0x50, 0x80, 0x01, 0x1F, 0xFC]),
        Some("audio/aac")
    );
    assert_eq!(sniff_audio(b"#!AMR\n"), Some("audio/amr"));
    assert_eq!(sniff_audio(b"caff\0\x01"), Some("audio/x-caf"));
    assert_eq!(sniff_audio(b"\x89PNG"), None);
    // The content must be the type that was given.
    assert!(clean(wav(), "audio/mpeg").is_err());
    assert!(clean(wav(), "audio/flac").is_err());
    assert!(matches!(
        clean(wav(), "audio/x-ms-wma"),
        Err(MediaError::Unsupported(_))
    ));
    for (bytes, mime) in [
        (&b"#!AMR\n\0\0"[..], "audio/amr"),
        (b"caff\0\x01\0\0", "audio/x-caf"),
    ] {
        assert!(matches!(
            clean(bytes.to_vec(), mime).unwrap(),
            Cleaned::NeedsConversion { .. }
        ));
    }
    assert_eq!(audio_canonical("audio/x-m4a"), Some("audio/mp4"));
    assert_eq!(audio_canonical("audio/x-flac"), Some("audio/flac"));
    assert!(is_audio("audio/ogg") && is_audio("audio/webm") && !is_audio("audio/opus"));
}
