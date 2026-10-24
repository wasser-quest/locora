//! Progress that is the same on every device: XP for due reviews and new loci, learning days, a
//! weekly goal, trophies and palace medals. Every device stores facts under keys of its own and
//! derives the totals from the merged facts (see `ledger`), so a sync never loses progress and
//! never counts it twice. The runtime supplies the learning day, this device's id and the game
//! epoch (a new one after the progress was deleted), so the plugin needs no clock of its own.
//! Gamification sends no notifications.

mod ledger;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use ledger::{DayFact, Facts, Look, MedalFact, OldProgress, WeekGoalPref};
use locora_sdk::api::core::{JourneyStarted, ReviewedAnswer, Timing};
use locora_sdk::api::gamification::{
    Migrate, Reset, Seen, SessionView, Status, StatusResponse, StatusV2, StatusV2Request,
    StatusV2Response, TrophyUnlocked, WeekGoal,
};
use locora_sdk::prelude::*;
use serde::Serialize;

#[locora_plugin(name = "Erfolge", permissions = [storage])]
struct Gamification;

/// What the plugin keeps in memory: a mirror of its storage (refreshed with `storage.list@1`),
/// and this journey on this device, which is never stored.
#[derive(Default)]
struct State {
    mirror: BTreeMap<String, Vec<u8>>,
    rev: u64,
    /// The epoch of the last request from the host (`status@2`, `reset@1`); old facts were
    /// emptied for it. Events never change it.
    epoch: Option<String>,
    /// The learning day that epoch began (see `StatusV2Request::epoch_day`).
    epoch_day: Option<String>,
    /// Where the last answer was given: (epoch, day, device).
    context: Option<(String, String, String)>,
    session: Option<SessionView>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with_borrow_mut(f)
}

/// Takes in what changed in storage since the last look, here or through a sync.
fn refresh(ctx: &mut Ctx) -> Result {
    loop {
        let after = with(|state| state.rev);
        let page = ctx.store().list("", after)?;
        with(|state| {
            for entry in page.entries {
                if entry.value.is_empty() {
                    state.mirror.remove(&entry.key);
                } else {
                    state.mirror.insert(entry.key, entry.value);
                }
            }
            state.rev = state.rev.max(page.rev);
        });
        if !page.more {
            return Ok(());
        }
    }
}

/// Writes a key unless it already holds these bytes (empty bytes blank it).
fn write(ctx: &mut Ctx, key: &str, bytes: Vec<u8>) -> Result<bool> {
    let same = with(|state| match state.mirror.get(key) {
        Some(current) => *current == bytes,
        None => bytes.is_empty(),
    });
    if same {
        return Ok(false);
    }
    ctx.store().set_raw(key, &bytes)?;
    with(|state| {
        if bytes.is_empty() {
            state.mirror.remove(key);
        } else {
            state.mirror.insert(key.to_owned(), bytes);
        }
    });
    Ok(true)
}

fn write_value<T: Serialize>(ctx: &mut Ctx, key: &str, value: &T) -> Result<bool> {
    write(ctx, key, locora_sdk::api::encode(value)?)
}

/// Follows the epoch of a request. A new epoch means the progress was deleted (here or on
/// another device): facts of other epochs and the progress from before the sync are emptied
/// on this device too, once. Answers how many keys were emptied.
fn adopt_epoch(ctx: &mut Ctx, epoch: &str) -> Result<u32> {
    if with(|state| state.epoch.as_deref() == Some(epoch)) {
        return Ok(0);
    }
    let mut emptied = 0;
    if epoch != ledger::FIRST_EPOCH {
        let current = format!("g/{epoch}/");
        let old: Vec<String> = with(|state| {
            state
                .mirror
                .keys()
                .filter(|key| {
                    (key.starts_with("g/") && !key.starts_with(&current))
                        || key.starts_with("legacy/")
                })
                .cloned()
                .collect()
        });
        for key in old {
            emptied += u32::from(write(ctx, &key, Vec::new())?);
        }
        // Older app versions read this blob: they show zero instead of failing to read it.
        let zero = locora_sdk::api::encode(&OldProgress::default())?;
        let stored = with(|state| state.mirror.get(ledger::PROGRESS).cloned());
        if stored.is_some_and(|bytes| bytes != zero) {
            emptied += u32::from(write(ctx, ledger::PROGRESS, zero)?);
        }
        with(|state| {
            state.session = None;
            state.context = None;
        });
    }
    with(|state| state.epoch = Some(epoch.to_owned()));
    Ok(emptied)
}

