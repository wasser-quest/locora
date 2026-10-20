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
        /// Adresse des Servers (Standard: LOCORA_CLOUD_URL, dann der zuletzt benutzte, sonst https://api.locora.cloud)
        #[arg(long)]
        server: Option<String>,
        /// Gastkonto: Daten bleiben sechs Monate, Zugang wie in einer Testphase; ein Abo übernimmt die Daten
        #[arg(long)]
        guest: bool,
    },
    /// Anmelden; das Passwort wird abgefragt, nicht als Argument übergeben
    Login {
        email: String,
        #[arg(long)]
        server: Option<String>,
    },
    /// Passwort des angemeldeten Kontos ändern; alle Geräte werden abgemeldet
    Password,
    /// Konto, Abo und Freischaltung ohne Netz anzeigen
    Status,
    /// Cloud-Aufbewahrungsfrist beim Server abfragen (verlängert sie nicht)
    Retention,
    /// Cloud-Bestand nach Ablauf aus diesem lokalen Gerät neu aufbauen
    Rebuild,
    /// Freischaltung beim Server erneuern (nur mit aktivem Abo)
    Refresh,
    /// Alle synchronisierten Daten (Änderungen und Medien) vom Server löschen; die Geräte behalten ihre
    Erase,
    /// Das Cloud-Konto mit allen Daten auf dem Server löschen (ein Abo beim Zahlungsanbieter bleibt)
    DeleteAccount,
    /// Änderungen mit Locora Cloud austauschen (Ende-zu-Ende-verschlüsselt, fragt das Verschlüsselungs-Passwort ab)
    Sync,
    /// Verschlüsselung einrichten oder wiederherstellen
    #[command(subcommand)]
    Encrypt(EncryptCommand),
    /// Den XP-Stand zeigen, den der Server berechnet hat
    Xp,
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

#[derive(Subcommand)]
pub enum EncryptCommand {
    /// Verschlüsselung für das Konto einrichten; zeigt den Wiederherstellungsschlüssel einmal
    Setup,
    /// Mit dem Wiederherstellungsschlüssel ein neues Verschlüsselungs-Passwort setzen
    Recover,
}

