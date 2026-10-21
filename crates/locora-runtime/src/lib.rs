//! What the Locora CLI and app share: opening a profile (store, plugin host,
//! built-in and approved installed plugins) and answering a locus (Core
//! evaluation, score hook, events for review and other plugins).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

use anyhow::{Context, Result, bail, ensure};
use locora_api::Service as _;
use locora_api::core::{Grade, JourneyScore, LearningDayAnswer, LocusAnswered, Score};
use locora_api::gamification::{
    Status as GamificationStatus, StatusRequest as GamificationStatusRequest,
    StatusResponse as GamificationStatusResponse,
};
use locora_api::journey::{Evaluate, EvaluateRequest, Scored, Submission};
use locora_api::notify::{Due, DueRequest, Message};
use locora_api::tutorial::{Tour, TourAction};
use locora_api::ui::{
    Block, HighlightRequest, MAX_BLOCKS, MAX_PAGES, Page, Remove, Tile, valid_id,
};
use serde::{Deserialize, Serialize};

/// Latest `ui.tile` per plugin, kept in the profile (reserved id `host`).
const TILES: &str = "tiles";
/// The pages and blocks each plugin declared (`ui.page`, `ui.block`, `ui.remove`).
const UI: &str = "ui";

/// What one plugin declared for the app's pages: its blocks and the pages it suggests.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginUi {
    pub pages: BTreeMap<String, Page>,
    pub blocks: BTreeMap<String, Block>,
}
use locora_api::review::{CardState, Schedule, ScheduleRequest};
use locora_host::{ApprovalReceipt, HostEvent, KvStore, Level, Manifest, PluginHost};
use locora_store::{PalaceTree, Store};

