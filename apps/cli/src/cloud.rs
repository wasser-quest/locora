//! `locora cloud …`: sign in to Locora Cloud and check the subscription offline.

use std::io::{self, BufRead, IsTerminal};
use std::path::Path;

use anyhow::{Result, bail};
use clap::Subcommand;
use locora_cloud_api::api::{Friend, FriendInvite, Ranking, parse_invite_code};
use locora_runtime::cloud::{self, CloudState};
use locora_runtime::{cloud_http, cloud_sync};

use crate::shown;

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
    /// Die Ranglistenpunkte zeigen, die der Server berechnet hat
    Xp,
    /// Rangliste: alle Lernenden oder nur Freunde (Abo nötig); neben jedem Namen sein Verweis zum
    /// Melden
    Ranking {
        #[arg(long)]
        friends: bool,
    },
    /// Einen anstößigen oder seltsamen Namen melden. Nach Meldungen mehrerer Konten mit bestätigter
    /// Adresse sehen andere ihn als „Nutzer-…“, bis er geprüft ist
    Report {
        /// Verweis neben dem Namen in einer Rangliste (nr_…) oder Kennung aus `locora cloud friend`
        #[arg(value_name = "VERWEIS")]
        reference: String,
        /// Grund in wenigen Worten (höchstens 300 Zeichen)
        #[arg(long, value_name = "TEXT")]
        reason: Option<String>,
    },
    /// Freunde und offene Anfragen anzeigen, jemanden anfragen (E-Mail-Adresse) oder per
    /// Einladungslink einladen, Freunde entfernen und Anfragen zurückziehen oder ablehnen
    Friend {
        /// E-Mail-Adresse der Person; ohne Angabe werden Freunde und offene Anfragen gezeigt
        email: Option<String>,
        #[command(subcommand)]
        action: Option<FriendCommand>,
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
pub enum FriendCommand {
    /// Eine Freundschaft beenden (für beide Seiten)
    Remove {
        /// Kennung aus `locora cloud friend`
        id: String,
    },
    /// Eine eigene Anfrage zurückziehen
    Withdraw {
        /// Kennung aus `locora cloud friend`
        id: String,
    },
    /// Eine Anfrage an dich ablehnen; die Person kann dich dann erst wieder anfragen, wenn du sie anfragst
    Decline {
        /// Kennung aus `locora cloud friend`
        id: String,
    },
    /// Einladungslink erstellen (Abo nötig): Wer ihn annimmt, ist mit dir befreundet. Er gilt
    /// einmal, 14 Tage lang
    Invite {
        #[command(subcommand)]
        action: Option<InviteCommand>,
    },
    /// Die Einladung einer anderen Person annehmen (Abo nötig); ihr seid dann befreundet
    AcceptInvite {
        /// Code (ABCD-EFGH-JKLM-NPQR) oder der ganze Link
        #[arg(value_name = "CODE_ODER_LINK")]
        code: String,
    },
}

#[derive(Subcommand)]
pub enum InviteCommand {
    /// Die offenen Einladungslinks zeigen
    List,
    /// Einen offenen Einladungslink zurückziehen; er funktioniert dann nicht mehr
    Revoke {
        /// Code (ABCD-EFGH-JKLM-NPQR) oder der ganze Link
        #[arg(value_name = "CODE_ODER_LINK")]
        code: String,
    },
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
            println!(
                "✓ Konto angelegt. Angemeldet als {}.",
                shown::address(&state.account.email)
            );
            print_status(&state);
            Ok(())
        }
        CloudCommand::Login { email, server } => {
            let server = server_or_default(server, &data)?;
            let password = read_password("Passwort: ")?;
            let state = cloud::login(&server, &email, &password)?;
            cloud::save(&data, &state)?;
            println!("✓ Angemeldet als {}.", shown::address(&state.account.email));
            print_status(&state);
            retry_rankings(&data);
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
            retry_rankings(&data);
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
            if cloud::get_keys(&state)?.is_none() {
                bail!(
                    "Die Verschlüsselung ist noch nicht eingerichtet: `locora cloud encrypt setup`"
                );
            }
            // The sync unlocks the key itself (Argon2 runs once); a wrong password stops it there.
            let passphrase = read_password("Verschlüsselungs-Passwort: ")?;
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
            if report.compacted > 0 {
                println!(
                    "  {} überholte Änderungen nicht gesendet (spätere ersetzen sie)",
                    report.compacted
                );
            }
            if report.too_large > 0 {
                println!(
                    "  ! {} Änderungen sind zu groß für die Cloud und bleiben auf diesem Gerät",
                    report.too_large
                );
            }
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
            if !confirm("Alle synchronisierten Daten auf dem Server löschen? Tippe „löschen“: ")?
            {
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
            cloud::create_keys(&state, &account.bundle, &account.dek)?;
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
            // With the key proof: a server that has one replaces the bundle only for a holder of
            // the key.
            cloud::replace_keys(&state, &new_bundle, &dek)?;
            println!(
                "✓ Neues Verschlüsselungs-Passwort gesetzt. Der Wiederherstellungsschlüssel bleibt gültig."
            );
            Ok(())
        }
        CloudCommand::Xp => {
            let state = signed_in(&data)?;
            let xp = cloud::xp(&state)?;
            println!("Ranglistenpunkte auf dem Server: {}", xp.xp_total);
            println!(
                "Abgeschlossene Journeys: {} · offen: {}",
                xp.journeys_finished, xp.journeys_open
            );
            Ok(())
        }
        CloudCommand::Ranking { friends } => {
            let state = signed_in(&data)?;
            let ranking = cloud::ranking(&state, friends)?;
            print_lines(&ranking_lines(&ranking));
            Ok(())
        }
        CloudCommand::Report { reference, reason } => {
            let mut state = signed_in(&data)?;
            cloud::report_name(&state, &reference, reason.as_deref())?;
            println!("✓ Gemeldet. Danke, gemeldete Namen werden geprüft.");
            // Only reports of confirmed addresses count; the stored account may be older.
            if state.account.email_verified == Some(false) && cloud::me(&mut state).is_ok() {
                cloud::save(&data, &state)?;
            }
            if state.account.email_verified == Some(false) {
                println!("  Deine Meldung zählt, sobald deine E-Mail-Adresse bestätigt ist.");
            }
            Ok(())
        }
        CloudCommand::Friend {
            email: None,
            action: None,
        } => {
            let state = signed_in(&data)?;
            print_lines(&friend_lines(&cloud::friends(&state)?));
            Ok(())
        }
        // `--data` may stand before the command, so clap lets an address and a command come
        // together; one of them is meant.
        CloudCommand::Friend {
            email: Some(email),
            action: Some(_),
        } => bail!(
            "Entweder eine E-Mail-Adresse anfragen oder einen Befehl wie remove oder invite, nicht beides ({})",
            shown::address(&email)
        ),
        CloudCommand::Friend {
            email: Some(email),
            action: None,
        } => {
            let state = signed_in(&data)?;
            let friend = cloud::request_friend(&state, &email)?;
            if friend.accepted {
                println!(
                    "✓ Ihr seid jetzt befreundet: {}",
                    shown::name(&friend.display_name)
                );
            } else {
                // A newer server names nobody before they answer, an older one the account asked.
                let asked = if friend.display_name.is_empty() {
                    shown::address(&email)
                } else {
                    shown::name(&friend.display_name)
                };
                println!("✓ Anfrage an {asked} gesendet.");
                println!(
                    "  Angenommen ist sie, sobald die Person dich auch anfragt. Offene Anfragen: locora cloud friend"
                );
                println!(
                    "  Schneller geht es mit einem Einladungslink: locora cloud friend invite"
                );
            }
            Ok(())
        }
        CloudCommand::Friend {
            email: None,
            action: Some(action),
        } => friend_action(&signed_in(&data)?, action),
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
            let id = shown::address(&challenge.id);
            println!(
                "✓ Challenge „{}“ angelegt: {id}",
                shown::name(&challenge.name)
            );
            println!("  Mitmachen: locora cloud challenge join {id}");
        }
        ChallengeCommand::Join { id } => {
            let challenge = cloud::join_challenge(&state, &id)?;
            println!(
                "✓ Dabei: „{}“ (bis {})",
                shown::name(&challenge.name),
                date(challenge.ends_at)
            );
        }
        ChallengeCommand::Ranking { id } => {
            print_lines(&ranking_lines(&cloud::challenge_ranking(&state, &id)?));
        }
    }
    Ok(())
}

