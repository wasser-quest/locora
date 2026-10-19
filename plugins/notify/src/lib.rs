//! Local notifications: one place for quiet hours, muting, the daily
//! per-channel limit and bundling. The host polls `due` (at start and while
//! the app runs) because plugins have no clock, then confirms every shown
//! message with `notify.displayed`.
//!
//! Storage: `queue` (waiting messages), `schedules` (planned), `fired` (daily
//! schedules that already fired today), `shown/<day>` (per-channel counters),
//! `settings` (quiet hours, muted channels), `history` (shown messages).
use locora_sdk::api::notify::{
    Configure, Displayed, Due, History, Message, Plan, Queued, Schedule, ScheduleEntry, Send,
    Settings,
};
use locora_sdk::prelude::*;

#[locora_plugin(name = "Benachrichtigungen", permissions = [storage])]
struct Notify;

/// Messages of the same channel within this window are bundled into one.
const BUNDLE_WINDOW: u64 = 600;
/// How many notifications one channel may show per day.
const DAILY_LIMIT: u32 = 8;
/// How many shown messages the history keeps.
const HISTORY: usize = 50;

fn queue(ctx: &mut Ctx) -> Result<Vec<Queued>> {
    Ok(ctx.store().get("queue")?.unwrap_or_default())
}

fn set_queue(ctx: &mut Ctx, queue: &[Queued]) -> Result {
    ctx.store().set("queue", &queue.to_vec())
}

fn settings(ctx: &mut Ctx) -> Result<Settings> {
    Ok(ctx.store().get("settings")?.unwrap_or_default())
}

/// How often one channel was shown on `day` (days since the Unix epoch).
fn shown(ctx: &mut Ctx, day: u64) -> Result<Vec<(String, u32)>> {
    Ok(ctx
        .store()
        .get(&format!("shown/{day}"))?
        .unwrap_or_default())
}

/// Bundle same-channel messages queued within [`BUNDLE_WINDOW`] seconds:
/// one notification per channel, highest priority, bodies joined with ` · `.
fn bundle(queue: Vec<Queued>) -> Vec<Queued> {
    let mut out: Vec<Queued> = Vec::new();
    for message in queue {
        match out.iter_mut().find(|m| {
            m.channel == message.channel && message.queued_at.abs_diff(m.queued_at) <= BUNDLE_WINDOW
        }) {
            Some(bundle) => {
                bundle.body = format!("{} · {}", bundle.body, message.body);
                bundle.priority = bundle.priority.max(message.priority);
            }
            None => out.push(message),
        }
    }
    out
}

/// The part of the queue that may show now: not muted, daily limit spare,
/// not in quiet hours; oldest first, bundled per channel.
fn deliverable(ctx: &mut Ctx, now: u64, day_seconds: u32) -> Result<Vec<Queued>> {
    let settings = settings(ctx)?;
    if settings.is_quiet(day_seconds) {
        return Ok(Vec::new());
    }
    let day = now / 86_400;
    let shown = shown(ctx, day)?;
    let mut queue: Vec<Queued> = queue(ctx)?;
    queue.retain(|message| {
        !settings.muted.contains(&message.channel)
            && shown
                .iter()
                .find(|(channel, _)| *channel == message.channel)
                .is_none_or(|(_, count)| *count < DAILY_LIMIT)
    });
    queue.sort_by_key(|message| message.queued_at);
    Ok(bundle(queue))
}

/// Count one shown notification for its channel.
fn note_shown(ctx: &mut Ctx, day: u64, channel: &str) -> Result {
    let mut shown = shown(ctx, day)?;
    match shown.iter_mut().find(|(c, _)| c == channel) {
        Some((_, count)) => *count += 1,
        None => shown.push((channel.to_owned(), 1)),
    }
    ctx.store().set(&format!("shown/{day}"), &shown)
}

fn push_history(ctx: &mut Ctx, message: &Message) -> Result {
    let mut history: Vec<Message> = ctx.store().get("history")?.unwrap_or_default();
    history.insert(0, message.clone());
    history.truncate(HISTORY);
    ctx.store().set("history", &history)
}

