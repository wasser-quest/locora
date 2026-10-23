//! App settings stored in the profile under the reserved plugin id `host`,
//! and the daily limit for new loci.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context, Result, bail};
use locora_api::Service;
use locora_api::core::Grade;
use locora_api::review::{
    Configure, ConfigureRequest, Log, LogEntry, LogRequest, ScheduledLocus, Settings,
};
use serde::{Deserialize, Serialize};

use locora_fsrs::DEFAULT_WEIGHTS;
pub use locora_fsrs::optimize::MIN_REVIEWS;
use locora_fsrs::optimize::{Fitted, History, optimize, usable_reviews};

use crate::{Profile, now};

pub const DAY: u64 = 86_400;
/// Upper bound for new loci per day.
pub const MAX_NEW_PER_DAY: usize = 1000;
pub const NEW_PER_DAY: &str = "settings.new_per_day";
/// Usable reviews when personal weights were last fitted (for the hint).
const OPTIMIZED_AT: &str = "settings.optimized_at_reviews";
/// Suggest fitting again after this many new usable reviews.
pub const OPTIMIZE_AGAIN_AFTER: usize = 1000;
pub const DEFAULT_NEW_PER_DAY: usize = 10;

/// UTC offsets with the time each began: `[[since, minutes], …]`, oldest first.
const UTC_OFFSETS: &str = "settings.utc_offsets";
/// Oldest offset changes are dropped beyond this (two per year with DST).
const MAX_OFFSET_CHANGES: usize = 400;
const DAY_START_HOUR: &str = "settings.day_start_hour";
/// Learning days begin at 4:00 local time: a session just after midnight
/// still counts for the day before.
pub const DEFAULT_DAY_START_HOUR: u8 = 4;

/// When a learning day begins: a full local hour, at the UTC offset that
/// applied at the time (offsets change with summer time; old answers keep
/// the offset they were given under).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayClock {
    pub start_hour: u8,
    /// (since, minutes east of UTC), oldest first; empty means UTC.
    pub offsets: Vec<(u64, i32)>,
}

impl Default for DayClock {
    fn default() -> Self {
        Self::fixed(0, DEFAULT_DAY_START_HOUR)
    }
}

impl DayClock {
    /// One offset for all times.
    pub fn fixed(utc_offset_minutes: i32, start_hour: u8) -> Self {
        Self {
            start_hour,
            offsets: vec![(0, utc_offset_minutes)],
        }
    }

    /// Minutes east of UTC at time `t` (before the first record: the first).
    pub fn offset_at(&self, t: u64) -> i32 {
        self.offsets
            .iter()
            .rev()
            .find(|(since, _)| *since <= t)
            .or(self.offsets.first())
            .map_or(0, |(_, m)| *m)
    }

    /// The offset in use now (the latest).
    pub fn utc_offset_minutes(&self) -> i32 {
        self.offsets.last().map_or(0, |(_, m)| *m)
    }

    /// The latest day start at or before `t` if `offset` applied throughout.
    fn boundary(&self, t: u64, offset: i32) -> u64 {
        let shift = i64::from(offset) * 60 - i64::from(self.start_hour) * 3600;
        let local = t as i64 + shift;
        (local.div_euclid(DAY as i64) * DAY as i64 - shift).max(0) as u64
    }

    /// Start of the learning day containing `t` (seconds since the epoch):
    /// the latest start hour at or before `t` under the offset that applied
    /// at that boundary itself (an offset change after the day began does
    /// not move its start).
    pub fn day_start(&self, t: u64) -> u64 {
        if self.offsets.is_empty() {
            return self.boundary(t, 0);
        }
        let local_day = |at: u64, minutes: i32| {
            (at as i64 + i64::from(minutes) * 60 - i64::from(self.start_hour) * 3600)
                .div_euclid(DAY as i64)
        };
        let mut latest = 0;
        for (i, &(since, offset)) in self.offsets.iter().enumerate() {
            let begin = if i == 0 { 0 } else { since };
            if begin > t {
                break;
            }
            let end = self
                .offsets
                .get(i + 1)
                .map_or(t, |(next, _)| t.min(next.saturating_sub(1)));
            let candidate = self.boundary(end, offset);
            let repeats = i > 0
                && since > 0
                && local_day(candidate, offset) <= local_day(since - 1, self.offsets[i - 1].1);
            if candidate >= begin && !repeats {
                latest = latest.max(candidate);
            }
            // A forward jump across the start hour creates one boundary at
            // the switch itself, not at either hypothetical local start.
            if i > 0 && since > 0 {
                let previous_offset = self.offsets[i - 1].1;
                if local_day(since, offset) > local_day(since - 1, previous_offset) {
                    latest = latest.max(since);
                }
            }
        }
        latest
    }

