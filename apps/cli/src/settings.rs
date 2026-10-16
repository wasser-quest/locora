//! `locora settings`, `locora optimize`, `locora stats` and the daily limit
//! for new loci. App settings live in the profile under the reserved id
//! `host`; scheduler settings belong to the review plugin.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use locora_api::Service;
use locora_api::core::Grade;
use locora_api::review::{
    Configure, ConfigureRequest, Log, LogEntry, Schedule, ScheduleRequest, Settings,
};
use locora_fsrs::DEFAULT_WEIGHTS;
use locora_fsrs::optimize::{History, MIN_REVIEWS, optimize, usable_reviews};

use crate::app::{App, now};

const DAY: u64 = 86_400;
use locora_runtime::settings::{NEW_PER_DAY, day_start, introduced_today};

#[derive(Subcommand)]
pub enum SettingsCommand {
    /// Wie viele neue Loci pro Tag höchstens dazukommen (Standard 10)
    NewPerDay { count: usize },
    /// Ziel-Behaltensrate der Wiederholung, 0,70 bis 0,99 (Standard 0,90)
    Retention { value: String },
}

fn review_required(app: &App) -> Result<()> {
    if !app.host.provides(Log::NAME) || !app.host.provides(Configure::NAME) {
        bail!("Dafür braucht es das Plugin `review` (Wiederholung)");
    }
    Ok(())
}

pub fn new_per_day(app: &App) -> Result<usize> {
    app.new_per_day()
}

/// The whole review log (empty without the review plugin).
fn log(app: &mut App) -> Result<Vec<LogEntry>> {
    let entries = app.review_log()?;
    app.report();
    Ok(entries)
}

/// How many new loci may still start today.
pub fn new_budget(app: &mut App) -> Result<usize> {
    let limit = new_per_day(app)?;
    Ok(limit.saturating_sub(introduced_today(&log(app)?, now())))
}

fn configure(app: &mut App, request: ConfigureRequest) -> Result<Settings> {
    review_required(app)?;
    let settings = app.host.call::<Configure>(&request)?;
    app.report();
    Ok(settings)
}

fn percent(v: f64) -> String {
    format!("{:.0} %", v * 100.0)
}

pub fn settings(app: &mut App, command: Option<SettingsCommand>) -> Result<()> {
    match command {
        Some(SettingsCommand::NewPerDay { count }) => {
            if count > 1000 {
                bail!("Höchstens 1000 neue Loci pro Tag");
            }
            app.store.kv_set(
                "host",
                NEW_PER_DAY,
                serde_json::to_string(&count)?.as_bytes(),
            )?;
            println!("✓ Höchstens {count} neue Loci pro Tag");
        }
        Some(SettingsCommand::Retention { value }) => {
            let v: f64 = value
                .trim()
                .replace(',', ".")
                .trim_end_matches('%')
                .trim()
                .parse()
                .context("Zahl erwartet, z. B. 0,9 oder 90 %")?;
            let v = if v > 1.0 { v / 100.0 } else { v };
            let s = configure(
                app,
                ConfigureRequest {
                    desired_retention: Some(v),
                    ..Default::default()
                },
            )?;
            println!("✓ Ziel-Behaltensrate {}", percent(s.desired_retention));
            println!("  Gilt ab der nächsten Antwort für jeden Locus.");
        }
        None => {
            println!("Neue Loci pro Tag:     {}", new_per_day(app)?);
            if app.host.provides(Configure::NAME) {
                let s = configure(app, ConfigureRequest::default())?;
                println!("Ziel-Behaltensrate:    {}", percent(s.desired_retention));
                println!(
                    "FSRS-Gewichte:         {}",
                    if s.weights.is_some() {
                        "persönlich (locora optimize)"
                    } else {
                        "Standard"
                    }
                );
            }
            println!("\nÄndern: locora settings new-per-day 15 · locora settings retention 0,85");
        }
    }
    Ok(())
}

