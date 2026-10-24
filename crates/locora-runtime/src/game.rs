//! The game progress around the gamification plugin: the epoch that deleting the progress starts,
//! the one-time takeover of the progress from before the sync, the app's own medals and what the
//! runtime tells the plugin (this device, the learning day, what is due, how strong each palace is).
//!
//! The epoch is one key in the runtime's namespace (`host/game-epoch`), synced like the tiles: the
//! last reset wins on every device, and facts of an older epoch never count again, even when a
//! device that was offline sends them later.
//!
//! What is due works by learning day: a review counts as due on the whole learning day its due
//! time falls on ([`DueDay`]), so the time of day someone learns at changes nothing.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context, Result};
use locora_api::Service as _;
use locora_api::core::{Grade, Timing};
use locora_api::gamification::{
    Migrate, MigrateRequest, PalaceStrength, Reset, ResetRequest, Seen, SeenRequest, StatusV2,
    StatusV2Request, StatusV2Response, WeekGoal, WeekGoalRequest,
};
use locora_api::review::{CardState, ConfigureRequest, LogEntry, Schedule};
use locora_fsrs::Params;
use locora_store::{PalaceTree, Store};
use serde::{Deserialize, Serialize};

use crate::settings::{DAY, DayClock};
use crate::{Profile, now};

/// The epoch, in the runtime's namespace `host`.
pub const EPOCH_KEY: &str = "game-epoch";
/// This device took over its progress from before the sync (local, never synced).
const MIGRATED: &str = "game.migrated.v1";
/// Progress this device kept for itself before the sync and never synced (the web app's, from
/// the browser's storage), hex-encoded postcard; local, read once by the migration.
pub const OWN_PROGRESS: &str = "game.own-progress.v1";
/// The app's own medals: lessons, built-in palaces, the tour, the first own palace.
pub const APP_MEDALS: &str = "achievements";
/// The plugin that keeps the game.
const PLUGIN: &str = "gamification";

/// Which progress counts: a reset starts a new epoch. `0` is the time before any reset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameEpoch {
    pub n: u32,
    pub id: String,
    pub at: u64,
}

impl Default for GameEpoch {
    fn default() -> Self {
        Self {
            n: 0,
            id: "0".into(),
            at: 0,
        }
    }
}

/// The current epoch (the first one when there was no reset).
pub fn epoch(store: &Store) -> Result<GameEpoch> {
    match store.kv_get("host", EPOCH_KEY)? {
        Some(bytes) if !bytes.is_empty() => {
            serde_json::from_slice(&bytes).context("Der Stand des Spielfortschritts ist beschädigt")
        }
        _ => Ok(GameEpoch::default()),
    }
}

/// An app medal as a device stored it: the learning day it earned it on. The shared key `<id>`
/// holds `1` (what older versions write and read) or, from development builds, `{"e", "day"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AppMedal {
    #[serde(default)]
    day: Option<String>,
}

/// The app's own medals (lessons, built-in palaces, the tour, the first own palace): learning
/// progress, not part of the game, so deleting the game progress keeps them. Every device that
/// earns one writes `<id>/<device>` with its learning day, and `<id>` = `1` for older versions;
/// the earliest day of any device is the medal's day, so two devices never overwrite each other.
/// Answers id → the day (`None` when only older versions earned it).
pub fn app_medals(store: &Store) -> Result<BTreeMap<String, Option<String>>> {
    let mut medals: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut after = 0;
    loop {
        let page = store.kv_list(APP_MEDALS, "", after, 1000)?;
        for (key, value) in &page.entries {
            if value.is_empty() {
                continue;
            }
            let id = key.split_once('/').map_or(key.as_str(), |(id, _)| id);
            let day = serde_json::from_slice::<AppMedal>(value)
                .ok()
                .and_then(|medal| medal.day);
            let slot = medals.entry(id.to_owned()).or_insert(None);
            if let Some(day) = day
                && slot.as_ref().is_none_or(|earliest| day < *earliest)
            {
                *slot = Some(day);
            }
        }
        if !page.more {
            return Ok(medals);
        }
        after = page.rev;
    }
}

