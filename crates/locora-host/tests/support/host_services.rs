//! Host-owned services and filters exercised across the actual component boundary.
use super::{load, tiles, wasm_dir};
use locora_api::{
    Hook, Service,
    core::{JourneyScore, Score},
    palaces::{self as p, Request as Q, Response as R},
};
use locora_host::{HostEvent, MemoryKv, PluginHost};
use std::sync::Arc;

fn score(mode: &str) -> Score {
    Score {
        palace_id: mode.into(),
        locus_id: "l".into(),
        points: 1,
    }
}

fn palace_host(store: Arc<locora_store::Store>) -> PluginHost {
    PluginHost::with_palaces(None, Arc::new(MemoryKv::default()), store).unwrap()
}

fn proxy(host: &mut PluginHost, request: Q) -> anyhow::Result<R> {
    let bytes = host.call_raw("fixture.palaces@1", &locora_api::encode(&request)?)?;
    Ok(locora_api::decode(&bytes)?)
}

fn raw(host: &mut PluginHost, service: &str, bytes: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let bytes = host.call_raw("fixture.raw@1", &locora_api::encode(&(service, bytes))?)?;
    Ok(locora_api::decode(&bytes)?)
}

#[test]
fn chain_is_typed_ordered_and_independent_of_load_order() {
    let mut host = PluginHost::new(None).unwrap();
    assert_eq!(host.filter::<JourneyScore>(score("p")).unwrap(), score("p"));
    // Equal-priority handlers sort by plugin id; fixture's negative priority runs first.
    for name in ["palace_writer", "fixture", "palace_reader"] {
        load(&mut host, name);
    }
    assert_eq!(host.filter::<JourneyScore>(score("p")).unwrap().points, 7);
    assert_eq!(
        host.filter::<JourneyScore>(score("writer-error"))
            .unwrap()
            .points,
        4
    );
    assert_eq!(
        host.filter::<JourneyScore>(score("duplicate"))
            .unwrap()
            .points,
        5
    );
    // Duplicate registration neither replaces the handler nor changes priority.
    assert_eq!(host.filter::<JourneyScore>(score("p")).unwrap().points, 7);
    load(&mut host, "fixture"); // Reload must not duplicate a registration.
    assert_eq!(host.filter::<JourneyScore>(score("p")).unwrap().points, 7);
    assert!(host.unload("fixture"));
    assert_eq!(host.filter::<JourneyScore>(score("p")).unwrap().points, 5);
    load(&mut host, "fixture");
    assert_eq!(host.filter::<JourneyScore>(score("p")).unwrap().points, 7);
}

#[test]
fn filter_errors_malformed_responses_traps_and_fuel_are_isolated() {
    for mode in ["error", "panic", "loop"] {
        let mut host = PluginHost::new(None).unwrap();
        for name in ["fixture", "palace_reader", "palace_writer", "malformed"] {
            load(&mut host, name);
        }
        host.drain_events();
        assert_eq!(host.filter::<JourneyScore>(score(mode)).unwrap().points, 5);
        let events = host.drain_events();
        assert!(events.iter().any(
            |e| matches!(e, HostEvent::HandlerError { plugin, topic, .. }
            if plugin == "malformed" && topic == JourneyScore::NAME)
        ));
        if mode == "error" {
            assert!(events.iter().any(
                |e| matches!(e, HostEvent::HandlerError { plugin, .. } if plugin == "fixture")
            ));
            assert!(host.plugins().any(|p| p.id == "fixture"));
        } else {
            assert!(
                events.iter().any(
                    |e| matches!(e, HostEvent::Disabled { plugin, .. } if plugin == "fixture")
                )
            );
            assert!(!host.plugins().any(|p| p.id == "fixture"));
        }
        assert!(host.plugins().any(|p| p.id == "palace-writer"));
    }
}

#[test]
fn filter_events_are_flushed_after_chain_without_reentry() {
    let mut host = PluginHost::new(None).unwrap();
    for name in ["fixture", "palace_reader", "palace_writer"] {
        load(&mut host, name);
    }
    host.drain_events();
    assert_eq!(
        host.filter::<JourneyScore>(score("event")).unwrap().points,
        7
    );
    let events = host.drain_events();
    assert_eq!(tiles(&events), ["echo", "echo"]);
    // Only the next chain sees the state changes made by the queued event.
    assert_eq!(
        host.filter::<JourneyScore>(score("event")).unwrap().points,
        102
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, HostEvent::Published { plugin, topic, .. }
        if plugin == "fixture" && topic == "core.palace.created"))
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, HostEvent::Disabled { .. }))
    );
}

