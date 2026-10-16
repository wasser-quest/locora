//! `locora settings`, `locora optimize`, `locora stats` and the daily limit
//! for new loci. App settings live in the profile under the reserved id
//! `host`; scheduler settings belong to the review plugin.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use clap::Subcommand;
use locora_api::Service;
use locora_api::review::{Configure, ConfigureRequest, Log, LogEntry, Settings};
use locora_fsrs::DEFAULT_WEIGHTS;
use locora_fsrs::optimize::{History, MIN_REVIEWS, optimize, usable_reviews};
use locora_runtime::settings::{introduced_today, parse_retention};

use crate::app::{App, now};

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
            app.set_new_per_day(count)?;
            println!("✓ Höchstens {count} neue Loci pro Tag");
        }
        Some(SettingsCommand::Retention { value }) => {
            let v = parse_retention(&value)?;
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
    let stats = app.stats()?.unwrap_or_default();
    app.report();
    println!("Antworten insgesamt:   {}", stats.answers);
    println!(
        "Heute:                 {} Antworten, {} neue Loci",
        stats.answered_today, stats.new_today
    );
    if stats.recent == 0 {
        println!("Trefferquote 30 Tage:  –");
    } else {
        println!(
            "Trefferquote 30 Tage:  {} ({} von {})",
            percent(stats.recent_known as f64 / stats.recent as f64),
            stats.recent_known,
            stats.recent
        );
    }
    println!(
        "Serie:                 {} {}",
        stats.streak,
        if stats.streak == 1 { "Tag" } else { "Tage" }
    );
    println!(
        "Geplante Loci:         {} (davon {} fällig)",
        stats.scheduled, stats.due
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_api::core::Grade;
    use locora_runtime::settings::day_start;

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