/// Unlocks an app medal on this device on the learning day `day`; whether it was new.
pub fn unlock_app_medal(store: &Store, id: &str, day: &str) -> Result<bool> {
    anyhow::ensure!(!id.contains('/'), "Ungültiger Erfolg: {id}");
    if app_medals(store)?.contains_key(id) {
        return Ok(false);
    }
    let medal = AppMedal {
        day: Some(day.to_owned()),
    };
    let own = format!("{id}/{}", store.node_id());
    store.kv_set(APP_MEDALS, &own, &serde_json::to_vec(&medal)?)?;
    store.kv_set(APP_MEDALS, id, b"1")?;
    Ok(true)
}

/// How long each locus has shown that it holds, from the review log: its answers replayed with
/// FSRS, only the first answer of each learning day (answers in one sitting build nothing), and
/// only for loci answered right (Hard or better) on a later learning day than their first answer.
/// A first sight, however easy, shows nothing yet. Locus id → stability in days.
pub fn spaced_stability(
    log: &[LogEntry],
    clock: &DayClock,
    params: &Params,
) -> HashMap<String, f64> {
    let mut answers: HashMap<&str, Vec<(u64, Grade)>> = HashMap::new();
    for entry in log {
        answers
            .entry(entry.locus_id.as_str())
            .or_default()
            .push((entry.answered_at, entry.grade));
    }
    let mut shown = HashMap::new();
    for (locus, mut answers) in answers {
        answers.sort_by_key(|(at, _)| *at);
        let mut card: Option<CardState> = None;
        let mut last_day = None;
        let mut proven = false;
        for (at, grade) in answers {
            let day = clock.day_start(at);
            if last_day == Some(day) {
                continue;
            }
            proven |= last_day.is_some() && grade != Grade::Again;
            card = Some(params.review(card.as_ref(), grade, at));
            last_day = Some(day);
        }
        if let (true, Some(card)) = (proven, card) {
            shown.insert(locus.to_owned(), card.stability);
        }
    }
    shown
}

/// How strong a palace is: of its loci, how many hold for 7, 30 and 180 days. A locus counts
/// with the stability its spaced reviews have shown (see [`spaced_stability`]) and no more than
/// its card has now; without such reviews it holds for no day yet. Cards of loci that are no
/// longer in the palace are left out.
pub fn strength(
    tree: &PalaceTree,
    cards: &HashMap<String, CardState>,
    shown: &HashMap<String, f64>,
) -> PalaceStrength {
    let mut strength = PalaceStrength {
        palace_id: tree.palace.id.clone(),
        loci: 0,
        hold_7: 0,
        hold_30: 0,
        hold_180: 0,
    };
    for locus in tree.route() {
        strength.loci += 1;
        let stability = match (cards.get(&locus.id), shown.get(&locus.id)) {
            (Some(card), Some(shown)) => card.stability.min(*shown),
            _ => 0.0,
        };
        strength.hold_7 += u32::from(stability >= 7.0);
        strength.hold_30 += u32::from(stability >= 30.0);
        strength.hold_180 += u32::from(stability >= 180.0);
    }
    strength
}

/// A learning day, to tell what is due on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DueDay {
    pub now: u64,
    /// Start of this learning day and of the next one.
    pub start: u64,
    pub end: u64,
}

impl DueDay {
    pub fn at(clock: &DayClock, now: u64) -> Self {
        let start = clock.day_start(now);
        Self {
            now,
            start,
            end: clock.next_day(start),
        }
    }

    /// Answered earlier on this learning day (it counted then).
    pub fn answered_today(&self, card: &CardState) -> bool {
        card.last_review >= self.start
    }

    /// A review due on this learning day, not answered yet today: part of the day's goal. It is
    /// due all day, whatever time of day its due time is.
    pub fn due(&self, card: &CardState) -> bool {
        !self.answered_today(card) && card.due < self.end
    }

    /// A locus answered today that is back for practice now (after „Again“).
    pub fn repeat(&self, card: &CardState) -> bool {
        self.answered_today(card) && card.due <= self.now
    }

    /// What a journey of due loci asks now: the day's due reviews and the loci back for practice.
    pub fn asks(&self, card: &CardState) -> bool {
        self.due(card) || self.repeat(card)
    }

    /// The learning day (its start) a card's next review counts on: a review due before today,
    /// today; a locus answered today, tomorrow at the earliest.
    fn review_day(&self, clock: &DayClock, card: &CardState) -> u64 {
        if self.answered_today(card) {
            clock.day_start(card.due).max(self.end)
        } else {
            clock.day_start(card.due).max(self.start)
        }
    }
}

