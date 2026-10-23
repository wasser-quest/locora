//! `locora game`: the game progress (XP, learning days, the weekly goal, trophies) and deleting
//! it on every device. The numbers come from the gamification plugin and are the same on every
//! device that syncs.

use std::path::Path;

use anyhow::{Result, bail};
use clap::Subcommand;
use locora_api::gamification::Goal;
use locora_runtime::cloud::RankingsDeletion;

use crate::app::App;

#[derive(Subcommand)]
pub enum GameCommand {
    /// Spielfortschritt anzeigen: XP, heute fällig, Woche, Trophäen
    Status,
    /// Spielfortschritt auf allen Geräten löschen (XP, Trophäen, Medaillen, Lerntage, Wochenserien).
    /// Paläste, Wiederholungsplan, Statistik und Einstellungen bleiben.
    Reset {
        /// Nicht nachfragen
        #[arg(long)]
        yes: bool,
    },
    /// Wochenziel: an so vielen Tagen pro Woche lernen (1–7, ab dieser Woche)
    WeekGoal { days: u8 },
}

pub fn run(app: &mut App, data: Option<&Path>, command: GameCommand) -> Result<()> {
    match command {
        GameCommand::Status => status(app),
        GameCommand::Reset { yes } => reset(app, data, yes),
        GameCommand::WeekGoal { days } => {
            let goal = app.set_week_goal(days)?;
            app.report();
            println!("✓ Wochenziel gespeichert: an {goal} Tagen pro Woche lernen");
            Ok(())
        }
    }
}

fn status(app: &mut App) -> Result<()> {
    let Some(game) = app.gamification_status(true)? else {
        bail!("Dafür braucht es das Plugin `gamification` (Erfolge)");
    };
    app.report();
    println!("XP:                    {}", game.xp);
    println!("{}", today_line(&game.today));
    println!(
        "Diese Woche:           {} von {} Lerntagen · {} Lerntage insgesamt",
        game.week.days, game.week.goal, game.learning_days
    );
    if game.week.streak > 0 {
        println!(
            "Wochenziel:            {} Wochen in Folge erreicht",
            game.week.streak
        );
    } else if game.week.missed_last {
        println!(
            "Wochenziel:            Neue Woche, neues Ziel. Dein Bestwert: {} Wochen in Folge.",
            game.week.best
        );
    }
    let trophies: Vec<String> = game.trophies.iter().map(|t| t.id.clone()).collect();
    println!(
        "Trophäen:              {}",
        if trophies.is_empty() {
            "–".to_owned()
        } else {
            trophies.join(", ")
        }
    );
    let medals = game
        .palace_medals
        .iter()
        .filter(|m| m.unlocked.is_some())
        .count();
    println!("Palastmedaillen:       {medals}");
    Ok(())
}

/// Today's due goal in one line.
pub fn today_line(today: &locora_api::gamification::Today) -> String {
    let text = match today.goal {
        Goal::Open => format!(
            "{} von {} erledigt",
            today.due_done,
            today.due_done + today.due_open
        ),
        Goal::AllDone => "alle fälligen Wiederholungen erledigt".to_owned(),
        Goal::Free => "nichts fällig – ein freier Tag".to_owned(),
        Goal::Enough => format!(
            "{} fällige Wiederholungen erledigt – der Rest kann warten",
            today.due_done
        ),
    };
    format!("Heute fällig:          {text}")
}

fn reset(app: &mut App, data: Option<&Path>, yes: bool) -> Result<()> {
    if !yes
        && !crate::palace::confirm(
            "Spielfortschritt auf allen Geräten löschen? Paläste und Wiederholungsplan bleiben.",
        )?
    {
        println!("Nichts gelöscht.");
        return Ok(());
    }
    app.reset_game()?;
    app.report();
    println!("✓ Spielfortschritt gelöscht.");
    println!("{}", rankings_text(rankings(data)?));
    Ok(())
}

/// Asks the server to delete the rankings when an account is signed in; without an answer the
/// request waits for the next sync.
fn rankings(data: Option<&Path>) -> Result<RankingsDeletion> {
    locora_runtime::cloud::delete_rankings(&locora_runtime::data_dir(data)?)
}

pub fn rankings_text(outcome: RankingsDeletion) -> &'static str {
    match outcome {
        RankingsDeletion::NotSignedIn => {
            "Dieses Gerät ist nicht bei Locora Cloud angemeldet; Ranglisteneinträge auf dem Server bleiben unberührt."
        }
        RankingsDeletion::Deleted => "Ranglisteneinträge vom Server gelöscht.",
        RankingsDeletion::ServerTooOld => {
            "Der Server kann Ranglisteneinträge noch nicht löschen; sie bleiben, bis er aktualisiert ist. Dein Spielfortschritt ist trotzdem gelöscht."
        }
        RankingsDeletion::Pending => {
            "Der Server war nicht erreichbar. Die Ranglisteneinträge werden beim nächsten Sync gelöscht."
        }
        RankingsDeletion::SignInAgain => {
            "Melde dich wieder bei Locora Cloud an, um deine Ranglisteneinträge zu löschen."
        }
    }
}