    /// The local date (`YYYY-MM-DD`) of the learning day starting at `day_start`: the date of
    /// its start hour, at the offset that applied then.
    pub fn date(&self, day_start: u64) -> String {
        let local = day_start as i64 + i64::from(self.offset_at(day_start)) * 60;
        let (year, month, day) = civil_date(local.div_euclid(DAY as i64));
        format!("{year:04}-{month:02}-{day:02}")
    }

    /// Start of the learning day before the one starting at `day_start`.
    pub fn previous_day(&self, day_start: u64) -> u64 {
        self.day_start(day_start.saturating_sub(1))
    }

    /// „4:00 (UTC+2)“
    pub fn describe(&self) -> String {
        let m = self.utc_offset_minutes();
        let sign = if m < 0 { '-' } else { '+' };
        let (h, min) = (m.abs() / 60, m.abs() % 60);
        let offset = if min == 0 {
            format!("UTC{sign}{h}")
        } else {
            format!("UTC{sign}{h}:{min:02}")
        };
        format!("{}:00 ({offset})", self.start_hour)
    }
}

/// (year, month, day) of a count of days since 1970-01-01 (Howard Hinnant's algorithm).
pub fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// An offset from UTC as people type it: `+2`, `-3`, `+5:30`, `0`.
pub fn parse_utc_offset(text: &str) -> Result<i32> {
    let text = text.trim().trim_start_matches("UTC").trim();
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let (h, m) = match rest.split_once(':') {
        Some((h, m)) => (h, m),
        None => (rest, "0"),
    };
    let (h, m): (i64, i64) = (
        h.trim().parse().context("Versatz wie +2, -3 oder +5:30")?,
        m.trim().parse().context("Versatz wie +2, -3 oder +5:30")?,
    );
    // Range first, so no arithmetic on huge input can overflow.
    if !(0..=14).contains(&h) || !(0..60).contains(&m) {
        bail!("Der Versatz muss zwischen UTC-12 und UTC+14 liegen");
    }
    let minutes = (sign * (h * 60 + m)) as i32;
    if !(-720..=840).contains(&minutes) {
        bail!("Der Versatz muss zwischen UTC-12 und UTC+14 liegen");
    }
    Ok(minutes)
}

/// Loci answered for the very first time on the learning day of `now`.
pub fn introduced_today(entries: &[LogEntry], now: u64, clock: &DayClock) -> usize {
    let mut first: HashMap<(&str, &str), u64> = HashMap::new();
    for e in entries {
        let slot = first
            .entry((&e.palace_id, &e.locus_id))
            .or_insert(e.answered_at);
        *slot = (*slot).min(e.answered_at);
    }
    first
        .values()
        .filter(|t| **t >= clock.day_start(now))
        .count()
}

/// Learning statistics from the review log and schedule.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub answers: usize,
    pub answered_today: usize,
    pub new_today: usize,
    /// Answers in the last 30 days and how many of them were not „Nochmal“.
    pub recent: usize,
    pub recent_known: usize,
    /// Days in a row with answers, up to today (or yesterday while today is open).
    pub streak: usize,
    pub scheduled: usize,
    pub due: usize,
}

/// Answers per learning day for the last `days` days up to today, oldest first. Days without
/// answers count as zero, so the overview shows the inactive days too.
pub fn activity_days(
    entries: &[LogEntry],
    now: u64,
    clock: &DayClock,
    days: usize,
) -> Vec<(u64, u32)> {
    let mut counts: HashMap<u64, u32> = HashMap::new();
    for entry in entries {
        *counts
            .entry(clock.day_start(entry.answered_at))
            .or_default() += 1;
    }
    let mut day = clock.day_start(now);
    let mut out = Vec::with_capacity(days);
    for _ in 0..days {
        out.push((day, counts.get(&day).copied().unwrap_or(0)));
        let previous = clock.previous_day(day);
        if previous >= day {
            break;
        }
        day = previous;
    }
    out.reverse();
    out
}