/// When a locus was answered compared with its review: no card yet is new, a locus answered
/// before on the same learning day is a repeat, a review due on this learning day is due, any
/// other card is early.
pub fn timing(card: Option<&CardState>, day: &DueDay) -> Timing {
    match card {
        None => Timing::New,
        Some(card) if day.answered_today(card) => Timing::Repeat,
        Some(card) if day.due(card) => Timing::Due,
        Some(_) => Timing::Early,
    }
}

/// The ISO week (`YYYY-Www`) of a date `YYYY-MM-DD`.
pub fn iso_week(date: &str) -> Option<String> {
    let mut parts = date.splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    let days = days_from_civil(year, month, day)?;
    // 1970-01-01 was a Thursday; ISO weeks start on Monday and belong to their Thursday's year.
    let thursday = days - (days + 3).rem_euclid(7) + 3;
    let (iso_year, _, _) = crate::settings::civil_date(thursday);
    let first = days_from_civil(iso_year, 1, 1)?;
    Some(format!("{iso_year:04}-W{:02}", (thursday - first) / 7 + 1))
}

fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (month, day) = (i64::from(month), i64::from(day));
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    Some(era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468)
}

impl Profile {
    /// Whether the gamification plugin runs.
    pub fn has_game(&self) -> bool {
        self.host.plugin_provides(PLUGIN, StatusV2::NAME)
    }

    pub fn game_epoch(&self) -> Result<GameEpoch> {
        epoch(&self.store)
    }

    /// The learning day now: when it began, and its local date.
    pub fn learning_day(&self) -> Result<(u64, String)> {
        let clock = self.day_clock()?;
        let start = clock.day_start(now());
        Ok((start, clock.date(start)))
    }

    /// The learning day now, to tell what is due.
    pub fn due_day(&self) -> Result<DueDay> {
        Ok(DueDay::at(&self.day_clock()?, now()))
    }

    /// The FSRS settings the review plugin schedules with (the defaults without it).
    fn review_params(&mut self) -> Params {
        let mut params = Params::default();
        if let Ok(settings) = self.configure(ConfigureRequest::default()) {
            params.desired_retention = settings.desired_retention;
            if let Some(weights) = settings
                .weights
                .filter(|w| locora_fsrs::optimize::valid_weights(w))
            {
                params.weights.copy_from_slice(&weights);
            }
        }
        params
    }

    /// How long each locus has shown that it holds (see [`spaced_stability`]).
    pub fn spaced_stability(&mut self) -> Result<HashMap<String, f64>> {
        if !self.has_review() {
            return Ok(HashMap::new());
        }
        let log = self.review_log()?;
        let params = self.review_params();
        Ok(spaced_stability(&log, &self.day_clock()?, &params))
    }

    /// Takes over this device's progress from before the sync, once: the old progress blob and
    /// the learning days of the review log become a fact of this device (`legacy/<device>`).
    pub fn ensure_game_migrated(&mut self) -> Result<()> {
        if self.store.local_value(MIGRATED)?.is_some()
            || !self.host.plugin_provides(PLUGIN, Migrate::NAME)
        {
            return Ok(());
        }
        let clock = self.day_clock()?;
        let days: BTreeSet<String> = self
            .review_log()?
            .iter()
            .map(|entry| clock.date(clock.day_start(entry.answered_at)))
            .collect();
        let own = self
            .store
            .local_value(OWN_PROGRESS)?
            .and_then(|hex| decode_hex(&hex));
        self.host.call_plugin::<Migrate>(
            PLUGIN,
            &MigrateRequest {
                device: self.store.node_id().to_owned(),
                days: days.into_iter().collect(),
                own,
            },
        )?;
        self.store.set_local_value(MIGRATED, "1")?;
        let reported = self.report();
        self.notes.extend(reported);
        Ok(())
    }

    /// Keeps the game progress this device had for itself before the sync (postcard bytes of the
    /// old progress blob), for the migration to add on top of the shared progress. Only before
    /// the migration ran; whether it was kept.
    pub fn keep_own_game_progress(&self, bytes: &[u8]) -> Result<bool> {
        if bytes.is_empty() || self.store.local_value(MIGRATED)?.is_some() {
            return Ok(false);
        }
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        self.store.set_local_value(OWN_PROGRESS, &hex)?;
        Ok(true)
    }

