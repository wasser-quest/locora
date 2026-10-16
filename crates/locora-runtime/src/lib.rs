//! What the Locora CLI and app share: opening a profile (store, plugin host,
//! built-in and approved installed plugins) and answering a locus (Core
//! evaluation, score hook, events for review and other plugins).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

use anyhow::{Context, Result, bail};
use locora_api::core::{Grade, JourneyScore, LocusAnswered, Score};
use locora_api::journey::{Evaluate, EvaluateRequest, Scored, Submission};
use locora_api::ui::Tile;

/// Latest `ui.tile` per plugin, kept in the profile (reserved id `host`).
const TILES: &str = "tiles";
use locora_api::review::{CardState, Schedule, ScheduleRequest};
use locora_host::{HostEvent, KvStore, Level, Manifest, PluginHost};
use locora_store::{PalaceTree, Store};

pub mod journey;
pub mod settings;
pub mod transfer;
pub mod tutorial;

pub mod embedded {
    //! Official plugins compiled into this crate by `build.rs`.
    include!(concat!(env!("OUT_DIR"), "/embedded_plugins.rs"));
}

/// Plugin storage backed by the local database.
struct StoreKv(Arc<Store>);

impl KvStore for StoreKv {
    fn get(&self, plugin: &str, key: &str) -> Result<Option<Vec<u8>>, String> {
        self.0.kv_get(plugin, key).map_err(|e| e.to_string())
    }

    fn set(&self, plugin: &str, key: &str, value: &[u8]) -> Result<(), String> {
        self.0.kv_set(plugin, key, value).map_err(|e| e.to_string())
    }
}

/// Profile directory: explicit, `$LOCORA_DATA` or the platform data dir.
pub fn data_dir(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = explicit {
        return Ok(dir.to_owned());
    }
    if let Some(dir) = env::var_os("LOCORA_DATA") {
        return Ok(PathBuf::from(dir));
    }
    Ok(dirs::data_dir()
        .context("Kein Datenordner gefunden; gib einen mit --data an")?
        .join("locora"))
}

/// Installed components must live in a real directory beneath the chosen profile.
/// The profile itself may be a user-selected symlink; its plugin directory may not.
pub fn plugin_dir(data: &Path) -> Result<PathBuf> {
    let dir = data.join("plugins");
    match fs::symlink_metadata(&dir) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            bail!(
                "Plugin-Ordner {} muss ein Verzeichnis ohne symbolischen Link sein",
                dir.display()
            );
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).context("Plugin-Ordner prüfen"),
    }
    Ok(dir)
}

/// Read a file that must be a regular file (no symlink, FIFO or directory).
pub fn read_regular(file: &Path) -> Result<Vec<u8>> {
    if !fs::symlink_metadata(file)?.file_type().is_file() {
        bail!("Keine reguläre Plugin-Datei: {}", file.display());
    }
    Ok(fs::read(file)?)
}

/// Current time in seconds; `LOCORA_NOW` overrides it (tests, demos).
pub fn now() -> u64 {
    env::var("LOCORA_NOW")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        })
}

/// An open profile: local store plus plugin host.
pub struct Profile {
    pub store: Arc<Store>,
    pub host: PluginHost,
    /// Problems noticed while opening (plugins that did not load).
    pub notes: Vec<String>,
}

