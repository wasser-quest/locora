//! `locora cloud …`: sign in to Locora Cloud and check the subscription offline.

use std::io::{self, BufRead, IsTerminal};
use std::path::Path;

use anyhow::{Result, bail};
use clap::Subcommand;
use locora_runtime::cloud::{self, CloudState};
use locora_runtime::cloud_sync;

#[derive(Subcommand)]
pub enum CloudCommand {
    /// Neues Konto bei Locora Cloud anlegen
    Register {
        email: String,
        /// Anzeigename in Ranglisten
        #[arg(long, default_value = "Lernende")]
        name: String,
        /// Adresse des Servers (sonst LOCORA_CLOUD_URL oder der zuletzt benutzte)
        #[arg(long)]
        server: Option<String>,
    },
    /// Anmelden; das Passwort wird abgefragt, nicht als Argument übergeben
    Login {
        email: String,
        #[arg(long)]
        server: Option<String>,
    },
    /// Konto, Abo und Freischaltung ohne Netz anzeigen
    Status,
    /// Freischaltung beim Server erneuern (nur mit aktivem Abo)
    Refresh,
    /// Änderungen mit Locora Cloud austauschen (Sync)
    Sync,
    /// Rangliste: alle Lernenden oder nur Freunde (Abo nötig)
    Ranking {
        #[arg(long)]
        friends: bool,
    },
    /// Freunde anzeigen oder anfragen (E-Mail-Adresse)
    Friend {
        /// E-Mail-Adresse der Person; ohne Angabe werden die Freunde gezeigt
        email: Option<String>,
    },
    /// Gemeinsame Challenges
    #[command(subcommand)]
    Challenge(ChallengeCommand),
    /// Lokale Journeys mit Kennung anzeigen
    Journeys,
    /// Abmelden und die lokale Sitzung löschen
    Logout,
}

#[derive(Subcommand)]
pub enum ChallengeCommand {
    /// Neue Challenge anlegen; du bist automatisch dabei
    Create {
        name: String,
        /// Tage bis zum Ende
        #[arg(long, default_value_t = 7)]
        days: u64,
    },
    /// Einer Challenge beitreten
    Join { id: String },
    /// Rangliste einer Challenge
    Ranking { id: String },
}

pub fn run(cmd: CloudCommand, data: Option<&Path>) -> Result<()> {
    let data = crate::app::data_dir(data)?;
    match cmd {
        CloudCommand::Register {
            email,
            name,
            server,
        } => {
            let server = server_or_default(server, &data)?;
            let password = read_password("Neues Passwort (mind. 10 Zeichen): ")?;
            let state = cloud::register(&server, &email, &password, &name)?;
            cloud::save(&data, &state)?;
            println!("✓ Konto angelegt. Angemeldet als {}.", state.account.email);
            print_status(&state);
            Ok(())
        }
        CloudCommand::Login { email, server } => {
            let server = server_or_default(server, &data)?;
            let password = read_password("Passwort: ")?;
            let state = cloud::login(&server, &email, &password)?;
            cloud::save(&data, &state)?;
            println!("✓ Angemeldet als {}.", state.account.email);
            print_status(&state);
            Ok(())
        }
        CloudCommand::Status => {
            let Some(state) = cloud::load(&data)? else {
                println!("Nicht bei Locora Cloud angemeldet.");
                return Ok(());
            };
            print_status(&state);
            Ok(())
        }
        CloudCommand::Refresh => {
            let Some(mut state) = cloud::load(&data)? else {
                bail!("Nicht angemeldet: zuerst `locora cloud login <E-Mail>`");
            };
            cloud::me(&mut state)?;
            cloud::refresh(&mut state)?;
            cloud::save(&data, &state)?;
            println!("✓ Freischaltung erneuert.");
            print_status(&state);
            Ok(())
        }
        CloudCommand::Sync => {
            let Some(state) = cloud::load(&data)? else {
                bail!("Nicht angemeldet: zuerst `locora cloud login <E-Mail>`");
            };
            let app = crate::app::App::open(Some(&data))?;
            let report = cloud_sync::sync(&state, &app.store)?;
            println!(
                "✓ Sync: {} gesendet, {} Medien hochgeladen, {} empfangen, {} Medien geladen",
                report.pushed, report.media_uploaded, report.pulled, report.media_downloaded
            );
            println!(
                "  angewendet: {}, schon bekannt: {}, Ort fehlt noch: {}",
                report.applied.applied, report.applied.stale, report.applied.missing
            );
            for problem in &report.applied.failed {
                println!("  ! nicht angewendet: {problem}");
            }
            Ok(())
        }
        CloudCommand::Ranking { friends } => {
            let state = signed_in(&data)?;
            let ranking = cloud::ranking(&state, friends)?;
            print_ranking(&ranking);
            Ok(())
        }
        CloudCommand::Friend { email: None } => {
            let state = signed_in(&data)?;
            let friends = cloud::friends(&state)?;
            if friends.is_empty() {
                println!("Noch keine Freunde. Hinzufügen: locora cloud friend <E-Mail>");
            }
            for friend in friends {
                let status = if friend.accepted {
                    ""
                } else {
                    " (Anfrage offen)"
                };
                println!("{}{status}", friend.display_name);
            }
            Ok(())
        }
        CloudCommand::Friend { email: Some(email) } => {
            let state = signed_in(&data)?;
            let friend = cloud::request_friend(&state, &email)?;
            if friend.accepted {
                println!("✓ Ihr seid jetzt befreundet: {}", friend.display_name);
            } else {
                println!("✓ Anfrage an {} gesendet.", friend.display_name);
            }
            Ok(())
        }
        CloudCommand::Challenge(command) => challenge(command, &data),
        CloudCommand::Journeys => {
            let app = crate::app::App::open(Some(&data))?;
            let journeys = app.store.journeys(20)?;
            if journeys.is_empty() {
                println!("Noch keine Journey abgeschlossen.");
            }
            for journey in journeys {
                let correct = journey.answers.iter().filter(|a| a.correct).count();
                println!(
                    "{}  {}  {} von {} richtig  {}",
                    journey.id,
                    journey.palace_name,
                    correct,
                    journey.answers.len(),
                    date(journey.recorded_at)
                );
            }
            Ok(())
        }
        CloudCommand::Logout => {
            let Some(state) = cloud::load(&data)? else {
                println!("Nicht angemeldet.");
                return Ok(());
            };
            cloud::logout(&data, &state)?;
            println!("✓ Abgemeldet.");
            Ok(())
        }
    }
}

