//! Colour themes. "System" follows the operating system; the others force a palette
//! through the `data-theme` attribute of the document (the palettes live in style.css).
use crate::i18n::t;

const KEY: &str = "locora-theme-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    System,
    Light,
    Dark,
    Contrast,
}

impl Theme {
    pub const ALL: [Theme; 4] = [Theme::System, Theme::Light, Theme::Dark, Theme::Contrast];

    pub fn key(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
            Theme::Contrast => "contrast",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::System => t("System"),
            Theme::Light => t("Hell"),
            Theme::Dark => t("Dunkel"),
            Theme::Contrast => t("Hoher Kontrast"),
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|theme| theme.key() == key)
            .unwrap_or(Theme::System)
    }
}

/// The saved theme; "System" when nothing is saved or storage is unavailable.
pub fn current() -> Theme {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(KEY).ok().flatten())
        .map(|key| Theme::from_key(&key))
        .unwrap_or(Theme::System)
}

pub fn save(theme: Theme) {
    if let Some(storage) =
        web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    {
        let _ = storage.set_item(KEY, theme.key());
    }
}

/// Puts the theme on the document root, where style.css picks the palette.
pub fn apply(theme: Theme) {
    if let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    {
        let _ = root.set_attribute("data-theme", theme.key());
    }
}