/// What an entry of the friend list is. A newer server sends an id with every entry and, with a
/// request this account sent, no name but the time it was sent (`since`); a server of the 0.1.6
/// test builds sent the address asked instead, which is ignored now. An older one sends neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    Friend,
    /// A request to this account.
    Incoming,
    /// A request (or an invite to an address without an account) this account sent.
    Outgoing,
    /// A request listed by an older server, which does not tell who sent it.
    Open,
}

fn entry(friend: &Friend) -> Entry {
    match (friend.accepted, friend.since, &friend.id) {
        (true, ..) => Entry::Friend,
        (false, Some(_), _) => Entry::Outgoing,
        // A request received always shows its sender's name.
        (false, None, Some(_)) if friend.display_name.is_empty() => Entry::Outgoing,
        (false, None, Some(_)) => Entry::Incoming,
        (false, None, None) => Entry::Open,
    }
}

/// Who an entry is: the name, or for a request this account sent when it was sent (the server
/// names nobody before they answer).
fn who(friend: &Friend) -> String {
    if entry(friend) != Entry::Outgoing {
        return shown::name(&friend.display_name);
    }
    match friend.since {
        Some(since) => format!("Anfrage gesendet {}", date(since)),
        None => "Anfrage gesendet".into(),
    }
}

