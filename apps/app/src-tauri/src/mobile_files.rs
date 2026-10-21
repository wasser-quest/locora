//! Bridge native mobile document pickers to the existing checked path imports.
//! Android grants content URIs; iOS grants security-scoped file URLs.

use std::io::{Read, Write};
use std::path::Path;

use tauri::AppHandle;
use tauri_plugin_dialog::FilePath;
use tauri_plugin_fs::{FsExt, OpenOptions};

pub enum Kind {
    Picture,
    /// A locus sound clip (mp3, m4a, wav, ogg, weba, flac, aac; amr, 3gp and caf to convert).
    Audio,
    Media,
    Archive,
}

pub struct Staged {
    file: tempfile::NamedTempFile,
    name: Option<String>,
}

impl Staged {
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

pub fn stage(app: &AppHandle, selected: FilePath, kind: Kind) -> Result<Staged, String> {
    let name = if matches!(kind, Kind::Media) {
        selected_name(&selected)
    } else {
        None
    };
    let mut input = app
        .fs()
        .open(selected.clone(), OpenOptions::new())
        .map_err(|e| format!("Ausgewählte Datei öffnen: {e}"))?;
    let result = stage_open(&mut input, &selected, kind);
    drop(input);
    #[cfg(target_os = "ios")]
    if matches!(selected, FilePath::Url(_)) {
        app.fs()
            .stop_accessing_security_scoped_resource(selected)
            .map_err(|e| format!("Dateizugriff beenden: {e}"))?;
    }
    result.map(|file| Staged { file, name })
}

/// A staged video as Locora keeps it. Videos in codecs that devices play (H.264, HEVC, AV1, VP9,
/// VP8 …) stay as they are; only a video the core would refuse is converted to MP4 with H.264 by
/// the device (see `tauri-plugin-locora-video`). Pictures stay as they are. The core then removes
/// the metadata.
#[cfg(target_os = "android")]
pub fn converted_if_needed(app: &AppHandle, staged: Staged) -> Result<Staged, String> {
    if !locora_app_core::convert::video_needs_conversion(staged.path()) {
        return Ok(staged);
    }
    let output = tempfile::Builder::new()
        .suffix(".mp4")
        .tempfile()
        .map_err(|e| format!("Temporäre Datei anlegen: {e}"))?;
    tauri_plugin_locora_video::convert(app, staged.path(), output.path())?;
    Ok(Staged {
        file: output,
        name: staged.name,
    })
}

fn selected_name(selected: &FilePath) -> Option<String> {
    let file_name = match selected {
        FilePath::Path(path) => path.file_name()?.to_str()?.to_owned(),
        FilePath::Url(url) if url.scheme() == "file" => {
            url.to_file_path().ok()?.file_name()?.to_str()?.to_owned()
        }
        FilePath::Url(url) => {
            percent_encoding::percent_decode_str(url.path_segments()?.next_back()?)
                .decode_utf8()
                .ok()?
                .into_owned()
        }
    };
    if file_name.contains('/') || file_name.contains('\\') {
        return None;
    }
    let path = Path::new(&file_name);
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    if ![
        "png", "jpg", "jpeg", "webp", "gif", "avif", "sply", "mp4", "m4v", "mov", "webm", "mkv",
        "3gp",
    ]
    .contains(&extension.as_str())
    {
        return None;
    }
    let stem = path.file_stem()?.to_str()?.trim();
    if stem.is_empty() || stem.chars().count() > 100 || stem.chars().any(char::is_control) {
        return None;
    }
    Some(stem.to_owned())
}

fn stage_open(
    input: &mut impl Read,
    selected: &FilePath,
    kind: Kind,
) -> Result<tempfile::NamedTempFile, String> {
    let mut header = [0; 512];
    let mut size = 0;
    while size < header.len() {
        let read = input
            .read(&mut header[size..])
            .map_err(|e| format!("Ausgewählte Datei lesen: {e}"))?;
        if read == 0 {
            break;
        }
        size += read;
    }
    let extension = extension(selected, &header[..size], &kind)?;
    let mut staged = tempfile::Builder::new()
        .suffix(&format!(".{extension}"))
        .tempfile()
        .map_err(|e| format!("Temporäre Datei anlegen: {e}"))?;
    staged
        .write_all(&header[..size])
        .map_err(|e| format!("Temporäre Datei schreiben: {e}"))?;
    let limit = match kind {
        Kind::Picture | Kind::Audio | Kind::Media => locora_format::limits::ADDED_MEDIA_BYTES,
        Kind::Archive => locora_format::limits::MEDIA_BYTES,
    };
    copy_bounded(input, &mut staged, size as u64, limit)?;
    staged.flush().map_err(|e| format!("Datei sichern: {e}"))?;
    Ok(staged)
}

fn copy_bounded(
    input: &mut impl Read,
    output: &mut impl Write,
    already_read: u64,
    limit: u64,
) -> Result<(), String> {
    let remaining = limit.saturating_sub(already_read);
    let copied = std::io::copy(&mut input.take(remaining + 1), output)
        .map_err(|e| format!("Ausgewählte Datei kopieren: {e}"))?;
    if already_read + copied > limit {
        return Err(format!(
            "Ausgewählte Datei ist größer als {} MiB",
            limit >> 20
        ));
    }
    Ok(())
}

fn extension(selected: &FilePath, header: &[u8], kind: &Kind) -> Result<&'static str, String> {
    let name = match selected {
        FilePath::Path(path) => path.file_name().and_then(|n| n.to_str()),
        FilePath::Url(url) => url.path_segments().and_then(|mut s| s.next_back()),
    };
    let suffix = name
        .and_then(|n| Path::new(n).extension())
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);
    let known = match kind {
        Kind::Picture => &["png", "jpg", "jpeg", "webp", "gif", "avif", "sply"][..],
        Kind::Audio => &locora_app_core::locus_audio::EXTENSIONS[..],
        Kind::Media => &[
            "png", "jpg", "jpeg", "webp", "gif", "avif", "sply", "mp4", "m4v", "mov", "webm",
            "mkv", "3gp",
        ][..],
        Kind::Archive => &["locora", "toml"][..],
    };
    if let Some(suffix) = suffix.as_deref().filter(|s| known.contains(s)) {
        return Ok(match suffix {
            "jpeg" => "jpg",
            "m4v" => "mp4",
            other => known.iter().copied().find(|s| *s == other).unwrap(),
        });
    }
    let detected = if header.starts_with(b"\x89SPLY\r\n\x1a") {
        Some("sply")
    } else if header.starts_with(b"PK\x03\x04") && matches!(kind, Kind::Archive) {
        Some("locora")
    } else if matches!(kind, Kind::Archive) {
        Some("toml")
    } else if matches!(kind, Kind::Audio) {
        // Phones often hand over sound without a file name; its first bytes tell the format.
        infer::get(header).and_then(|t| match t.mime_type() {
            "audio/mpeg" => Some("mp3"),
            "audio/m4a" | "video/mp4" | "video/quicktime" => Some("m4a"),
            "audio/x-wav" => Some("wav"),
            "audio/ogg" | "audio/opus" => Some("ogg"),
            // Browser and phone recordings: WebM sound, FLAC, AAC, AMR of older phones.
            "video/webm" | "audio/webm" => Some("weba"),
            "audio/x-flac" | "audio/flac" => Some("flac"),
            "audio/aac" => Some("aac"),
            "audio/amr" => Some("amr"),
            "video/3gpp" | "audio/3gpp" => Some("3gp"),
            _ => None,
        })
    } else {
        infer::get(header).and_then(|t| match t.mime_type() {
            "image/png" => Some("png"),
            "image/jpeg" => Some("jpg"),
            "image/webp" => Some("webp"),
            "image/gif" => Some("gif"),
            "image/avif" => Some("avif"),
            "video/mp4" | "video/x-m4v" => Some("mp4"),
            "video/webm" => Some("webm"),
            "video/x-matroska" => Some("mkv"),
            "video/quicktime" => Some("mov"),
            "video/3gpp" => Some("3gp"),
            _ => None,
        })
    };
    detected
        .filter(|s| known.contains(s))
        .ok_or_else(|| "Dateiformat der mobilen Auswahl nicht erkannt".to_owned())
}