pub fn run(cmd: CloudCommand, data: Option<&Path>) -> Result<()> {
    let data = crate::app::data_dir(data)?;
    match cmd {
        CloudCommand::Register {
            email,
            name,
            server,
            guest,
        } => {
            let server = server_or_default(server, &data)?;
            let password = read_password("Neues Passwort (mind. 10 Zeichen): ")?;
            let state = cloud::register(&server, &email, &password, &name, guest)?;
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
        CloudCommand::Password => {
            let state = signed_in(&data)?;
            let current = read_password("Bisheriges Cloud-Passwort: ")?;
            let new = read_password("Neues Cloud-Passwort (mind. 10 Zeichen): ")?;
            let confirmation = read_password("Neues Cloud-Passwort wiederholen: ")?;
            if new != confirmation {
                bail!("Die neuen Passwörter stimmen nicht überein");
            }
            cloud::change_password(&state, &current, &new)?;
            cloud::remove(&data)?;
            println!(
                "✓ Cloud-Passwort geändert. Alle Geräte wurden abgemeldet; bitte erneut anmelden."
            );
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
            let state = signed_in(&data)?;
            let bundle = cloud::get_keys(&state)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "Die Verschlüsselung ist noch nicht eingerichtet: `locora cloud encrypt setup`"
                )
            })?;
            let passphrase = read_password("Verschlüsselungs-Passwort: ")?;
            locora_crypto::unlock(&bundle, &passphrase).map_err(|e| anyhow::anyhow!("{e}"))?;
            let app = crate::app::App::open(Some(&data))?;
            // Also starts over with a new generation of the server (deleted or expired cloud data).
            let report = cloud_sync::sync_with_passphrase(&data, &app.store, &passphrase)?;
            if report.new_generation.is_some() {
                println!("  Neuer Cloud-Stand: alle Daten dieses Geräts werden gesendet.");
            }
            println!(
                "✓ Sync: {} gesendet, {} Medien hochgeladen, {} empfangen, {} Medien geladen",
                report.pushed, report.media_uploaded, report.pulled, report.media_downloaded
            );
            println!(
                "  angewendet: {}, schon bekannt: {}, Ort fehlt noch: {}",
                report.applied.applied, report.applied.stale, report.applied.missing
            );
            if let Some(expires_at) = report.cloud_expires_at {
                println!(
                    "  Cloud-Daten bleiben bis {} erhalten.",
                    date(expires_at as u64)
                );
            }
            for problem in &report.applied.failed {
                println!("  ! nicht angewendet: {problem}");
            }
            for problem in &report.quarantined {
                println!("  ! in Quarantäne: {problem}");
            }
            Ok(())
        }
        CloudCommand::Retention => {
            let state = signed_in(&data)?;
            let retention = cloud::retention(&state)?;
            println!("Cloud-Status: {}", retention.state);
            println!(
                "Letzter erfolgreicher Sync: {}",
                date(retention.last_successful_sync_at as u64)
            );
            println!("Cloud-Löschung ab: {}", date(retention.expires_at as u64));
            println!("Sync-Generation: {}", retention.generation);
            Ok(())
        }
        CloudCommand::Erase => {
            let mut state = signed_in(&data)?;
            if !confirm("Alle synchronisierten Daten auf dem Server löschen? Tippe „löschen“: ")? {
                bail!("Abgebrochen");
            }
            let remote = cloud::erase(&state)?;
            let app = crate::app::App::open(Some(&data))?;
            cloud::adopt_generation(&mut state, &app.store, remote.generation)?;
            cloud::save(&data, &state)?;
            println!("✓ Cloud-Daten gelöscht. Die Daten auf diesem Gerät bleiben.");
            println!("  Andere angemeldete Geräte senden ihre Daten beim nächsten Sync wieder.");
            Ok(())
        }
        CloudCommand::DeleteAccount => {
            let state = signed_in(&data)?;
            if !confirm("Das Cloud-Konto mit allen Daten löschen? Tippe „löschen“: ")? {
                bail!("Abgebrochen");
            }
            let password = read_password("Cloud-Passwort: ")?;
            cloud::delete_account(&state, &password)?;
            cloud::remove(&data)?;
            println!("✓ Cloud-Konto gelöscht. Die Daten auf diesem Gerät bleiben.");
            println!("  Ein laufendes Abo beim Zahlungsanbieter bitte dort kündigen.");
            Ok(())
        }
        CloudCommand::Rebuild => {
            let mut state = signed_in(&data)?;
            let remote = cloud::retention(&state)?;
            match remote.state.as_str() {
                "expired" => {
                    cloud::begin_rebuild(&state, remote.generation)?;
                }
                "rebuilding" => {}
                "active" if remote.generation != state.sync_generation => {}
                _ => bail!("Kein abgelaufener Cloud-Bestand zum Neuaufbau vorhanden"),
            }
            let app = crate::app::App::open(Some(&data))?;
            app.store.set_sync_cursor("push", 0)?;
            app.store.set_sync_cursor("pull", 0)?;
            state.sync_generation = remote.generation;
            cloud::save(&data, &state)?;
            println!(
                "Sync-Cursor zurückgesetzt. Jetzt `locora cloud sync` mit vollständigen lokalen Daten ausführen."
            );
            Ok(())
        }
        CloudCommand::Encrypt(EncryptCommand::Setup) => {
            let state = signed_in(&data)?;
            if cloud::get_keys(&state)?.is_some() {
                bail!("Die Verschlüsselung ist schon eingerichtet");
            }
            let first = read_password("Neues Verschlüsselungs-Passwort (mind. 12 Zeichen): ")?;
            let again = read_password("Wiederholen: ")?;
            if first != again {
                bail!("Die Passwörter stimmen nicht überein");
            }
            let account = locora_crypto::create(&first).map_err(|e| anyhow::anyhow!("{e}"))?;
            cloud::create_keys(&state, &account.bundle)?;
            println!("✓ Verschlüsselung eingerichtet.");
            println!();
            println!("  Wiederherstellungsschlüssel (nur jetzt angezeigt, sicher aufbewahren):");
            println!("  {}", account.recovery_key);
            println!();
            println!(
                "  Ohne Passwort und ohne diesen Schlüssel sind die synchronisierten Daten nicht mehr lesbar."
            );
            Ok(())
        }
        CloudCommand::Encrypt(EncryptCommand::Recover) => {
            let state = signed_in(&data)?;
            let bundle = cloud::get_keys(&state)?.ok_or_else(|| {
                anyhow::anyhow!("Die Verschlüsselung ist noch nicht eingerichtet")
            })?;
            let recovery = read_password("Wiederherstellungsschlüssel: ")?;
            let dek = locora_crypto::unlock_with_recovery(&bundle, &recovery)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let first = read_password("Neues Verschlüsselungs-Passwort (mind. 12 Zeichen): ")?;
            let again = read_password("Wiederholen: ")?;
            if first != again {
                bail!("Die Passwörter stimmen nicht überein");
            }
            let new_bundle = locora_crypto::rewrap(&dek, &recovery, &first)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            cloud::replace_keys(&state, &new_bundle)?;
            println!(
                "✓ Neues Verschlüsselungs-Passwort gesetzt. Der Wiederherstellungsschlüssel bleibt gültig."
            );
            Ok(())
        }
        CloudCommand::Xp => {
            let state = signed_in(&data)?;
            let xp = cloud::xp(&state)?;
            println!("XP auf dem Server: {}", xp.xp_total);
            println!(
                "Abgeschlossene Journeys: {} · offen: {}",
                xp.journeys_finished, xp.journeys_open
            );
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

/// The cloud session for timed journeys, if the account has a subscription.
/// Practice then reports each answer to the server. Offline or without a subscription
/// the journeys are local only, and a challenge is refused.
pub fn live_session(
    data: &Path,
    challenge: Option<String>,
) -> Result<Option<crate::practice::Live>> {
    let state = match cloud::load(data)? {
        Some(state)
            if state.account.subscription != locora_cloud_api::api::SubscriptionState::None =>
        {
            state
        }
        _ if challenge.is_some() => {
            bail!("Challenges brauchen ein Abo und eine Anmeldung: `locora cloud login`")
        }
        _ => return Ok(None),
    };
    if cloud::access(&state, cloud::now_secs()).is_none() {
        if challenge.is_some() {
            bail!("Keine gültige Freischaltung: `locora cloud refresh` ausführen");
        }
        println!("(Freischaltung abgelaufen: diese Journeys werden nicht auf dem Server gewertet)");
        return Ok(None);
    }
    Ok(Some(crate::practice::Live { state, challenge }))
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
        None => Ok(locora_runtime::cloud::DEFAULT_SERVER.to_owned()),
    }
}

/// Reads a password from the terminal without echo, or one line from a pipe.
/// Asks for the word „löschen“ before something is deleted.
fn confirm(prompt: &str) -> Result<bool> {
    use std::io::Write;
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim() == "löschen")
}

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
    if account.email.is_empty() {
        println!("Konto: {} (ohne E-Mail-Adresse)", account.display_name);
    } else {
        println!("Konto: {} ({})", account.display_name, account.email);
    }
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
