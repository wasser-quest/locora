//! `locora notify`: what the app would show now. The app acknowledges each
//! message when it is handled; from the CLI, showing counts as handling.

use anyhow::{Result, bail};
use clap::Subcommand;
use locora_runtime::now;

use crate::app::App;

#[derive(Subcommand)]
pub enum NotifyCommand {
    /// Erinnerungen zeigen, die jetzt anstehen (und als gesehen markieren)
    Ls,
    /// Verlauf der gezeigten Erinnerungen (neueste zuerst, ohne etwas zu ändern)
    Show {
        /// Höchstens so viele Einträge (Standard 20)
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Nachricht an mich selbst schicken (Kanal, Titel, Text)
    Send {
        channel: String,
        title: String,
        body: String,
    },
}

fn notify_required(app: &App) -> Result<()> {
    if !app.has_notify() {
        bail!("Dafür braucht es das Plugin `notify` (Benachrichtigungen)");
    }
    Ok(())
}

/// Unix seconds as `YYYY-MM-DD HH:MM` (UTC; the plugin stores Unix time).
fn at(seconds: u64) -> String {
    let days = seconds / 86_400;
    let secs = seconds % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs / 3600,
        secs % 3600 / 60
    )
}

/// Days since the Unix epoch to a civil date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 - doe / 36_524 + doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn run(command: NotifyCommand, app: &mut App) -> Result<()> {
    notify_required(app)?;
    match command {
        NotifyCommand::Ls => {
            let messages = app.notifications()?;
            app.report();
            if messages.is_empty() {
                println!("Keine Erinnerungen.");
            }
            for message in &messages {
                println!("[{}] {} — {}", message.channel, message.title, message.body);
                if !message.action.is_empty() {
                    println!("  Aktion: {}", message.action);
                }
            }
            for message in messages {
                app.shown_notification(&message)?;
            }
            app.report();
        }
        NotifyCommand::Show { limit } => {
            let history = app.notification_history(limit)?;
            app.report();
            for message in &history {
                println!(
                    "{} [{}] {} — {}",
                    at(message.shown_at),
                    message.channel,
                    message.title,
                    message.body
                );
            }
        }
        NotifyCommand::Send {
            channel,
            title,
            body,
        } => {
            app.send_notification(&channel, &title, &body, "")?;
            let messages = app.notifications()?;
            app.report();
            for message in messages {
                println!(
                    "Gesendet: [{}] {} — {}",
                    message.channel, message.title, message.body
                );
                app.shown_notification(&message)?;
            }
            app.report();
            let _ = now(); // timestamps come from the plugin/host
        }
    }
    Ok(())
}