impl Plugin for Notify {
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.provide::<Send>(|ctx, request| {
            if request.channel.trim().is_empty() {
                return Err(Error::msg("Der Kanal fehlt"));
            }
            if request.title.trim().is_empty() {
                return Err(Error::msg("Der Titel fehlt"));
            }
            if request.body.trim().is_empty() {
                return Err(Error::msg("Der Text fehlt"));
            }
            if request.priority > 2 {
                return Err(Error::msg("Die Priorität muss 0, 1 oder 2 sein"));
            }
            let mut queue = queue(ctx)?;
            queue.push(Queued {
                channel: request.channel,
                priority: request.priority,
                title: request.title,
                body: request.body,
                action: request.action,
                queued_at: request.now,
            });
            set_queue(ctx, &queue)?;
            Ok(true)
        })?;
        ctx.provide::<Schedule>(|ctx, request| {
            if request.key.trim().is_empty() {
                return Err(Error::msg("Der Schlüssel fehlt"));
            }
            if request.channel.trim().is_empty() {
                return Err(Error::msg("Der Kanal fehlt"));
            }
            if request.priority > 2 {
                return Err(Error::msg("Die Priorität muss 0, 1 oder 2 sein"));
            }
            match &request.plan {
                Some(Plan::At { at }) if *at == 0 => {
                    return Err(Error::msg("Der Zeitpunkt muss in der Zukunft liegen"));
                }
                Some(Plan::Daily { seconds }) if *seconds >= 86_400 => {
                    return Err(Error::msg(
                        "Die Tageszeit muss zwischen 00:00 und 23:59 liegen",
                    ));
                }
                _ => {}
            }
            let mut schedules: Vec<ScheduleEntry> =
                ctx.store().get("schedules")?.unwrap_or_default();
            schedules.retain(|schedule| schedule.key != request.key);
            if let Some(plan) = request.plan {
                schedules.push(ScheduleEntry {
                    key: request.key,
                    channel: request.channel,
                    priority: request.priority,
                    title: request.title,
                    body: request.body,
                    action: request.action,
                    plan,
                });
            }
            schedules.sort_by(|a, b| a.key.cmp(&b.key));
            ctx.store().set("schedules", &schedules)?;
            // The home tile counts the daily reminders, so it follows every change.
            update_tile(ctx)?;
            Ok(schedules)
        })?;
        ctx.provide::<Due>(|ctx, request| {
            // One-shot schedules that are past fire once and are dropped.
            let mut schedules: Vec<ScheduleEntry> =
                ctx.store().get("schedules")?.unwrap_or_default();
            let mut fired: Vec<(String, u64)> = ctx.store().get("fired")?.unwrap_or_default();
            let mut queue = queue(ctx)?;
            let day = request.now / 86_400;
            let mut remove = Vec::new();
            for schedule in &schedules {
                let due = match &schedule.plan {
                    Plan::At { at } => request.now >= *at,
                    Plan::Daily { seconds } => {
                        // Fires once per day; the marker resets with the day key.
                        request.day_seconds >= *seconds as u32
                            && !fired
                                .iter()
                                .any(|(key, d)| *d == day && key == &schedule.key)
                    }
                };
                if due {
                    queue.push(Queued {
                        channel: schedule.channel.clone(),
                        priority: schedule.priority,
                        title: schedule.title.clone(),
                        body: schedule.body.clone(),
                        action: schedule.action.clone(),
                        queued_at: request.now,
                    });
                    if let Plan::Daily { .. } = schedule.plan {
                        fired.push((schedule.key.clone(), day));
                    }
                    if let Plan::At { .. } = schedule.plan {
                        remove.push(schedule.key.clone());
                    }
                }
            }
            schedules.retain(|schedule| !remove.contains(&schedule.key));
            fired.retain(|(_, d)| *d >= day);
            ctx.store().set("schedules", &schedules)?;
            ctx.store().set("fired", &fired)?;
            set_queue(ctx, &queue)?;
            deliverable(ctx, request.now, request.day_seconds)
        })?;
        ctx.on::<Displayed>(|ctx, event| {
            // The host showed the (bundled) message; drop every part of the
            // bundle from the queue, count the channel for the daily limit
            // and keep the history.
            let message = &event.message;
            let day = message.shown_at / 86_400;
            let parts: Vec<&str> = message.body.split(" · ").collect();
            let mut queue = queue(ctx)?;
            let before = queue.len();
            queue.retain(|queued| {
                !(queued.channel == message.channel
                    && queued.title == message.title
                    && parts.contains(&queued.body.as_str()))
            });
            set_queue(ctx, &queue)?;
            if queue.len() < before {
                note_shown(ctx, day, &message.channel)?;
                push_history(ctx, message)?;
            }
            Ok(())
        })?;
        ctx.provide::<Configure>(|ctx, request| {
            let mut current = settings(ctx)?;
            if let Some(channel) = &request.mute
                && !current.muted.contains(channel)
            {
                current.muted.push(channel.clone());
            }
            if let Some(muted) = request.muted {
                current.muted = muted;
            }
            if let Some(quiet) = &request.quiet {
                if quiet.is_empty() {
                    current.quiet_start = 0;
                    current.quiet_end = 0;
                } else {
                    let (start, end) = quiet.split_once('-').ok_or_else(|| {
                        Error::msg(format!("Ruhezeiten als HH:MM-HH:MM angeben, nicht {quiet}"))
                    })?;
                    current.quiet_start =
                        locora_sdk::api::notify::parse_hhmm(start).map_err(Error::msg)?;
                    current.quiet_end =
                        locora_sdk::api::notify::parse_hhmm(end).map_err(Error::msg)?;
                }
            }
            current.muted.sort();
            current.muted.dedup();
            ctx.store().set("settings", &current)?;
            Ok(current)
        })?;
        ctx.provide::<History>(|ctx, request| {
            let history: Vec<Message> = ctx.store().get("history")?.unwrap_or_default();
            let limit = if request.limit == 0 {
                history.len().min(HISTORY)
            } else {
                request.limit as usize
            };
            Ok(history.into_iter().take(limit).collect())
        })?;
        update_tile(ctx)
    }
}

