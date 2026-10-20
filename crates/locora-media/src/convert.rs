//! A picture or video file as Locora stores it: cleaned, and a video that needs a conversion
//! converted by ffmpeg where it is installed (desktop, CLI). Android and the browser convert
//! before the file reaches the core; there, and without ffmpeg, such a video is refused.

use std::path::Path;

use crate::{Cleaned, MediaError, Result, clean};

/// Largest converted video that is kept (the limit for added media).
const LIMIT: usize = 500 << 20;

/// The bytes and media type to store for the file at `path`, read as `bytes` of type `mime`.
pub fn prepare(path: &Path, bytes: Vec<u8>, mime: &str) -> Result<(Vec<u8>, &'static str)> {
    match clean(bytes, mime)? {
        Cleaned::Ready { bytes, mime } => Ok((bytes, mime)),
        Cleaned::NeedsConversion { reason } => {
            let Some(converted) = with_ffmpeg(path)? else {
                return Err(MediaError::Conversion(format!(
                    "{reason}. Locora speichert Videos als MP4 (H.264): mit installiertem ffmpeg wandelt \
                     Locora sie selbst um, sonst das Video vorher umwandeln"
                )));
            };
            match clean(converted, "video/mp4")? {
                Cleaned::Ready { bytes, mime } => Ok((bytes, mime)),
                Cleaned::NeedsConversion { reason } => Err(MediaError::Conversion(format!(
                    "Die Umwandlung ergab kein passendes MP4: {reason}"
                ))),
            }
        }
    }
}

/// The video as MP4 (H.264, AAC, no metadata) by ffmpeg, or `None` where ffmpeg or an H.264
/// encoder is missing.
#[cfg(not(target_family = "wasm"))]
fn with_ffmpeg(input: &Path) -> Result<Option<Vec<u8>>> {
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
    let video: &[&str] = if encoders.contains(" libx264 ") {
        &["-c:v", "libx264", "-preset", "veryfast", "-crf", "23"]
    } else if encoders.contains(" libopenh264 ") {
        &["-c:v", "libopenh264", "-b:v", "6M"]
    } else {
        return Ok(None);
    };
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let run = RUNS.fetch_add(1, Ordering::Relaxed);
    let output =
        std::env::temp_dir().join(format!("locora-convert-{}-{run}.mp4", std::process::id()));
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(input)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
        ])
        .args(video)
        .args([
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "160k",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ])
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
        Some(bytes) if bytes.len() <= LIMIT => Ok(Some(bytes)),
        Some(_) => Err(MediaError::Conversion(
            "Das umgewandelte Video ist größer als 500 MiB".into(),
        )),
        None => Err(MediaError::Conversion(
            "ffmpeg konnte das Video nicht in MP4 (H.264) umwandeln".into(),
        )),
    }
}

#[cfg(target_family = "wasm")]
fn with_ffmpeg(_input: &Path) -> Result<Option<Vec<u8>>> {
    Ok(None)
}
