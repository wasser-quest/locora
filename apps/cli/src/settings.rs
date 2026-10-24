//! `locora settings`, `locora optimize`, `locora stats` and the daily limit
//! for new loci. App settings live in the profile under the reserved id
//! `host`; scheduler settings belong to the review plugin.

use anyhow::{Result, bail};
use clap::Subcommand;
use locora_api::Service;
use locora_api::review::{Configure, ConfigureRequest, Log, LogEntry, Settings};
use locora_runtime::settings::{Optimized, introduced_today, parse_retention, parse_utc_offset};

use crate::app::{App, now};

#[derive(Subcommand)]
pub enum SettingsCommand {
    /// Wie viele neue Loci pro Tag höchstens dazukommen (Standard 10)
    NewPerDay { count: usize },
    /// Ziel-Behaltensrate der Wiederholung, 0,70 bis 0,99 (Standard 0,90)
    Retention { value: String },
    /// Volle Stunde, zu der ein Lerntag beginnt, 0 bis 23 (Standard 4)
    DayStart { hour: u8 },
    /// Versatz der eigenen Uhrzeit zu UTC, z. B. +2 oder -5:30 (die App stellt ihn selbst ein)
    UtcOffset {
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
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
    let clock = app.day_clock()?;
    Ok(limit.saturating_sub(introduced_today(&log(app)?, now(), &clock)))
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
        Some(SettingsCommand::DayStart { hour }) => {
            app.set_day_start_hour(hour)?;
            println!("✓ Ein Lerntag beginnt um {}", app.day_clock()?.describe());
        }
        Some(SettingsCommand::UtcOffset { value }) => {
            app.set_utc_offset(parse_utc_offset(&value)?)?;
            println!("✓ Ein Lerntag beginnt um {}", app.day_clock()?.describe());
        }
        None => {
            println!("Neue Loci pro Tag:     {}", new_per_day(app)?);
            println!("Lerntag beginnt:       {}", app.day_clock()?.describe());
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
            println!(
                "\nÄndern: locora settings new-per-day 15 · locora settings retention 0,85 · locora settings day-start 4"
            );
        }
    }
    Ok(())
}

pub fn optimize_command(app: &mut App, dry_run: bool, reset: bool) -> Result<()> {
    review_required(app)?;
    if reset {
        app.reset_weights()?;
        app.report();
        println!("✓ FSRS-Gewichte auf Standard zurückgesetzt");
        return Ok(());
    }
    let result = app.optimize(!dry_run)?;
    app.report();
    let saved = matches!(result, Optimized::Fitted { saved: true, .. });
    println!("{}{}", if saved { "✓ " } else { "" }, result.message());
    Ok(())
}

pub fn stats(app: &mut App) -> Result<()> {
    review_required(app)?;
    let stats = app.stats()?.unwrap_or_default();
    let game = app.gamification_status(false)?;
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
    // With the game, learning days on every device; otherwise the review log's run of days.
    match &game {
        Some(game) => {
            println!("Lerntage:              {}", game.learning_days);
            println!(
                "Wochenziel in Folge:   {} {}",
                game.week.streak,
                if game.week.streak == 1 {
                    "Woche"
                } else {
                    "Wochen"
                }
            );
        }
        None => println!(
            "Serie:                 {} {}",
            stats.streak,
            if stats.streak == 1 { "Tag" } else { "Tage" }
        ),
    }
    println!(
        "Geplante Loci:         {} (davon {} fällig)",
        stats.scheduled, stats.due
    );
    if let Some(game) = game {
        println!(
            "Erfolge:               {} XP, {} Trophäen",
            game.xp,
            game.trophies.len()
        );
        println!("{}", crate::game::today_line(&game.today));
        println!(
            "Diese Woche:           {}",
            crate::game::week_line(&game.week)
        );
    }
    let (usable, worth) = app.optimize_hint()?;
    if worth {
        println!(
            "\nTipp: Aus {usable} Wiederholungen kann Locora persönliche Gewichte berechnen: locora optimize"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_api::core::Grade;
    use locora_runtime::settings::{DayClock, histories};

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
        let clock = DayClock::default();
        let today = clock.day_start(now);
        let entries = vec![
            entry("old", today - 10),
            entry("old", today + 5),
            entry("a", today + 1),
            entry("b", today + 2),
        ];
        assert_eq!(introduced_today(&entries, now, &clock), 2);
        let h = histories(&entries);
        assert_eq!(h.len(), 3);
        assert!(h.iter().all(|h| h.windows(2).all(|w| w[0].0 <= w[1].0)));
    }
}
