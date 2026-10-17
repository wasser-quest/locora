//! Local progress. The runtime supplies learning-day boundaries so the same
//! 4:00/local-offset convention applies to reviews and daily challenges.
use std::collections::BTreeMap;

use locora_sdk::api::core::{JourneyStarted, LearningDayAnswer};
use locora_sdk::api::gamification::{Status, StatusResponse, Trophy, TrophyUnlocked};
use locora_sdk::prelude::*;
use serde::{Deserialize, Serialize};

#[locora_plugin(name = "Erfolge", permissions = [storage])]
struct Gamification;

const HEARTS: u8 = 5;
const DAILY_GOAL: u32 = 10;

#[derive(Default, Serialize, Deserialize)]
struct Progress {
    xp: u64,
    max_combo: u32,
    trophies: Vec<Trophy>,
    sessions: BTreeMap<String, Session>,
    last_palace: Option<String>,
    daily_day: Option<u64>,
    daily_correct: u32,
}

#[derive(Serialize, Deserialize)]
struct Session {
    combo: u32,
    max_combo: u32,
    hearts: u8,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            combo: 0,
            max_combo: 0,
            hearts: HEARTS,
        }
    }
}

fn load(ctx: &mut Ctx) -> Result<Progress> {
    Ok(ctx.store().get("progress")?.unwrap_or_default())
}

fn save(ctx: &mut Ctx, progress: &Progress) -> Result {
    ctx.store().set("progress", progress)?;
    ctx.tile(
        "Erfolge",
        format!(
            "{} XP · {}/{} heute · {} Trophäen",
            progress.xp,
            progress.daily_correct.min(DAILY_GOAL),
            DAILY_GOAL,
            progress.trophies.len()
        ),
    )
}

fn unlock(ctx: &mut Ctx, progress: &mut Progress, trophy: Trophy) -> Result {
    if !progress.trophies.contains(&trophy) {
        progress.trophies.push(trophy);
        ctx.emit(&TrophyUnlocked { trophy })?;
    }
    Ok(())
}

/// Port of mnemodim/shared/journeyScoring.ts: rounded integer accuracy.
fn journey_xp(correct: u32, total: u32, max_combo: u32) -> u64 {
    let accuracy = if total == 0 {
        0
    } else {
        (u64::from(correct).saturating_mul(100) + u64::from(total) / 2) / u64::from(total)
    };
    let multiplier = if accuracy >= 90 {
        3
    } else if accuracy >= 70 {
        2
    } else {
        1
    };
    (u64::from(correct)
        .saturating_mul(10)
        .saturating_add(u64::from(max_combo).saturating_mul(5)))
    .saturating_mul(multiplier)
}

impl Plugin for Gamification {
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.on::<Started>(|ctx, _| {
            let progress = load(ctx)?;
            save(ctx, &progress)
        })?;
        ctx.on::<JourneyStarted>(|ctx, event| {
            let mut progress = load(ctx)?;
            progress.sessions.clear();
            progress.last_palace = None;
            for palace in event.palace_ids {
                progress.sessions.insert(palace.clone(), Session::default());
                progress.last_palace = Some(palace);
            }
            save(ctx, &progress)
        })?;
        ctx.on::<LearningDayAnswer>(|ctx, event| {
            let mut progress = load(ctx)?;
            if progress.daily_day != Some(event.day_start) {
                progress.daily_day = Some(event.day_start);
                progress.daily_correct = 0;
            }
            if event.correct {
                progress.daily_correct = progress.daily_correct.saturating_add(1);
            }
            let session = progress
                .sessions
                .entry(event.palace_id.clone())
                .or_default();
            if event.correct {
                session.combo = session.combo.saturating_add(1);
                session.max_combo = session.max_combo.max(session.combo);
                progress.max_combo = progress.max_combo.max(session.combo);
            } else {
                session.combo = 0;
                session.hearts = session.hearts.saturating_sub(1);
            }
            progress.last_palace = Some(event.palace_id);
            save(ctx, &progress)
        })?;
        ctx.on::<JourneyCompleted>(|ctx, event| {
            let mut progress = load(ctx)?;
            // Completion is reported once by each client. Repeated events or
            // an empty journey cannot award XP without an active session.
            let Some(session) = progress.sessions.remove(&event.palace_id) else {
                return Ok(());
            };
            if event.total == 0 || event.correct > event.total {
                return save(ctx, &progress);
            }
            progress.xp = progress.xp.saturating_add(journey_xp(
                event.correct,
                event.total,
                session.max_combo,
            ));
            unlock(ctx, &mut progress, Trophy::FirstJourney)?;
            if event.correct == event.total {
                unlock(ctx, &mut progress, Trophy::PerfectJourney)?;
            }
            if progress.xp >= 100 {
                unlock(ctx, &mut progress, Trophy::HundredXp)?;
            }
            save(ctx, &progress)
        })?;
        ctx.provide::<Status>(|ctx, request| {
            let mut progress = load(ctx)?;
            if request.streak >= 7 {
                unlock(ctx, &mut progress, Trophy::SevenDayStreak)?;
                save(ctx, &progress)?;
            }
            let session = progress
                .last_palace
                .as_ref()
                .and_then(|palace| progress.sessions.get(palace));
            let daily_correct = if progress.daily_day == Some(request.day_start) {
                progress.daily_correct.min(DAILY_GOAL)
            } else {
                0
            };
            ctx.tile(
                "Erfolge",
                format!(
                    "{} XP · {}/{} heute · {} Trophäen",
                    progress.xp,
                    daily_correct,
                    DAILY_GOAL,
                    progress.trophies.len()
                ),
            )?;
            Ok(StatusResponse {
                xp: progress.xp,
                combo: session.map_or(0, |s| s.combo),
                max_combo: progress.max_combo,
                hearts: session.map_or(HEARTS, |s| s.hearts),
                trophies: progress.trophies,
                daily_correct,
                daily_goal: DAILY_GOAL,
                streak: request.streak,
            })
        })?;
        let progress = load(ctx)?;
        save(ctx, &progress)
    }
}
