//! Native palace service adapter. Authorization lives at the WIT boundary.
use locora_api::palaces::{self as api, Request, Response};
use locora_store::{self as db, Store};

pub(crate) struct Outcome {
    pub response: Response,
    pub events: Vec<(String, Vec<u8>)>,
}

impl Outcome {
    fn emit<E: locora_api::Event>(&mut self, event: E) {
        self.events.push((
            E::TOPIC.into(),
            locora_api::encode(&event).expect("serializable event"),
        ));
    }
}

pub(crate) fn call(store: &Store, request: Request) -> Result<Outcome, String> {
    execute(store, request).map_err(|e| e.to_string())
}

fn execute(store: &Store, request: Request) -> db::Result<Outcome> {
    let mut outcome = Outcome {
        response: Response::Done,
        events: vec![],
    };
    outcome.response = match request {
        Request::List => Response::List(
            store
                .palaces()?
                .into_iter()
                .map(|p| api::Summary {
                    id: p.id,
                    name: p.name,
                    rooms: p.rooms,
                    loci: p.loci,
                    updated_at: p.updated_at,
                })
                .collect(),
        ),
        Request::Get { query } => {
            let tree = store.palace(&query)?;
            Response::Palace(api::Palace {
                id: tree.palace.id,
                name: tree.palace.name,
                rooms: tree
                    .rooms
                    .into_iter()
                    .map(|r| room(r.room, r.loci))
                    .collect(),
            })
        }
        Request::Create { name } => {
            let p = store.create_palace(&name)?;
            outcome.emit(locora_api::core::PalaceCreated {
                palace_id: p.id.clone(),
                name: p.name.clone(),
            });
            Response::Created {
                id: p.id,
                name: p.name,
            }
        }
        Request::Rename { id, name } => {
            store.rename_palace(&id, &name)?;
            Response::Done
        }
        Request::Delete { id } => {
            let deleted = store.delete_palace(&id)?;
            if deleted {
                outcome.emit(locora_api::core::PalaceDeleted { palace_id: id });
            }
            Response::Deleted(deleted)
        }
        Request::AddRoom {
            palace_id,
            name,
            kind,
            media,
        } => {
            let kind = match kind {
                api::RoomKind::Blank => db::RoomKind::Blank,
                api::RoomKind::Image => db::RoomKind::Image,
                api::RoomKind::Video => db::RoomKind::Video,
            };
            if kind == db::RoomKind::Blank && media.is_some() {
                return Err(db::StoreError::Invalid(
                    "Ein leerer Raum hat kein Medium".into(),
                ));
            }
            Response::Room(room(
                store.add_room(&palace_id, &name, kind, media.as_deref())?,
                vec![],
            ))
        }
        Request::AddLocus {
            room_id,
            locus: new,
        } => {
            check_time(new.time)?;
            Response::Locus(locus(store.add_locus(
                &room_id,
                db::NewLocus {
                    place: new.place,
                    item: new.item,
                    x: new.x,
                    y: new.y,
                    time: new.time,
                    answer: new.answer,
                },
            )?))
        }
        Request::UpdateLocus { id, patch } => {
            if let Some(time) = patch.time {
                check_time(time)?;
            }
            store.update_locus(
                &id,
                &db::LocusPatch {
                    place: patch.place,
                    item: patch.item,
                    x: patch.x,
                    y: patch.y,
                    time: patch.time,
                    answer: patch.answer,
                },
            )?;
            // A write-only caller must not receive unchanged, previously stored fields.
            Response::Done
        }
        Request::DeleteLocus { id } => {
            let palace_id = store.delete_locus(&id)?;
            let deleted = palace_id.is_some();
            if let Some(palace_id) = palace_id {
                outcome.emit(locora_api::core::LocusDeleted {
                    palace_id,
                    locus_id: id,
                });
            }
            Response::Deleted(deleted)
        }
    };
    Ok(outcome)
}

fn check_time(time: Option<f64>) -> db::Result<()> {
    if time.is_some_and(|t| !t.is_finite() || t < 0.0) {
        return Err(db::StoreError::Invalid(
            "Die Videozeit muss endlich und nicht negativ sein".into(),
        ));
    }
    Ok(())
}

fn locus(l: db::Locus) -> api::Locus {
    api::Locus {
        id: l.id,
        room_id: l.room_id,
        place: l.place,
        item: l.item,
        x: l.x,
        y: l.y,
        time: l.time,
        answer: l.answer,
    }
}

fn room(r: db::Room, loci: Vec<db::Locus>) -> api::Room {
    api::Room {
        id: r.id,
        palace_id: r.palace_id,
        name: r.name,
        kind: match r.kind {
            db::RoomKind::Blank => api::RoomKind::Blank,
            db::RoomKind::Image => api::RoomKind::Image,
            db::RoomKind::Video => api::RoomKind::Video,
        },
        media: r.media,
        loci: loci.into_iter().map(locus).collect(),
    }
}
