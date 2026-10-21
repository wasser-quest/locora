//! A picture, video or sound file as Locora stores it: cleaned, and kept in its own codec when
//! the devices that play it decode that codec. Only a file outside that list is converted, by an
//! ffmpeg the person installed (desktop, CLI); Locora bundles no encoder. Android and the browser
//! convert such files before they reach the core; there, and without ffmpeg, they are refused.

use std::path::Path;

use crate::{Cleaned, MediaError, Result, clean};

/// Largest converted file that is kept (the limit for added media).
const LIMIT: usize = 500 << 20;

/// The bytes and media type to store for the file at `path`, read as `bytes` of type `mime`.
pub fn prepare(path: &Path, bytes: Vec<u8>, mime: &str) -> Result<(Vec<u8>, &'static str)> {
    let sound = mime.starts_with("audio/");
    match clean(bytes, mime)? {
        Cleaned::Ready { bytes, mime } => Ok((bytes, mime)),
        Cleaned::NeedsConversion { reason } => {
            let Some((converted, mime)) = with_ffmpeg(path, sound)? else {
                let hint = if sound {
                    "Mit installiertem ffmpeg wandelt Locora den Ton selbst um, sonst ihn vorher als \
                     MP3, M4A (AAC), Ogg, WebM oder FLAC speichern"
                } else {
                    "Locora behält Videos in H.264, HEVC, AV1, VP9 und VP8 (MP4, MOV oder WebM); mit \
                     installiertem ffmpeg wandelt Locora andere selbst um, sonst das Video vorher \
                     umwandeln"
                };
                return Err(MediaError::Conversion(format!("{reason}. {hint}")));
            };
            match clean(converted, mime)? {
                Cleaned::Ready { bytes, mime } => Ok((bytes, mime)),
                Cleaned::NeedsConversion { reason } => Err(MediaError::Conversion(format!(
                    "Die Umwandlung ergab keine passende Datei: {reason}"
                ))),
            }
        }
    }
}

/// The file converted by an installed ffmpeg, without metadata, and its media type: sound as
/// M4A (AAC), a video as MP4 (H.264, AAC) or, where ffmpeg has no H.264 encoder, as WebM (VP9,
/// Opus). `None` where ffmpeg or a fitting encoder is missing.
#[cfg(not(target_family = "wasm"))]
fn with_ffmpeg(input: &Path, sound: bool) -> Result<Option<(Vec<u8>, &'static str)>> {
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let Ok(encoders) = Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .stderr(Stdio::null())
        .output()
    else {
        return Ok(None);
    };
    let encoders = String::from_utf8_lossy(&encoders.stdout);
    let has = |name: &str| encoders.contains(&format!(" {name} "));
    let (codecs, mime, extension): (&[&str], _, _) = if sound {
        (&["-vn", "-c:a", "aac", "-b:a", "160k"], "audio/mp4", "m4a")
    } else if has("libx264") {
        (
            &[
                "-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-pix_fmt", "yuv420p",
                "-c:a", "aac", "-b:a", "160k",
            ],
            "video/mp4",
            "mp4",
        )
    } else if has("libopenh264") {
        (
            &[
                "-c:v",
                "libopenh264",
                "-b:v",
                "6M",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-b:a",
                "160k",
            ],
            "video/mp4",
            "mp4",
        )
    } else if has("libvpx-vp9") && has("libopus") {
        (
            &[
                "-c:v",
                "libvpx-vp9",
                "-crf",
                "32",
                "-b:v",
                "0",
                "-row-mt",
                "1",
                "-deadline",
                "realtime",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "libopus",
                "-b:a",
                "128k",
            ],
            "video/webm",
            "webm",
        )
    } else {
        return Ok(None);
    };
    let streams: &[&str] = if sound {
        &["-map", "0:a:0"]
    } else {
        &["-map", "0:v:0", "-map", "0:a:0?"]
    };
    let container: &[&str] = match extension {
        "webm" => &["-f", "webm"],
        _ => &["-movflags", "+faststart", "-f", "mp4"],
    };
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let run = RUNS.fetch_add(1, Ordering::Relaxed);
    let output = std::env::temp_dir().join(format!(
        "locora-convert-{}-{run}.{extension}",
        std::process::id()
    ));
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(input)
        .args(streams)
        .args(["-map_metadata", "-1", "-map_chapters", "-1"])
        .args(codecs)
        .args(container)
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let converted = match status {
        Ok(status) if status.success() => std::fs::read(&output).ok(),
        _ => None,
    };
    let _ = std::fs::remove_file(&output);
    match converted {
        Some(bytes) if bytes.len() <= LIMIT => Ok(Some((bytes, mime))),
        Some(_) => Err(MediaError::Conversion(
            "Die umgewandelte Datei ist größer als 500 MiB".into(),
        )),
        None => Err(MediaError::Conversion(
            "ffmpeg konnte die Datei nicht umwandeln".into(),
        )),
    }
}

#[cfg(target_family = "wasm")]
fn with_ffmpeg(_input: &Path, _sound: bool) -> Result<Option<(Vec<u8>, &'static str)>> {
    Ok(None)
}