impl Profile {
    /// Open the profile in `data` (see [`data_dir`]) with all built-in plugins
    /// and every installed plugin that has a matching approval.
    pub fn open(data: Option<&Path>) -> Result<Self> {
        let data_dir = data_dir(data)?;
        let installed = plugin_dir(&data_dir)?;
        let store = Arc::new(
            Store::open(&data_dir)
                .with_context(|| format!("Datenbank in {} öffnen", data_dir.display()))?,
        );
        let cache = dirs::cache_dir().map(|d| d.join("locora").join("plugins"));
        let mut host =
            PluginHost::with_palaces(cache, Arc::new(StoreKv(store.clone())), store.clone())?;
        let mut notes = Vec::new();
        for (name, bytes) in embedded::EMBEDDED {
            if let Err(err) = host.load_bytes(bytes) {
                notes.push(format!(
                    "Eingebautes Plugin `{name}` konnte nicht laden: {err:#}"
                ));
            }
        }
        if let Ok(entries) = fs::read_dir(&installed) {
            let mut files: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "wasm"))
                .collect();
            files.sort();
            for file in files {
                let load = (|| -> Result<Manifest> {
                    let approval = read_regular(&file.with_extension("wasm.json")).context(
                        "Keine Installationszustimmung; bitte `locora plugin install` verwenden",
                    )?;
                    let approved = serde_json::from_slice(&approval)?;
                    host.load_approved_file(&file, &approved)
                })();
                if let Err(err) = load {
                    notes.push(format!(
                        "Plugin {} konnte nicht laden: {err:#}",
                        file.display()
                    ));
                }
            }
        }
        let mut profile = Self { store, host, notes };
        let reported = profile.report();
        profile.notes.extend(reported);
        Ok(profile)
    }

    /// Publish an event; returns plugin problems it caused.
    pub fn publish<E: locora_api::Event>(&mut self, event: &E) -> Result<Vec<String>> {
        self.host.publish(event)?;
        Ok(self.report())
    }

    /// Plugin warnings and errors since the last call; normal chatter is dropped.
    pub fn report(&mut self) -> Vec<String> {
        let events = self.host.drain_events();
        let mut notes = Vec::new();
        for event in &events {
            if let HostEvent::Published {
                plugin,
                topic,
                payload,
            } = event
                && topic == <Tile as locora_api::Event>::TOPIC
            {
                match locora_api::decode::<Tile>(payload) {
                    Ok(tile) => {
                        if let Err(e) = self.keep_tile(plugin, tile) {
                            notes.push(format!("[{plugin}] Kachel nicht gespeichert: {e:#}"));
                        }
                    }
                    Err(e) => notes.push(format!("[{plugin}] ungültige Kachel: {e}")),
                }
            }
        }
        notes.extend(events.into_iter().filter_map(|event| match event {
            HostEvent::Log {
                plugin,
                level: Level::Warn | Level::Error,
                message,
            } => Some(format!("[{plugin}] {message}")),
            HostEvent::HandlerError {
                plugin,
                topic,
                message,
            } => Some(format!("[{plugin}] Fehler bei {topic}: {message}")),
            HostEvent::Disabled { plugin, reason } => {
                Some(format!("Plugin `{plugin}` wurde deaktiviert: {reason}"))
            }
            _ => None,
        }));
        notes
    }

    /// The latest tile of every loaded plugin, in plugin order (`ui.tile`).
    /// Call [`Profile::report`] first so tiles published by earlier calls are
    /// stored. Unreadable saved tiles show as none (and are kept, see `keep_tile`).
    pub fn tiles(&self) -> Result<Vec<(String, Tile)>> {
        let mut saved = self.saved_tiles().unwrap_or_default();
        Ok(self
            .host
            .plugins()
            .filter_map(|m| saved.remove(&m.id).map(|tile| (m.id.clone(), tile)))
            .collect())
    }

    fn saved_tiles(&self) -> Result<BTreeMap<String, Tile>> {
        Ok(match self.store.kv_get("host", TILES)? {
            // An error, not an empty map: the next write would drop every tile.
            Some(bytes) => {
                serde_json::from_slice(&bytes).context("Gespeicherte Kacheln sind unlesbar")?
            }
            None => BTreeMap::new(),
        })
    }

    fn keep_tile(&self, plugin: &str, tile: Tile) -> Result<()> {
        let mut saved = self.saved_tiles()?;
        saved.insert(plugin.to_owned(), tile);
        self.store
            .kv_set("host", TILES, &serde_json::to_vec(&saved)?)?;
        Ok(())
    }

    /// Grade one answer: Core evaluates it, plugins may adjust the points
    /// (`core.journey.score`), and review and others hear about it.
    /// `expected` is the resolved item (needed for typed answers).
    /// Review cards by locus id, for one palace or all; empty without the
    /// review plugin.
    pub fn schedule(&mut self, palace: Option<&str>) -> Result<HashMap<String, CardState>> {
        if !self.host.provides(<Schedule as locora_api::Service>::NAME) {
            return Ok(HashMap::new());
        }
        let request = ScheduleRequest {
            palace_id: palace.map(str::to_owned),
        };
        Ok(self
            .host
            .call::<Schedule>(&request)
            .context("Wiederholungsplan nicht verfügbar")?
            .into_iter()
            .map(|s| (s.locus_id, s.card))
            .collect())
    }

    pub fn answer(&mut self, request: AnswerRequest<'_>) -> Result<Answered> {
        let evaluation = self.host.call_plugin::<Evaluate>(
            "core",
            &EvaluateRequest {
                answer: request.answer.clone(),
                expected: request.expected.to_owned(),
                submission: request.submission,
                elapsed_ms: request.elapsed_ms,
            },
        )?;
        let mut notes = self.report();
        let score = self.host.filter::<JourneyScore>(Score {
            palace_id: request.palace_id.to_owned(),
            locus_id: request.locus_id.to_owned(),
            points: evaluation.points,
        })?;
        notes.extend(self.report());
        notes.extend(self.publish(&LocusAnswered {
            palace_id: request.palace_id.to_owned(),
            locus_id: request.locus_id.to_owned(),
            correct: evaluation.correct,
            grade: evaluation.grade,
            elapsed_ms: request.elapsed_ms,
            answered_at: now(),
        })?);
        // Trusted route identity, even if a points filter changed ids.
        notes.extend(self.publish(&Scored {
            palace_id: request.palace_id.to_owned(),
            locus_id: request.locus_id.to_owned(),
            points: score.points,
        })?);
        Ok(Answered {
            correct: evaluation.correct,
            grade: evaluation.grade,
            points: score.points,
            notes,
        })
    }
}