fn writes() -> Vec<Q> {
    vec![
        Q::Create { name: "X".into() },
        Q::Rename {
            id: "p".into(),
            name: "X".into(),
        },
        Q::Delete { id: "p".into() },
        Q::AddRoom {
            palace_id: "p".into(),
            name: "R".into(),
            kind: p::RoomKind::Blank,
            media: None,
        },
        Q::AddLocus {
            room_id: "r".into(),
            locus: new_locus(),
        },
        Q::UpdateLocus {
            id: "l".into(),
            patch: p::LocusPatch::default(),
        },
        Q::DeleteLocus { id: "l".into() },
    ]
}

fn new_locus() -> p::NewLocus {
    p::NewLocus {
        place: "Tür".into(),
        item: "Apfel".into(),
        x: 0.2,
        y: 0.8,
        time: None,
        answer: Default::default(),
    }
}

#[test]
fn read_and_write_capabilities_are_independent_and_checked_in_host() {
    for (plugin, denied, granted) in [
        (
            "fixture",
            {
                let mut q = writes();
                q.extend([Q::List, Q::Get { query: "p".into() }]);
                q
            },
            false,
        ),
        ("palace_reader", writes(), true),
        (
            "palace_writer",
            vec![Q::List, Q::Get { query: "p".into() }],
            false,
        ),
    ] {
        let store = Arc::new(locora_store::Store::in_memory().unwrap());
        store.create_palace("Vorhanden").unwrap();
        let before = store.ops_since(0).unwrap().len();
        let mut host = palace_host(store.clone());
        load(&mut host, plugin);
        for q in denied {
            let message = proxy(&mut host, q.clone()).unwrap_err().to_string();
            assert!(message.contains(q.permission()), "{message}");
            // Direct WIT import, not just the SDK convenience method.
            assert!(
                raw(&mut host, p::Palaces::NAME, locora_api::encode(&q).unwrap())
                    .unwrap_err()
                    .to_string()
                    .contains(q.permission())
            );
        }
        assert_eq!(store.ops_since(0).unwrap().len(), before);
        if granted {
            assert!(matches!(proxy(&mut host, Q::List).unwrap(), R::List(v) if v.len() == 1));
            assert!(matches!(
                proxy(
                    &mut host,
                    Q::Get {
                        query: "Vorhanden".into()
                    }
                )
                .unwrap(),
                R::Palace(_)
            ));
        }
        assert!(raw(&mut host, "nicht.vorhanden", vec![]).is_err());
        assert!(raw(&mut host, p::Palaces::NAME, vec![255]).is_err());
        assert!(host.plugins().any(|p| p.id == plugin.replace('_', "-")));
    }
}

#[test]
fn native_service_is_reserved_and_missing_backends_are_explicit() {
    let mut host = PluginHost::new(None).unwrap();
    load(&mut host, "malformed"); // Attempts to register the native service itself.
    assert!(!host.provides(p::Palaces::NAME));
    assert!(
        host.call::<p::Palaces>(&Q::List)
            .unwrap_err()
            .to_string()
            .contains("Kein Palastspeicher")
    );
    load(&mut host, "palace_reader");
    assert!(
        proxy(&mut host, Q::List)
            .unwrap_err()
            .to_string()
            .contains("Kein Palastspeicher")
    );
    let store = Arc::new(locora_store::Store::in_memory().unwrap());
    let mut host = palace_host(store);
    load(&mut host, "malformed");
    assert!(host.provides(p::Palaces::NAME));
    assert_eq!(host.call::<p::Palaces>(&Q::List).unwrap(), R::List(vec![]));
}