    /// The strength of every palace from its review cards and the spaced reviews of its loci.
    pub fn palace_strengths(
        &mut self,
        cards: &HashMap<String, CardState>,
    ) -> Result<Vec<PalaceStrength>> {
        let shown = self.spaced_stability()?;
        let mut out = Vec::new();
        for summary in self.store.palaces()? {
            out.push(strength(&self.store.palace(&summary.id)?, cards, &shown));
        }
        Ok(out)
    }

    /// The game progress, derived from the facts of every device. `full` adds the palace medals
    /// (their strength is worked out from every palace). `None` without the plugin.
    pub fn gamification_status(&mut self, full: bool) -> Result<Option<StatusV2Response>> {
        if !self.has_game() {
            return Ok(None);
        }
        self.ensure_game_migrated()?;
        let clock = self.day_clock()?;
        let today = DueDay::at(&clock, now());
        let day = clock.date(today.start);
        let cards = if self.host.provides(Schedule::NAME) {
            self.schedule(None)?
        } else {
            HashMap::new()
        };
        let due_open = cards.values().filter(|card| today.due(card)).count() as u32;
        let repeat_open = cards.values().filter(|card| today.repeat(card)).count() as u32;
        // The next learning day with a due review, as the schedule stands.
        let next_due = cards
            .values()
            .map(|card| today.review_day(&clock, card))
            .min()
            .map(|start| clock.date(start));
        let palaces = if full {
            Some(self.palace_strengths(&cards)?)
        } else {
            None
        };
        let epoch = self.game_epoch()?;
        let epoch_day = (epoch.at > 0).then(|| clock.date(clock.day_start(epoch.at)));
        let request = StatusV2Request {
            day,
            day_start: today.start,
            epoch: epoch.id,
            device: self.store.node_id().to_owned(),
            due_open,
            repeat_open,
            has_cards: !cards.is_empty(),
            palaces,
            at: today.now,
            next_due,
            epoch_day,
        };
        let epoch_id = request.epoch.clone();
        let status = self.host.call_plugin::<StatusV2>(PLUGIN, &request)?;
        self.game_epoch_told = Some(epoch_id);
        let reported = self.report();
        self.notes.extend(reported);
        Ok(Some(status))
    }

    /// Deletes the game progress on this device and, through the sync, on every other one: a new
    /// epoch, and the plugin empties the facts of the old ones. The app's own medals (lessons,
    /// tour, tutorial: learning progress), review cards and log, palaces and settings stay.
    /// Returns the new epoch.
    pub fn reset_game(&mut self) -> Result<GameEpoch> {
        let current = self.game_epoch().unwrap_or_default();
        let device = self.store.node_id().to_owned();
        let n = current.n.saturating_add(1);
        let next = GameEpoch {
            n,
            id: format!("{n}-{device}"),
            at: now(),
        };
        self.store
            .kv_set("host", EPOCH_KEY, &serde_json::to_vec(&next)?)?;
        if self.host.plugin_provides(PLUGIN, Reset::NAME) {
            self.host.call_plugin::<Reset>(
                PLUGIN,
                &ResetRequest {
                    epoch: next.id.clone(),
                    device,
                },
            )?;
            self.game_epoch_told = Some(next.id.clone());
        }
        // Nothing from before the sync is left to take over.
        self.store.set_local_value(MIGRATED, "1")?;
        self.review_cache.clear();
        let reported = self.report();
        self.notes.extend(reported);
        Ok(next)
    }

    /// Learning days per week the learner aims for, from this week on.
    pub fn set_week_goal(&mut self, days: u8) -> Result<u8> {
        anyhow::ensure!((1..=7).contains(&days), "Wochenziel: 1 bis 7 Lerntage");
        let (_, day) = self.learning_day()?;
        let week = iso_week(&day).context("Kein gültiges Datum für das Wochenziel")?;
        let goal = self
            .host
            .call_plugin::<WeekGoal>(PLUGIN, &WeekGoalRequest { days, week })?;
        let reported = self.report();
        self.notes.extend(reported);
        Ok(goal)
    }

    /// A one-time hint was dismissed (on every device).
    pub fn game_seen(&mut self, hint: &str) -> Result<()> {
        self.host.call_plugin::<Seen>(
            PLUGIN,
            &SeenRequest {
                hint: hint.to_owned(),
            },
        )?;
        let reported = self.report();
        self.notes.extend(reported);
        Ok(())
    }

