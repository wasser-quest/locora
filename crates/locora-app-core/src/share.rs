//! A share card's graphic: the app's own medal or trophy, drawn still and without scripts as a
//! PNG the UI puts on its card. Only the app's graphics (assets/ui) are drawn, never a palace's
//! media. The picture the UI makes from it comes back here to be checked before it is saved.

use anyhow::{Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use locora_store::Store;
use splinely_document::Project;

use crate::assets::Assets;

/// The moment drawn: the graphics' resting pose (their autoplay clips start there).
pub const ART_TIME: f64 = 0.0;
/// The graphic's width on the card, in pixels (the card is 1080 wide).
pub const ART_WIDTH: u32 = 520;
/// The largest picture `share_send` takes.
pub const PNG_MAX: usize = 8 * 1024 * 1024;
/// The first bytes of every PNG file.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// Characters of a file name between `locora-` and `.png`.
const NAME_MAX: usize = 64;

/// The graphic `hash` as a PNG, base64, when it is one of the app's own graphics.
pub fn art_png(assets: &Assets, store: &Store, hash: &str) -> Result<String> {
    if !assets.knows(hash) {
        bail!("Grafik nicht gefunden");
    }
    draw(store, hash)
}

/// Draws the graphic `hash` as a PNG, base64. The caller checks first that it is one of the
/// app's own graphics (`Assets::knows`; `art_png` does both); the desktop checks under the
/// backend's lock and draws without it.
pub fn draw(store: &Store, hash: &str) -> Result<String> {
    let file = store
        .media_file(hash)
        .ok_or_else(|| anyhow!("Grafik nicht gefunden"))?;
    let bytes = std::fs::read(file)?;
    let project = Project::from_bytes(&bytes).map_err(|error| anyhow!("{error}"))?;
    let png = project
        .render_png(ART_TIME, ART_WIDTH)
        .map_err(|error| anyhow!("{error}"))?;
    Ok(STANDARD.encode(png))
}

/// The bytes of a picture from the UI: base64 of a PNG no larger than `PNG_MAX`.
pub fn png_bytes(base64: &str) -> Result<Vec<u8>> {
    // Four characters carry three bytes: a longer text cannot be a picture that is allowed.
    if base64.len() > PNG_MAX / 3 * 4 + 4 {
        bail!("Das Bild ist zu groß");
    }
    let bytes = STANDARD
        .decode(base64.trim())
        .map_err(|_| anyhow!("Das Bild konnte nicht erstellt werden."))?;
    if bytes.len() > PNG_MAX {
        bail!("Das Bild ist zu groß");
    }
    if !bytes.starts_with(&PNG_SIGNATURE) {
        bail!("Das Bild konnte nicht erstellt werden.");
    }
    Ok(bytes)
}

/// A file name for a shared picture: `locora-<lowercase ascii letters, digits and dashes>.png`,
/// at most 64 between them, or `locora.png`. Never a path.
pub fn file_name(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    let lower = lower.strip_suffix(".png").unwrap_or(&lower);
    let lower = lower.strip_prefix("locora-").unwrap_or(lower);
    let mut rest = String::new();
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            rest.push(c);
        } else if !rest.ends_with('-') {
            rest.push('-');
        }
    }
    let rest: String = rest.trim_matches('-').chars().take(NAME_MAX).collect();
    let rest = rest.trim_end_matches('-');
    if rest.is_empty() {
        "locora.png".into()
    } else {
        format!("locora-{rest}.png")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_graphics_become_pngs_and_nothing_else_does() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let assets = Assets::register(&store).unwrap();
        let ui = assets.ui();
        let hash = &ui.medals["game-seven-day-streak"].unlocked.hash;
        let png = STANDARD
            .decode(art_png(&assets, &store, hash).unwrap())
            .unwrap();
        assert!(png.starts_with(&PNG_SIGNATURE));
        // IHDR: the width is the first field after the chunk's length and name.
        let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        assert_eq!(width, ART_WIDTH);
        // A palace's media is not one of the app's graphics, nor is a made-up hash.
        let palace = store
            .put_media(
                &[&PNG_SIGNATURE[..], b"a palace's photo"].concat(),
                "image/png",
            )
            .unwrap();
        assert!(!assets.knows(&palace.hash));
        assert_eq!(
            art_png(&assets, &store, &palace.hash)
                .unwrap_err()
                .to_string(),
            "Grafik nicht gefunden"
        );
        assert!(art_png(&assets, &store, &"0".repeat(64)).is_err());
        assert!(art_png(&assets, &store, "../cloud.json").is_err());
    }

    #[test]
    fn pictures_from_the_ui_are_checked() {
        let png = STANDARD.encode([&PNG_SIGNATURE[..], b"rest"].concat());
        assert_eq!(png_bytes(&png).unwrap().len(), 12);
        assert!(png_bytes(&STANDARD.encode(b"GIF89a....")).is_err());
        assert!(png_bytes("not base64 at all!").is_err());
        let large = STANDARD.encode(vec![0u8; PNG_MAX + 1]);
        assert_eq!(
            png_bytes(&large).unwrap_err().to_string(),
            "Das Bild ist zu groß"
        );
        assert_eq!(file_name("../Küche/x"), "locora-k-che-x.png");
        assert_eq!(
            file_name("locora-game-hundred-xp.png"),
            "locora-game-hundred-xp.png"
        );
        assert_eq!(
            file_name("game-seven-day-streak"),
            "locora-game-seven-day-streak.png"
        );
        assert_eq!(file_name(""), "locora.png");
        assert_eq!(file_name("../.."), "locora.png");
        assert_eq!(file_name(&"a".repeat(200)).len(), "locora-.png".len() + 64);
        for name in ["../Küche/x", "C:\\x\\y", "..", "a/../../b", "x.png.png"] {
            let file = file_name(name);
            assert!(
                !file.contains('/') && !file.contains('\\') && !file.contains(".."),
                "{file}"
            );
        }
    }
}