#[test]
fn palace_crud_roundtrips_validation_persistence_and_self_notifications() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(locora_store::Store::open(dir.path()).unwrap());
    let mut host = palace_host(store.clone());
    load(&mut host, "palace_writer");
    let R::Created { id, .. } = proxy(
        &mut host,
        Q::Create {
            name: "Garten".into(),
        },
    )
    .unwrap() else {
        panic!()
    };
    // Host-origin event reaches even the caller, after returning from its service handler.
    assert_eq!(tiles(&host.drain_events()), ["Garten"]);
    let R::Room(room) = proxy(
        &mut host,
        Q::AddRoom {
            palace_id: id.clone(),
            name: "Hof".into(),
            kind: p::RoomKind::Blank,
            media: None,
        },
    )
    .unwrap() else {
        panic!()
    };
    let R::Locus(locus) = proxy(
        &mut host,
        Q::AddLocus {
            room_id: room.id.clone(),
            locus: new_locus(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(locus.item, "Apfel");
    assert_eq!(
        proxy(
            &mut host,
            Q::UpdateLocus {
                id: locus.id.clone(),
                patch: p::LocusPatch {
                    item: Some("Birne".into()),
                    ..Default::default()
                },
            }
        )
        .unwrap(),
        R::Done
    );
    // Empty updates must never leak the existing locus to a write-only plugin.
    assert_eq!(
        proxy(
            &mut host,
            Q::UpdateLocus {
                id: locus.id.clone(),
                patch: Default::default()
            }
        )
        .unwrap(),
        R::Done
    );
    assert!(
        proxy(
            &mut host,
            Q::AddRoom {
                palace_id: id.clone(),
                name: "Bild".into(),
                kind: p::RoomKind::Image,
                media: Some("/etc/passwd".into())
            }
        )
        .is_err()
    );
    assert!(
        proxy(
            &mut host,
            Q::AddRoom {
                palace_id: id.clone(),
                name: "Leer".into(),
                kind: p::RoomKind::Blank,
                media: Some("x".into())
            }
        )
        .is_err()
    );
    for invalid in [f64::NAN, f64::INFINITY, -1.0] {
        let mut new = new_locus();
        new.x = invalid;
        assert!(
            proxy(
                &mut host,
                Q::AddLocus {
                    room_id: room.id.clone(),
                    locus: new
                }
            )
            .is_err()
        );
        let mut new = new_locus();
        new.time = Some(invalid);
        assert!(
            proxy(
                &mut host,
                Q::AddLocus {
                    room_id: room.id.clone(),
                    locus: new
                }
            )
            .is_err()
        );
    }
    assert!(proxy(&mut host, Q::Get { query: id.clone() }).is_err());
    assert_eq!(
        proxy(
            &mut host,
            Q::Rename {
                id: id.clone(),
                name: "Neuer Garten".into()
            }
        )
        .unwrap(),
        R::Done
    );
    drop(host);
    drop(store);

    let store = Arc::new(locora_store::Store::open(dir.path()).unwrap());
    let mut host = palace_host(store.clone());
    load(&mut host, "palace_reader");
    let R::Palace(palace) = proxy(
        &mut host,
        Q::Get {
            query: "Neuer Garten".into(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(palace.rooms[0].loci[0].item, "Birne");
    assert_eq!(palace.rooms[0].loci[0].id, locus.id);
    assert!(matches!(proxy(&mut host, Q::List).unwrap(), R::List(v) if v[0].loci == 1));
    host.unload("palace-reader");
    load(&mut host, "palace_writer");
    assert_eq!(
        proxy(
            &mut host,
            Q::DeleteLocus {
                id: locus.id.clone()
            }
        )
        .unwrap(),
        R::Deleted(true)
    );
    assert_eq!(
        proxy(&mut host, Q::DeleteLocus { id: locus.id }).unwrap(),
        R::Deleted(false)
    );
    assert_eq!(
        proxy(&mut host, Q::Delete { id: id.clone() }).unwrap(),
        R::Deleted(true)
    );
    assert_eq!(
        proxy(&mut host, Q::Delete { id }).unwrap(),
        R::Deleted(false)
    );
    assert!(store.palaces().unwrap().is_empty());
    assert!(store.ops_since(0).unwrap().len() >= 7);
}

#[test]
fn media_hashes_timestamps_and_locus_deletion_notifications() {
    use locora_api::{
        core::{Grade, LocusAnswered},
        review::{Schedule, ScheduleRequest},
    };
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(locora_store::Store::open(dir.path()).unwrap());
    let media = store.put_media(b"test video", "video/mp4").unwrap();
    let mut host = palace_host(store.clone());
    load(&mut host, "review");
    load(&mut host, "palace_writer");
    let R::Created { id, .. } = host
        .call::<p::Palaces>(&Q::Create {
            name: "Film".into(),
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(tiles(&host.drain_events()), ["Film"]);
    let R::Room(room) = proxy(
        &mut host,
        Q::AddRoom {
            palace_id: id.clone(),
            name: "Szene".into(),
            kind: p::RoomKind::Video,
            media: Some(media.hash.clone()),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(room.media, Some(media.hash));
    let mut new = new_locus();
    new.time = Some(12.5);
    let R::Locus(locus) = proxy(
        &mut host,
        Q::AddLocus {
            room_id: room.id,
            locus: new,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(locus.time, Some(12.5));
    assert!(
        proxy(
            &mut host,
            Q::UpdateLocus {
                id: locus.id.clone(),
                patch: p::LocusPatch {
                    time: Some(Some(f64::NAN)),
                    ..Default::default()
                }
            }
        )
        .is_err()
    );
    assert_eq!(
        store.palace(&id).unwrap().route().next().unwrap().time,
        Some(12.5)
    );
    proxy(
        &mut host,
        Q::UpdateLocus {
            id: locus.id.clone(),
            patch: p::LocusPatch {
                time: Some(None),
                ..Default::default()
            },
        },
    )
    .unwrap();
    assert_eq!(
        store.palace(&id).unwrap().route().next().unwrap().time,
        None
    );
    host.publish(&LocusAnswered {
        palace_id: id.clone(),
        locus_id: locus.id.clone(),
        correct: true,
        grade: Grade::Good,
        elapsed_ms: 100,
        answered_at: 100,
    })
    .unwrap();
    let request = ScheduleRequest {
        palace_id: Some(id.clone()),
    };
    assert_eq!(host.call::<Schedule>(&request).unwrap().len(), 1);
    assert_eq!(
        proxy(&mut host, Q::DeleteLocus { id: locus.id }).unwrap(),
        R::Deleted(true)
    );
    assert!(host.call::<Schedule>(&request).unwrap().is_empty());
    assert_eq!(
        host.call::<p::Palaces>(&Q::Delete { id }).unwrap(),
        R::Deleted(true)
    );
}

#[test]
fn committed_mutations_notify_review_even_on_failed_activation_or_shutdown() {
    use locora_api::{
        core::{Grade, LocusAnswered},
        review::{Schedule, ScheduleRequest},
    };
    for mode in ["activation-error", "activation-trap", "deactivation"] {
        let store = Arc::new(locora_store::Store::in_memory().unwrap());
        let palace = store.create_palace(mode).unwrap();
        let mut host = palace_host(store.clone());
        load(&mut host, "review");
        host.publish(&LocusAnswered {
            palace_id: palace.id.clone(),
            locus_id: "l".into(),
            grade: Grade::Good,
            correct: true,
            elapsed_ms: 1000,
            answered_at: 100,
        })
        .unwrap();
        let request = ScheduleRequest {
            palace_id: Some(palace.id.clone()),
        };
        assert!(!host.call::<Schedule>(&request).unwrap().is_empty());
        let result = host.load_file(&wasm_dir().join("lifecycle.wasm"));
        if mode == "deactivation" {
            result.unwrap();
            assert!(host.unload("lifecycle"));
        } else {
            assert!(result.is_err());
        }
        assert!(store.palaces().unwrap().is_empty());
        assert!(host.call::<Schedule>(&request).unwrap().is_empty());
    }
}

#[test]
fn guest_answer_dtos_round_trip_and_invalid_patches_are_atomic() {
    use locora_api::journey::{Answer, AnswerKind, QuizOption};
    let store = Arc::new(locora_store::Store::in_memory().unwrap());
    let palace = store.create_palace("P").unwrap();
    let room = store
        .add_room(&palace.id, "R", locora_store::RoomKind::Blank, None)
        .unwrap();
    let mut host = palace_host(store.clone());
    load(&mut host, "palace_writer");
    let mut locus = new_locus();
    locus.answer = Answer {
        kind: AnswerKind::Quiz,
        options: vec![
            QuizOption {
                text: "a".into(),
                correct: true,
            },
            QuizOption {
                text: "b".into(),
                correct: false,
            },
        ],
    };
    let R::Locus(created) = proxy(
        &mut host,
        Q::AddLocus {
            room_id: room.id,
            locus: locus.clone(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(created.answer, locus.answer);
    let before = store.ops_since(0).unwrap().len();
    assert!(
        proxy(
            &mut host,
            Q::UpdateLocus {
                id: created.id.clone(),
                patch: p::LocusPatch {
                    item: Some("bad".into()),
                    answer: Some(Answer {
                        kind: AnswerKind::Quiz,
                        options: vec![]
                    }),
                    ..Default::default()
                }
            }
        )
        .is_err()
    );
    assert_eq!(store.ops_since(0).unwrap().len(), before);
    assert_eq!(store.palace("P").unwrap().rooms[0].loci[0].item, "Apfel");
    proxy(
        &mut host,
        Q::UpdateLocus {
            id: created.id.clone(),
            patch: p::LocusPatch {
                answer: Some(Answer {
                    kind: AnswerKind::Guess,
                    options: vec![],
                }),
                ..Default::default()
            },
        },
    )
    .unwrap();
    load(&mut host, "palace_reader");
    let R::Palace(snapshot) = proxy(&mut host, Q::Get { query: "P".into() }).unwrap() else {
        panic!()
    };
    assert_eq!(snapshot.rooms[0].loci[0].answer.kind, AnswerKind::Guess);
    host.call::<p::Palaces>(&Q::UpdateLocus {
        id: created.id,
        patch: p::LocusPatch {
            answer: Some(Answer::default()),
            ..Default::default()
        },
    })
    .unwrap();
    assert!(
        store.palace("P").unwrap().rooms[0].loci[0]
            .answer
            .is_recall()
    );
}
