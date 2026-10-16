//! Home screen and room view, built from the store and the review schedule.

use std::collections::HashMap;

use anyhow::Result;
use locora_api::review::CardState;
use locora_app_api::{
    Home, LocusView, Media, MediaKind, PalaceCard, PalaceView, RoomView, Status, TileView,
};
use locora_runtime::{Profile, count_due, now};
use locora_store::{PalaceTree, SPLINELY, Store};

/// A stored file as the UI sees it; `None` when the hash is unknown.
pub fn media(store: &Store, hash: &str) -> Option<Media> {
    let found = store.media(hash).ok()??;
    let kind = if found.mime == SPLINELY {
        MediaKind::Sply
    } else if found.mime.starts_with("video/") {
        MediaKind::Video
    } else {
        MediaKind::Image
    };
    Some(Media {
        hash: found.hash,
        kind,
    })
}

pub fn status(cards: &HashMap<String, CardState>, locus_id: &str, now: u64) -> Status {
    match cards.get(locus_id) {
        None => Status::Neu,
        Some(card) if card.due <= now => Status::Faellig,
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
    let now = now();
    // Tiles published while scheduling are stored before they are read.
    let reported = profile.report();
    let mut home = Home {
        tutorial: locora_runtime::tutorial::find(profile)?.is_some(),
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
        let (due, fresh) = count_due(&tree, &cards, now);
        home.due += due as u32;
        fresh_total += fresh;
        home.palaces.push(PalaceCard {
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
        });
    }
    home.fresh = fresh_total.min(budget) as u32;
    Ok(home)
}

pub fn palace(profile: &mut Profile, id: &str, default_marker: &str) -> Result<PalaceView> {
    let tree = profile.store.palace(id)?;
    let cards = profile.schedule(Some(&tree.palace.id))?;
    Ok(palace_view(&profile.store, &tree, &cards, default_marker))
}

/// Projection is infallible, including immediately after an editor mutation.
pub fn palace_view(
    store: &Store,
    tree: &PalaceTree,
    cards: &HashMap<String, CardState>,
    default_marker: &str,
) -> PalaceView {
    let now = now();
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
                        id: l.id.clone(),
                        number,
                        place: l.place.clone(),
                        item: l.item.clone(),
                        x: l.x,
                        y: l.y,
                        status: status(cards, &l.id, now),
                        picture: l.image.as_deref().and_then(|h| media(store, h)),
                        image_size: l.image_size,
                        zoom: l.zoom,
                        note: l.note.clone(),
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
    }
}
