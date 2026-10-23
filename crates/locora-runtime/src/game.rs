//! The game progress around the gamification plugin: the epoch that deleting the progress starts,
//! the one-time takeover of the progress from before the sync, the app's own medals and what the
//! runtime tells the plugin (this device, the learning day, what is due, how strong each palace is).
//!
//! The epoch is one key in the runtime's namespace (`host/game-epoch`), synced like the tiles: the
//! last reset wins on every device, and facts of an older epoch never count again, even when a
//! device that was offline sends them later.

use std::collections::{BTreeSet, HashMap};

use anyhow::{Context, Result};
use locora_api::Service as _;
use locora_api::core::Timing;
use locora_api::gamification::{
    Migrate, MigrateRequest, PalaceStrength, Reset, ResetRequest, Seen, SeenRequest, StatusV2,
    StatusV2Request, StatusV2Response, WeekGoal, WeekGoalRequest,
};
use locora_api::review::{CardState, Schedule};
use locora_store::{PalaceTree, Store};
use serde::{Deserialize, Serialize};

use crate::settings::DAY;
use crate::{Profile, now};

/// The epoch, in the runtime's namespace `host`.
pub const EPOCH_KEY: &str = "game-epoch";
/// This device took over its progress from before the sync (local, never synced).
const MIGRATED: &str = "game.migrated.v1";
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

/// An app medal as stored: the epoch it was earned in and the learning day. Older versions wrote
/// `1` (epoch `0`, no day); empty bytes are a medal that was taken back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AppMedal {
    e: String,
    #[serde(default)]
    day: Option<String>,
}

/// Whether the app medal `id` counts in `epoch`, and since which day (`None` when unknown).
pub fn app_medal(store: &Store, id: &str, epoch: &str) -> Result<Option<Option<String>>> {
    Ok(match store.kv_get(APP_MEDALS, id)? {
        None => None,
        Some(bytes) if bytes.is_empty() => None,
        Some(bytes) if bytes == b"1" => (epoch == "0").then_some(None),
        Some(bytes) => match serde_json::from_slice::<AppMedal>(&bytes) {
            Ok(medal) if medal.e == epoch => Some(medal.day),
            _ => None,
        },
    })
}

/// Unlocks an app medal in `epoch`; whether it was new.
pub fn unlock_app_medal(store: &Store, id: &str, epoch: &str, day: &str) -> Result<bool> {
    if app_medal(store, id, epoch)?.is_some() {
        return Ok(false);
    }
    let medal = AppMedal {
        e: epoch.to_owned(),
        day: Some(day.to_owned()),
    };
    store.kv_set(APP_MEDALS, id, &serde_json::to_vec(&medal)?)?;
    Ok(true)
}

/// Empties every app medal (a reset); how many there were.
pub fn blank_app_medals(store: &Store) -> Result<usize> {
    let mut blanked = 0;
    let mut after = 0;
    loop {
        let page = store.kv_list(APP_MEDALS, "", after, 1000)?;
        for (key, value) in &page.entries {
            if !value.is_empty() {
                store.kv_set(APP_MEDALS, key, &[])?;
                blanked += 1;
            }
        }
        if !page.more {
            return Ok(blanked);
        }
        after = page.rev;
    }
}

/// How strong a palace is: of its loci, how many hold for 7, 30 and 180 days (FSRS stability).
/// Cards of loci that are no longer in the palace are left out.
pub fn strength(tree: &PalaceTree, cards: &HashMap<String, CardState>) -> PalaceStrength {
    let mut strength = PalaceStrength {
        palace_id: tree.palace.id.clone(),
        loci: 0,
        hold_7: 0,
        hold_30: 0,
        hold_180: 0,
    };
    for locus in tree.route() {
        strength.loci += 1;
        let stability = cards.get(&locus.id).map_or(0.0, |card| card.stability);
        strength.hold_7 += u32::from(stability >= 7.0);
        strength.hold_30 += u32::from(stability >= 30.0);
        strength.hold_180 += u32::from(stability >= 180.0);
    }
    strength
}

