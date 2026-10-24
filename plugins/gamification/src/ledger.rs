//! The game's stored facts and everything that follows from them. Pure: no storage, no clock.
//!
//! Every device writes only facts of its own, each under a key of its own, and only adds to them
//! (see `docs/PLAN.md`, section 3). The sync merges keys last-writer-wins, so the newest write of
//! a key is a superset of every earlier one and nothing is lost. Totals, learning days, weeks,
//! trophies and their dates are worked out here from the merged facts; no counter is stored.
//!
//! Keys (in the plugin's namespace):
//! - `g/<epoch>/day/<YYYY-MM-DD>/<device>`: [`DayFact`]
//! - `g/<epoch>/medal/<palace>/<level>/<device>`: [`MedalFact`]
//! - `legacy/<device>`: [`LegacyFact`], the progress from before the sync (the shared blob)
//! - `legacy/<device>/own`: [`LegacyFact`], progress a device kept for itself and never synced
//!   (the web app's), added on top of the shared blob
//! - `progress`: the blob of older versions ([`OldProgress`])
//! - `pref/week-goal/<YYYY-Www>`: [`WeekGoalPref`]
//! - `seen/<hint>`: a dismissed hint
//!
//! Only facts under the current epoch count; `legacy/*` and `progress` only before the first
//! reset (epoch `0`). Empty values are keys that were blanked.

use std::collections::{BTreeMap, BTreeSet};

use locora_sdk::api::gamification::{
    ENOUGH, Earned, Goal, PalaceMedal, PalaceStrength, Today, Trophy, Week,
};
use serde::{Deserialize, Serialize};

pub const DUE_XP: u64 = 10;
pub const NEW_XP: u64 = 6;
pub const CLEARED_XP: u64 = 20;
pub const DEFAULT_WEEK_GOAL: u8 = 4;
/// Palaces with fewer loci earn no medals.
pub const MEDAL_LOCI: u32 = 5;
/// Medal levels: days a locus holds (FSRS stability).
pub const LEVELS: [u16; 3] = [7, 30, 180];
/// The epoch before any reset.
pub const FIRST_EPOCH: &str = "0";
pub const PROGRESS: &str = "progress";

pub const FIRST_JOURNEY: &str = "first-journey";
pub const HUNDRED_XP: &str = "hundred-xp";
pub const SEVEN_DAYS: &str = "seven-day-streak";
pub const DUE_CLEARED: &str = "due-cleared";
pub const RHYTHM_4: &str = "rhythm-4";
pub const PERFECT: &str = "perfect-journey";
/// The trophies that can be earned, in the order they are shown.
pub const TROPHIES: [&str; 5] = [FIRST_JOURNEY, HUNDRED_XP, SEVEN_DAYS, DUE_CLEARED, RHYTHM_4];

/// What one device did on one learning day. Its owner only ever adds: sets gain members, the
/// counter rises, flags go from false to true; only `look` is replaced by a newer one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayFact {
    pub v: u8,
    /// Sorted hashes (see [`locus_hash`]) of due loci reviewed, and of new loci learned.
    pub due: Vec<u64>,
    pub new: Vec<u64>,
    pub journeys: u32,
    /// Every due review of the day was done (and there was at least one).
    pub cleared: bool,
    /// What this device last saw of the schedule that day.
    pub look: Option<Look>,
}

/// The schedule as a device saw it: as of `at` (seconds since the Unix epoch), the next learning
/// day with a due review (`None`: no review cards at all). Learning days before `next` have
/// nothing due, which the weekly goal counts without the app being opened on them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Look {
    pub at: u64,
    pub next: Option<String>,
}

impl DayFact {
    pub fn empty() -> Self {
        Self {
            v: 1,
            ..Self::default()
        }
    }

    /// Adds a due review; whether that changed the fact.
    pub fn add_due(&mut self, hash: u64) -> bool {
        insert_sorted(&mut self.due, hash)
    }

    /// Adds a newly learned locus; whether that changed the fact.
    pub fn add_new(&mut self, hash: u64) -> bool {
        insert_sorted(&mut self.new, hash)
    }
}

fn insert_sorted(list: &mut Vec<u64>, hash: u64) -> bool {
    match list.binary_search(&hash) {
        Ok(_) => false,
        Err(at) => {
            list.insert(at, hash);
            true
        }
    }
}

/// A palace medal a device saw earned, and on which learning day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MedalFact {
    pub day: String,
}

/// The progress a device had before the sync, written once by the migration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyFact {
    pub xp: u64,
    pub trophies: Vec<String>,
    /// Learning dates from the review log.
    pub days: Vec<String>,
    /// Progress only this device ever had (never synced before): added to the others, where
    /// the shared blob counts with its largest value.
    pub independent: bool,
}

/// Learning days per week the learner aims for, from that week on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeekGoalPref {
    pub days: u8,
}

/// The single progress blob of the versions before the sync (`progress`), as they wrote it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OldProgress {
    pub xp: u64,
    pub max_combo: u32,
    pub trophies: Vec<Trophy>,
    pub sessions: BTreeMap<String, OldSession>,
    pub last_palace: Option<String>,
    pub daily_day: Option<u64>,
    pub daily_correct: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OldSession {
    pub combo: u32,
    pub max_combo: u32,
    pub hearts: u8,
}

pub fn day_key(epoch: &str, day: &str, device: &str) -> String {
    format!("g/{epoch}/day/{day}/{device}")
}

pub fn medal_key(epoch: &str, palace: &str, level: u16, device: &str) -> String {
    format!("g/{epoch}/medal/{palace}/{level}/{device}")
}

pub fn legacy_key(device: &str) -> String {
    format!("legacy/{device}")
}

/// The key of the progress a device kept for itself (see [`LegacyFact::independent`]).
pub fn own_legacy_key(device: &str) -> String {
    format!("legacy/{device}/own")
}