pub mod cloud;
pub mod cloud_http;
pub mod cloud_sync;
pub mod formula_guide;
pub mod journey;
mod palace_i18n;
pub mod publishers;
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
/// Which built-in palace an id belongs to, in any language: "tutorial" or "formulas".
pub fn built_in_kind(palace_id: &str) -> Option<&'static str> {
    let is = |namespaces: &[&str]| {
        namespaces
            .iter()
            .any(|namespace| locora_source::bundled_palace_id(namespace) == palace_id)
    };
    if is(&tutorial::NAMESPACES) {
        Some("tutorial")
    } else if is(&formula_guide::NAMESPACES) {
        Some("formulas")
    } else {
        None
    }
}

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
    /// The folder the profile lives in; scratch files go below it.
    pub data_dir: PathBuf,
    pub host: PluginHost,
    /// Problems noticed while opening (plugins that did not load).
    pub notes: Vec<String>,
    /// Latest tour highlight, consumed by the app command.
    pub highlight: Option<HighlightRequest>,
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
                    let approved: ApprovalReceipt = serde_json::from_slice(&approval).context(
                        "Ungültige oder veraltete Installationszustimmung; Plugin erneut installieren",
                    )?;
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
        let mut profile = Self {
            store,
            data_dir,
            host,
            notes,
            highlight: None,
        };
        let reported = profile.report();
        profile.notes.extend(reported);
        Ok(profile)
    }

    /// Seconds past local midnight now (daily notification plans fire at
    /// or after this; the offset comes from the recorded UTC offsets).
    fn day_seconds(&self) -> Result<u32> {
        let offset = self.day_clock()?.utc_offset_minutes();
        let now = now();
        let local = i128::from(now) + i128::from(offset) * 60;
        Ok(local.rem_euclid(86_400) as u32)
    }

    /// Whether the `notify` plugin is loaded (notifications and schedules).
    pub fn has_notify(&self) -> bool {
        self.host.provides(Due::NAME)
    }

    /// Everything that may be shown now: due schedules plus queued messages,
    /// already filtered and bundled by the plugin (quiet hours, muting, the
    /// daily per-channel limit). Empty without the `notify` plugin.
    pub fn notifications(&mut self) -> Result<Vec<Message>> {
        if !self.has_notify() {
            return Ok(Vec::new());
        }
        let shown_at = now();
        let queued = self.host.call::<Due>(&DueRequest {
            now: shown_at,
            day_seconds: self.day_seconds()?,
        })?;
        Ok(queued
            .into_iter()
            .map(|queued| Message {
                plugin: "notify".into(),
                channel: queued.channel,
                priority: queued.priority,
                title: queued.title,
                body: queued.body,
                action: queued.action,
                shown_at,
            })
            .collect())
    }

    /// Tell the plugin that `message` was shown, so the queue drops it and
    /// the history keeps it (quiet hours, daily limit, bundling all apply
    /// again on the next [`Profile::notifications`]).
    pub fn shown_notification(&mut self, message: &Message) -> Result<Vec<String>> {
        self.publish(&locora_api::notify::Displayed {
            message: message.clone(),
        })
    }

    /// Queue a message on a channel without showing it (for the CLI and
    /// tests; the app taps `notify.send@1` through the plugin service).
    pub fn send_notification(
        &mut self,
        channel: &str,
        title: &str,
        body: &str,
        action: &str,
    ) -> Result<()> {
        self.host
            .call::<locora_api::notify::Send>(&locora_api::notify::SendRequest {
                now: now(),
                channel: channel.to_owned(),
                priority: 0,
                title: title.to_owned(),
                body: body.to_owned(),
                action: action.to_owned(),
            })?;
        Ok(())
    }

    /// The plugin's history, newest first; empty without `notify`.
    /// `limit` 0 lets the plugin choose (its cap).
    pub fn notification_history(&mut self, limit: u32) -> Result<Vec<locora_api::notify::Message>> {
        if !self.has_notify() {
            return Ok(Vec::new());
        }
        self.host
            .call::<locora_api::notify::History>(&locora_api::notify::HistoryRequest { limit })
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
            if let HostEvent::Highlighted { step, .. } = event {
                self.highlight = Some(step.clone());
            }
            if let HostEvent::Published {
                plugin,
                topic,
                payload,
            } = event
                && [
                    <Page as locora_api::Event>::TOPIC,
                    <Block as locora_api::Event>::TOPIC,
                    <Remove as locora_api::Event>::TOPIC,
                ]
                .contains(&topic.as_str())
                && let Err(e) = self.keep_ui(plugin, topic, payload) {
                    notes.push(format!("[{plugin}] Seiten/Blöcke nicht übernommen: {e:#}"));
                }
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

    /// Advance the official tutorial tour and return its next declarative step.
    pub fn tour(&mut self, action: TourAction) -> Result<Option<HighlightRequest>> {
        if !self.host.plugin_provides("tutorial", Tour::NAME) {
            bail!("Die App-Tour ist in diesem Build nicht verfügbar");
        }
        self.highlight = None;
        self.host.call_plugin::<Tour>("tutorial", &action)?;
        let notes = self.report();
        self.notes.extend(notes);
        Ok(self.highlight.take())
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

    /// The pages and blocks of every loaded plugin, in plugin order. Unreadable saved
    /// declarations show as none (and are kept, see `keep_ui`).
    pub fn ui_declarations(&self) -> Result<Vec<(String, PluginUi)>> {
        let mut saved = self.saved_ui().unwrap_or_default();
        Ok(self
            .host
            .plugins()
            .filter_map(|m| saved.remove(&m.id).map(|ui| (m.id.clone(), ui)))
            .collect())
    }

    fn saved_ui(&self) -> Result<BTreeMap<String, PluginUi>> {
        Ok(match self.store.kv_get("host", UI)? {
            // An error, not an empty map: the next write would drop every declaration.
            Some(bytes) => {
                serde_json::from_slice(&bytes).context("Gespeicherte Seiten sind unlesbar")?
            }
            None => BTreeMap::new(),
        })
    }

    /// Apply one `ui.*` event of `plugin`. Ids, sizes and counts are checked here, so the app
    /// only ever sees declarations within the limits.
    fn keep_ui(&self, plugin: &str, topic: &str, payload: &[u8]) -> Result<()> {
        let mut saved = self.saved_ui()?;
        let ui = saved.entry(plugin.to_owned()).or_default();
        if topic == <Page as locora_api::Event>::TOPIC {
            let page: Page = locora_api::decode(payload).context("ungültige Seite")?;
            ensure!(valid_id(&page.id), "Seiten-Id `{}` ist ungültig", page.id);
            ensure!(
                ui.pages.contains_key(&page.id) || ui.pages.len() < MAX_PAGES,
                "höchstens {MAX_PAGES} Seiten"
            );
            ensure!(
                page.blocks.len() <= MAX_BLOCKS && page.blocks.iter().all(|id| valid_id(id)),
                "ungültige Blöcke in Seite `{}`",
                page.id
            );
            ui.pages.insert(page.id.clone(), page);
        } else if topic == <Block as locora_api::Event>::TOPIC {
            let block: Block = locora_api::decode(payload).context("ungültiger Block")?;
            ensure!(valid_id(&block.id), "Block-Id `{}` ist ungültig", block.id);
            ensure!(
                ui.blocks.contains_key(&block.id) || ui.blocks.len() < MAX_BLOCKS,
                "höchstens {MAX_BLOCKS} Blöcke"
            );
            ensure!(
                block.title.chars().count() <= 60 && block.text.chars().count() <= 240,
                "Block `{}` ist zu lang (Titel 60, Text 240 Zeichen)",
                block.id
            );
            ui.blocks.insert(block.id.clone(), block);
        } else {
            let remove: Remove = locora_api::decode(payload).context("ungültiges Entfernen")?;
            ui.pages.remove(&remove.id);
            ui.blocks.remove(&remove.id);
        }
        self.store
            .kv_set("host", UI, &serde_json::to_vec(&saved)?)?;
        Ok(())
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
        let answered_at = now();
        notes.extend(self.publish(&LocusAnswered {
            palace_id: request.palace_id.to_owned(),
            locus_id: request.locus_id.to_owned(),
            correct: evaluation.correct,
            grade: evaluation.grade,
            elapsed_ms: request.elapsed_ms,
            answered_at,
        })?);
        if self
            .host
            .plugin_provides("gamification", GamificationStatus::NAME)
        {
            let day_start = self.day_clock()?.day_start(answered_at);
            notes.extend(self.publish(&LearningDayAnswer {
                palace_id: request.palace_id.to_owned(),
                correct: evaluation.correct,
                answered_at,
                day_start,
            })?);
        }
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

    /// Local game progress, using the same learning-day boundary as review.
    pub fn gamification_status(&mut self) -> Result<Option<GamificationStatusResponse>> {
        if !self
            .host
            .plugin_provides("gamification", GamificationStatus::NAME)
        {
            return Ok(None);
        }
        let day_start = self.day_clock()?.day_start(now());
        let streak = self
            .stats()?
            .map_or(0, |stats| stats.streak.min(u32::MAX as usize) as u32);
        let status = self.host.call_plugin::<GamificationStatus>(
            "gamification",
            &GamificationStatusRequest { day_start, streak },
        )?;
        let reported = self.report();
        self.notes.extend(reported);
        Ok(Some(status))
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
        let review_tile = |tiles: &[(String, Tile)]| {
            tiles
                .iter()
                .find(|(plugin, _)| plugin == "review")
                .map(|(_, tile)| tile.clone())
        };
        assert!(review_tile(&profile.tiles().unwrap()).is_none());
        let tile = |text: &str| Tile {
            title: "Wiederholung".into(),
            text: text.into(),
        };
        profile.keep_tile("review", tile("3 fällig")).unwrap();
        profile.keep_tile("review", tile("2 fällig")).unwrap();
        profile.keep_tile("entfernt", tile("alt")).unwrap();
        let tiles = profile.tiles().unwrap();
        assert_eq!(review_tile(&tiles), Some(tile("2 fällig")));
        assert_eq!(tiles.iter().filter(|(p, _)| p == "entfernt").count(), 0);
        drop(profile);
        let again = Profile::open(Some(dir.path())).unwrap();
        assert_eq!(
            review_tile(&again.tiles().unwrap()),
            Some(tile("2 fällig")),
            "kept across restarts"
        );

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
mod notify_tests {
    use super::*;
    use locora_api::notify::{Plan, ScheduleRequest};

    #[test]
    fn messages_flow_send_due_shown_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        assert!(profile.has_notify(), "notify is built in");

        // Nothing scheduled, nothing queued.
        assert!(profile.notifications().unwrap().is_empty());

        // Send two messages on one channel and one on another.
        profile
            .send_notification("test.hello", "Titel", "Erste", "")
            .unwrap();
        profile
            .send_notification("test.hello", "Titel", "Zweite", "")
            .unwrap();
        profile
            .send_notification("test.other", "Andere", "Dritte", "review.start@P")
            .unwrap();
        let due = profile.notifications().unwrap();
        assert_eq!(due.len(), 2, "bundled per channel: {due:?}");
        assert_eq!(due[0].channel, "test.hello");
        assert_eq!(due[0].body, "Erste · Zweite", "bodies are bundled");
        assert_eq!(due[0].plugin, "notify");
        assert_eq!(due[1].channel, "test.other");
        assert_eq!(due[1].action, "review.start@P");

        // Both again until they are shown …
        assert_eq!(profile.notifications().unwrap().len(), 2);
        for message in &due {
            profile.shown_notification(message).unwrap();
        }
        // … then the queue is empty and the history keeps them.
        assert!(profile.notifications().unwrap().is_empty());
        let history = profile.notification_history(0).unwrap();
        let notes = profile.report();
        assert!(notes.is_empty(), "plugin problems: {notes:?}");
        assert_eq!(history.len(), 2);
        assert!(history.iter().any(|m| m.body == "Erste · Zweite"));
        assert!(history.iter().any(|m| m.action == "review.start@P"));
        assert!(history.iter().all(|m| m.plugin == "notify"));
    }

    #[test]
    fn sends_use_host_time_and_duplicate_acknowledgments_do_not_use_daily_quota() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let before = now();
        profile
            .send_notification("test", "Title", "First", "")
            .unwrap();
        let queued: Vec<locora_api::notify::Queued> = profile
            .host
            .call::<Due>(&DueRequest {
                now: now(),
                day_seconds: 0,
            })
            .unwrap();
        assert_eq!(queued.len(), 1);
        assert!((before..=now()).contains(&queued[0].queued_at));
        let message = profile.notifications().unwrap().remove(0);
        for _ in 0..10 {
            profile.shown_notification(&message).unwrap();
        }
        assert_eq!(profile.notification_history(0).unwrap().len(), 1);
        profile
            .send_notification("test", "Title", "Second", "")
            .unwrap();
        let due = profile.notifications().unwrap();
        assert_eq!(
            due.len(),
            1,
            "duplicate acknowledgments must not exhaust quota"
        );
        profile.shown_notification(&due[0]).unwrap();
        assert_eq!(profile.notification_history(1).unwrap()[0].body, "Second");
    }

    #[test]
    fn daily_plans_fire_and_burn_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        profile
            .host
            .call::<locora_api::notify::Schedule>(&ScheduleRequest {
                key: "evening".into(),
                channel: "test.daily".into(),
                priority: 1,
                title: "Abendlernen".into(),
                body: "5 Loci warten".into(),
                action: String::new(),
                plan: Some(Plan::Daily { seconds: 19 * 3600 }),
            })
            .unwrap();

        // Drive the clock explicitly: wall-clock time can be past 19:00.
        let due: Vec<locora_api::notify::Queued> = profile
            .host
            .call::<locora_api::notify::Due>(&DueRequest {
                now: 0,
                day_seconds: 0,
            })
            .unwrap();
        assert!(due.is_empty(), "{due:?}");
        // The runtime does not fake time, so this test drives the service
        // directly with a late `day_seconds`.
        let queued = profile
            .host
            .call::<locora_api::notify::Due>(&DueRequest {
                now: 1,
                day_seconds: 20 * 3600,
            })
            .unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].title, "Abendlernen");
        // A second call at a later time of day fires again (the plan is
        // daily), but showing drops the message.
        let message = Message {
            plugin: "notify".into(),
            channel: queued[0].channel.clone(),
            priority: queued[0].priority,
            title: queued[0].title.clone(),
            body: queued[0].body.clone(),
            action: queued[0].action.clone(),
            shown_at: 2,
        };
        profile.shown_notification(&message).unwrap();
        assert!(
            profile
                .host
                .call::<locora_api::notify::Due>(&DueRequest {
                    now: 3,
                    day_seconds: 20 * 3600
                })
                .unwrap()
                .is_empty(),
            "shown messages are gone until the next day"
        );
    }

    #[test]
    fn quiet_hours_hold_messages() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        profile
            .send_notification("test.night", "Nachts", "Spät", "")
            .unwrap();
        // Quiet hours 22:00–08:00.
        profile
            .host
            .call::<locora_api::notify::Configure>(&locora_api::notify::ConfigureRequest {
                quiet: Some("22:00-08:00".into()),
                ..Default::default()
            })
            .unwrap();
        let due = profile
            .host
            .call::<locora_api::notify::Due>(&DueRequest {
                now: 1,
                day_seconds: 23 * 3600,
            })
            .unwrap();
        assert!(due.is_empty(), "quiet: {due:?}");
        let due = profile
            .host
            .call::<locora_api::notify::Due>(&DueRequest {
                now: 2,
                day_seconds: 9 * 3600,
            })
            .unwrap();
        assert_eq!(due.len(), 1, "after quiet hours");
        // Muting drops the channel entirely.
        profile
            .host
            .call::<locora_api::notify::Configure>(&locora_api::notify::ConfigureRequest {
                muted: Some(vec!["test.night".into()]),
                ..Default::default()
            })
            .unwrap();
        let due = profile
            .host
            .call::<locora_api::notify::Due>(&DueRequest {
                now: 3,
                day_seconds: 9 * 3600,
            })
            .unwrap();
        assert!(due.is_empty(), "muted: {due:?}");
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
                    zoom: locora_store::default_zoom(),
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
        let (plugin, tile) = tiles
            .iter()
            .find(|(plugin, _)| plugin == "review")
            .expect("the review plugin published its tile");
        assert_eq!(tile.title, "Wiederholung");
        // Tile texts are templates the app translates: `@@{0} …` plus its arguments.
        assert!(
            tile.text.starts_with("@@{0} Locus · fällig ab {1}"),
            "{}",
            tile.text
        );
        let _ = plugin;
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;

    fn block(id: &str, text: &str) -> Block {
        Block {
            id: id.into(),
            title: "Karte".into(),
            text: text.into(),
        }
    }

    fn keep_block(profile: &Profile, plugin: &str, block: &Block) -> Result<()> {
        profile.keep_ui(
            plugin,
            <Block as locora_api::Event>::TOPIC,
            &locora_api::encode(block)?,
        )
    }

    #[test]
    fn declared_blocks_and_pages_are_kept_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile::open(Some(dir.path())).unwrap();
        let mine = |profile: &Profile| {
            profile
                .ui_declarations()
                .unwrap()
                .into_iter()
                .find(|(plugin, _)| plugin == "review")
                .map(|(_, ui)| ui)
                .unwrap_or_default()
        };
        assert!(mine(&profile).blocks.is_empty());

        keep_block(&profile, "review", &block("streak", "3")).unwrap();
        keep_block(&profile, "review", &block("streak", "4")).unwrap();
        let page = Page {
            id: "tag".into(),
            name: "Heute".into(),
            blocks: vec!["streak".into()],
        };
        profile
            .keep_ui(
                "review",
                <Page as locora_api::Event>::TOPIC,
                &locora_api::encode(&page).unwrap(),
            )
            .unwrap();
        assert_eq!(mine(&profile).blocks["streak"].text, "4", "declared again");
        assert_eq!(mine(&profile).pages["tag"], page);

        assert!(keep_block(&profile, "review", &block("Mit Leerzeichen", "x")).is_err());
        assert!(keep_block(&profile, "review", &block("lang", &"x".repeat(241))).is_err());
        for i in 0..MAX_BLOCKS - 1 {
            keep_block(&profile, "review", &block(&format!("b{i}"), "x")).unwrap();
        }
        assert!(
            keep_block(&profile, "review", &block("zu-viel", "x")).is_err(),
            "at most {MAX_BLOCKS} blocks"
        );

        profile
            .keep_ui(
                "review",
                <Remove as locora_api::Event>::TOPIC,
                &locora_api::encode(&Remove {
                    id: "streak".into(),
                })
                .unwrap(),
            )
            .unwrap();
        assert!(!mine(&profile).blocks.contains_key("streak"));

        drop(profile);
        let again = Profile::open(Some(dir.path())).unwrap();
        assert_eq!(
            mine(&again).blocks.len(),
            MAX_BLOCKS - 1,
            "kept across restarts"
        );
        assert_eq!(mine(&again).pages["tag"], page);
    }
}
