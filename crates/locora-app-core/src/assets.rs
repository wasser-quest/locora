//! The app's own graphics (assets/ui, built by assets/ui/src/ui.py), stored
//! in the profile at start so they are served like any other `.sply`.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use locora_app_api::{LEARN_LESSONS, MedalArt, Media, MediaKind, UiAssets};
use locora_store::{SPLINELY, Store};

macro_rules! ui {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../../../assets/ui/", $name, ".sply")))),*]
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
    "combo",
    "pokal",
    "serie",
    "raum-leer",
    "raum-bild",
    "raum-video",
    "erfolg-tutorial",
    "erfolg-tutorial-gesperrt",
    "erfolg-formeln",
    "erfolg-formeln-gesperrt",
    "erfolg-tour",
    "erfolg-tour-gesperrt",
    "erfolg-palast",
    "erfolg-palast-gesperrt",
    "erfolg-lernen-alle",
    "erfolg-lernen-alle-gesperrt",
    "erfolg-lernen-gesperrt",
    "erfolg-lernen-basics",
    "erfolg-lernen-visualize",
    "erfolg-lernen-exaggerate",
    "erfolg-lernen-motion",
    "erfolg-lernen-senses",
    "erfolg-lernen-link",
    "erfolg-lernen-abstract",
    "erfolg-lernen-numbers",
    "erfolg-lernen-palace",
    "erfolg-lernen-loci",
    "erfolg-lernen-route",
    "erfolg-lernen-review",
    "erfolg-lernen-reuse",
    "erfolg-lernen-mistakes",
    "erfolg-lernen-history",
    "erfolg-lernen-research",
    "erfolg-lernen-sport",
    "erfolg-spiel-erste",
    "erfolg-spiel-100xp",
    "erfolg-spiel-7tage",
    "erfolg-spiel-faellig",
    "erfolg-spiel-rhythmus",
    "erfolg-spiel-perfekt",
    "erfolg-spiel-gesperrt",
    "erfolg-gefestigt-7",
    "erfolg-gefestigt-30",
    "erfolg-gefestigt-180",
    "erfolg-gefestigt-gesperrt",
];

/// The journey trophies (`game-<id>`) and their medals; they share one grey medal while locked.
const GAME_MEDALS: [(&str, &str); 6] = [
    ("first-journey", "erfolg-spiel-erste"),
    ("hundred-xp", "erfolg-spiel-100xp"),
    ("seven-day-streak", "erfolg-spiel-7tage"),
    ("due-cleared", "erfolg-spiel-faellig"),
    ("rhythm-4", "erfolg-spiel-rhythmus"),
    ("perfect-journey", "erfolg-spiel-perfekt"),
];

/// Simo's friends who teach „Mnemotechnik lernen“ (assets/mascot).
const CHARACTERS: [&str; 4] = ["mina", "cami", "bruno", "hafiza"];

/// Graphics from elsewhere in `assets/`.
const MORE: &[(&str, &[u8])] = &[
    ("logo", include_bytes!("../../../assets/brand/icon.sply")),
    ("simo", locora_runtime::tutorial::SIMO),
    ("mina", include_bytes!("../../../assets/mascot/mina.sply")),
    ("cami", include_bytes!("../../../assets/mascot/cami.sply")),
    ("bruno", include_bytes!("../../../assets/mascot/bruno.sply")),
    (
        "hafiza",
        include_bytes!("../../../assets/mascot/hafiza.sply"),
    ),
];

/// Name → hash of every UI graphic.
pub struct Assets(HashMap<&'static str, String>);

impl Assets {
    pub fn register(store: &Store) -> Result<Self> {
        let mut map = HashMap::new();
        for (name, bytes) in FILES.iter().chain(MORE) {
            map.insert(*name, store.put_media(bytes, SPLINELY)?.hash);
        }
        Ok(Self(map))
    }

    /// Whether `hash` is one of the app's own graphics (never a palace's media).
    pub fn knows(&self, hash: &str) -> bool {
        self.0.values().any(|known| known == hash)
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
            combo: self.media("combo"),
            trophy: self.media("pokal"),
            streak: self.media("serie"),
            room_blank: self.media("raum-leer"),
            room_image: self.media("raum-bild"),
            room_video: self.media("raum-video"),
            logo: self.media("logo"),
            simo: self.media("simo"),
            medal_tutorial: self.media("erfolg-tutorial"),
            medal_tutorial_locked: self.media("erfolg-tutorial-gesperrt"),
            medal_formulas: self.media("erfolg-formeln"),
            medal_formulas_locked: self.media("erfolg-formeln-gesperrt"),
            medals: self.medals(),
            characters: CHARACTERS
                .iter()
                .map(|name| ((*name).to_owned(), self.media(name)))
                .collect(),
        }
    }

    /// The medals besides tutorial and formulas, by achievement id. The lessons share one
    /// grey medal while locked.
    fn medals(&self) -> BTreeMap<String, MedalArt> {
        let pair = |unlocked: &str, locked: &str| MedalArt {
            unlocked: self.media(unlocked),
            locked: self.media(locked),
        };
        let mut medals: BTreeMap<String, MedalArt> = LEARN_LESSONS
            .iter()
            .map(|lesson| {
                (
                    format!("learn-{lesson}"),
                    pair(&format!("erfolg-lernen-{lesson}"), "erfolg-lernen-gesperrt"),
                )
            })
            .collect();
        medals.insert(
            "learn-all".into(),
            pair("erfolg-lernen-alle", "erfolg-lernen-alle-gesperrt"),
        );
        medals.insert(
            "tour-done".into(),
            pair("erfolg-tour", "erfolg-tour-gesperrt"),
        );
        medals.insert(
            "first-palace".into(),
            pair("erfolg-palast", "erfolg-palast-gesperrt"),
        );
        for (id, file) in GAME_MEDALS {
            medals.insert(format!("game-{id}"), pair(file, "erfolg-spiel-gesperrt"));
        }
        // Palace medals by level (`palace-7`, `palace-30`, `palace-180`).
        for level in [7, 30, 180] {
            medals.insert(
                format!("palace-{level}"),
                pair(
                    &format!("erfolg-gefestigt-{level}"),
                    "erfolg-gefestigt-gesperrt",
                ),
            );
        }
        medals
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_graphic_converts_to_svg() {
        for (name, bytes) in super::FILES.iter().chain(super::MORE) {
            assert!(crate::sply::to_svg(bytes).is_ok(), "{name}");
        }
    }
}