pub fn stats(
    entries: &[LogEntry],
    schedule: &[ScheduledLocus],
    now: u64,
    clock: &DayClock,
) -> Stats {
    let today = clock.day_start(now);
    let recent: Vec<&LogEntry> = entries
        .iter()
        .filter(|e| e.answered_at + 30 * DAY >= now)
        .collect();
    let days: BTreeSet<u64> = entries
        .iter()
        .map(|e| clock.day_start(e.answered_at))
        .collect();
    // Days are not always 24 h long (summer time): step by learning days.
    let mut day = if days.contains(&today) {
        today
    } else {
        clock.previous_day(today)
    };
    let mut streak = 0;
    while days.contains(&day) {
        streak += 1;
        let previous = clock.previous_day(day);
        if previous >= day {
            break;
        }
        day = previous;
    }
    Stats {
        answers: entries.len(),
        answered_today: entries.iter().filter(|e| e.answered_at >= today).count(),
        new_today: introduced_today(entries, now, clock),
        recent: recent.len(),
        recent_known: recent.iter().filter(|e| e.grade != Grade::Again).count(),
        streak,
        scheduled: schedule.len(),
        due: schedule.iter().filter(|s| s.card.due <= now).count(),
    }
}

/// Answers per locus in time order.
pub fn histories(entries: &[LogEntry]) -> Vec<History> {
    let mut by_locus: BTreeMap<(&str, &str), History> = BTreeMap::new();
    for e in entries {
        by_locus
            .entry((&e.palace_id, &e.locus_id))
            .or_default()
            .push((e.answered_at, e.grade));
    }
    by_locus
        .into_values()
        .map(|mut h| {
            h.sort_by_key(|(t, _)| *t);
            h
        })
        .collect()
}

/// What fitting personal FSRS weights found.
#[derive(Debug, Clone, PartialEq)]
pub enum Optimized {
    /// Not enough reviews at least a day apart yet.
    TooFew { usable: usize, needed: usize },
    /// Fitted; `saved` tells whether the weights were stored (they are not
    /// when nothing improved or on a dry run).
    Fitted { fitted: Fitted, saved: bool },
}

impl Optimized {
    /// For people, e.g. „Aus 412 Wiederholungen: Vorhersagefehler … (3,1 % besser)“.
    pub fn message(&self) -> String {
        match self {
            Optimized::TooFew { usable, needed } => format!(
                "Noch zu wenig Daten: {usable} von {needed} Wiederholungen mit mindestens einem Tag Abstand."
            ),
            Optimized::Fitted { fitted, saved } => {
                let gain = if fitted.loss_before > 0.0 {
                    1.0 - fitted.loss_after / fitted.loss_before
                } else {
                    0.0
                };
                let what = if *saved {
                    "Persönliche Gewichte gespeichert; sie gelten ab der nächsten Antwort."
                } else if fitted.loss_after >= fitted.loss_before {
                    "Die bisherigen Gewichte passen schon am besten; nichts geändert."
                } else {
                    "Nur berechnet, nichts geändert."
                };
                format!(
                    "Aus {} Wiederholungen: Vorhersagefehler {:.4} → {:.4} ({:.1} % besser). {what}",
                    fitted.reviews,
                    fitted.loss_before,
                    fitted.loss_after,
                    gain * 100.0
                )
            }
        }
    }
}

/// A target retention as people type it: `0,9`, `0.85`, `90 %`.
pub fn parse_retention(text: &str) -> Result<f64> {
    let text = text.trim().replace(',', ".");
    // „85 %“ and „0,85 %“ are percentages; a bare number above 1 is one too.
    let (number, percent) = match text.strip_suffix('%') {
        Some(n) => (n.trim(), true),
        None => (text.as_str(), false),
    };
    let v: f64 = number
        .parse()
        .context("Zahl erwartet, z. B. 0,9 oder 90 %")?;
    let v = if percent || v > 1.0 { v / 100.0 } else { v };
    // Same range as the review plugin, checked before anything is written.
    if !v.is_finite() || !(0.7..=0.99).contains(&v) {
        bail!("Die Ziel-Behaltensrate muss zwischen 0,70 und 0,99 liegen");
    }
    Ok(v)
}