pub fn goal_key(week: &str) -> String {
    format!("pref/week-goal/{week}")
}

pub fn seen_key(hint: &str) -> String {
    format!("seen/{hint}")
}

/// FNV-1a (64 bit) of `<palace>/<locus>`: the same number on every platform.
pub fn locus_hash(palace: &str, locus: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in palace.bytes().chain(*b"/").chain(locus.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// The facts of every device for one learning day, merged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Day {
    pub due: BTreeSet<u64>,
    pub new: BTreeSet<u64>,
    pub journeys: u32,
    pub cleared: bool,
    /// The latest look at the schedule of any device that day (the earlier `next` on a tie).
    pub look: Option<Look>,
}

impl Day {
    /// Due reviews that were not also learned new that day.
    pub fn due_done(&self) -> u32 {
        self.due.difference(&self.new).count() as u32
    }

    /// The bonus counts only on a day with due reviews done.
    pub fn xp(&self) -> u64 {
        let done = self.due_done();
        DUE_XP * u64::from(done)
            + NEW_XP * self.new.len() as u64
            + if self.cleared && done > 0 {
                CLEARED_XP
            } else {
                0
            }
    }

    /// Something due was reviewed or something new learned (opening the app is not learning).
    pub fn learned(&self) -> bool {
        !self.due.is_empty() || !self.new.is_empty()
    }

    fn merge(&mut self, fact: &DayFact) {
        self.due.extend(fact.due.iter().copied());
        self.new.extend(fact.new.iter().copied());
        self.journeys = self.journeys.saturating_add(fact.journeys);
        self.cleared |= fact.cleared;
        if let Some(look) = &fact.look {
            let newer = match &self.look {
                None => true,
                Some(current) => {
                    look.at > current.at || (look.at == current.at && look.next < current.next)
                }
            };
            if newer {
                self.look = Some(look.clone());
            }
        }
    }
}

/// Everything stored, merged over the devices, for one epoch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub days: BTreeMap<String, Day>,
    /// XP from before the sync: the largest shared value plus what devices kept for themselves.
    pub legacy_xp: u64,
    pub legacy_trophies: BTreeSet<String>,
    pub legacy_days: BTreeSet<String>,
    /// (palace, level) → the earliest day any device saw it earned.
    pub medals: BTreeMap<(String, u16), String>,
    /// Week (`YYYY-Www`) → the goal chosen from then on.
    pub goals: BTreeMap<String, u8>,
    pub seen: BTreeSet<String>,
}

/// Merges the stored entries (`key`, bytes) under `epoch`. Unreadable entries are left out.
pub fn collect<'a>(entries: impl IntoIterator<Item = (&'a str, &'a [u8])>, epoch: &str) -> Facts {
    let mut facts = Facts::default();
    let current = format!("g/{epoch}/");
    let before_any_reset = epoch == FIRST_EPOCH;
    // Every device had the same synced blob: the largest counts, never the sum. Progress a
    // device kept for itself is added once per device.
    let (mut shared_xp, mut own_xp) = (0u64, 0u64);
    for (key, bytes) in entries {
        if bytes.is_empty() {
            continue;
        }
        if let Some(rest) = key.strip_prefix(&current) {
            if let Some(rest) = rest.strip_prefix("day/") {
                let Some((day, _device)) = rest.split_once('/') else {
                    continue;
                };
                if let Ok(fact) = locora_sdk::api::decode::<DayFact>(bytes) {
                    facts.days.entry(day.to_owned()).or_default().merge(&fact);
                }
            } else if let Some(rest) = rest.strip_prefix("medal/") {
                // The palace id comes first; level and device are the last two parts.
                let mut parts = rest.rsplitn(3, '/');
                let (Some(_device), Some(level), Some(palace)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let (Ok(level), Ok(fact)) = (
                    level.parse::<u16>(),
                    locora_sdk::api::decode::<MedalFact>(bytes),
                ) else {
                    continue;
                };
                let slot = facts
                    .medals
                    .entry((palace.to_owned(), level))
                    .or_insert_with(|| fact.day.clone());
                if fact.day < *slot {
                    *slot = fact.day;
                }
            }
        } else if let Some(week) = key.strip_prefix("pref/week-goal/") {
            if let Ok(goal) = locora_sdk::api::decode::<WeekGoalPref>(bytes)
                && (1..=7).contains(&goal.days)
            {
                facts.goals.insert(week.to_owned(), goal.days);
            }
        } else if let Some(hint) = key.strip_prefix("seen/") {
            facts.seen.insert(hint.to_owned());
        } else if before_any_reset && key.starts_with("legacy/") {
            if let Ok(legacy) = locora_sdk::api::decode::<LegacyFact>(bytes) {
                if legacy.independent {
                    own_xp = own_xp.saturating_add(legacy.xp);
                } else {
                    shared_xp = shared_xp.max(legacy.xp);
                }
                facts.legacy_trophies.extend(legacy.trophies);
                facts.legacy_days.extend(legacy.days);
            }
        } else if before_any_reset
            && key == PROGRESS
            && let Ok(progress) = locora_sdk::api::decode::<OldProgress>(bytes)
        {
            shared_xp = shared_xp.max(progress.xp);
            facts
                .legacy_trophies
                .extend(progress.trophies.iter().map(|t| t.id().to_owned()));
        }
    }
    facts.legacy_xp = shared_xp.saturating_add(own_xp);
    facts
}

/// What the stored facts amount to on the learning day `today`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derived {
    pub xp: u64,
    pub learning_days: BTreeSet<String>,
    /// Days with nothing due and nothing learned (they lower their week's target).
    pub free_days: BTreeSet<String>,
    /// Earned trophies with the day each was earned (`None`: earned before the sync).
    pub trophies: Vec<Earned>,
    pub locked: Vec<String>,
    pub week: Week,
}

impl Derived {
    pub fn earned(&self) -> BTreeSet<String> {
        self.trophies.iter().map(|t| t.id.clone()).collect()
    }
}