/// One answer to grade (see [`Profile::answer`]).
pub struct AnswerRequest<'a> {
    pub palace_id: &'a str,
    pub locus_id: &'a str,
    pub answer: &'a locora_api::journey::Answer,
    pub expected: &'a str,
    pub submission: Submission,
    pub elapsed_ms: u64,
}

/// The result of [`Profile::answer`].
#[derive(Debug, Clone, PartialEq)]
pub struct Answered {
    pub correct: bool,
    pub grade: Grade,
    pub points: u64,
    /// Plugin problems that came up.
    pub notes: Vec<String>,
}

/// Due and new (never answered) loci of a palace at `now`.
pub fn count_due(
    tree: &PalaceTree,
    cards: &HashMap<String, CardState>,
    now: u64,
) -> (usize, usize) {
    tree.route()
        .fold((0, 0), |(due, fresh), l| match cards.get(&l.id) {
            Some(card) if card.due <= now => (due + 1, fresh),
            Some(_) => (due, fresh),
            None => (due, fresh + 1),
        })
}

#[cfg(test)]
mod tile_tests {
    use super::*;

    #[test]
    fn tiles_are_kept_per_loaded_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile::open(Some(dir.path())).unwrap();
        assert!(profile.tiles().unwrap().is_empty());
        let tile = |text: &str| Tile {
            title: "Wiederholung".into(),
            text: text.into(),
        };
        profile.keep_tile("review", tile("3 fällig")).unwrap();
        profile.keep_tile("review", tile("2 fällig")).unwrap();
        profile.keep_tile("entfernt", tile("alt")).unwrap();
        let tiles = profile.tiles().unwrap();
        assert_eq!(tiles, [("review".to_owned(), tile("2 fällig"))]);
        drop(profile);
        let again = Profile::open(Some(dir.path())).unwrap();
        assert_eq!(again.tiles().unwrap().len(), 1, "kept across restarts");

        again.store.kv_set("host", TILES, b"kaputt").unwrap();
        assert!(again.tiles().unwrap().is_empty());
        assert!(again.keep_tile("review", tile("neu")).is_err());
        assert_eq!(
            again.store.kv_get("host", TILES).unwrap().unwrap(),
            b"kaputt",
            "not overwritten"
        );
    }
}

#[cfg(test)]
mod review_tile_tests {
    use super::*;
    use locora_store::{NewLocus, RoomKind};

    #[test]
    fn answering_updates_the_review_tile() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let p = profile.store.create_palace("P").unwrap();
        let r = profile
            .store
            .add_room(&p.id, "R", RoomKind::Blank, None)
            .unwrap();
        let locus = profile
            .store
            .add_locus(
                &r.id,
                NewLocus {
                    place: "Tor".into(),
                    item: "Sonne".into(),
                    x: 0.5,
                    y: 0.5,
                    time: None,
                    answer: Default::default(),
                    image: None,
                    note: String::new(),
                    image_size: 0.25,
                },
            )
            .unwrap();
        profile
            .answer(AnswerRequest {
                palace_id: &p.id,
                locus_id: &locus.id,
                answer: &locus.answer,
                expected: "",
                submission: Submission::Recall(Grade::Good),
                elapsed_ms: 1_000,
            })
            .unwrap();
        let tiles = profile.tiles().unwrap();
        let (plugin, tile) = &tiles[0];
        assert_eq!(plugin, "review");
        assert_eq!(tile.title, "Wiederholung");
        assert!(
            tile.text.starts_with("1 Locus · fällig ab "),
            "{}",
            tile.text
        );
    }
}