pub fn save(app: &AppHandle, selected: FilePath, source: &Path) -> Result<(), String> {
    let mut source = std::fs::File::open(source).map_err(|e| format!("Exportdatei öffnen: {e}"))?;
    let mut options = OpenOptions::new();
    options.read(false).write(true).truncate(true).create(true);
    let mut destination = app
        .fs()
        .open(selected.clone(), options)
        .map_err(|e| format!("Exportziel öffnen: {e}"))?;
    let result = std::io::copy(&mut source, &mut destination)
        .and_then(|_| destination.flush())
        .map_err(|e| format!("Exportdatei schreiben: {e}"));
    drop(destination);
    #[cfg(target_os = "ios")]
    if matches!(selected, FilePath::Url(_)) {
        app.fs()
            .stop_accessing_security_scoped_resource(selected)
            .map_err(|e| format!("Dateizugriff beenden: {e}"))?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_android_uri_uses_file_signature() {
        let uri = "content://provider/document/42".parse().unwrap();
        assert_eq!(
            extension(&uri, b"\x89SPLY\r\n\x1a", &Kind::Picture).unwrap(),
            "sply"
        );
        assert_eq!(
            extension(&uri, b"PK\x03\x04", &Kind::Archive).unwrap(),
            "locora"
        );
        assert_eq!(
            extension(&uri, b"RIFF\x24\0\0\0WAVEfmt ", &Kind::Audio).unwrap(),
            "wav"
        );
        assert!(extension(&uri, b"\x89SPLY\r\n\x1a", &Kind::Audio).is_err());
    }

    #[test]
    fn mobile_name_comes_from_selected_file_not_temporary_copy() {
        let named = "content://provider/document/Mein%20Bild.png"
            .parse()
            .unwrap();
        let opaque = "content://provider/document/42".parse().unwrap();
        let local = FilePath::Path("/pictures/Palast.mov".into());
        assert_eq!(selected_name(&named).as_deref(), Some("Mein Bild"));
        assert_eq!(selected_name(&opaque), None);
        assert_eq!(selected_name(&local).as_deref(), Some("Palast"));
    }

    #[test]
    fn mobile_copy_is_bounded() {
        let mut input = std::io::repeat(0);
        let mut output = Vec::new();
        assert!(copy_bounded(&mut input, &mut output, 4, 16).is_err());
        assert_eq!(output.len(), 13);
    }
}