/// `epoch_day` is the day the progress was last deleted, which is never a free day.
pub fn derive(facts: &Facts, today: &str, epoch_day: Option<&str>) -> Derived {
    let xp = facts
        .days
        .values()
        .fold(facts.legacy_xp, |sum, day| sum.saturating_add(day.xp()));
    let learning_days: BTreeSet<String> = facts
        .days
        .iter()
        .filter(|(_, day)| day.learned())
        .map(|(date, _)| date.clone())
        .chain(facts.legacy_days.iter().cloned())
        .collect();
    let free_days = free_days(facts, &learning_days, today, epoch_day);
    let weeks = Weeks::new(&learning_days, &free_days, &facts.goals, today);
    let legacy = |id: &str| facts.legacy_trophies.contains(id);
    let earned = |id: &str, day: Option<String>| -> Option<Earned> {
        if legacy(id) {
            Some(Earned {
                id: id.to_owned(),
                day: None,
            })
        } else {
            day.map(|day| Earned {
                id: id.to_owned(),
                day: Some(day),
            })
        }
    };
    let first_journey = facts
        .days
        .iter()
        .find(|(_, day)| day.journeys > 0)
        .map(|(date, _)| date.clone());
    let hundred = if legacy(HUNDRED_XP) || facts.legacy_xp >= 100 {
        Some(Earned {
            id: HUNDRED_XP.to_owned(),
            day: None,
        })
    } else {
        let mut sum = facts.legacy_xp;
        facts.days.iter().find_map(|(date, day)| {
            sum = sum.saturating_add(day.xp());
            (sum >= 100).then(|| Earned {
                id: HUNDRED_XP.to_owned(),
                day: Some(date.clone()),
            })
        })
    };
    let seventh = learning_days.iter().nth(6).cloned();
    let cleared = facts
        .days
        .iter()
        .find(|(_, day)| day.cleared)
        .map(|(date, _)| date.clone());
    let rhythm = weeks.nth_reached(4);
    let mut trophies: Vec<Earned> = [
        earned(FIRST_JOURNEY, first_journey),
        hundred,
        earned(SEVEN_DAYS, seventh),
        earned(DUE_CLEARED, cleared),
        earned(RHYTHM_4, rhythm),
    ]
    .into_iter()
    .flatten()
    .collect();
    if legacy(PERFECT) {
        trophies.push(Earned {
            id: PERFECT.to_owned(),
            day: None,
        });
    }
    let locked = TROPHIES
        .iter()
        .filter(|id| !trophies.iter().any(|t| t.id == **id))
        .map(|id| (*id).to_owned())
        .collect();
    Derived {
        xp,
        week: weeks.at(),
        learning_days,
        free_days,
        trophies,
        locked,
    }
}

/// Days up to `today` on which nothing was due and nothing was learned. A day counts when the
/// latest look at the schedule on or before it saw the next due review after it, and no
/// learning day came between that look and the day (it may have changed the schedule).
fn free_days(
    facts: &Facts,
    learning_days: &BTreeSet<String>,
    today: &str,
    epoch_day: Option<&str>,
) -> BTreeSet<String> {
    let mut free = BTreeSet::new();
    let looks: Vec<(i64, &Look)> = facts
        .days
        .iter()
        .filter_map(|(date, day)| Some((day_number(date)?, day.look.as_ref()?)))
        .collect();
    let (Some(&(first, _)), Some(end)) = (looks.first(), day_number(today)) else {
        return free;
    };
    let learned: BTreeSet<i64> = learning_days
        .iter()
        .filter_map(|date| day_number(date))
        .collect();
    let deleted = epoch_day.and_then(day_number);
    let mut next_look = looks.iter().peekable();
    let mut latest: Option<(i64, &Look)> = None;
    for day in first..=end {
        while let Some(&&(date, look)) = next_look.peek() {
            if date > day {
                break;
            }
            latest = Some((date, look));
            next_look.next();
        }
        let Some((seen, look)) = latest else {
            continue;
        };
        if learned.contains(&day) || deleted == Some(day) {
            continue;
        }
        if day > seen + 1 && learned.range(seen + 1..day).next().is_some() {
            continue;
        }
        if look
            .next
            .as_deref()
            .and_then(day_number)
            .is_some_and(|next| next > day)
        {
            free.insert(date_name(day));
        }
    }
    free
}

/// Today's due goal. `due_open` counts reviews due today that were not done today;
/// `repeat_open` the loci back for practice (not part of the goal).
pub fn today(facts: &Facts, today: &str, due_open: u32, repeat_open: u32) -> Today {
    let day = facts.days.get(today).cloned().unwrap_or_default();
    let due_done = day.due_done();
    let goal = if due_open == 0 {
        // Learning only new loci is no due review done: nothing was due.
        if due_done > 0 {
            Goal::AllDone
        } else {
            Goal::Free
        }
    } else if due_done >= ENOUGH {
        Goal::Enough
    } else {
        Goal::Open
    };
    Today {
        due_done,
        due_open,
        new_done: day.new.len() as u32,
        repeat_open,
        goal,
    }
}

/// Medals of the palaces in `strengths` (palaces that no longer exist are not shown), and those
/// that qualify now without being earned yet: the caller stores them, earned `today`.
pub fn medals(
    facts: &Facts,
    strengths: &[PalaceStrength],
    today: &str,
) -> (Vec<PalaceMedal>, Vec<(String, u16)>) {
    let mut shown = Vec::new();
    let mut new = Vec::new();
    for strength in strengths {
        for level in LEVELS {
            let hold = match level {
                7 => strength.hold_7,
                30 => strength.hold_30,
                _ => strength.hold_180,
            };
            let mut unlocked = facts
                .medals
                .get(&(strength.palace_id.clone(), level))
                .cloned();
            let qualifies =
                strength.loci >= MEDAL_LOCI && u64::from(hold) * 10 >= u64::from(strength.loci) * 9;
            if qualifies && unlocked.is_none() {
                new.push((strength.palace_id.clone(), level));
                unlocked = Some(today.to_owned());
            }
            shown.push(PalaceMedal {
                palace_id: strength.palace_id.clone(),
                level,
                loci: strength.loci,
                hold,
                unlocked,
            });
        }
    }
    (shown, new)
}