impl Profile {
    fn setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match self.store.kv_get("host", key)? {
            Some(bytes) => {
                Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
                    format!("Einstellung {key} ist beschädigt")
                })?))
            }
            None => Ok(None),
        }
    }

    /// When learning days begin (see [`DayClock`]).
    pub fn day_clock(&self) -> Result<DayClock> {
        Ok(DayClock {
            start_hour: self
                .setting(DAY_START_HOUR)?
                .unwrap_or(DEFAULT_DAY_START_HOUR),
            offsets: self.setting(UTC_OFFSETS)?.unwrap_or_default(),
        })
    }

    pub fn set_day_start_hour(&self, hour: u8) -> Result<()> {
        if hour > 23 {
            bail!("Der Tag beginnt zu einer vollen Stunde von 0 bis 23");
        }
        self.store.kv_set(
            "host",
            DAY_START_HOUR,
            serde_json::to_string(&hour)?.as_bytes(),
        )?;
        Ok(())
    }

    /// Minutes east of UTC from now on (the app sends the system's offset at
    /// start and whenever it changes). Earlier answers keep their offset.
    pub fn set_utc_offset(&self, minutes: i32) -> Result<()> {
        if !(-720..=840).contains(&minutes) {
            bail!("Der Versatz muss zwischen UTC-12 und UTC+14 liegen");
        }
        let mut offsets = self.day_clock()?.offsets;
        if offsets.last().map(|(_, m)| *m) == Some(minutes) {
            return Ok(());
        }
        // A change at the same moment (or a clock set back) replaces, so a
        // short-lived value never decides how earlier answers are counted.
        let at = now();
        offsets.retain(|(since, _)| *since < at);
        if offsets.last().map(|(_, m)| *m) != Some(minutes) {
            offsets.push((at, minutes));
        }
        if offsets.len() > MAX_OFFSET_CHANGES {
            offsets.drain(..offsets.len() - MAX_OFFSET_CHANGES);
        }
        self.store.kv_set(
            "host",
            UTC_OFFSETS,
            serde_json::to_string(&offsets)?.as_bytes(),
        )?;
        Ok(())
    }

    pub fn set_new_per_day(&self, count: usize) -> Result<()> {
        if count > MAX_NEW_PER_DAY {
            bail!("Höchstens {MAX_NEW_PER_DAY} neue Loci pro Tag");
        }
        self.store.kv_set(
            "host",
            NEW_PER_DAY,
            serde_json::to_string(&count)?.as_bytes(),
        )?;
        Ok(())
    }

    /// Is the review plugin there (log and settings)?
    pub fn has_review(&self) -> bool {
        self.host.provides(Log::NAME) && self.host.provides(Configure::NAME)
    }

    /// Read (empty request) or change the review plugin's settings.
    pub fn configure(&mut self, request: ConfigureRequest) -> Result<Settings> {
        if !self.has_review() {
            bail!("Dafür braucht es das Plugin `review` (Wiederholung)");
        }
        self.host.call::<Configure>(&request)
    }

    /// Fit personal FSRS weights to the review log; with `save`, store them
    /// when they predict better than the current ones.
    pub fn optimize(&mut self, save: bool) -> Result<Optimized> {
        let entries = self.review_log()?;
        let histories = histories(&entries);
        let usable = usable_reviews(&histories);
        let current = self.configure(ConfigureRequest::default())?;
        let start: [f64; 19] = match &current.weights {
            Some(w) => w.as_slice().try_into().unwrap_or(DEFAULT_WEIGHTS),
            None => DEFAULT_WEIGHTS,
        };
        let Some(fitted) = optimize(&histories, &start) else {
            return Ok(Optimized::TooFew {
                usable,
                needed: MIN_REVIEWS,
            });
        };
        let saved = save && fitted.weights != start;
        if saved {
            self.configure(ConfigureRequest {
                weights: Some(Some(fitted.weights.to_vec())),
                ..Default::default()
            })?;
        }
        if save {
            self.store.kv_set(
                "host",
                OPTIMIZED_AT,
                serde_json::to_string(&usable)?.as_bytes(),
            )?;
        }
        Ok(Optimized::Fitted { fitted, saved })
    }

    /// Back to the default FSRS weights.
    pub fn reset_weights(&mut self) -> Result<()> {
        self.configure(ConfigureRequest {
            weights: Some(None),
            ..Default::default()
        })?;
        Ok(())
    }

    /// Usable reviews in the log (at least a day apart per locus) and whether
    /// fitting weights is worth suggesting: never fitted and enough data, or
    /// `OPTIMIZE_AGAIN_AFTER` new usable reviews since the last fit.
    pub fn optimize_hint(&mut self) -> Result<(usize, bool)> {
        if !self.has_review() {
            return Ok((0, false));
        }
        let usable = usable_reviews(&histories(&self.review_log()?));
        let last: Option<usize> = match self.store.kv_get("host", OPTIMIZED_AT)? {
            Some(bytes) => serde_json::from_slice(&bytes).ok(),
            None => None,
        };
        let worth = match last {
            None => usable >= MIN_REVIEWS,
            Some(at) => usable >= at + OPTIMIZE_AGAIN_AFTER,
        };
        Ok((usable, worth))
    }

    /// Statistics; `None` without the review plugin.
    /// Answers per learning day for the last `days` days (see `activity_days`).
    pub fn activity(&mut self, days: usize) -> Result<Vec<(u64, u32)>> {
        if !self.has_review() {
            return Ok(Vec::new());
        }
        let entries = self.review_log()?;
        Ok(activity_days(&entries, now(), &self.day_clock()?, days))
    }

    pub fn stats(&mut self) -> Result<Option<Stats>> {
        if !self.has_review() {
            return Ok(None);
        }
        let entries = self.review_log()?;
        let schedule = self.host.call::<locora_api::review::Schedule>(
            &locora_api::review::ScheduleRequest { palace_id: None },
        )?;
        Ok(Some(stats(&entries, &schedule, now(), &self.day_clock()?)))
    }

    pub fn new_per_day(&self) -> Result<usize> {
        Ok(match self.store.kv_get("host", NEW_PER_DAY)? {
            Some(bytes) => serde_json::from_slice(&bytes)
                .context("Einstellung neue Loci pro Tag ist beschädigt")?,
            None => DEFAULT_NEW_PER_DAY,
        })
    }

    /// The whole review log (empty without the review plugin).
    pub fn review_log(&mut self) -> Result<Vec<LogEntry>> {
        if !self.host.provides(Log::NAME) {
            return Ok(Vec::new());
        }
        self.host.call::<Log>(&LogRequest { palace_id: None })
    }

    /// How many new loci may still start today.
    pub fn new_budget(&mut self) -> Result<usize> {
        let limit = self.new_per_day()?;
        let clock = self.day_clock()?;
        Ok(limit.saturating_sub(introduced_today(&self.review_log()?, now(), &clock)))
    }

    /// The stored reminder settings; off until the user turns them on.
    pub fn reminders(&self) -> Result<Reminders> {
        Ok(self.setting(REMINDERS)?.unwrap_or_default())
    }

    /// Store new reminder settings and make the notify plugin's daily schedules match
    /// them. Without the plugin only the settings are stored.
    pub fn set_reminders(&mut self, reminders: Reminders) -> Result<()> {
        let times = reminders.times()?;
        if self.has_notify() {
            // Cancel what the previous settings planted, then plant the new times.
            for at in self.reminders()?.times().unwrap_or_default() {
                self.schedule_reminder(at, false)?;
            }
            if reminders.enabled {
                for at in times {
                    self.schedule_reminder(at, true)?;
                }
            }
        }
        self.store.kv_set(
            "host",
            REMINDERS,
            serde_json::to_string(&reminders)?.as_bytes(),
        )?;
        Ok(())
    }

    /// Plant (`on`) or cancel the daily reminder at `at` seconds past midnight.
    fn schedule_reminder(&mut self, at: u64, on: bool) -> Result<()> {
        self.host
            .call::<locora_api::notify::Schedule>(&locora_api::notify::ScheduleRequest {
                key: format!("reminder-{at}"),
                channel: REMINDER_CHANNEL.into(),
                priority: 1,
                title: "Locora".into(),
                body: REMINDER_TEXT.into(),
                action: String::new(),
                plan: on.then_some(locora_api::notify::Plan::Daily { seconds: at }),
            })?;
        Ok(())
    }
}

