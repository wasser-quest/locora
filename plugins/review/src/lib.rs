//! Spaced repetition: every answered locus becomes a card scheduled with FSRS.
//!
//! Storage: `palaces` lists palace ids with cards, `cards/<palace>` maps locus
//! ids to their card state, `settings` holds personal weights and the target
//! retention. Every answer is also logged in chunks: `log/<palace>/count` and
//! `log/<palace>/<n>` (256 entries each), so one answer never rewrites the
//! whole history.
use std::collections::BTreeMap;

use locora_fsrs::Params;
use locora_fsrs::optimize::valid_weights;
use locora_sdk::api::review::{
    CardState, Configure, Log, LogEntry, Restore, Schedule, ScheduledLocus, Settings,
};
use locora_sdk::prelude::*;

#[locora_plugin(name = "Wiederholung", permissions = [storage])]
struct Review;

type Cards = BTreeMap<String, CardState>;

fn cards_key(palace: &str) -> String {
    format!("cards/{palace}")
}

fn load(ctx: &mut Ctx, palace: &str) -> Result<Cards> {
    Ok(ctx.store().get(&cards_key(palace))?.unwrap_or_default())
}

fn palaces(ctx: &mut Ctx) -> Result<Vec<String>> {
    Ok(ctx.store().get("palaces")?.unwrap_or_default())
}

fn save(ctx: &mut Ctx, palace: &str, cards: &Cards) -> Result {
    ctx.store().set(&cards_key(palace), cards)?;
    let mut list = palaces(ctx)?;
    let listed = list.iter().any(|p| p == palace);
    if cards.is_empty() && listed {
        list.retain(|p| p != palace);
        ctx.store().set("palaces", &list)?;
    } else if !cards.is_empty() && !listed {
        list.push(palace.to_owned());
        ctx.store().set("palaces", &list)?;
    }
    Ok(())
}

const CHUNK: u32 = 256;

/// (locus, grade, answered_at, elapsed_ms)
type Logged = (String, Grade, u64, u64);

fn settings(ctx: &mut Ctx) -> Result<Settings> {
    Ok(ctx.store().get("settings")?.unwrap_or(Settings {
        weights: None,
        desired_retention: 0.9,
    }))
}

fn params(ctx: &mut Ctx) -> Result<Params> {
    let s = settings(ctx)?;
    let mut params = Params {
        desired_retention: s.desired_retention,
        ..Params::default()
    };
    if let Some(w) = s.weights.filter(|w| valid_weights(w)) {
        params.weights.copy_from_slice(&w);
    }
    Ok(params)
}

fn log_answer(ctx: &mut Ctx, palace: &str, entry: Logged) -> Result {
    let count_key = format!("log/{palace}/count");
    let count: u32 = ctx.store().get(&count_key)?.unwrap_or(0);
    let chunk_key = format!("log/{palace}/{}", count / CHUNK);
    let mut chunk: Vec<Logged> = ctx.store().get(&chunk_key)?.unwrap_or_default();
    chunk.push(entry);
    ctx.store().set(&chunk_key, &chunk)?;
    ctx.store().set(&count_key, &(count + 1))
}

fn read_log(ctx: &mut Ctx, palace: &str) -> Result<Vec<LogEntry>> {
    let count: u32 = ctx
        .store()
        .get(&format!("log/{palace}/count"))?
        .unwrap_or(0);
    let mut out = Vec::with_capacity(count as usize);
    for n in 0..count.div_ceil(CHUNK) {
        let chunk: Vec<Logged> = ctx
            .store()
            .get(&format!("log/{palace}/{n}"))?
            .unwrap_or_default();
        out.extend(chunk.into_iter().map(|(locus, grade, at, ms)| LogEntry {
            palace_id: palace.to_owned(),
            locus_id: locus,
            grade,
            answered_at: at,
            elapsed_ms: ms,
        }));
    }
    Ok(out)
}

