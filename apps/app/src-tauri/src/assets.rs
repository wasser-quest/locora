//! The app's own graphics (assets/ui, built by assets/ui/src/ui.py), stored
//! in the profile at start so they are served like any other `.sply`.

use std::collections::HashMap;

use anyhow::Result;
use locora_app_api::{Media, MediaKind, UiAssets};
use locora_store::{SPLINELY, Store};

macro_rules! ui {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../../../../assets/ui/", $name, ".sply")))),*]
    };
}

const FILES: &[(&str, &[u8])] = ui![
    "kachel-core",
    "kachel-review",
    "kachel-notify",
    "kachel-tutorial",
    "kachel-gamification",
    "kachel-plugin",
    "richtig",
    "nochmal",
    "geschafft",
];

/// Name → hash of every UI graphic.
pub struct Assets(HashMap<&'static str, String>);

impl Assets {
    pub fn register(store: &Store) -> Result<Self> {
        let mut map = HashMap::new();
        for (name, bytes) in FILES {
            map.insert(*name, store.put_media(bytes, SPLINELY)?.hash);
        }
        Ok(Self(map))
    }

    fn media(&self, name: &str) -> Media {
        Media {
            hash: self.0[name].clone(),
            kind: MediaKind::Sply,
        }
    }

    /// The tile icon for a plugin; any other plugin gets the puzzle piece.
    pub fn tile_icon(&self, plugin: &str) -> Media {
        let name = match plugin {
            "core" => "kachel-core",
            "review" => "kachel-review",
            "notify" => "kachel-notify",
            "tutorial" => "kachel-tutorial",
            "gamification" => "kachel-gamification",
            _ => "kachel-plugin",
        };
        self.media(name)
    }

    pub fn ui(&self) -> UiAssets {
        UiAssets {
            right: self.media("richtig"),
            again: self.media("nochmal"),
            done: self.media("geschafft"),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_graphic_converts_to_svg() {
        for (name, bytes) in super::FILES {
            assert!(crate::sply::to_svg(bytes).is_ok(), "{name}");
        }
    }
}
