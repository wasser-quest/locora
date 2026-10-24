//! Home screen and room view, built from the store and the review schedule.

use std::collections::HashMap;

use anyhow::Result;
use locora_api::review::CardState;
use locora_app_api::{
    Home, LocusView, Media, MediaKind, PalaceCard, PalaceView, PluginBlock, PluginPage, RoomView,
    Status, Strength, TileView,
};
use locora_runtime::game::DueDay;
use locora_runtime::{Profile, count_due};
use locora_store::{PalaceTree, SPLINELY, Store};

/// A stored file as the UI sees it; `None` when the hash is unknown.
pub fn media(store: &Store, hash: &str) -> Option<Media> {
    let found = store.media(hash).ok()??;
    let kind = if found.mime == SPLINELY {
        MediaKind::Sply
    } else if found.mime.starts_with("video/") {
        MediaKind::Video
    } else if locora_store::is_audio(&found.mime) {
        MediaKind::Audio
    } else {
        MediaKind::Image
    };
    Some(Media {
        hash: found.hash,
        kind,
    })
}

/// A locus's marker state: due when a due journey would ask it today (see `DueDay::asks`).
pub fn status(cards: &HashMap<String, CardState>, locus_id: &str, day: &DueDay) -> Status {
    match cards.get(locus_id) {
        None => Status::Neu,
        Some(card) if day.asks(card) => Status::Faellig,
        Some(_) => Status::Gewusst,
    }
}

/// The palace's marker, or the built-in default.
pub fn marker(store: &Store, tree: &PalaceTree, default: &str) -> Media {
    tree.palace
        .marker
        .as_deref()
        .and_then(|hash| media(store, hash))
        .unwrap_or_else(|| Media {
            hash: default.to_owned(),
            kind: MediaKind::Sply,
        })
}

pub fn home(profile: &mut Profile) -> Result<Home> {
    let cards = profile.schedule(None)?;
    let budget = profile.new_budget()?;
    // Brings the game's tile up to date; the home works without it.
    if let Err(error) = profile.gamification_status(false) {
        profile
            .notes
            .push(format!("Fortschritt nicht verfügbar: {error:#}"));
    }
    let has_review = profile.has_review();
    let day = profile.due_day()?;
    // What spaced reviews have shown of each locus, for the palaces' strength.
    let shown = if has_review {
        profile.spaced_stability()?
    } else {
        HashMap::new()
    };
    // Tiles published while scheduling are stored before they are read.
    let reported = profile.report();
    let (blocks, pages) = declared(profile.ui_declarations()?);
    let mut home = Home {
        blocks,
        pages,
        tutorial: locora_runtime::tutorial::find(profile)?.is_some(),
        formula_guide: locora_runtime::formula_guide::find(profile)?.is_some(),
        notes: profile.notes.iter().cloned().chain(reported).collect(),
        tiles: profile
            .tiles()?
            .into_iter()
            .map(|(plugin, tile)| TileView {
                plugin,
                title: tile.title,
                text: tile.text,
                icon: None,
            })
            .collect(),
        ..Home::default()
    };
    let mut fresh_total = 0;
    for summary in profile.store.palaces()? {
        let tree = profile.store.palace(&summary.id)?;
        let (due, fresh) = count_due(&tree, &cards, &day);
        home.due += due as u32;
        fresh_total += fresh;
        home.palaces.push(PalaceCard {
            strength: has_review.then(|| strength(&tree, &cards, &shown)),
            built_in: locora_runtime::built_in_kind(&summary.id).map(str::to_owned),
            id: summary.id,
            name: summary.name,
            rooms: summary.rooms,
            loci: summary.loci,
            due: due as u32,
            fresh: fresh as u32,
            cover: tree
                .rooms
                .iter()
                .find_map(|r| r.room.media.as_deref())
                .and_then(|hash| media(&profile.store, hash)),
            previews: tree
                .rooms
                .iter()
                .filter_map(|room| {
                    let hash = room.room.media.as_deref()?;
                    Some(locora_app_api::RoomPreview {
                        id: room.room.id.clone(),
                        name: room.room.name.clone(),
                        media: media(&profile.store, hash)?,
                    })
                })
                .collect(),
        });
    }
    home.fresh = fresh_total.min(budget) as u32;
    Ok(home)
}

pub fn palace(profile: &mut Profile, id: &str, default_marker: &str) -> Result<PalaceView> {
    let tree = profile.store.palace(id)?;
    let cards = profile.schedule(Some(&tree.palace.id))?;
    let mut view = palace_view(&profile.store, &tree, &cards, default_marker);
    if profile.has_review() {
        let shown = profile.spaced_stability()?;
        view.strength = Some(strength(&tree, &cards, &shown));
    }
    Ok(view)
}

/// How many loci of a palace are consolidated: they have shown in spaced reviews that they hold
/// for 30 days or more.
pub fn strength(
    tree: &PalaceTree,
    cards: &HashMap<String, CardState>,
    shown: &HashMap<String, f64>,
) -> Strength {
    let strength = locora_runtime::game::strength(tree, cards, shown);
    Strength {
        loci: strength.loci,
        solid: strength.hold_30,
    }
}

/// Projection is infallible, including immediately after an editor mutation.
pub fn palace_view(
    store: &Store,
    tree: &PalaceTree,
    cards: &HashMap<String, CardState>,
    default_marker: &str,
) -> PalaceView {
    let clock = locora_runtime::settings::day_clock(store).unwrap_or_default();
    let day = DueDay::at(&clock, locora_runtime::now());
    let mut number = 0;
    let rooms = tree
        .rooms
        .iter()
        .map(|room| RoomView {
            video: room.room.kind == locora_store::RoomKind::Video,
            id: room.room.id.clone(),
            name: room.room.name.clone(),
            backdrop: room.room.media.as_deref().and_then(|h| media(store, h)),
            loci: room
                .loci
                .iter()
                .map(|l| {
                    number += 1;
                    LocusView {
                        answer: l.answer.clone(),
                        image: l.image.clone(),
                        bound: !l.bindings.is_empty(),
                        bound_fields: l
                            .bindings
                            .iter()
                            .map(|b| b.field.name().to_owned())
                            .collect(),
                        id: l.id.clone(),
                        number,
                        time: l.time,
                        place: l.place.clone(),
                        item: l.item.clone(),
                        x: l.x,
                        y: l.y,
                        status: status(cards, &l.id, &day),
                        picture: l.image.as_deref().and_then(|h| media(store, h)),
                        image_size: l.image_size,
                        zoom: l.zoom,
                        note: l.note.clone(),
                        audio: l.audio.clone(),
                    }
                })
                .collect(),
        })
        .collect();
    PalaceView {
        id: tree.palace.id.clone(),
        name: tree.palace.name.clone(),
        marker: marker(store, tree, default_marker),
        rooms,
        strength: None,
    }
}

/// The cards and suggested pages plugins declared, in plugin order.
fn declared(
    plugins: Vec<(String, locora_runtime::PluginUi)>,
) -> (Vec<PluginBlock>, Vec<PluginPage>) {
    let mut blocks = Vec::new();
    let mut pages = Vec::new();
    for (plugin, ui) in plugins {
        blocks.extend(ui.blocks.into_values().map(|block| PluginBlock {
            plugin: plugin.clone(),
            id: block.id,
            title: block.title,
            text: block.text,
        }));
        pages.extend(ui.pages.into_values().map(|page| PluginPage {
            plugin: plugin.clone(),
            id: page.id,
            name: page.name,
            blocks: page.blocks,
        }));
    }
    (blocks, pages)
}