fn facts(epoch: &str) -> Facts {
    with(|state| {
        ledger::collect(
            state
                .mirror
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_slice())),
            epoch,
        )
    })
}

/// This device's fact for a day, as stored.
fn own_day(key: &str) -> DayFact {
    with(|state| {
        state
            .mirror
            .get(key)
            .and_then(|bytes| locora_sdk::api::decode(bytes).ok())
            .unwrap_or_else(DayFact::empty)
    })
}

/// Changes this device's fact for a day; stores it only when it changed.
fn change_day(ctx: &mut Ctx, key: &str, change: impl FnOnce(&mut DayFact) -> bool) -> Result {
    let mut fact = own_day(key);
    if change(&mut fact) {
        write_value(ctx, key, &fact)?;
    }
    Ok(())
}

/// What the facts amount to on `day`, with the day the epoch began.
fn derived(epoch: &str, day: &str) -> ledger::Derived {
    let epoch_day = with(|state| state.epoch_day.clone());
    ledger::derive(&facts(epoch), day, epoch_day.as_deref())
}

/// XP and earned trophies on `day`, to see what a change brought.
fn standing(epoch: &str, day: &str) -> (u64, BTreeSet<String>) {
    let derived = derived(epoch, day);
    (derived.xp, derived.earned())
}

/// After a change on this device: the XP it brought goes to this journey, newly earned trophies
/// are noted for the journey (only this device's own changes, never what a sync brought: the
/// facts were read before `before`), and those of the older kinds are announced
/// (`TrophyUnlocked`).
fn settle(ctx: &mut Ctx, before: (u64, BTreeSet<String>), epoch: &str, day: &str) -> Result {
    let (xp, earned) = standing(epoch, day);
    let new: Vec<&String> = earned.difference(&before.1).collect();
    with(|state| {
        if let Some(session) = state.session.as_mut() {
            session.xp = session.xp.saturating_add(xp.saturating_sub(before.0));
            session
                .unlocked
                .extend(new.iter().map(|id| format!("game-{id}")));
        }
    });
    for id in new {
        if let Some(trophy) = ledger::old_trophy(id) {
            ctx.emit(&TrophyUnlocked { trophy })?;
        }
    }
    Ok(())
}

fn answered(ctx: &mut Ctx, event: ReviewedAnswer) -> Result {
    refresh(ctx)?;
    // Only the host's own requests change the epoch: an event (which any plugin could publish)
    // of another epoch is not counted, and never empties anything.
    if with(|state| {
        state
            .epoch
            .as_ref()
            .is_some_and(|epoch| *epoch != event.epoch)
    }) {
        return Ok(());
    }
    let before = standing(&event.epoch, &event.day);
    with(|state| {
        state.context = Some((event.epoch.clone(), event.day.clone(), event.device.clone()));
        let session = state.session.get_or_insert_with(SessionView::default);
        match event.timing {
            Timing::Due => session.due += 1,
            Timing::New => session.new += 1,
            Timing::Early => session.early += 1,
            Timing::Repeat => session.repeat += 1,
        }
        // Only typed and chosen answers make a run; recall grades are the learner's own.
        if event.kind != AnswerKind::Recall {
            session.combo = if event.correct { session.combo + 1 } else { 0 };
        }
    });
    let hash = ledger::locus_hash(&event.palace_id, &event.locus_id);
    let key = ledger::day_key(&event.epoch, &event.day, &event.device);
    match event.timing {
        // Practising early or again the same day is allowed and earns nothing.
        Timing::Early | Timing::Repeat => {}
        Timing::Due => change_day(ctx, &key, |fact| fact.add_due(hash))?,
        Timing::New => change_day(ctx, &key, |fact| fact.add_new(hash))?,
    }
    settle(ctx, before, &event.epoch, &event.day)?;
    show(ctx, &event.epoch, &event.day)
}

fn completed(ctx: &mut Ctx) -> Result {
    refresh(ctx)?;
    let Some((epoch, day, device)) = with(|state| state.context.clone()) else {
        return Ok(());
    };
    let before = standing(&epoch, &day);
    let key = ledger::day_key(&epoch, &day, &device);
    change_day(ctx, &key, |fact| {
        fact.journeys = fact.journeys.saturating_add(1);
        true
    })?;
    settle(ctx, before, &epoch, &day)?;
    show(ctx, &epoch, &day)
}

