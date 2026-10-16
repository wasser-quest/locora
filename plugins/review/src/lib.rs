//! Spaced repetition: every answered locus becomes a card scheduled with FSRS.
//!
//! Storage: `palaces` lists palace ids with cards, `cards/<palace>` maps locus
//! ids to their card state.
use std::collections::BTreeMap;

use locora_fsrs::Params;
use locora_sdk::api::review::{CardState, Restore, Schedule, ScheduledLocus};
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

impl Plugin for Review {
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.on::<LocusAnswered>(|ctx, ev| {
            let mut cards = load(ctx, &ev.palace_id)?;
            let next = Params::default().review(cards.get(&ev.locus_id), ev.grade, ev.answered_at);
            cards.insert(ev.locus_id, next);
            save(ctx, &ev.palace_id, &cards)
        })?;
        ctx.on::<LocusDeleted>(|ctx, ev| {
            let mut cards = load(ctx, &ev.palace_id)?;
            if cards.remove(&ev.locus_id).is_some() {
                save(ctx, &ev.palace_id, &cards)?;
            }
            Ok(())
        })?;
        ctx.on::<PalaceDeleted>(|ctx, ev| save(ctx, &ev.palace_id, &Cards::new()))?;
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
