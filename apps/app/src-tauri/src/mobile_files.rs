//! Bridge native mobile document pickers to the existing checked path imports.
//! Android grants content URIs; iOS grants security-scoped file URLs.

use std::io::{Read, Write};
use std::path::Path;

use tauri::AppHandle;
use tauri_plugin_dialog::FilePath;
use tauri_plugin_fs::{FsExt, OpenOptions};

pub enum Kind {
    Picture,
    Media,
    Archive,
}

pub fn stage(
    app: &AppHandle,
    selected: FilePath,
    kind: Kind,
) -> Result<tempfile::NamedTempFile, String> {
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
    result
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
        Kind::Picture | Kind::Media => 64 * 1024 * 1024,
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
        Kind::Media => &[
            "png", "jpg", "jpeg", "webp", "gif", "avif", "sply", "mp4", "m4v", "webm", "mov",
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
    } else {
        infer::get(header).and_then(|t| match t.mime_type() {
            "image/png" => Some("png"),
            "image/jpeg" => Some("jpg"),
            "image/webp" => Some("webp"),
            "image/gif" => Some("gif"),
            "image/avif" => Some("avif"),
            "video/mp4" | "video/x-m4v" => Some("mp4"),
            "video/webm" => Some("webm"),
            "video/quicktime" => Some("mov"),
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
    }

    #[test]
    fn mobile_copy_is_bounded() {
        let mut input = std::io::repeat(0);
        let mut output = Vec::new();
        assert!(copy_bounded(&mut input, &mut output, 4, 16).is_err());
        assert_eq!(output.len(), 13);
    }
}