/// The progress of before the sync, for `legacy/<device>`; `None` when there is none.
pub fn legacy(progress: Option<&OldProgress>, mut days: Vec<String>) -> Option<LegacyFact> {
    days.sort();
    days.dedup();
    let fact = LegacyFact {
        xp: progress.map_or(0, |p| p.xp),
        trophies: progress
            .map(|p| p.trophies.iter().map(|t| t.id().to_owned()).collect())
            .unwrap_or_default(),
        days,
        independent: false,
    };
    (fact.xp > 0 || !fact.trophies.is_empty() || !fact.days.is_empty()).then_some(fact)
}

/// The progress a device kept for itself, for `legacy/<device>/own`; `None` when there is none.
pub fn own_legacy(progress: &OldProgress) -> Option<LegacyFact> {
    let fact = LegacyFact {
        independent: true,
        ..legacy(Some(progress), Vec::new())?
    };
    (fact.xp > 0 || !fact.trophies.is_empty()).then_some(fact)
}

/// A trophy id as the old enum, for `gamification.status@1` and `TrophyUnlocked`.
pub fn old_trophy(id: &str) -> Option<Trophy> {
    match id {
        FIRST_JOURNEY => Some(Trophy::FirstJourney),
        PERFECT => Some(Trophy::PerfectJourney),
        HUNDRED_XP => Some(Trophy::HundredXp),
        SEVEN_DAYS => Some(Trophy::SevenDayStreak),
        _ => None,
    }
}

// ---------------------------------------------------------------- dates and weeks