/// When a locus was answered compared with its review: no card yet is new, a card due by then is
/// due, any other card is early.
pub fn timing(card: Option<&CardState>, at: u64) -> Timing {
    match card {
        None => Timing::New,
        Some(card) if card.due <= at => Timing::Due,
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
        self.host.call_plugin::<Migrate>(
            PLUGIN,
            &MigrateRequest {
                device: self.store.node_id().to_owned(),
                days: days.into_iter().collect(),
            },
        )?;
        self.store.set_local_value(MIGRATED, "1")?;
        let reported = self.report();
        self.notes.extend(reported);
        Ok(())
    }

    /// The strength of every palace from its review cards.
    pub fn palace_strengths(
        &self,
        cards: &HashMap<String, CardState>,
    ) -> Result<Vec<PalaceStrength>> {
        let mut out = Vec::new();
        for summary in self.store.palaces()? {
            out.push(strength(&self.store.palace(&summary.id)?, cards));
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
        let now = now();
        let (day_start, day) = self.learning_day()?;
        let cards = if self.host.provides(Schedule::NAME) {
            self.schedule(None)?
        } else {
            HashMap::new()
        };
        // Due now and not reviewed yet this learning day.
        let due_open = cards
            .values()
            .filter(|card| card.due <= now && card.last_review < day_start)
            .count() as u32;
        let palaces = if full {
            Some(self.palace_strengths(&cards)?)
        } else {
            None
        };
        let request = StatusV2Request {
            day,
            day_start,
            epoch: self.game_epoch()?.id,
            device: self.store.node_id().to_owned(),
            due_open,
            has_cards: !cards.is_empty(),
            palaces,
        };
        let status = self.host.call_plugin::<StatusV2>(PLUGIN, &request)?;
        let reported = self.report();
        self.notes.extend(reported);
        Ok(Some(status))
    }

    /// Deletes the game progress on this device and, through the sync, on every other one: a new
    /// epoch, the plugin empties the facts of the old ones, and the app medals are taken back.
    /// Review cards and log, palaces and settings stay. Returns the new epoch.
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
        }
        blank_app_medals(&self.store)?;
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
        let cards = self.review_cache.entry(palace.to_owned()).or_default();
        let found = timing(cards.get(locus), at);
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
        let card = |due| CardState {
            stability: 1.0,
            difficulty: 5.0,
            last_review: 0,
            due,
            reps: 1,
            lapses: 0,
        };
        assert_eq!(timing(None, 100), Timing::New);
        assert_eq!(timing(Some(&card(100)), 100), Timing::Due);
        assert_eq!(timing(Some(&card(101)), 100), Timing::Early);
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
        // A card of a locus that is gone counts nowhere.
        cards.insert("deleted".into(), card(500.0));
        let strength = strength(&store.palace(&palace.id).unwrap(), &cards);
        assert_eq!(
            (
                strength.loci,
                strength.hold_7,
                strength.hold_30,
                strength.hold_180
            ),
            (4, 2, 1, 0)
        );
    }

    #[test]
    fn the_plan_promises_no_reminders_that_frame_a_loss() {
        let plan = include_str!("../../../docs/PLAN.md");
        for loss in ["Streak in Gefahr", "Streak endet heute"] {
            assert!(!plan.contains(loss), "docs/PLAN.md: {loss}");
        }
    }

    #[test]
    fn app_medals_count_only_in_their_epoch() {
        let store = Store::in_memory().unwrap();
        store.kv_set(APP_MEDALS, "tour-done", b"1").unwrap();
        assert_eq!(app_medal(&store, "tour-done", "0").unwrap(), Some(None));
        assert_eq!(app_medal(&store, "tour-done", "1-a").unwrap(), None);
        assert!(unlock_app_medal(&store, "tour-done", "1-a", "2026-10-20").unwrap());
        assert!(!unlock_app_medal(&store, "tour-done", "1-a", "2026-10-21").unwrap());
        assert_eq!(
            app_medal(&store, "tour-done", "1-a").unwrap(),
            Some(Some("2026-10-20".into()))
        );
        assert_eq!(blank_app_medals(&store).unwrap(), 1);
        assert_eq!(app_medal(&store, "tour-done", "1-a").unwrap(), None);
        assert_eq!(blank_app_medals(&store).unwrap(), 0);
        // A late `1` from an older version does not count after a reset.
        store.kv_set(APP_MEDALS, "tour-done", b"1").unwrap();
        assert_eq!(app_medal(&store, "tour-done", "1-a").unwrap(), None);
    }
}