/// Daily reminders in local time: from `start` to `end` (both included), every `every`
/// hours. The notify plugin holds the matching schedules; this is what the user sets.
const REMINDERS: &str = "settings.reminders";
const REMINDER_CHANNEL: &str = "erinnerung";
pub const REMINDER_TEXT: &str = "Zeit zum Lernen: fällige Orte warten.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Reminders {
    pub enabled: bool,
    /// Minutes past midnight of the first reminder (local time).
    pub start: u16,
    /// Minutes past midnight of the last reminder that may fire (local time).
    pub end: u16,
    /// Hours between two reminders, 1 to 12.
    pub every: u8,
}

impl Default for Reminders {
    fn default() -> Self {
        Self {
            enabled: false,
            start: 9 * 60,
            end: 21 * 60,
            every: 4,
        }
    }
}

impl Reminders {
    /// The reminder times of a day, seconds past midnight, oldest first.
    pub fn times(&self) -> Result<Vec<u64>> {
        if self.start > 23 * 60 + 59 || self.end > 23 * 60 + 59 {
            bail!("Erinnerungen liegen zwischen 00:00 und 23:59");
        }
        if self.start > self.end {
            bail!("Die erste Erinnerung liegt nach der letzten");
        }
        if !(1..=12).contains(&self.every) {
            bail!("Erinnerungen lassen sich alle 1 bis 12 Stunden einstellen");
        }
        let step = u64::from(self.every) * 3600;
        let last = u64::from(self.end) * 60;
        let mut at = u64::from(self.start) * 60;
        let mut times = Vec::new();
        while at <= last {
            times.push(at);
            at += step;
        }
        Ok(times)
    }
}