fn signed_in(data: &Path) -> Result<CloudState> {
    match cloud::load(data)? {
        Some(state) => Ok(state),
        None => bail!("Nicht angemeldet: zuerst `locora cloud login <E-Mail>`"),
    }
}

fn challenge(command: ChallengeCommand, data: &Path) -> Result<()> {
    let state = signed_in(data)?;
    match command {
        ChallengeCommand::Create { name, days } => {
            let ends_at = cloud::now_secs() + days.clamp(1, 365) * 86_400;
            let challenge = cloud::create_challenge(&state, &name, ends_at)?;
            println!(
                "✓ Challenge „{}“ angelegt: {}",
                challenge.name, challenge.id
            );
            println!("  Mitmachen: locora cloud challenge join {}", challenge.id);
        }
        ChallengeCommand::Join { id } => {
            let challenge = cloud::join_challenge(&state, &id)?;
            println!(
                "✓ Dabei: „{}“ (bis {})",
                challenge.name,
                date(challenge.ends_at)
            );
        }
        ChallengeCommand::Ranking { id } => {
            print_ranking(&cloud::challenge_ranking(&state, &id)?);
        }
    }
    Ok(())
}

fn print_ranking(ranking: &locora_cloud_api::api::Ranking) {
    if ranking.entries.is_empty() {
        println!("Noch keine Einträge.");
    }
    for (place, entry) in ranking.entries.iter().enumerate() {
        println!(
            "{:>3}. {:<24} {} XP",
            place + 1,
            entry.display_name,
            entry.xp
        );
    }
}

fn server_or_default(server: Option<String>, data: &Path) -> Result<String> {
    if let Some(server) = server.or_else(|| std::env::var("LOCORA_CLOUD_URL").ok()) {
        return Ok(server);
    }
    match cloud::load(data)? {
        Some(state) => Ok(state.server),
        None => bail!("Kein Server angegeben: --server <URL> oder LOCORA_CLOUD_URL setzen"),
    }
}

/// Reads a password from the terminal without echo, or one line from a pipe.
fn read_password(prompt: &str) -> Result<String> {
    if io::stdin().is_terminal() {
        return Ok(rpassword::prompt_password(prompt)?);
    }
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

fn print_status(state: &CloudState) {
    let account = &state.account;
    println!("Konto: {} ({})", account.display_name, account.email);
    println!("Server: {}", state.server);
    match account.period_end {
        Some(end) if account.subscription != locora_cloud_api::api::SubscriptionState::None => {
            let state = match account.subscription {
                locora_cloud_api::api::SubscriptionState::Canceled => "gekündigt, läuft bis",
                _ => "aktiv bis",
            };
            println!("Abo: {} ({state} {})", plan_name(account.plan), date(end));
        }
        _ => println!("Abo: keines (Ranglisten, Freunde, Challenges und Sync brauchen ein Abo)"),
    }
    match cloud::access(state, cloud::now_secs()) {
        Some((claims, locora_cloud_api::entitlement::Access::Active)) => {
            println!("Freischaltung: gültig bis {}", date(claims.expires));
        }
        Some((claims, locora_cloud_api::entitlement::Access::Grace)) => {
            println!(
                "Freischaltung: abgelaufen am {}, Kulanzzeit ohne Netz bis {}",
                date(claims.expires),
                date(claims.expires + locora_cloud_api::OFFLINE_GRACE_SECS)
            );
        }
        None => println!("Freischaltung: keine gültige (bitte `locora cloud refresh`)"),
    }
}

fn plan_name(plan: Option<locora_cloud_api::Plan>) -> &'static str {
    match plan {
        Some(locora_cloud_api::Plan::Trial) => "Testphase",
        Some(locora_cloud_api::Plan::Monthly) => "Monatsabo",
        Some(locora_cloud_api::Plan::Annual) => "Jahresabo",
        None => "-",
    }
}

/// Date in UTC for a Unix timestamp (civil calendar, no time zone database needed).
fn date(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::date;

    #[test]
    fn unix_days_become_calendar_dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_800_000_000), "2027-01-15");
        assert_eq!(date(951_782_400), "2000-02-29");
    }
}