    /// How the answer of a locus at `at` relates to its review, from the cards as they were when
    /// the journey reached the palace (the review plugin moves a card on each answer).
    pub(crate) fn answer_timing(&mut self, palace: &str, locus: &str, at: u64) -> Timing {
        if !self.host.provides(Schedule::NAME) {
            // Without a schedule every answer is a review.
            return Timing::Due;
        }
        if !self.review_cache.contains_key(palace) {
            match self.schedule(Some(palace)) {
                Ok(cards) => {
                    self.review_cache.insert(palace.to_owned(), cards);
                }
                Err(_) => return Timing::Due,
            }
        }
        let day = match self.day_clock() {
            Ok(clock) => DueDay::at(&clock, at),
            Err(_) => return Timing::Due,
        };
        let cards = self.review_cache.entry(palace.to_owned()).or_default();
        let found = timing(cards.get(locus), &day);
        // Answered now: a second answer in the same journey is practice.
        cards.insert(
            locus.to_owned(),
            CardState {
                stability: 0.0,
                difficulty: 0.0,
                last_review: at,
                due: at.saturating_add(DAY),
                reps: 1,
                lapses: 0,
            },
        );
        found
    }
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
#[path = "game_tests.rs"]
mod game_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weeks_and_timings() {
        assert_eq!(iso_week("2026-10-25").as_deref(), Some("2026-W43"));
        assert_eq!(iso_week("2026-10-26").as_deref(), Some("2026-W44"));
        assert_eq!(iso_week("2021-01-03").as_deref(), Some("2020-W53"));
        assert_eq!(iso_week("2024-12-30").as_deref(), Some("2025-W01"));
        assert_eq!(iso_week("2026-13-01"), None);
        let card = |last_review, due| CardState {
            stability: 1.0,
            difficulty: 5.0,
            last_review,
            due,
            reps: 1,
            lapses: 0,
        };
        // 09:00 on a learning day that began at 04:00.
        let clock = DayClock::fixed(0, 4);
        let start = clock.day_start(1_792_400_000);
        let day = DueDay::at(&clock, start + 5 * 3_600);
        assert_eq!(day.end, start + DAY);
        assert_eq!(timing(None, &day), Timing::New);
        assert_eq!(timing(Some(&card(0, start - 60)), &day), Timing::Due);
        // Due at 20:00 today: due all day, not early in the morning.
        assert_eq!(
            timing(Some(&card(0, start + 16 * 3_600)), &day),
            Timing::Due
        );
        assert_eq!(timing(Some(&card(0, start + DAY)), &day), Timing::Early);
        // Answered earlier today: again is practice, also after „Again“.
        let relearn = card(start + 3_600, start + 3_600 + 600);
        assert_eq!(timing(Some(&relearn), &day), Timing::Repeat);
        assert!(!day.due(&relearn) && day.repeat(&relearn) && day.asks(&relearn));
        assert!(
            !day.repeat(&card(start + 3_600, start + 8 * 3_600)),
            "not yet"
        );
        assert_eq!(
            day.review_day(&clock, &card(start + 3_600, start + 2 * 3_600)),
            day.end,
            "a locus answered today is reviewed tomorrow at the earliest"
        );
        assert_eq!(day.review_day(&clock, &card(0, start - DAY)), start);
        assert_eq!(decode_hex("00ff10"), Some(vec![0, 255, 16]));
        assert_eq!(decode_hex("0"), None);
    }