/// Minutes past midnight for a 24-hour „HH:MM“.
pub fn parse_clock(text: &str) -> Result<u16> {
    let invalid = || anyhow::anyhow!("Uhrzeit als HH:MM angeben, nicht „{text}“");
    let (hours, minutes) = text.trim().split_once(':').ok_or_else(invalid)?;
    let hours: u16 = hours.parse().map_err(|_| invalid())?;
    let minutes: u16 = minutes.parse().map_err(|_| invalid())?;
    if hours > 23 || minutes > 59 {
        return Err(invalid());
    }
    Ok(hours * 60 + minutes)
}

/// „HH:MM“ for minutes past midnight.
pub fn format_clock(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminders_repeat_daily_and_stop_when_switched_off() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        assert!(!profile.reminders().unwrap().enabled);
        profile
            .set_reminders(Reminders {
                enabled: true,
                start: 9 * 60,
                end: 10 * 60,
                every: 1,
            })
            .unwrap();
        assert_eq!(
            profile.reminders().unwrap().times().unwrap(),
            [9 * 3600, 10 * 3600]
        );

        let day = 10 * DAY;
        assert_eq!(reminders_due(&mut profile, day + 9 * 3600 + 1), 1);
        assert_eq!(
            reminders_due(&mut profile, day + 9 * 3600 + 2),
            0,
            "each reminder fires once a day"
        );
        assert_eq!(reminders_due(&mut profile, day + 10 * 3600 + 1), 1);

        profile
            .set_reminders(Reminders {
                enabled: false,
                start: 9 * 60,
                end: 10 * 60,
                every: 1,
            })
            .unwrap();
        assert_eq!(
            reminders_due(&mut profile, day + DAY + 9 * 3600 + 1),
            0,
            "switched off: nothing fires"
        );
    }

    /// Reminders due at `now`, acknowledged as shown the way the app does.
    fn reminders_due(profile: &mut Profile, now: u64) -> usize {
        let due = profile
            .host
            .call::<locora_api::notify::Due>(&locora_api::notify::DueRequest {
                now,
                day_seconds: (now % DAY) as u32,
            })
            .unwrap();
        let mine: Vec<_> = due
            .into_iter()
            .filter(|queued| queued.body == REMINDER_TEXT)
            .collect();
        for queued in &mine {
            profile
                .shown_notification(&locora_api::notify::Message {
                    plugin: "notify".into(),
                    channel: queued.channel.clone(),
                    priority: queued.priority,
                    title: queued.title.clone(),
                    body: queued.body.clone(),
                    action: queued.action.clone(),
                    shown_at: now,
                })
                .unwrap();
        }
        mine.len()
    }

    #[test]
    fn clock_times_are_checked() {
        assert_eq!(parse_clock("09:30").unwrap(), 9 * 60 + 30);
        assert_eq!(format_clock(9 * 60 + 5), "09:05");
        assert!(parse_clock("24:00").is_err());
        assert!(parse_clock("9-30").is_err());
        assert!(
            Reminders {
                enabled: true,
                start: 600,
                end: 540,
                every: 1
            }
            .times()
            .is_err()
        );
    }

    fn entry(locus: &str, at: u64, grade: Grade) -> LogEntry {
        LogEntry {
            palace_id: "p".into(),
            locus_id: locus.into(),
            grade,
            answered_at: at,
            elapsed_ms: 1,
        }
    }

    #[test]
    fn stats_count_today_recent_and_streak() {
        let now = 1_800_000_000;
        let clock = DayClock::fixed(0, 0);
        let today = clock.day_start(now);
        let entries = vec![
            entry("a", today - 2 * DAY + 5, Grade::Good),
            entry("a", today - DAY + 5, Grade::Again),
            entry("b", today + 1, Grade::Easy),
            entry("old", today - 40 * DAY, Grade::Good),
        ];
        let s = stats(&entries, &[], now, &clock);
        assert_eq!(s.answers, 4);
        assert_eq!((s.answered_today, s.new_today), (1, 1));
        assert_eq!((s.recent, s.recent_known), (3, 2));
        assert_eq!(s.streak, 3);
        let s = stats(&entries[..2], &[], now, &clock);
        assert_eq!(s.streak, 2, "today still open counts up to yesterday");
    }

    #[test]
    fn learning_days_follow_local_time_and_start_hour() {
        // 2027-01-15 01:30 UTC = 02:30 in Berlin (UTC+1), before 4:00:
        // still the learning day that began on the 14th at 4:00 local time.
        let t = 1_800_000_000 - 6 * 3600 - 30 * 60;
        let berlin = DayClock::fixed(60, 4);
        let start = berlin.day_start(t);
        assert_eq!(start, 1_800_000_000 - 8 * 3600 - 86_400 + 3 * 3600);
        assert!(start <= t && t < start + DAY);
        assert_eq!(berlin.day_start(start + DAY), start + DAY);
        assert_eq!(berlin.describe(), "4:00 (UTC+1)");
        let india = DayClock::fixed(330, 0);
        assert_eq!(india.describe(), "0:00 (UTC+5:30)");
        assert_eq!((india.day_start(t) as i64 + 330 * 60) % DAY as i64, 0);
        assert_eq!(parse_utc_offset("+2").unwrap(), 120);
        assert_eq!(parse_utc_offset("UTC-3").unwrap(), -180);
        assert_eq!(parse_utc_offset("+5:30").unwrap(), 330);
        assert!(parse_utc_offset("+15").is_err());
        assert!(parse_utc_offset("zwei").is_err());
        assert!(parse_utc_offset("+71582789").is_err(), "no overflow");
        assert!(parse_utc_offset("-99999999999999999").is_err());
    }

    #[test]
    fn a_day_that_began_before_a_switch_keeps_its_start() {
        // UTC+1 until `switch` (06:00 UTC), then UTC+2. The day began at
        // 04:00 local under UTC+1 = 03:00 UTC, before the switch.
        let midnight = 1_800_000_000 - 1_800_000_000 % DAY;
        let switch = midnight + 6 * 3600;
        let clock = DayClock {
            start_hour: 4,
            offsets: vec![(0, 60), (switch, 120)],
        };
        assert_eq!(clock.day_start(switch + 3600), midnight + 3 * 3600);
        // An answer at 03:30 UTC belongs to today, also seen after the switch.
        let entries = vec![entry("x", midnight + 3 * 3600 + 1800, Grade::Good)];
        let s = stats(&entries, &[], switch + 3600, &clock);
        assert_eq!((s.answered_today, s.streak), (1, 1));
        // The next day starts at 04:00 under UTC+2 = 02:00 UTC.
        assert_eq!(
            clock.day_start(midnight + DAY + 2 * 3600),
            midnight + DAY + 2 * 3600
        );
        assert_eq!(
            clock.previous_day(midnight + DAY + 2 * 3600),
            midnight + 3 * 3600
        );
    }

    #[test]
    fn repeated_start_hour_does_not_start_a_second_day() {
        let midnight = 1_800_000_000 - 1_800_000_000 % DAY;
        // Europe repeats 02:00 local; the US repeats 01:00 local.
        for (old, new, hour, switch_hour) in [(120, 60, 2, 1), (-240, -300, 1, 6)] {
            let switch = midnight + switch_hour * 3600;
            let clock = DayClock {
                start_hour: hour,
                offsets: vec![(0, old), (switch, new)],
            };
            let start = clock.day_start(switch - 1);
            let entries = vec![entry("x", start + 60, Grade::Good)];
            for now in [switch, switch + 1800, switch + 3600] {
                assert_eq!(clock.day_start(now), start);
                let s = stats(&entries, &[], now, &clock);
                assert_eq!((s.answered_today, s.new_today, s.streak), (1, 1, 1));
            }
            let next = switch + DAY;
            assert_eq!(clock.day_start(next), next);
            assert_eq!(clock.previous_day(next), start);
        }
    }

    #[test]
    fn skipped_day_start_occurs_once_at_the_switch() {
        let midnight = 1_800_000_000 - 1_800_000_000 % DAY;
        let switch = midnight + 2 * 3600 + 30 * 60;
        let clock = DayClock {
            start_hour: 4,
            offsets: vec![(0, 60), (switch, 120)],
        };
        let previous = midnight - DAY + 3 * 3600;
        assert_eq!(clock.day_start(switch - 1), previous);
        assert_eq!(clock.day_start(switch), switch);
        let entries = vec![entry("x", switch + 60, Grade::Good)];
        for now in [midnight + 3 * 3600 - 1, midnight + 3 * 3600 + 1] {
            assert_eq!(clock.day_start(now), switch);
            let s = stats(&entries, &[], now, &clock);
            assert_eq!((s.answered_today, s.new_today, s.streak), (1, 1, 1));
        }
        let next = midnight + DAY + 2 * 3600;
        assert_eq!(clock.day_start(next), next);
        assert_eq!(clock.previous_day(next), switch);
        assert_eq!(clock.previous_day(switch), previous);
    }

    #[test]
    fn old_answers_keep_the_offset_of_their_time() {
        // Summer time (UTC+2) until `switch`, then winter time (UTC+1).
        let switch = 1_800_000_000;
        let clock = DayClock {
            start_hour: 4,
            offsets: vec![(0, 120), (switch, 60)],
        };
        // 04:30 local summer time = 02:30 UTC: after the 4:00 start that day.
        let summer_answer = switch - 3 * DAY + 2 * 3600 + 30 * 60 - switch % DAY;
        let start = clock.day_start(summer_answer);
        assert_eq!(summer_answer - start, 30 * 60, "half an hour into the day");
        // A streak from summer time, looked at after the switch, keeps every day.
        let days: Vec<u64> = (0..4).map(|i| summer_answer + i * DAY).collect();
        let entries: Vec<LogEntry> = days.iter().map(|t| entry("x", *t, Grade::Good)).collect();
        // Asked in winter time, after the switch.
        let now = switch + 3600;
        assert!(*days.last().unwrap() < switch && now - days.last().unwrap() < DAY);
        assert_eq!(stats(&entries, &[], now, &clock).streak, 4);
        assert_eq!(clock.utc_offset_minutes(), 60);
        assert_eq!(clock.offset_at(switch - 1), 120);
    }

    #[test]
    fn a_learning_day_has_the_local_date_of_its_start() {
        // 2026-10-20 03:30 UTC: in Berlin (UTC+2) 5:30, after the 4:00 start.
        let t = 1_792_467_000;
        let berlin = DayClock::fixed(120, 4);
        assert_eq!(berlin.date(berlin.day_start(t)), "2026-10-20");
        // In New York (UTC-4) it is 23:30 on the 19th.
        let new_york = DayClock::fixed(-240, 4);
        assert_eq!(new_york.date(new_york.day_start(t)), "2026-10-19");
        // Just after midnight still counts for the day before.
        let night = DayClock::fixed(0, 4);
        assert_eq!(night.date(night.day_start(1_792_461_600)), "2026-10-19");
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(-1), (1969, 12, 31));
    }

    #[test]
    fn retention_accepts_comma_and_percent() {
        assert_eq!(parse_retention("0,85").unwrap(), 0.85);
        assert_eq!(parse_retention("90 %").unwrap(), 0.9);
        assert!(parse_retention("viel").is_err());
        assert!(parse_retention("0,5").is_err());
        assert!(parse_retention("100 %").is_err());
        assert!(parse_retention("NaN").is_err());
        assert!(parse_retention("0,85 %").is_err(), "0,85 % is not 85 %");
        assert_eq!(parse_retention("85,5 %").unwrap(), 0.855);
    }
}