/// Day and month (UTC) of a Unix time, e.g. `17.10.`.
fn day_month(t: u64) -> String {
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = (t / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{day}.{month}.")
}

/// The home screen tile: loci in the plan and when the next one is due.
/// Only absolute facts, so it does not go stale between updates.
fn tile_text(cards: impl IntoIterator<Item = CardState>) -> String {
    let (count, next) = cards
        .into_iter()
        .fold((0usize, None::<u64>), |(n, next), card| {
            (n + 1, Some(next.map_or(card.due, |d| d.min(card.due))))
        });
    match next {
        None => "Noch keine Loci im Plan".into(),
        Some(due) => localized(
            if count == 1 {
                "{0} Locus · fällig ab {1}"
            } else {
                "{0} Loci · fällig ab {1}"
            },
            &[&count, &day_month(due)],
        ),
    }
}

fn update_tile(ctx: &mut Ctx) -> Result {
    let mut all = Vec::new();
    for palace in palaces(ctx)? {
        all.extend(load(ctx, &palace)?.into_values());
    }
    let text = tile_text(all);
    ctx.tile("Wiederholung", text.clone())?;
    // The same card can go on any page; the app offers a "Lernen" page with it.
    ctx.block("wiederholung", "Wiederholung", text)?;
    ctx.page("lernen", "Lernen", &["wiederholung"])
}

fn clear_log(ctx: &mut Ctx, palace: &str) -> Result {
    let count: u32 = ctx
        .store()
        .get(&format!("log/{palace}/count"))?
        .unwrap_or(0);
    for n in 0..count.div_ceil(CHUNK) {
        ctx.store()
            .set(&format!("log/{palace}/{n}"), &Vec::<Logged>::new())?;
    }
    ctx.store().set(&format!("log/{palace}/count"), &0u32)?;
    let mut logged: Vec<String> = ctx.store().get("logged")?.unwrap_or_default();
    logged.retain(|p| p != palace);
    ctx.store().set("logged", &logged)
}

impl Plugin for Review {
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.on::<LocusAnswered>(|ctx, ev| {
            let mut cards = load(ctx, &ev.palace_id)?;
            let next = params(ctx)?.review(cards.get(&ev.locus_id), ev.grade, ev.answered_at);
            cards.insert(ev.locus_id.clone(), next);
            save(ctx, &ev.palace_id, &cards)?;
            let mut logged: Vec<String> = ctx.store().get("logged")?.unwrap_or_default();
            if !logged.contains(&ev.palace_id) {
                logged.push(ev.palace_id.clone());
                ctx.store().set("logged", &logged)?;
            }
            log_answer(
                ctx,
                &ev.palace_id,
                (ev.locus_id, ev.grade, ev.answered_at, ev.elapsed_ms),
            )?;
            update_tile(ctx)
        })?;
        ctx.on::<LocusDeleted>(|ctx, ev| {
            let mut cards = load(ctx, &ev.palace_id)?;
            if cards.remove(&ev.locus_id).is_some() {
                save(ctx, &ev.palace_id, &cards)?;
                update_tile(ctx)?;
            }
            Ok(())
        })?;
        ctx.on::<PalaceDeleted>(|ctx, ev| {
            save(ctx, &ev.palace_id, &Cards::new())?;
            clear_log(ctx, &ev.palace_id)?;
            update_tile(ctx)
        })?;
        ctx.provide::<Log>(|ctx, request| {
            let ids = match request.palace_id {
                Some(id) => vec![id],
                None => ctx.store().get("logged")?.unwrap_or_default(),
            };
            let mut out = Vec::new();
            for palace in ids {
                out.extend(read_log(ctx, &palace)?);
            }
            Ok(out)
        })?;
        ctx.provide::<Configure>(|ctx, request| {
            let mut current = settings(ctx)?;
            if let Some(weights) = request.weights {
                if let Some(w) = &weights
                    && !valid_weights(w)
                {
                    return Err(Error::msg("Ungültige FSRS-Gewichte"));
                }
                current.weights = weights;
            }
            if let Some(r) = request.desired_retention {
                if !(0.7..=0.99).contains(&r) {
                    return Err(Error::msg(
                        "Die Ziel-Behaltensrate muss zwischen 0,70 und 0,99 liegen",
                    ));
                }
                current.desired_retention = r;
            }
            ctx.store().set("settings", &current)?;
            Ok(current)
        })?;
        ctx.provide::<Restore>(|ctx, request| {
            if let Some((locus, _)) = request.cards.iter().find(|(_, card)| !card.is_valid()) {
                return Err(Error::msg(format!(
                    "Ungültiger Wiederholungsstand für Locus {locus}"
                )));
            }
            let mut cards = load(ctx, &request.palace_id)?;
            let count = request.cards.len() as u32;
            cards.extend(request.cards);
            save(ctx, &request.palace_id, &cards)?;
            update_tile(ctx)?;
            Ok(count)
        })?;
        ctx.provide::<Schedule>(|ctx, request| {
            let ids = match request.palace_id {
                Some(id) => vec![id],
                None => palaces(ctx)?,
            };
            let mut scheduled = Vec::new();
            for palace in ids {
                for (locus, card) in load(ctx, &palace)? {
                    scheduled.push(ScheduledLocus {
                        palace_id: palace.clone(),
                        locus_id: locus,
                        card,
                    });
                }
            }
            Ok(scheduled)
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

    fn card(due: u64) -> CardState {
        CardState {
            stability: 1.0,
            difficulty: 5.0,
            last_review: 0,
            due,
            reps: 1,
            lapses: 0,
        }
    }

    #[test]
    fn tile_names_count_and_next_due_day() {
        assert_eq!(day_month(0), "1.1.");
        assert_eq!(day_month(1_792_195_200), "17.10.");
        assert_eq!(day_month(951_782_400), "29.2.");
        assert_eq!(tile_text([]), "Noch keine Loci im Plan");
        // Templates the app fills in its language: the text, then the arguments after U+001F.
        assert_eq!(
            tile_text([card(1_792_195_200)]),
            "@@{0} Locus · fällig ab {1}\u{1f}1\u{1f}17.10."
        );
        assert_eq!(
            tile_text([card(1_792_281_600), card(1_792_195_200)]),
            "@@{0} Loci · fällig ab {1}\u{1f}2\u{1f}17.10."
        );
    }
}