/// The home tile: XP, this week against its target and the trophies, or how to begin.
fn show(ctx: &mut Ctx, epoch: &str, day: &str) -> Result {
    let derived = derived(epoch, day);
    ctx.tile("Fortschritt", tile_text(&derived))
}

fn tile_text(derived: &ledger::Derived) -> String {
    let trophies = derived.trophies.len();
    let week = &derived.week;
    // Beyond the target the week is simply reached.
    let days = week.days.min(week.target);
    match (derived.xp, trophies) {
        (0, 0) => localized("Starte eine Journey, um XP zu sammeln.", &[]),
        (_, 1) => localized(
            "{0} XP · diese Woche {1}/{2} Tage · 1 Trophäe",
            &[&derived.xp, &days, &week.target],
        ),
        _ => localized(
            "{0} XP · diese Woche {1}/{2} Tage · {3} Trophäen",
            &[&derived.xp, &days, &week.target, &trophies],
        ),
    }
}

fn status(ctx: &mut Ctx, request: StatusV2Request) -> Result<StatusV2Response> {
    refresh(ctx)?;
    adopt_epoch(ctx, &request.epoch)?;
    with(|state| state.epoch_day = request.epoch_day.clone());
    let epoch = request.epoch.as_str();
    let before = standing(epoch, &request.day);
    // What this device sees of the schedule today (it tells the days with nothing due), and
    // whether every due review of the day is done; stored only when that changed.
    let due_done = facts(epoch)
        .days
        .get(&request.day)
        .map_or(0, ledger::Day::due_done);
    let key = ledger::day_key(epoch, &request.day, &request.device);
    change_day(ctx, &key, |fact| {
        let mut changed = false;
        if fact.look.as_ref().map(|look| &look.next) != Some(&request.next_due) {
            fact.look = Some(Look {
                at: request.at,
                next: request.next_due.clone(),
            });
            changed = true;
        }
        if request.has_cards && request.due_open == 0 && due_done > 0 && !fact.cleared {
            fact.cleared = true;
            changed = true;
        }
        changed
    })?;
    let mut palace_medals = Vec::new();
    if let Some(strengths) = &request.palaces {
        let (shown, new) = ledger::medals(&facts(epoch), strengths, &request.day);
        for (palace, level) in new {
            let key = ledger::medal_key(epoch, &palace, level, &request.device);
            let fact = MedalFact {
                day: request.day.clone(),
            };
            write_value(ctx, &key, &fact)?;
            with(|state| {
                if let Some(session) = state.session.as_mut() {
                    session.unlocked.push(format!("palace-{level}@{palace}"));
                }
            });
        }
        palace_medals = shown;
    }
    settle(ctx, before, epoch, &request.day)?;
    let facts = facts(epoch);
    let derived = derived(epoch, &request.day);
    show(ctx, epoch, &request.day)?;
    Ok(StatusV2Response {
        xp: derived.xp,
        today: ledger::today(&facts, &request.day, request.due_open, request.repeat_open),
        week: derived.week,
        learning_days: derived.learning_days.len() as u32,
        trophies: derived.trophies,
        locked: derived.locked,
        palace_medals,
        session: with(|state| state.session.clone()),
        intro_seen: facts.seen.contains("game-intro"),
        legacy: facts.legacy_xp > 0 || !facts.legacy_trophies.is_empty(),
    })
}

/// `gamification.status@1` for older callers: the same numbers, on the last known learning day.
fn status_v1(ctx: &mut Ctx, streak: u32) -> Result<StatusResponse> {
    refresh(ctx)?;
    let (epoch, day) = with(|state| {
        let epoch = state
            .epoch
            .clone()
            .unwrap_or_else(|| ledger::FIRST_EPOCH.to_owned());
        let day = state
            .context
            .as_ref()
            .map(|(_, day, _)| day.clone())
            .unwrap_or_default();
        (epoch, day)
    });
    let facts = facts(&epoch);
    let derived = derived(&epoch, &day);
    let today = facts.days.get(&day).cloned().unwrap_or_default();
    Ok(StatusResponse {
        xp: derived.xp,
        combo: with(|state| state.session.as_ref().map_or(0, |s| s.combo)),
        max_combo: 0,
        // Hearts are gone: a mistake costs nothing.
        hearts: 5,
        trophies: derived
            .trophies
            .iter()
            .filter_map(|t| ledger::old_trophy(&t.id))
            .collect(),
        daily_correct: today.due_done() + today.new.len() as u32,
        daily_goal: 0,
        streak,
    })
}

