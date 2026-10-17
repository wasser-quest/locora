//! Byte-range responses for immutable, content-addressed media.
use std::{
    borrow::Cow,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
use tauri::http::{Response, StatusCode, header};

/// Resolve a single byte range. Unsupported multipart or malformed ranges
/// are ignored (a full response); unsatisfiable ranges return an error.
fn range(value: &str, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(value) = value.strip_prefix("bytes=") else {
        return Ok(None);
    };
    let Some((start, end)) = value.split_once('-') else {
        return Ok(None);
    };
    if value.contains(',') {
        return Ok(None);
    }
    if start.is_empty() {
        let Ok(suffix) = end.parse::<u64>() else {
            return Ok(None);
        };
        if suffix == 0 || len == 0 {
            return Err(());
        }
        return Ok(Some((len.saturating_sub(suffix), len - 1)));
    }
    let Ok(start) = start.parse::<u64>() else {
        return Ok(None);
    };
    let end = if end.is_empty() {
        len.saturating_sub(1)
    } else {
        let Ok(end) = end.parse::<u64>() else {
            return Ok(None);
        };
        if end < start {
            return Ok(None);
        }
        end
    };
    if start >= len {
        return Err(());
    }
    Ok(Some((start, end.min(len - 1))))
}

/// Read only the requested bytes; HEAD ignores Range and returns full metadata.
pub fn serve(
    path: &Path,
    mime: &str,
    requested: Option<&str>,
    head: bool,
) -> std::io::Result<Response<Cow<'static, [u8]>>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let selected = if head {
        Ok(None)
    } else {
        requested.map_or(Ok(None), |r| range(r, len))
    };
    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
        .header("X-Content-Type-Options", "nosniff")
        // The WebView reads videos over XHR to hand them to <video> as a blob;
        // its page origin differs from this scheme, so allow reading.
        .header("Access-Control-Allow-Origin", "*");
    let (start, count) = match selected {
        Err(()) => {
            return Ok(builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .header(header::CONTENT_LENGTH, 0)
                .body(Cow::Owned(Vec::new()))
                .expect("valid response"));
        }
        Ok(Some((start, end))) => {
            builder = builder
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"));
            (start, end - start + 1)
        }
        Ok(None) => (0, len),
    };
    let mut bytes = Vec::new();
    if !head {
        file.seek(SeekFrom::Start(start))?;
        file.take(count).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != count {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "media changed while reading",
            ));
        }
    }
    Ok(builder
        .header(header::CONTENT_LENGTH, count)
        .body(Cow::Owned(bytes))
        .expect("valid response"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_responses_and_head() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("video");
        std::fs::write(&file, b"0123456789").unwrap();
        for (request, expected) in [
            ("bytes=2-4", "234"),
            ("bytes=7-", "789"),
            ("bytes=-3", "789"),
            ("bytes=8-99", "89"),
        ] {
            let response = serve(&file, "video/mp4", Some(request), false).unwrap();
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(response.body().as_ref(), expected.as_bytes());
            assert_eq!(
                response.headers()[header::CONTENT_LENGTH],
                expected.len().to_string()
            );
        }
        let response = serve(&file, "video/mp4", Some("bytes=10-"), false).unwrap();
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes */10");
        for request in [None, Some("bytes=bad"), Some("bytes=0-1,4-5")] {
            let response = serve(&file, "video/mp4", request, false).unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.body().as_ref(), b"0123456789");
        }
        let response = serve(&file, "video/mp4", Some("bytes=2-4"), true).unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "10");
        assert!(response.body().is_empty());
        std::fs::write(&file, b"").unwrap();
        assert_eq!(
            serve(&file, "video/mp4", Some("bytes=0-"), false)
                .unwrap()
                .status(),
            StatusCode::RANGE_NOT_SATISFIABLE
        );
    }
}
