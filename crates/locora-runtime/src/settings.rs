//! App settings stored in the profile under the reserved plugin id `host`,
//! and the daily limit for new loci.

use std::collections::HashMap;

use anyhow::{Context, Result};
use locora_api::Service;
use locora_api::review::{Log, LogEntry, LogRequest};

use crate::{Profile, now};

const DAY: u64 = 86_400;
pub const NEW_PER_DAY: &str = "settings.new_per_day";
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

impl Profile {
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
