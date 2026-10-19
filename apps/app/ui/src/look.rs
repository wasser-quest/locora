//! How the app looks beyond the theme: the accent colour and the density. Kept per viewer and
//! applied to the page at start and after every change.

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

const KEY: &str = "locora-look-v1";

/// Accent presets: id, label, colour. The colour replaces the indigo of buttons and marks.
pub const ACCENTS: &[(&str, &str, &str)] = &[
    ("indigo", "Indigo", "#3a3d92"),
    ("forest", "Wald", "#2f6b3a"),
    ("amber", "Bernstein", "#8a5a12"),
    ("rose", "Rose", "#9b3163"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Look {
    #[serde(default = "indigo")]
    pub accent: String,
    /// `normal` or `compact`.
    #[serde(default = "normal")]
    pub density: String,
}

fn indigo() -> String {
    "indigo".to_owned()
}

fn normal() -> String {
    "normal".to_owned()
}

impl Default for Look {
    fn default() -> Self {
        Self {
            accent: indigo(),
            density: normal(),
        }
    }
}

pub fn load() -> Look {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(KEY).ok().flatten())
        .and_then(|value| serde_json::from_str::<Look>(&value).ok())
        .unwrap_or_default()
}

pub fn save(look: &Look) {
    let storage = web_sys::window().and_then(|window| window.local_storage().ok().flatten());
    if let (Some(storage), Ok(json)) = (storage, serde_json::to_string(look)) {
        let _ = storage.set_item(KEY, &json);
    }
    apply(look);
}

/// Put the look on the page: the accent as the indigo variable, the density as an attribute.
pub fn apply(look: &Look) {
    let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    else {
        return;
    };
    let accent = ACCENTS
        .iter()
        .find(|(id, _, _)| *id == look.accent)
        .or_else(|| ACCENTS.first())
        .map(|(_, _, colour)| *colour)
        .unwrap_or("#3a3d92");
    if let Ok(element) = root.clone().dyn_into::<web_sys::HtmlElement>() {
        let _ = element.style().set_property("--indigo", accent);
    }
    let _ = root.set_attribute("data-density", &look.density);
}