/// The friend list: friends, requests to this account, requests it sent, each with its id and
/// the command that answers it. An older server lists names alone, as before.
fn friend_lines(friends: &[Friend]) -> Vec<String> {
    if friends.is_empty() {
        return vec![
            "Noch keine Freunde. Hinzufügen: locora cloud friend <E-Mail> oder mit einem Einladungslink: locora cloud friend invite".into(),
        ];
    }
    let with_ids = friends.iter().any(|friend| friend.id.is_some());
    let sections = [
        (
            Entry::Friend,
            "Freunde",
            " (beenden: locora cloud friend remove <Kennung>)",
        ),
        (
            Entry::Incoming,
            "Anfragen an dich",
            " (annehmen: locora cloud friend <E-Mail-Adresse der Person>; ablehnen: locora cloud friend decline <Kennung>)",
        ),
        (
            Entry::Outgoing,
            "Deine Anfragen, noch nicht angenommen",
            " (zurückziehen: locora cloud friend withdraw <Kennung>)",
        ),
        (Entry::Open, "Offene Anfragen", ""),
    ];
    let mut lines = Vec::new();
    for (kind, heading, commands) in sections {
        let entries: Vec<&Friend> = friends.iter().filter(|f| entry(f) == kind).collect();
        if entries.is_empty() {
            continue;
        }
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(format!(
            "{heading}{}:",
            if with_ids { commands } else { "" }
        ));
        for friend in entries {
            lines.push(match &friend.id {
                Some(id) => format!("  {:<32}  {}", who(friend), shown::address(id)),
                None => format!("  {}", who(friend)),
            });
        }
    }
    // The id of a friend or of a request to this account is the reference of the name shown.
    let named = |friend: &&Friend| matches!(entry(friend), Entry::Friend | Entry::Incoming);
    if friends
        .iter()
        .filter(named)
        .any(|friend| friend.id.is_some())
    {
        lines.push(String::new());
        lines.push("Einen Namen melden: locora cloud report <Kennung>".into());
    }
    lines
}

/// The commands of `locora cloud friend`: the invite links, or an answer to an entry of the list.
fn friend_action(state: &CloudState, action: FriendCommand) -> Result<()> {
    let (id, wanted) = match &action {
        FriendCommand::Remove { id } => (id.as_str(), Entry::Friend),
        FriendCommand::Withdraw { id } => (id.as_str(), Entry::Outgoing),
        FriendCommand::Decline { id } => (id.as_str(), Entry::Incoming),
        FriendCommand::Invite { action: None } => {
            print_lines(&created_lines(&cloud::create_invite(state)?));
            return Ok(());
        }
        FriendCommand::Invite {
            action: Some(InviteCommand::List),
        } => {
            print_lines(&invite_lines(&cloud::invites(state)?));
            return Ok(());
        }
        FriendCommand::Invite {
            action: Some(InviteCommand::Revoke { code }),
        } => {
            let code = invite_code(code)?;
            cloud::revoke_invite(state, &code)?;
            println!("✓ Einladung {code} zurückgezogen. Der Link funktioniert nicht mehr.");
            return Ok(());
        }
        FriendCommand::AcceptInvite { code } => {
            let friend = cloud::accept_invite(state, &invite_code(code)?)?;
            println!(
                "✓ Ihr seid jetzt befreundet: {}",
                shown::name(&friend.display_name)
            );
            return Ok(());
        }
    };
    answer_friend(state, id, wanted)
}

