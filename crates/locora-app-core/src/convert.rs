//! Pictures and videos a person adds, as Locora stores them (see `locora_media::prepare`).

use anyhow::Result;

/// The bytes and media type to store for a picture or video read from `path`; `.sply` as it is.
pub fn prepare(
    path: &std::path::Path,
    bytes: Vec<u8>,
    mime: &'static str,
) -> Result<(Vec<u8>, &'static str)> {
    if mime == locora_store::SPLINELY {
        return Ok((bytes, mime));
    }
    Ok(locora_media::prepare(path, bytes, mime)?)
}
