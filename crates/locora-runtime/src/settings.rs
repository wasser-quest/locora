//! App settings stored in the profile under the reserved plugin id `host`,
//! and the daily limit for new loci.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context, Result, bail};
use locora_api::Service;
use locora_api::core::Grade;
use locora_api::review::{
    Configure, ConfigureRequest, Log, LogEntry, LogRequest, ScheduledLocus, Settings,
};

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

/// Start of the (UTC) day containing `t`.
pub fn day_start(t: u64) -> u64 {
    t - t % DAY
}

/// Loci answered for the very first time today (UTC day).
pub fn introduced_today(entries: &[LogEntry], now: u64) -> usize {
    let mut first: HashMap<(&str, &str), u64> = HashMap::new();
    for e in entries {
        let slot = first
            .entry((&e.palace_id, &e.locus_id))
            .or_insert(e.answered_at);
        *slot = (*slot).min(e.answered_at);
    }
    first.values().filter(|t| **t >= day_start(now)).count()
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

pub fn stats(entries: &[LogEntry], schedule: &[ScheduledLocus], now: u64) -> Stats {
    let today = day_start(now);
    let recent: Vec<&LogEntry> = entries
        .iter()
        .filter(|e| e.answered_at + 30 * DAY >= now)
        .collect();
    let days: BTreeSet<u64> = entries.iter().map(|e| day_start(e.answered_at)).collect();
    let mut day = if days.contains(&today) {
        today
    } else {
        today.saturating_sub(DAY)
    };
    let mut streak = 0;
    while days.contains(&day) {
        streak += 1;
        day = day.saturating_sub(DAY);
    }
    Stats {
        answers: entries.len(),
        answered_today: entries.iter().filter(|e| e.answered_at >= today).count(),
        new_today: introduced_today(entries, now),
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
    pub fn stats(&mut self) -> Result<Option<Stats>> {
        if !self.has_review() {
            return Ok(None);
        }
        let entries = self.review_log()?;
        let schedule = self.host.call::<locora_api::review::Schedule>(
            &locora_api::review::ScheduleRequest { palace_id: None },
        )?;
        Ok(Some(stats(&entries, &schedule, now())))
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
        Ok(limit.saturating_sub(introduced_today(&self.review_log()?, now())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let today = day_start(now);
        let entries = vec![
            entry("a", today - 2 * DAY + 5, Grade::Good),
            entry("a", today - DAY + 5, Grade::Again),
            entry("b", today + 1, Grade::Easy),
            entry("old", today - 40 * DAY, Grade::Good),
        ];
        let s = stats(&entries, &[], now);
        assert_eq!(s.answers, 4);
        assert_eq!((s.answered_today, s.new_today), (1, 1));
        assert_eq!((s.recent, s.recent_known), (3, 2));
        assert_eq!(s.streak, 3);
        let s = stats(&entries[..2], &[], now);
        assert_eq!(s.streak, 2, "today still open counts up to yesterday");
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