impl Plugin for Gamification {
    /// No storage is read here: on the web, storage is connected after the plugins start.
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.on::<JourneyStarted>(|_, _| {
            with(|state| state.session = Some(SessionView::default()));
            Ok(())
        })?;
        ctx.on::<ReviewedAnswer>(answered)?;
        ctx.on::<JourneyCompleted>(|ctx, _| completed(ctx))?;
        ctx.provide::<StatusV2>(status)?;
        ctx.provide::<Status>(|ctx, request| status_v1(ctx, request.streak))?;
        ctx.provide::<Seen>(|ctx, request| {
            refresh(ctx)?;
            if request.hint.is_empty() || request.hint.len() > 48 {
                return Err(Error::msg("Ungültiger Hinweis"));
            }
            write(ctx, &ledger::seen_key(&request.hint), vec![1])?;
            Ok(true)
        })?;
        ctx.provide::<Migrate>(|ctx, request| {
            refresh(ctx)?;
            let progress = with(|state| {
                state
                    .mirror
                    .get(ledger::PROGRESS)
                    .and_then(|bytes| locora_sdk::api::decode::<OldProgress>(bytes).ok())
            });
            let mut wrote = match ledger::legacy(progress.as_ref(), request.days) {
                Some(fact) => write_value(ctx, &ledger::legacy_key(&request.device), &fact)?,
                None => false,
            };
            // What this device kept for itself counts on top of the shared progress.
            let own = request
                .own
                .as_deref()
                .and_then(|bytes| locora_sdk::api::decode::<OldProgress>(bytes).ok());
            if let Some(fact) = own.as_ref().and_then(ledger::own_legacy) {
                wrote |= write_value(ctx, &ledger::own_legacy_key(&request.device), &fact)?;
            }
            Ok(wrote)
        })?;
        ctx.provide::<Reset>(|ctx, request| {
            refresh(ctx)?;
            with(|state| state.epoch = None);
            adopt_epoch(ctx, &request.epoch)
        })?;
        ctx.provide::<WeekGoal>(|ctx, request| {
            refresh(ctx)?;
            if !(1..=7).contains(&request.days) || !ledger::is_week_name(&request.week) {
                return Err(Error::msg("Wochenziel: 1 bis 7 Lerntage"));
            }
            let goal = WeekGoalPref { days: request.days };
            write_value(ctx, &ledger::goal_key(&request.week), &goal)?;
            Ok(request.days)
        })
    }
}

/// A tile text the app fills in the current language: a template and its arguments,
/// separated by U+001F (see `plugin_text` in the app).
fn localized(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    args.iter().fold(format!("@@{template}"), |text, arg| {
        format!("{text}\u{1f}{arg}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn derived(xp: u64, trophies: usize, days: u32, target: u32) -> ledger::Derived {
        let mut derived = ledger::derive(&Facts::default(), "2026-10-20", None);
        derived.xp = xp;
        derived.trophies = (0..trophies)
            .map(|n| locora_sdk::api::gamification::Earned {
                id: format!("t{n}"),
                day: None,
            })
            .collect();
        derived.week.days = days;
        derived.week.target = target;
        derived
    }

    #[test]
    fn the_tile_counts_trophies_in_the_singular_and_stops_at_the_target() {
        assert_eq!(
            tile_text(&derived(0, 0, 0, 4)),
            "@@Starte eine Journey, um XP zu sammeln."
        );
        assert_eq!(
            tile_text(&derived(16, 1, 1, 4)),
            "@@{0} XP · diese Woche {1}/{2} Tage · 1 Trophäe\u{1f}16\u{1f}1\u{1f}4"
        );
        assert_eq!(
            tile_text(&derived(160, 3, 5, 4)),
            "@@{0} XP · diese Woche {1}/{2} Tage · {3} Trophäen\u{1f}160\u{1f}4\u{1f}4\u{1f}3"
        );
    }
}