/// The home tile names the daily reminder times, so it does not go stale.
fn update_tile(ctx: &mut Ctx) -> Result {
    let schedules: Vec<ScheduleEntry> = ctx.store().get("schedules")?.unwrap_or_default();
    let text = match schedules
        .iter()
        .filter(|schedule| matches!(schedule.plan, Plan::Daily { .. }))
        .count()
    {
        0 => localized("Keine täglichen Erinnerungen", &[]),
        1 => localized("1 tägliche Erinnerung", &[]),
        count => localized("{0} tägliche Erinnerungen", &[&count]),
    };
    ctx.tile("Benachrichtigungen", text)
}


/// A tile text the app fills in the current language: a template and its arguments,
/// separated by U+001F (see `plugin_text` in the app).
fn localized(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    args.iter()
        .fold(format!("@@{template}"), |text, arg| format!("{text}\u{1f}{arg}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queued(channel: &str, body: &str, at: u64) -> Queued {
        Queued {
            channel: channel.into(),
            priority: 0,
            title: "Titel".into(),
            body: body.into(),
            action: String::new(),
            queued_at: at,
        }
    }

    #[test]
    fn same_channel_within_window_bundles() {
        let bundled = bundle(vec![
            queued("review.due", "12 Loci", 100),
            queued("review.due", "14 Loci", 200),
        ]);
        assert_eq!(bundled.len(), 1);
        assert_eq!(bundled[0].body, "12 Loci · 14 Loci");
    }

    #[test]
    fn bundling_handles_reverse_timestamps_without_underflow() {
        assert_eq!(
            bundle(vec![queued("test", "new", 200), queued("test", "old", 100)]).len(),
            1
        );
        assert_eq!(
            bundle(vec![
                queued("test", "new", u64::MAX),
                queued("test", "old", 0)
            ])
            .len(),
            2
        );
    }

    #[test]
    fn other_channels_stay_separate() {
        let bundled = bundle(vec![
            queued("review.due", "12 Loci", 100),
            queued("core.journey", "Journey", 200),
        ]);
        assert_eq!(bundled.len(), 2);
    }

    #[test]
    fn old_messages_do_not_bundle() {
        let bundled = bundle(vec![
            queued("review.due", "alt", 0),
            queued("review.due", "neu", BUNDLE_WINDOW + 1),
        ]);
        assert_eq!(bundled.len(), 2);
    }
}