/// Ends a friendship, withdraws a request or declines one. The list says first what the id is,
/// so a friend's id given to `decline` is refused instead of ending the friendship, and the answer
/// can name the person. Without a subscription the list is refused (402) but saying no still
/// works; the server alone decides then.
fn answer_friend(state: &CloudState, id: &str, wanted: Entry) -> Result<()> {
    let found = match cloud::friends(state) {
        Ok(friends) => friends
            .into_iter()
            .find(|friend| friend.id.as_deref() == Some(id)),
        Err(error) if cloud_http::status_of(&error) == Some(402) => None,
        Err(error) => return Err(error),
    };
    if let Some(friend) = &found {
        let kind = entry(friend);
        if kind != wanted {
            bail!("{}", other_kind(kind, id));
        }
    }
    cloud::remove_friend(state, id)?;
    print_lines(&answered(wanted, found.as_ref()));
    Ok(())
}

/// What `answer_friend` says when it is done; `found` is the entry of the list, if it was read.
fn answered(wanted: Entry, found: Option<&Friend>) -> Vec<String> {
    let name = found.map(|friend| shown::name(&friend.display_name));
    match (wanted, name) {
        (Entry::Friend, Some(name)) => vec![format!("✓ Freundschaft mit {name} beendet.")],
        (Entry::Friend, None) => vec!["✓ Freundschaft beendet.".into()],
        (Entry::Incoming, Some(name)) => vec![
            format!("✓ Anfrage von {name} abgelehnt."),
            "  Neue Anfragen von dieser Person kommen erst wieder an, wenn du sie anfragst.".into(),
        ],
        (Entry::Incoming, None) => vec!["✓ Anfrage abgelehnt.".into()],
        // A request this account sent names nobody, only when it was sent.
        _ => match found.and_then(|friend| friend.since) {
            Some(since) => vec![format!("✓ Anfrage vom {} zurückgezogen.", date(since))],
            None => vec!["✓ Anfrage zurückgezogen.".into()],
        },
    }
}

/// The code in a code or link the user gave, as the server reads it.
fn invite_code(input: &str) -> Result<String> {
    match parse_invite_code(input) {
        Some(code) => Ok(code),
        None => bail!(
            "„{}“ ist kein Einladungscode (wie ABCD-EFGH-JKLM-NPQR) und kein Einladungslink",
            shown::address(input)
        ),
    }
}

/// What `locora cloud friend invite` shows for a new link.
fn created_lines(invite: &FriendInvite) -> Vec<String> {
    let code = shown::address(&invite.code);
    vec![
        format!(
            "✓ Einladungslink erstellt, einmal gültig bis {}:",
            date(invite.expires_at)
        ),
        format!("  {}", shown::address(&invite.url)),
        format!("  Code: {code}"),
        "  Schick ihn der Person, die du einladen möchtest. Wer ihn annimmt, ist mit dir befreundet."
            .into(),
        format!("  In der CLI annehmen: locora cloud friend accept-invite {code}"),
        format!("  Zurückziehen: locora cloud friend invite revoke {code}"),
    ]
}

/// The open invite links, oldest first.
fn invite_lines(invites: &[FriendInvite]) -> Vec<String> {
    if invites.is_empty() {
        return vec![
            "Keine offenen Einladungslinks. Neuer Link: locora cloud friend invite".into(),
        ];
    }
    let mut lines = vec![
        "Offene Einladungslinks (zurückziehen: locora cloud friend invite revoke <Code>):".into(),
    ];
    for invite in invites {
        lines.push(format!(
            "  {}  bis {}  {}",
            shown::address(&invite.code),
            date(invite.expires_at),
            shown::address(&invite.url)
        ));
    }
    lines
}

