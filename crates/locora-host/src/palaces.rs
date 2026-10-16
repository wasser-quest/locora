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
                    image_size: new.image_size,
                    place: new.place,
                    item: new.item,
                    x: new.x,
                    y: new.y,
                    time: new.time,
                    answer: new.answer,
                    image: new.image,
                    note: new.note,
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
                    image_size: patch.image_size,
                    place: patch.place,
                    item: patch.item,
                    x: patch.x,
                    y: patch.y,
                    time: patch.time,
                    image: patch.image,
                    answer: patch.answer,
                    note: patch.note,
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
        note: l.note,
        image: l.image,
        image_size: l.image_size,
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

#[cfg(test)]
mod tests {
    use super::*;
    use locora_api::palaces::{LocusPatch, NewLocus, RoomKind};

    fn new_locus(image: Option<String>) -> NewLocus {
        NewLocus {
            place: "Tor".into(),
            item: "Sonne".into(),
            x: 0.5,
            y: 0.5,
            time: None,
            answer: Default::default(),
            note: String::new(),
            image,
            image_size: 0.4,
        }
    }

    #[test]
    fn locus_pictures_travel_through_the_service() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let picture = store.put_media(b"\x89PNG", "image/png").unwrap().hash;
        let video = store.put_media(b"mp4", "video/mp4").unwrap().hash;
        let palace = store.create_palace("P").unwrap();
        let room = match call(
            &store,
            Request::AddRoom {
                palace_id: palace.id.clone(),
                name: "R".into(),
                kind: RoomKind::Blank,
                media: None,
            },
        )
        .unwrap()
        .response
        {
            Response::Room(r) => r,
            _ => panic!("room"),
        };
        let add = |image| Request::AddLocus {
            room_id: room.id.clone(),
            locus: new_locus(image),
        };
        let Response::Locus(locus) = call(&store, add(Some(picture.clone()))).unwrap().response
        else {
            panic!("locus")
        };
        assert_eq!(
            (locus.image.as_deref(), locus.image_size),
            (Some(picture.as_str()), 0.4)
        );
        assert!(
            call(&store, add(Some(video))).is_err(),
            "a video is no locus picture"
        );
        assert!(
            call(&store, add(Some("0".repeat(64)))).is_err(),
            "unknown media"
        );

        let patch = |image, image_size| Request::UpdateLocus {
            id: locus.id.clone(),
            patch: LocusPatch {
                image,
                image_size,
                ..Default::default()
            },
        };
        call(&store, patch(Some(None), Some(0.6))).unwrap();
        let stored = store.palace(&palace.id).unwrap();
        let l = stored.route().next().unwrap();
        assert_eq!((l.image.as_deref(), l.image_size), (None, 0.6));
        assert!(
            call(&store, patch(None, Some(2.0))).is_err(),
            "size out of range"
        );
    }
}