    #[test]
    fn strength_counts_the_loci_of_the_palace_by_how_long_they_hold() {
        let store = Store::in_memory().unwrap();
        let palace = store.create_palace("Küche").unwrap();
        let room = store
            .add_room(&palace.id, "R", locora_store::RoomKind::Blank, None)
            .unwrap();
        let mut ids = Vec::new();
        for n in 0..4 {
            let locus = locora_store::NewLocus {
                audio: None,
                place: format!("Ort {n}"),
                item: "Milch".into(),
                x: 0.5,
                y: 0.5,
                time: None,
                answer: Default::default(),
                image: None,
                note: String::new(),
                image_size: 0.25,
                zoom: 1.0,
            };
            ids.push(store.add_locus(&room.id, locus).unwrap().id);
        }
        let card = |stability| CardState {
            stability,
            difficulty: 5.0,
            last_review: 0,
            due: 0,
            reps: 3,
            lapses: 0,
        };
        let mut cards = HashMap::new();
        cards.insert(ids[0].clone(), card(2.0));
        cards.insert(ids[1].clone(), card(7.0));
        cards.insert(ids[2].clone(), card(45.0));
        cards.insert(ids[3].clone(), card(400.0));
        // A card of a locus that is gone counts nowhere.
        cards.insert("deleted".into(), card(500.0));
        // What spaced reviews have shown: the last locus never had one.
        let shown: HashMap<String, f64> = ids[..3]
            .iter()
            .map(|id| (id.clone(), 1_000.0))
            .chain([("deleted".to_owned(), 1_000.0)])
            .collect();
        let tree = store.palace(&palace.id).unwrap();
        let strength = strength(&tree, &cards, &shown);
        assert_eq!(
            (
                strength.loci,
                strength.hold_7,
                strength.hold_30,
                strength.hold_180
            ),
            (4, 2, 1, 0)
        );
        // And never more than they showed.
        let little: HashMap<String, f64> = ids.iter().map(|id| (id.clone(), 8.0)).collect();
        let capped = super::strength(&tree, &cards, &little);
        assert_eq!((capped.hold_7, capped.hold_30), (3, 0));
    }

    fn logged(locus: &str, grade: Grade, answered_at: u64) -> LogEntry {
        LogEntry {
            palace_id: "p".into(),
            locus_id: locus.into(),
            grade,
            answered_at,
            elapsed_ms: 1_000,
        }
    }

    #[test]
    fn only_right_answers_on_later_days_show_how_long_a_locus_holds() {
        let clock = DayClock::fixed(0, 4);
        let params = Params::default();
        let t0 = clock.day_start(1_792_400_000) + 5 * 3_600;
        // „Easy“ at first sight and ten more times that evening: nothing shown yet.
        let mut log: Vec<LogEntry> = (0..11)
            .map(|n| logged("cram", Grade::Easy, t0 + n * 300))
            .collect();
        assert!(spaced_stability(&log, &clock, &params).is_empty());
        // Right again on the next learning day: shown, as if only the first answer of each day
        // had been given.
        log.push(logged("cram", Grade::Easy, t0 + DAY));
        let shown = spaced_stability(&log, &clock, &params)["cram"];
        let first = params.review(None, Grade::Easy, t0);
        let spaced = params.review(Some(&first), Grade::Easy, t0 + DAY).stability;
        assert!((shown - spaced).abs() < 1e-9, "{shown} vs {spaced}");
        // Forgotten on the later day: not shown.
        let lapse = [
            logged("lapse", Grade::Good, t0),
            logged("lapse", Grade::Again, t0 + 3 * DAY),
        ];
        assert!(spaced_stability(&lapse, &clock, &params).is_empty());
    }

    #[test]
    fn app_medals_keep_the_earliest_day_of_any_device_and_older_ones_count() {
        let store = Store::in_memory().unwrap();
        store.kv_set(APP_MEDALS, "learn-loci", b"1").unwrap();
        assert_eq!(
            app_medals(&store).unwrap().get("learn-loci"),
            Some(&None),
            "from an older version"
        );
        assert!(unlock_app_medal(&store, "tour-done", "2026-10-21").unwrap());
        assert!(!unlock_app_medal(&store, "tour-done", "2026-10-22").unwrap());
        assert_eq!(
            store.kv_get(APP_MEDALS, "tour-done").unwrap().unwrap(),
            b"1"
        );
        // Another device earned it a day earlier, offline: its own key, the earlier day wins.
        store
            .kv_set(APP_MEDALS, "tour-done/other", br#"{"day":"2026-10-20"}"#)
            .unwrap();
        assert_eq!(
            app_medals(&store).unwrap()["tour-done"].as_deref(),
            Some("2026-10-20")
        );
        assert!(unlock_app_medal(&store, "a/b", "2026-10-20").is_err());
    }

    #[test]
    fn the_plan_promises_no_reminders_that_frame_a_loss() {
        let plan = include_str!("../../../docs/PLAN.md");
        for loss in ["Streak in Gefahr", "Streak endet heute"] {
            assert!(!plan.contains(loss), "docs/PLAN.md: {loss}");
        }
    }
}