/// The refusal for an id given to the command of another kind of entry, with the right command.
fn other_kind(kind: Entry, id: &str) -> String {
    let id = shown::address(id);
    match kind {
        Entry::Friend => {
            format!("{id} ist eine Freundschaft. Beenden: locora cloud friend remove {id}")
        }
        Entry::Incoming => {
            format!("{id} ist eine Anfrage an dich. Ablehnen: locora cloud friend decline {id}")
        }
        Entry::Outgoing => {
            format!(
                "{id} ist deine eigene Anfrage. Zurückziehen: locora cloud friend withdraw {id}"
            )
        }
        Entry::Open => format!("{id} ist eine offene Anfrage"),
    }
}

fn print_lines(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}

/// A ranking, each name with its reference for `locora cloud report` (an older server sends none).
fn ranking_lines(ranking: &Ranking) -> Vec<String> {
    if ranking.entries.is_empty() {
        return vec!["Noch keine Einträge.".into()];
    }
    let mut lines: Vec<String> = ranking
        .entries
        .iter()
        .enumerate()
        .map(|(place, entry)| {
            let line = format!(
                "{:>3}. {:<24} {:>7} Ranglistenpunkte",
                place + 1,
                shown::name(&entry.display_name),
                entry.xp
            );
            match &entry.reference {
                Some(reference) => format!("{line}  {}", shown::address(reference)),
                None => line,
            }
        })
        .collect();
    if ranking
        .entries
        .iter()
        .any(|entry| entry.reference.is_some())
    {
        lines.push(String::new());
        lines.push("Einen Namen melden: locora cloud report <Verweis>".into());
    }
    lines
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
    let name = shown::name(&account.display_name);
    if account.email.is_empty() {
        println!("Konto: {name} (ohne E-Mail-Adresse)");
    } else {
        println!("Konto: {name} ({})", shown::address(&account.email));
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

/// Asks the server again to delete the game scores when that still waits (see `locora game
/// reset`), and says what happened.
fn retry_rankings(data: &std::path::Path) {
    if let Ok(Some(outcome)) = cloud::retry_rankings(data) {
        println!("{}", crate::game::rankings_text(outcome));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CloudCommand, Entry, Friend, FriendCommand, FriendInvite, InviteCommand, Ranking, answered,
        created_lines, date, entry, friend_lines, invite_code, invite_lines, other_kind,
        ranking_lines,
    };
    use locora_cloud_api::api::RankingEntry;

    fn friend(name: &str, accepted: bool, id: Option<&str>, since: Option<u64>) -> Friend {
        Friend {
            display_name: name.into(),
            accepted,
            id: id.map(Into::into),
            since,
        }
    }

    #[test]
    fn the_friend_list_shows_ids_and_when_a_request_was_sent() {
        let list = [
            friend("Ada", true, Some("fr_1"), None),
            friend("Carol", false, Some("fr_2"), None),
            friend("", false, Some("fr_3"), Some(1_700_000_000)),
        ];
        assert_eq!(
            [entry(&list[0]), entry(&list[1]), entry(&list[2])],
            [Entry::Friend, Entry::Incoming, Entry::Outgoing]
        );
        let lines = friend_lines(&list);
        assert_eq!(
            lines,
            [
                "Freunde (beenden: locora cloud friend remove <Kennung>):".to_owned(),
                format!("  {:<32}  fr_1", "Ada"),
                String::new(),
                "Anfragen an dich (annehmen: locora cloud friend <E-Mail-Adresse der Person>; ablehnen: locora cloud friend decline <Kennung>):".into(),
                format!("  {:<32}  fr_2", "Carol"),
                String::new(),
                "Deine Anfragen, noch nicht angenommen (zurückziehen: locora cloud friend withdraw <Kennung>):".into(),
                format!("  {:<32}  fr_3", "Anfrage gesendet 2023-11-14"),
                String::new(),
                "Einen Namen melden: locora cloud report <Kennung>".into(),
            ]
        );
        // A server of the 0.1.6 test builds sent no time, but no name either.
        let between = friend("", false, Some("fr_4"), None);
        assert_eq!(entry(&between), Entry::Outgoing);
        assert_eq!(
            friend_lines(&[between]),
            [
                "Deine Anfragen, noch nicht angenommen (zurückziehen: locora cloud friend withdraw <Kennung>):".to_owned(),
                format!("  {:<32}  fr_4", "Anfrage gesendet"),
            ],
            "nothing to report among requests sent alone"
        );
        assert_eq!(
            friend_lines(&[]),
            [
                "Noch keine Freunde. Hinzufügen: locora cloud friend <E-Mail> oder mit einem Einladungslink: locora cloud friend invite"
            ]
        );
    }

    #[test]
    fn a_request_withdrawn_is_named_by_when_it_was_sent() {
        let sent = friend("", false, Some("fr_3"), Some(1_700_000_000));
        assert_eq!(
            answered(Entry::Outgoing, Some(&sent)),
            ["✓ Anfrage vom 2023-11-14 zurückgezogen."]
        );
        let between = friend("", false, Some("fr_4"), None);
        assert_eq!(
            answered(Entry::Outgoing, Some(&between)),
            ["✓ Anfrage zurückgezogen."]
        );
        assert_eq!(
            answered(Entry::Outgoing, None),
            ["✓ Anfrage zurückgezogen."]
        );
        let ada = friend("Ada", true, Some("fr_1"), None);
        assert_eq!(
            answered(Entry::Friend, Some(&ada)),
            ["✓ Freundschaft mit Ada beendet."]
        );
        let carol = friend("Carol\u{1b}[2J", false, Some("fr_2"), None);
        assert_eq!(
            answered(Entry::Incoming, Some(&carol))[0],
            "✓ Anfrage von Carol\\u{1b}[2J abgelehnt."
        );
    }

    fn ranked(name: &str, xp: u64, reference: Option<&str>) -> RankingEntry {
        RankingEntry {
            display_name: name.into(),
            xp,
            reference: reference.map(Into::into),
        }
    }

    #[test]
    fn a_ranking_shows_the_reference_of_each_name_to_report_it() {
        let ranking = Ranking {
            entries: vec![
                ranked("Ada", 1200, Some("nr_0a")),
                ranked("Nutzer-3F2A", 80, Some("nr_1b\u{1b}[2J")),
            ],
        };
        assert_eq!(
            ranking_lines(&ranking),
            [
                format!("  1. {:<24} {:>7} Ranglistenpunkte  nr_0a", "Ada", 1200),
                format!(
                    "  2. {:<24} {:>7} Ranglistenpunkte  nr_1b\\u{{1b}}[2J",
                    "Nutzer-3F2A", 80
                ),
                String::new(),
                "Einen Namen melden: locora cloud report <Verweis>".into(),
            ]
        );
        // An older server sends no references: nothing to report with.
        let older = Ranking {
            entries: vec![ranked("Ada", 1200, None)],
        };
        assert_eq!(
            ranking_lines(&older),
            [format!("  1. {:<24} {:>7} Ranglistenpunkte", "Ada", 1200)]
        );
        assert_eq!(
            ranking_lines(&Ranking { entries: vec![] }),
            ["Noch keine Einträge."]
        );
    }

    #[test]
    fn an_invite_link_is_shown_with_its_code_and_when_it_ends() {
        let invite = FriendInvite {
            code: "ABCD-EFGH-JKLM-NPQR".into(),
            url: "https://web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR".into(),
            expires_at: 1_800_000_000,
        };
        let lines = created_lines(&invite);
        assert_eq!(
            lines[..3],
            [
                "✓ Einladungslink erstellt, einmal gültig bis 2027-01-15:",
                "  https://web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR",
                "  Code: ABCD-EFGH-JKLM-NPQR",
            ]
        );
        assert!(lines.contains(
            &"  In der CLI annehmen: locora cloud friend accept-invite ABCD-EFGH-JKLM-NPQR".into()
        ));
        assert_eq!(
            invite_lines(std::slice::from_ref(&invite)),
            [
                "Offene Einladungslinks (zurückziehen: locora cloud friend invite revoke <Code>):",
                "  ABCD-EFGH-JKLM-NPQR  bis 2027-01-15  https://web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR",
            ]
        );
        assert_eq!(
            invite_lines(&[]),
            ["Keine offenen Einladungslinks. Neuer Link: locora cloud friend invite"]
        );
        // What the server sends is printed safely.
        let odd = FriendInvite {
            url: "https://x\u{1b}]52;c;aGk=\u{7}".into(),
            ..invite
        };
        let lines = [created_lines(&odd), invite_lines(&[odd])]
            .concat()
            .join("\n");
        assert!(
            !lines.chars().any(|c| c.is_control() && c != '\n'),
            "{lines:?}"
        );
    }

    #[test]
    fn an_invite_is_read_from_its_code_or_its_link() {
        for input in [
            "ABCD-EFGH-JKLM-NPQR",
            "abcd efgh jklm npqr",
            "https://web.locora.cloud/einladung#abcd-efgh-jklm-npqr",
        ] {
            assert_eq!(
                invite_code(input).unwrap(),
                "ABCD-EFGH-JKLM-NPQR",
                "{input}"
            );
        }
        let refused = invite_code("https://web.locora.cloud/einladung\u{1b}[2J")
            .unwrap_err()
            .to_string();
        assert_eq!(
            refused,
            "„https://web.locora.cloud/einladung\\u{1b}[2J“ ist kein Einladungscode (wie ABCD-EFGH-JKLM-NPQR) und kein Einladungslink"
        );
    }

    #[test]
    fn an_older_servers_friend_list_reads_as_before() {
        // No ids, no addresses, and the names of both sides of a request.
        let list = [
            friend("Ada", true, None, None),
            friend("Bob", false, None, None),
        ];
        assert_eq!(entry(&list[1]), Entry::Open);
        assert_eq!(
            friend_lines(&list),
            ["Freunde:", "  Ada", "", "Offene Anfragen:", "  Bob"]
        );
    }

    #[test]
    fn names_in_the_friend_list_are_printed_safely() {
        let list = [
            friend("Ada\u{202e}evol", true, Some("fr_1"), None),
            friend("\u{1b}]52;c;aGk=\u{7}", false, Some("fr_2\u{1b}[2J"), None),
            friend("", false, Some("fr_3\u{200b}"), Some(1_700_000_000)),
        ];
        let lines = friend_lines(&list).join("\n");
        assert!(lines.contains("Ada\\u{202e}evol"), "{lines}");
        assert!(lines.contains("\\u{1b}]52;c;aGk=\\u{7}"), "{lines}");
        assert!(lines.contains("fr_2\\u{1b}[2J"), "{lines}");
        assert!(lines.contains("fr_3\\u{200b}"), "{lines}");
        assert!(
            !lines.chars().any(|c| c.is_control() && c != '\n'),
            "{lines:?}"
        );
    }

    #[test]
    fn an_id_given_to_the_command_of_another_kind_names_the_right_one() {
        assert_eq!(
            other_kind(Entry::Friend, "fr_1"),
            "fr_1 ist eine Freundschaft. Beenden: locora cloud friend remove fr_1"
        );
        assert_eq!(
            other_kind(Entry::Incoming, "fr_2"),
            "fr_2 ist eine Anfrage an dich. Ablehnen: locora cloud friend decline fr_2"
        );
        assert_eq!(
            other_kind(Entry::Outgoing, "fr_3"),
            "fr_3 ist deine eigene Anfrage. Zurückziehen: locora cloud friend withdraw fr_3"
        );
    }

    /// `locora cloud friend …` as the shell passes it, or clap's refusal.
    fn friend_command(args: &[&str]) -> Result<CloudCommand, clap::Error> {
        use clap::Parser;
        let args = ["locora", "cloud", "friend"].iter().chain(args);
        match crate::Cli::try_parse_from(args)?.command {
            crate::Command::Cloud(command) => Ok(command),
            _ => panic!("not a cloud command"),
        }
    }

    #[test]
    fn the_friend_commands_take_the_data_folder_before_and_after_them() {
        for args in [
            &["--data", "dir", "remove", "fr_x"][..],
            &["remove", "fr_x", "--data", "dir"],
            &["remove", "fr_x"],
        ] {
            assert!(
                matches!(
                    friend_command(args),
                    Ok(CloudCommand::Friend {
                        email: None,
                        action: Some(FriendCommand::Remove { ref id }),
                    }) if id == "fr_x"
                ),
                "{args:?}"
            );
        }
        for (args, wanted) in [
            (&["--data", "dir", "withdraw", "fr_y"][..], "withdraw"),
            (&["--data", "dir", "decline", "fr_y"], "decline"),
        ] {
            let action = match friend_command(args) {
                Ok(CloudCommand::Friend {
                    email: None,
                    action: Some(FriendCommand::Withdraw { id }),
                }) => ("withdraw", id),
                Ok(CloudCommand::Friend {
                    email: None,
                    action: Some(FriendCommand::Decline { id }),
                }) => ("decline", id),
                _ => panic!("{args:?} is no {wanted}"),
            };
            assert_eq!(action, (wanted, "fr_y".to_owned()));
        }
        // An address asks, with the data folder anywhere; none lists.
        for args in [
            &["--data", "dir", "ada@example.org"][..],
            &["ada@example.org", "--data", "dir"],
        ] {
            assert!(
                matches!(
                    friend_command(args),
                    Ok(CloudCommand::Friend { email: Some(ref email), action: None })
                        if email == "ada@example.org"
                ),
                "{args:?}"
            );
        }
        assert!(matches!(
            friend_command(&["--data", "dir"]),
            Ok(CloudCommand::Friend {
                email: None,
                action: None
            })
        ));
        // A command without its id is refused rather than taken for an address.
        assert!(friend_command(&["--data", "dir", "remove"]).is_err());
        assert!(friend_command(&["remove"]).is_err());
        // An address and a command together: refused before anything is sent.
        let both = friend_command(&["ada@example.org", "remove", "fr_x"]).unwrap();
        let refused = super::run(both, Some(std::path::Path::new("unused")))
            .unwrap_err()
            .to_string();
        assert!(
            refused.starts_with("Entweder eine E-Mail-Adresse"),
            "{refused}"
        );
    }

    /// `locora cloud …` as the shell passes it.
    fn cloud_command(args: &[&str]) -> Result<CloudCommand, clap::Error> {
        use clap::Parser;
        let args = ["locora", "cloud"].iter().chain(args);
        match crate::Cli::try_parse_from(args)?.command {
            crate::Command::Cloud(command) => Ok(command),
            _ => panic!("not a cloud command"),
        }
    }

    #[test]
    fn invites_and_reports_are_commands_of_their_own() {
        for args in [
            &["invite"][..],
            &["--data", "dir", "invite"],
            &["invite", "--data", "dir"],
        ] {
            assert!(
                matches!(
                    friend_command(args),
                    Ok(CloudCommand::Friend {
                        email: None,
                        action: Some(FriendCommand::Invite { action: None }),
                    })
                ),
                "{args:?}"
            );
        }
        assert!(matches!(
            friend_command(&["invite", "list", "--data", "dir"]),
            Ok(CloudCommand::Friend {
                email: None,
                action: Some(FriendCommand::Invite {
                    action: Some(InviteCommand::List)
                }),
            })
        ));
        assert!(matches!(
            friend_command(&["--data", "dir", "invite", "revoke", "ABCD-EFGH-JKLM-NPQR"]),
            Ok(CloudCommand::Friend {
                email: None,
                action: Some(FriendCommand::Invite {
                    action: Some(InviteCommand::Revoke { ref code })
                }),
            }) if code == "ABCD-EFGH-JKLM-NPQR"
        ));
        let link = "https://web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR";
        assert!(matches!(
            friend_command(&["accept-invite", link, "--data", "dir"]),
            Ok(CloudCommand::Friend {
                email: None,
                action: Some(FriendCommand::AcceptInvite { ref code }),
            }) if code == link
        ));
        assert!(friend_command(&["accept-invite"]).is_err());
        assert!(friend_command(&["invite", "revoke"]).is_err());
        assert!(matches!(
            cloud_command(&["report", "nr_0a", "--reason", "beleidigend", "--data", "dir"]),
            Ok(CloudCommand::Report { ref reference, reason: Some(ref reason) })
                if reference == "nr_0a" && reason == "beleidigend"
        ));
        assert!(matches!(
            cloud_command(&["--data", "dir", "report", "fr_1"]),
            Ok(CloudCommand::Report { ref reference, reason: None }) if reference == "fr_1"
        ));
        assert!(cloud_command(&["report"]).is_err());
    }

    #[test]
    fn unix_days_become_calendar_dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_800_000_000), "2027-01-15");
        assert_eq!(date(951_782_400), "2000-02-29");
    }
}