/// Answers per locus in time order.
fn histories(entries: &[LogEntry]) -> Vec<History> {
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

pub fn optimize_command(app: &mut App, dry_run: bool, reset: bool) -> Result<()> {
    review_required(app)?;
    if reset {
        configure(
            app,
            ConfigureRequest {
                weights: Some(None),
                ..Default::default()
            },
        )?;
        println!("✓ FSRS-Gewichte auf Standard zurückgesetzt");
        return Ok(());
    }
    let entries = log(app)?;
    let histories = histories(&entries);
    let usable = usable_reviews(&histories);
    let current = configure(app, ConfigureRequest::default())?;
    let start: [f64; 19] = match &current.weights {
        Some(w) => w.as_slice().try_into().unwrap_or(DEFAULT_WEIGHTS),
        None => DEFAULT_WEIGHTS,
    };
    let Some(fitted) = optimize(&histories, &start) else {
        println!(
            "Noch zu wenig Daten: {usable} von {MIN_REVIEWS} Wiederholungen mit mindestens einem Tag Abstand."
        );
        return Ok(());
    };
    let gain = if fitted.loss_before > 0.0 {
        1.0 - fitted.loss_after / fitted.loss_before
    } else {
        0.0
    };
    println!(
        "Aus {} Wiederholungen: Vorhersagefehler {:.4} → {:.4} ({:.1} % besser)",
        fitted.reviews,
        fitted.loss_before,
        fitted.loss_after,
        gain * 100.0
    );
    if dry_run {
        println!("(nur berechnet, nichts geändert)");
    } else if fitted.weights == start {
        println!("Die bisherigen Gewichte passen schon am besten; nichts geändert.");
    } else {
        configure(
            app,
            ConfigureRequest {
                weights: Some(Some(fitted.weights.to_vec())),
                ..Default::default()
            },
        )?;
        println!("✓ Persönliche FSRS-Gewichte gespeichert. Sie gelten ab der nächsten Antwort.");
    }
    Ok(())
}

pub fn stats(app: &mut App) -> Result<()> {
    review_required(app)?;
    let now = now();
    let today = day_start(now);
    let entries = log(app)?;
    let schedule = app
        .host
        .call::<Schedule>(&ScheduleRequest { palace_id: None })?;
    app.report();
    let answered_today = entries.iter().filter(|e| e.answered_at >= today).count();
    let recent: Vec<&LogEntry> = entries
        .iter()
        .filter(|e| e.answered_at + 30 * DAY >= now)
        .collect();
    let known = recent.iter().filter(|e| e.grade != Grade::Again).count();
    let days: BTreeSet<u64> = entries.iter().map(|e| day_start(e.answered_at)).collect();
    // A streak counts up to today, or up to yesterday if today is still open.
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
    let due = schedule.iter().filter(|s| s.card.due <= now).count();
    println!("Antworten insgesamt:   {}", entries.len());
    println!(
        "Heute:                 {answered_today} Antworten, {} neue Loci",
        introduced_today(&entries, now)
    );
    if recent.is_empty() {
        println!("Trefferquote 30 Tage:  –");
    } else {
        println!(
            "Trefferquote 30 Tage:  {} ({known} von {})",
            percent(known as f64 / recent.len() as f64),
            recent.len()
        );
    }
    println!(
        "Serie:                 {streak} {}",
        if streak == 1 { "Tag" } else { "Tage" }
    );
    println!(
        "Geplante Loci:         {} (davon {due} fällig)",
        schedule.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(locus: &str, at: u64) -> LogEntry {
        LogEntry {
            palace_id: "p".into(),
            locus_id: locus.into(),
            grade: Grade::Good,
            answered_at: at,
            elapsed_ms: 1,
        }
    }

    #[test]
    fn only_first_answers_today_count_as_new() {
        let now = 1_800_000_000;
        let today = day_start(now);
        let entries = vec![
            entry("old", today - 10),
            entry("old", today + 5),
            entry("a", today + 1),
            entry("b", today + 2),
        ];
        assert_eq!(introduced_today(&entries, now), 2);
        let h = histories(&entries);
        assert_eq!(h.len(), 3);
        assert!(h.iter().all(|h| h.windows(2).all(|w| w[0].0 <= w[1].0)));
    }
}