/// Days since 1970-01-01 of a date `YYYY-MM-DD`.
pub fn day_number(date: &str) -> Option<i64> {
    let mut parts = date.splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// (year, month, day) of a day number.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `YYYY-MM-DD` of a day number.
fn date_name(days: i64) -> String {
    let (year, month, day) = civil(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The Monday (day number) of the ISO week of a day number.
fn monday(days: i64) -> i64 {
    // 1970-01-01 was a Thursday.
    days - (days + 3).rem_euclid(7)
}

/// `YYYY-Www` of the ISO week starting on the Monday `monday`.
pub fn week_name(monday: i64) -> String {
    let thursday = monday + 3;
    let (year, _, _) = civil(thursday);
    let first = day_number(&format!("{year:04}-01-01")).unwrap_or(thursday);
    let week = (thursday - first) / 7 + 1;
    format!("{year:04}-W{week:02}")
}

/// Whether `name` is an ISO week as `YYYY-Www` (week 01 to 53).
pub fn is_week_name(name: &str) -> bool {
    let Some((year, week)) = name.split_once("-W") else {
        return false;
    };
    year.len() == 4
        && week.len() == 2
        && year.bytes().all(|b| b.is_ascii_digit())
        && week
            .parse::<u8>()
            .is_ok_and(|week| (1..=53).contains(&week))
}

/// The ISO week of a date, `YYYY-Www`.
#[cfg(test)]
pub fn week_of(date: &str) -> Option<String> {
    day_number(date).map(|days| week_name(monday(days)))
}

/// Learning days and free days per ISO week, and the goal of each week.
struct Weeks {
    /// Monday → learning dates of that week, sorted.
    days: BTreeMap<i64, Vec<String>>,
    /// Monday → free dates of that week, sorted.
    free: BTreeMap<i64, Vec<String>>,
    goals: BTreeMap<String, u8>,
    /// The Monday of the current week.
    current: Option<i64>,
}

impl Weeks {
    fn new(
        learning_days: &BTreeSet<String>,
        free_days: &BTreeSet<String>,
        goals: &BTreeMap<String, u8>,
        today: &str,
    ) -> Self {
        let by_week = |dates: &BTreeSet<String>| {
            let mut weeks: BTreeMap<i64, Vec<String>> = BTreeMap::new();
            for date in dates {
                if let Some(number) = day_number(date) {
                    weeks.entry(monday(number)).or_default().push(date.clone());
                }
            }
            weeks
        };
        Self {
            days: by_week(learning_days),
            free: by_week(free_days),
            goals: goals.clone(),
            current: day_number(today).map(monday),
        }
    }

    /// The goal of the week starting on `monday`: the latest choice made for it or before.
    fn goal(&self, monday: i64) -> u8 {
        let name = week_name(monday);
        self.goals
            .range(..=name)
            .next_back()
            .map_or(DEFAULT_WEEK_GOAL, |(_, goal)| *goal)
    }

    fn count(&self, monday: i64) -> u32 {
        self.days.get(&monday).map_or(0, |days| days.len() as u32)
    }

    fn free(&self, monday: i64) -> u32 {
        self.free.get(&monday).map_or(0, |days| days.len() as u32)
    }

    /// Learning days the week needs: its goal less its free days, at least one.
    fn target(&self, monday: i64) -> u32 {
        u32::from(self.goal(monday))
            .saturating_sub(self.free(monday))
            .max(1)
    }

    fn reached(&self, monday: i64) -> bool {
        self.count(monday) >= self.target(monday)
    }

    /// Nothing was learned, and the days with nothing due cover the goal: the week neither
    /// counts nor breaks a run.
    fn neutral(&self, monday: i64) -> bool {
        self.count(monday) == 0 && self.free(monday) >= u32::from(self.goal(monday))
    }

    /// The day on which the `n`-th week that reached its target reached it.
    fn nth_reached(&self, n: usize) -> Option<String> {
        let monday = *self
            .days
            .keys()
            .filter(|monday| self.reached(**monday))
            .nth(n.checked_sub(1)?)?;
        let goal = u32::from(self.goal(monday));
        let learned = self.days.get(&monday)?;
        let free = self
            .free
            .get(&monday)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut dates: Vec<&String> = learned.iter().chain(free).collect();
        dates.sort();
        let (mut count, mut seen_free) = (0, 0);
        for date in dates {
            if learned.contains(date) {
                count += 1;
            } else {
                seen_free += 1;
            }
            if count >= goal.saturating_sub(seen_free).max(1) {
                return Some(date.clone());
            }
        }
        None
    }

    fn at(&self) -> Week {
        let Some(current) = self.current else {
            return Week {
                days: 0,
                goal: DEFAULT_WEEK_GOAL,
                free: 0,
                target: u32::from(DEFAULT_WEEK_GOAL),
                streak: 0,
                best: 0,
                missed_last: false,
            };
        };
        // An unfinished week does not break the run before it; neutral weeks are skipped.
        let first = self
            .days
            .keys()
            .chain(self.free.keys())
            .min()
            .copied()
            .unwrap_or(current);
        let mut week = if self.reached(current) {
            current
        } else {
            current - 7
        };
        let mut streak = 0;
        while week >= first {
            if self.reached(week) {
                streak += 1;
            } else if !self.neutral(week) {
                break;
            }
            week -= 7;
        }
        let (mut best, mut run) = (0, 0);
        let mut week = first;
        while week <= current {
            if self.reached(week) {
                run += 1;
                best = best.max(run);
            } else if week < current && !self.neutral(week) {
                run = 0;
            }
            week += 7;
        }
        let previous = current - 7;
        let missed_last = !self.reached(previous)
            && !self.neutral(previous)
            && self
                .days
                .keys()
                .any(|monday| *monday < previous && self.reached(*monday));
        Week {
            days: self.count(current),
            goal: self.goal(current),
            free: self.free(current),
            target: self.target(current),
            streak,
            best,
            missed_last,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(due: &[u64], new: &[u64]) -> DayFact {
        let mut fact = DayFact::empty();
        for hash in due {
            fact.add_due(*hash);
        }
        for hash in new {
            fact.add_new(*hash);
        }
        fact
    }

    fn bytes<T: Serialize>(value: &T) -> Vec<u8> {
        locora_sdk::api::encode(value).unwrap()
    }

    /// A store of keys as the sync leaves it: last writer wins per key.
    #[derive(Default, Clone)]
    struct Kv(BTreeMap<String, Vec<u8>>);

    impl Kv {
        fn set<T: Serialize>(&mut self, key: &str, value: &T) {
            self.0.insert(key.to_owned(), bytes(value));
        }
        fn merge(&mut self, other: &Kv) {
            for (key, value) in &other.0 {
                self.0.insert(key.clone(), value.clone());
            }
        }
        fn facts(&self, epoch: &str) -> Facts {
            collect(
                self.0.iter().map(|(k, v)| (k.as_str(), v.as_slice())),
                epoch,
            )
        }
        fn derive(&self, epoch: &str, today: &str) -> Derived {
            derive(&self.facts(epoch), today, None)
        }
    }

    fn progress(xp: u64, trophies: Vec<Trophy>) -> OldProgress {
        OldProgress {
            xp,
            trophies,
            ..OldProgress::default()
        }
    }

    #[test]
    fn the_owners_example_ends_at_170() {
        // Both devices synced the old blob (100 XP), then each learned offline.
        let mut base = Kv::default();
        base.set(PROGRESS, &progress(100, vec![]));
        let mut a = base.clone();
        let mut b = base.clone();
        a.set(
            &legacy_key("A"),
            &legacy(Some(&progress(100, vec![])), vec![]).unwrap(),
        );
        b.set(
            &legacy_key("B"),
            &legacy(Some(&progress(100, vec![])), vec![]).unwrap(),
        );
        a.set(
            &day_key("0", "2026-10-20", "A"),
            &fact(&[1, 2, 3, 4, 5], &[]),
        );
        b.set(&day_key("0", "2026-10-20", "B"), &fact(&[6, 7], &[]));
        assert_eq!(a.derive("0", "2026-10-20").xp, 150);
        assert_eq!(b.derive("0", "2026-10-20").xp, 120);
        let mut merged_ab = a.clone();
        merged_ab.merge(&b);
        let mut merged_ba = b.clone();
        merged_ba.merge(&a);
        assert_eq!(merged_ab.derive("0", "2026-10-20").xp, 170);
        assert_eq!(merged_ab.0, merged_ba.0, "either order");
        // The same operations again change nothing.
        merged_ab.merge(&a);
        merged_ab.merge(&b);
        assert_eq!(merged_ab.derive("0", "2026-10-20").xp, 170);
    }

    #[test]
    fn a_locus_counts_once_per_day_across_devices_and_new_beats_due() {
        let mut kv = Kv::default();
        kv.set(&day_key("0", "2026-10-20", "A"), &fact(&[1, 2], &[9]));
        kv.set(&day_key("0", "2026-10-20", "B"), &fact(&[1, 9], &[]));
        let facts = kv.facts("0");
        let day = &facts.days["2026-10-20"];
        // 1 and 2 due (1 on both devices), 9 new on A and due on B: new only.
        assert_eq!(day.due_done(), 2);
        assert_eq!(derive(&facts, "2026-10-20", None).xp, 2 * DUE_XP + NEW_XP);
        // Another day counts again.
        kv.set(&day_key("0", "2026-10-21", "A"), &fact(&[1], &[]));
        assert_eq!(kv.derive("0", "2026-10-21").xp, 3 * DUE_XP + NEW_XP);
    }

    #[test]
    fn a_later_write_of_the_owner_contains_the_earlier_one() {
        let mut first = fact(&[3, 1], &[7]);
        let earlier = first.clone();
        assert!(first.add_due(2));
        assert!(!first.add_due(2), "a locus is added once");
        first.journeys += 1;
        assert!(earlier.due.iter().all(|h| first.due.contains(h)));
        assert!(earlier.new.iter().all(|h| first.new.contains(h)));
        assert_eq!(first.due, vec![1, 2, 3], "sorted");
    }

    #[test]
    fn grades_do_not_matter_and_the_cleared_bonus_counts_once_per_day() {
        let mut kv = Kv::default();
        let mut day = fact(&[1], &[]);
        day.cleared = true;
        kv.set(&day_key("0", "2026-10-20", "A"), &day);
        kv.set(&day_key("0", "2026-10-20", "B"), &day);
        assert_eq!(kv.derive("0", "2026-10-20").xp, DUE_XP + CLEARED_XP);
        // Only new loci (relearned later that day) are no due review: no bonus, no trophy.
        let mut new_only = fact(&[5], &[5]);
        new_only.cleared = true;
        kv.set(&day_key("0", "2026-10-21", "A"), &new_only);
        let derived = kv.derive("0", "2026-10-21");
        assert_eq!(derived.xp, DUE_XP + CLEARED_XP + NEW_XP);
        assert_eq!(
            today(&kv.facts("0"), "2026-10-21", 0, 2).goal,
            Goal::Free,
            "nothing was due"
        );
    }

    #[test]
    fn legacy_progress_counts_once_and_only_before_a_reset() {
        let mut kv = Kv::default();
        let old = progress(340, vec![Trophy::FirstJourney, Trophy::PerfectJourney]);
        kv.set(PROGRESS, &old);
        kv.set(&legacy_key("A"), &legacy(Some(&old), vec![]).unwrap());
        kv.set(
            &legacy_key("B"),
            &legacy(Some(&old), vec!["2026-01-01".into()]).unwrap(),
        );
        let derived = kv.derive("0", "2026-10-20");
        assert_eq!(derived.xp, 340, "the same synced blob is not added up");
        let mut web = kv.clone();
        web.set(
            &own_legacy_key("W"),
            &own_legacy(&progress(300, vec![Trophy::HundredXp])).unwrap(),
        );
        assert_eq!(
            web.derive("0", "2026-10-20").xp,
            640,
            "a browser's own progress, never synced before, is added"
        );
        web.merge(&web.clone());
        assert_eq!(web.derive("0", "2026-10-20").xp, 640, "once");
        assert!(own_legacy(&progress(0, vec![])).is_none());
        assert_eq!(derived.learning_days.len(), 1);
        let ids: Vec<&str> = derived.trophies.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, [FIRST_JOURNEY, HUNDRED_XP, PERFECT]);
        assert!(derived.trophies.iter().all(|t| t.day.is_none()));
        assert!(!derived.locked.contains(&PERFECT.to_owned()));
        // After a reset nothing of it counts, and old-epoch facts do not either.
        kv.set(&day_key("0", "2026-10-20", "A"), &fact(&[1], &[]));
        let reset = kv.derive("1-A", "2026-10-20");
        assert_eq!(reset.xp, 0);
        assert!(reset.trophies.is_empty());
        assert!(reset.learning_days.is_empty());
        assert_eq!(reset.locked.len(), TROPHIES.len());
    }

    #[test]
    fn blanked_and_unreadable_keys_count_nothing() {
        let mut kv = Kv::default();
        kv.set(&day_key("0", "2026-10-20", "A"), &fact(&[1], &[]));
        kv.0.insert(day_key("0", "2026-10-21", "A"), Vec::new());
        kv.0.insert(day_key("0", "2026-10-22", "A"), vec![255; 3]);
        assert_eq!(kv.derive("0", "2026-10-22").xp, DUE_XP);
    }

    #[test]
    fn trophies_are_the_union_of_the_devices_with_the_earliest_day() {
        let mut kv = Kv::default();
        let mut journey = fact(&[1], &[]);
        journey.journeys = 1;
        kv.set(&day_key("0", "2026-10-21", "B"), &journey);
        kv.set(&day_key("0", "2026-10-20", "A"), &journey);
        let derived = kv.derive("0", "2026-10-21");
        assert_eq!(
            derived.trophies,
            vec![Earned {
                id: FIRST_JOURNEY.into(),
                day: Some("2026-10-20".into())
            }]
        );
    }

    #[test]
    fn a_hundred_xp_on_the_day_the_sum_reaches_it() {
        let mut kv = Kv::default();
        kv.set(
            &day_key("0", "2026-10-20", "A"),
            &fact(&[1, 2, 3, 4, 5, 6], &[]),
        );
        kv.set(&day_key("0", "2026-10-21", "A"), &fact(&[1, 2, 3, 4], &[]));
        let derived = kv.derive("0", "2026-10-21");
        assert_eq!(derived.xp, 100);
        assert!(derived.trophies.contains(&Earned {
            id: HUNDRED_XP.into(),
            day: Some("2026-10-21".into())
        }));
    }

    #[test]
    fn seven_learning_days_need_not_be_in_a_row_and_free_days_do_not_count() {
        let mut kv = Kv::default();
        for (i, date) in [
            "2026-09-01",
            "2026-09-05",
            "2026-09-09",
            "2026-09-20",
            "2026-10-01",
            "2026-10-07",
        ]
        .iter()
        .enumerate()
        {
            kv.set(&day_key("0", date, "A"), &fact(&[i as u64], &[]));
        }
        assert!(!kv.derive("0", "2026-10-20").earned().contains(SEVEN_DAYS));
        // Opening the app on a day with nothing due is no learning day.
        kv.set(
            &day_key("0", "2026-10-19", "B"),
            &looked(1, Some("2026-10-30")),
        );
        let derived = kv.derive("0", "2026-10-19");
        assert_eq!(derived.learning_days.len(), 6);
        assert_eq!(derived.free_days.len(), 1);
        assert!(!derived.earned().contains(SEVEN_DAYS));
        kv.set(&day_key("0", "2026-10-20", "B"), &fact(&[99], &[]));
        let derived = kv.derive("0", "2026-10-20");
        assert_eq!(derived.learning_days.len(), 7);
        assert!(derived.trophies.contains(&Earned {
            id: SEVEN_DAYS.into(),
            day: Some("2026-10-20".into())
        }));
    }

    #[test]
    fn iso_weeks_turn_on_monday() {
        assert_eq!(week_of("2026-10-25").unwrap(), "2026-W43", "a Sunday");
        assert_eq!(
            week_of("2026-10-26").unwrap(),
            "2026-W44",
            "the Monday after"
        );
        assert_eq!(week_of("2021-01-03").unwrap(), "2020-W53");
        assert_eq!(week_of("2024-12-30").unwrap(), "2025-W01");
        assert_eq!(day_number("1970-01-01"), Some(0));
        assert_eq!(day_number("2026-10-20").map(civil), Some((2026, 10, 20)));
        assert_eq!(week_of("kein Datum"), None);
        assert!(is_week_name("2026-W43"));
        assert!(!is_week_name("2026-W54") && !is_week_name("26-W43") && !is_week_name("2026-43"));
    }

    /// A device's look at the schedule, and nothing learned.
    fn looked(at: u64, next: Option<&str>) -> DayFact {
        DayFact {
            look: Some(Look {
                at,
                next: next.map(str::to_owned),
            }),
            ..DayFact::empty()
        }
    }

    #[test]
    fn days_with_nothing_due_lower_the_weeks_target_without_the_app_being_opened() {
        let mut kv = Kv::default();
        // Monday 2026-10-19: everything due done; the next review is on Thursday.
        let mut monday = fact(&[1, 2], &[]);
        monday.look = Some(Look {
            at: 10,
            next: Some("2026-10-22".into()),
        });
        kv.set(&day_key("0", "2026-10-19", "A"), &monday);
        // Wednesday: Tuesday and Wednesday had nothing due, though the app was never opened.
        let derived = kv.derive("0", "2026-10-21");
        assert_eq!(derived.learning_days.len(), 1);
        assert_eq!(
            derived.free_days,
            BTreeSet::from(["2026-10-20".to_owned(), "2026-10-21".to_owned()])
        );
        assert!(!derived.earned().contains(RHYTHM_4));
        let week = derived.week;
        assert_eq!((week.days, week.free, week.goal, week.target), (1, 2, 4, 2));
        // Thursday the review is due; learning on it reaches the target of two.
        kv.set(&day_key("0", "2026-10-22", "A"), &fact(&[3], &[]));
        let week = kv.derive("0", "2026-10-22").week;
        assert_eq!(
            (week.days, week.free, week.target, week.streak),
            (2, 2, 2, 1)
        );
        // Opening the app on a free day changes nothing.
        let mut opened = kv.clone();
        opened.set(
            &day_key("0", "2026-10-20", "B"),
            &looked(20, Some("2026-10-22")),
        );
        assert_eq!(
            opened.derive("0", "2026-10-22").week,
            kv.derive("0", "2026-10-22").week
        );
    }

    #[test]
    fn a_free_day_needs_a_look_at_the_schedule_that_is_not_out_of_date() {
        let mut kv = Kv::default();
        // No look at all: nothing is known about the days.
        learned_on(&mut kv, &["2026-10-19"]);
        assert!(kv.derive("0", "2026-10-21").free_days.is_empty());
        // A look that saw reviews due today: not free.
        kv.set(
            &day_key("0", "2026-10-20", "A"),
            &looked(5, Some("2026-10-20")),
        );
        assert!(kv.derive("0", "2026-10-20").free_days.is_empty());
        // The later look of another device that day wins.
        kv.set(
            &day_key("0", "2026-10-20", "B"),
            &looked(9, Some("2026-10-25")),
        );
        let derived = kv.derive("0", "2026-10-22");
        assert_eq!(derived.free_days.len(), 3, "{:?}", derived.free_days);
        // A learning day after the look (new loci, say) makes the days after it unknown.
        kv.set(&day_key("0", "2026-10-21", "A"), &fact(&[], &[7]));
        let derived = kv.derive("0", "2026-10-22");
        assert_eq!(
            derived.free_days,
            BTreeSet::from(["2026-10-20".to_owned()]),
            "Tuesday stays free, Thursday is not known"
        );
        // The day the progress was deleted is never free: its reviews are gone.
        let derived = derive(&kv.facts("0"), "2026-10-22", Some("2026-10-20"));
        assert!(derived.free_days.is_empty());
        // No cards at all: no free days.
        let mut empty = Kv::default();
        empty.set(&day_key("0", "2026-10-20", "A"), &looked(5, None));
        assert!(empty.derive("0", "2026-10-22").free_days.is_empty());
    }

    #[test]
    fn a_week_with_nothing_due_neither_counts_nor_breaks_the_run() {
        let mut kv = Kv::default();
        kv.set(&goal_key("2026-W01"), &WeekGoalPref { days: 2 });
        // W41 and W42 reached; W43 nothing due all week (the look of its Monday says so).
        learned_on(
            &mut kv,
            &["2026-10-05", "2026-10-06", "2026-10-12", "2026-10-13"],
        );
        let mut last = fact(&[9], &[]);
        last.look = Some(Look {
            at: 1,
            next: Some("2026-10-26".into()),
        });
        kv.set(&day_key("0", "2026-10-13", "A"), &last);
        let week = kv.derive("0", "2026-10-28").week;
        assert_eq!((week.streak, week.best), (2, 2), "{week:?}");
        assert!(!week.missed_last);
        assert!(!kv.derive("0", "2026-10-28").earned().contains(RHYTHM_4));
    }

    fn learned_on(kv: &mut Kv, dates: &[&str]) {
        for date in dates {
            kv.set(&day_key("0", date, "A"), &fact(&[1], &[]));
        }
    }

    #[test]
    fn the_weekly_run_keeps_its_best_and_an_open_week_does_not_break_it() {
        let mut kv = Kv::default();
        kv.set(&goal_key("2026-W01"), &WeekGoalPref { days: 2 });
        // W40 and W41 reached (2 days each), W42 missed, W43 reached.
        learned_on(
            &mut kv,
            &[
                "2026-09-28",
                "2026-09-29",
                "2026-10-05",
                "2026-10-06",
                "2026-10-12",
                "2026-10-19",
                "2026-10-20",
            ],
        );
        let week = kv.derive("0", "2026-10-21").week;
        assert_eq!((week.days, week.goal, week.streak, week.best), (2, 2, 1, 2));
        assert_eq!((week.free, week.target), (0, 2));
        assert!(week.missed_last);
        // The next week has just begun: the run of one goes on, nothing is lost yet.
        let week = kv.derive("0", "2026-10-26").week;
        assert_eq!((week.days, week.streak, week.best), (0, 1, 2));
        assert!(!week.missed_last);
        // A week later, W44 is missed: the run is over, the best stays.
        let week = kv.derive("0", "2026-11-02").week;
        assert_eq!((week.streak, week.best), (0, 2));
        assert!(week.missed_last);
    }

    #[test]
    fn each_week_keeps_the_goal_chosen_for_it() {
        let mut kv = Kv::default();
        kv.set(&goal_key("2026-W40"), &WeekGoalPref { days: 1 });
        kv.set(&goal_key("2026-W43"), &WeekGoalPref { days: 3 });
        learned_on(
            &mut kv,
            &["2026-09-28", "2026-10-05", "2026-10-12", "2026-10-19"],
        );
        let week = kv.derive("0", "2026-10-20").week;
        assert_eq!(week.goal, 3);
        assert_eq!(
            week.streak, 3,
            "W40–W42 reached their goal of 1, W43 is open"
        );
        assert_eq!(kv.derive("0", "2026-09-28").week.goal, 1);
        assert_eq!(Kv::default().derive("0", "2026-09-28").week.goal, 4);
    }

    #[test]
    fn the_weekly_goal_reached_four_times_in_any_weeks() {
        let mut kv = Kv::default();
        kv.set(&goal_key("2026-W01"), &WeekGoalPref { days: 1 });
        learned_on(&mut kv, &["2026-09-01", "2026-09-20", "2026-10-07"]);
        assert!(!kv.derive("0", "2026-10-20").earned().contains(RHYTHM_4));
        learned_on(&mut kv, &["2026-10-20"]);
        let derived = kv.derive("0", "2026-10-20");
        assert!(derived.trophies.contains(&Earned {
            id: RHYTHM_4.into(),
            day: Some("2026-10-20".into())
        }));
    }

    #[test]
    fn todays_goal_is_the_due_work_with_a_backlog_cap() {
        let mut kv = Kv::default();
        assert_eq!(today(&kv.facts("0"), "2026-10-20", 0, 0).goal, Goal::Free);
        assert_eq!(today(&kv.facts("0"), "2026-10-20", 3, 0).goal, Goal::Open);
        kv.set(&day_key("0", "2026-10-20", "A"), &fact(&[1, 2], &[3]));
        let done = today(&kv.facts("0"), "2026-10-20", 0, 1);
        assert_eq!(
            (done.due_done, done.new_done, done.repeat_open, done.goal),
            (2, 1, 1, Goal::AllDone)
        );
        let many: Vec<u64> = (100..100 + u64::from(ENOUGH)).collect();
        kv.set(&day_key("0", "2026-10-20", "B"), &fact(&many, &[]));
        assert_eq!(
            today(&kv.facts("0"), "2026-10-20", 80, 0).goal,
            Goal::Enough
        );
    }

    fn strength(palace: &str, loci: u32, hold: [u32; 3]) -> PalaceStrength {
        PalaceStrength {
            palace_id: palace.into(),
            loci,
            hold_7: hold[0],
            hold_30: hold[1],
            hold_180: hold[2],
        }
    }

    #[test]
    fn palace_medals_need_nine_in_ten_of_five_loci_and_stay() {
        let kv = Kv::default();
        let facts = kv.facts("0");
        let (shown, new) = medals(
            &facts,
            &[
                strength("p", 10, [9, 8, 0]),
                strength("klein", 4, [4, 4, 4]),
            ],
            "2026-10-20",
        );
        assert_eq!(new, vec![("p".to_owned(), 7)]);
        assert_eq!(shown.len(), 6);
        assert_eq!(shown[0].unlocked.as_deref(), Some("2026-10-20"));
        assert!(shown[1].unlocked.is_none());
        assert!(
            shown[3..].iter().all(|m| m.unlocked.is_none()),
            "below 5 loci"
        );
        // Earned on two devices: the earliest day; a weaker palace keeps it.
        let mut kv = Kv::default();
        kv.set(
            &medal_key("0", "p", 7, "B"),
            &MedalFact {
                day: "2026-10-21".into(),
            },
        );
        kv.set(
            &medal_key("0", "p", 7, "A"),
            &MedalFact {
                day: "2026-10-20".into(),
            },
        );
        let (shown, new) = medals(
            &kv.facts("0"),
            &[strength("p", 10, [1, 0, 0])],
            "2026-10-22",
        );
        assert!(new.is_empty());
        assert_eq!(shown[0].unlocked.as_deref(), Some("2026-10-20"));
        assert!(
            medals(&kv.facts("1-A"), &[strength("p", 10, [1, 0, 0])], "x").0[0]
                .unlocked
                .is_none()
        );
    }

    #[test]
    fn hashes_are_stable() {
        assert_eq!(locus_hash("", ""), 0xaf63_a24c_8601_89fe);
        assert_eq!(locus_hash("p", "l"), 0x7874_5419_5707_1944);
    }
}
