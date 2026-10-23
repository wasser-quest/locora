//! The sync against a small cloud in memory: batches, media checks, compaction, received changes.

use super::*;
use crate::cloud_http;
use crate::cloud_sync::{SyncHold, SyncOptions, UnlockSync};
use locora_cloud_api::api::{Account, SubscriptionState};
use locora_cloud_api::entitlement::Plan;
use locora_store::{ChangeKind, NewLocus, RoomKind};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

/// What the fake cloud stores, and the requests it saw.
#[derive(Default)]
struct Cloud {
    ops: Vec<(i64, String, Value)>,
    media: HashMap<String, Vec<u8>>,
    /// (method, path, body length)
    requests: Vec<(String, String, usize)>,
    /// Whether it answers `POST /v1/media/missing` (an older server does not).
    knows_missing: bool,
    asked: Vec<Vec<String>>,
    /// A smaller body limit for pushes (0: none but the 4 MiB every server takes).
    push_limit: usize,
    /// The answer to `DELETE /v1/game/data` (0: 204).
    game_status: u16,
}

fn respond(socket: &mut TcpStream, status: u16, body: &[u8]) {
    write!(
        socket,
        "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    socket.write_all(body).unwrap();
}

fn handle(cloud: &Mutex<Cloud>, mut socket: TcpStream) {
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(socket.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        if header == "\r\n" || header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let mut cloud = cloud.lock().unwrap();
    cloud
        .requests
        .push((method.clone(), path.to_owned(), body.len()));
    let param = |name: &str| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
            .unwrap_or("0")
            .to_owned()
    };
    let reply: (u16, Vec<u8>) = match (method.as_str(), path) {
        ("POST", "/v1/sync/ops") => {
            assert!(body.len() < 4 * 1024 * 1024, "a push over the server's limit");
            if cloud.push_limit > 0 && body.len() > cloud.push_limit {
                drop(cloud);
                respond(&mut socket, 413, br#"{"error":"too large"}"#);
                return;
            }
            let push: Value = serde_json::from_slice(&body).unwrap();
            let device = push["device"].as_str().unwrap().to_owned();
            let ops = push["ops"].as_array().unwrap();
            assert!(ops.len() <= 1000);
            // An older server (the one without the media question) takes 128 KiB per operation.
            let limit = if cloud.knows_missing { 512 } else { 128 } * 1024 * 4 / 3;
            if ops
                .iter()
                .any(|op| op["payload"].as_str().unwrap().len() > limit)
            {
                assert_eq!(ops.len(), 1, "a large operation goes alone");
                drop(cloud);
                respond(&mut socket, 400, br#"{"error":"invalid size"}"#);
                return;
            }
            for op in ops {
                let seq = cloud.ops.len() as i64 + 1;
                cloud.ops.push((seq, device.clone(), op.clone()));
            }
            (200, json!({"stored": ops.len()}).to_string().into_bytes())
        }
        ("GET", "/v1/sync/ops") => {
            let after: i64 = param("after").parse().unwrap();
            let limit: usize = param("limit").parse().unwrap();
            let page: Vec<Value> = cloud
                .ops
                .iter()
                .filter(|(seq, ..)| *seq > after)
                .take(limit)
                .map(|(seq, device, op)| {
                    json!({"seq": seq, "device": device, "op_id": op["op_id"], "payload": op["payload"]})
                })
                .collect();
            let cursor = page.last().map_or(after, |op| op["seq"].as_i64().unwrap());
            (200, json!({"ops": page, "cursor": cursor}).to_string().into_bytes())
        }
        ("POST", "/v1/media/missing") if cloud.knows_missing => {
            let ids: Vec<String> =
                serde_json::from_value(serde_json::from_slice::<Value>(&body).unwrap()["ids"].clone())
                    .unwrap();
            let missing: Vec<&String> =
                ids.iter().filter(|id| !cloud.media.contains_key(*id)).collect();
            let reply = json!({ "missing": missing }).to_string().into_bytes();
            cloud.asked.push(ids);
            (200, reply)
        }
        ("POST", "/v1/media/missing") => (404, Vec::new()),
        ("DELETE", "/v1/game/data") => match cloud.game_status {
            0 => (204, Vec::new()),
            status => (status, br#"{"error":"no"}"#.to_vec()),
        },
        ("POST", "/v1/sync/complete") => (
            200,
            json!({"state": "active", "last_successful_sync_at": 1, "expires_at": 99, "generation": 0})
                .to_string()
                .into_bytes(),
        ),
        (method, path) if path.starts_with("/v1/media/") => {
            let id = path.trim_start_matches("/v1/media/").to_owned();
            match method {
                "PUT" => {
                    cloud.media.insert(id, body);
                    (200, b"{}".to_vec())
                }
                "HEAD" => (if cloud.media.contains_key(&id) { 200 } else { 404 }, Vec::new()),
                _ => match cloud.media.get(&id) {
                    Some(bytes) => (200, bytes.clone()),
                    None => (404, Vec::new()),
                },
            }
        }
        _ => (404, Vec::new()),
    };
    drop(cloud);
    // A HEAD reply has no body, whatever its length says.
    if method == "HEAD" {
        write!(
            socket,
            "HTTP/1.1 {} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            reply.0
        )
        .unwrap();
    } else {
        respond(&mut socket, reply.0, &reply.1);
    }
}

/// A fake cloud on a local port; every connection is served on its own thread.
fn cloud(knows_missing: bool) -> (CloudState, Arc<Mutex<Cloud>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(Cloud {
        knows_missing,
        ..Cloud::default()
    }));
    let shared = state.clone();
    thread::spawn(move || {
        for socket in listener.incoming().flatten() {
            let shared = shared.clone();
            thread::spawn(move || handle(&shared, socket));
        }
    });
    let account = Account {
        id: "account".into(),
        email: "test@example.invalid".into(),
        display_name: "Test".into(),
        subscription: SubscriptionState::Active,
        plan: Some(Plan::Trial),
        period_end: Some(u64::MAX / 2),
        email_verified: None,
    };
    (
        CloudState {
            server,
            token: "synthetic-test-token".into(),
            account,
            sync_generation: 0,
            entitlement: None,
            last_sync_at: None,
            auto_sync: false,
            game_delete_pending: false,
        },
        state,
    )
}

fn dek() -> &'static Dek {
    static KEY: OnceLock<Dek> = OnceLock::new();
    KEY.get_or_init(|| {
        locora_crypto::create("test passphrase for sync")
            .unwrap()
            .dek
    })
}

fn locus(place: &str, image: Option<String>) -> NewLocus {
    NewLocus {
        audio: None,
        image_size: 0.25,
        zoom: 1.0,
        place: place.into(),
        item: "Item".into(),
        x: 0.4,
        y: 0.6,
        time: None,
        answer: Default::default(),
        image,
        note: String::new(),
    }
}

/// A profile with pictures (one used twice), a value set twice, many large values and one too
/// large for the server.
fn profile(dir: &std::path::Path) -> (Store, usize) {
    let store = Store::open(dir).unwrap();
    let pictures: Vec<String> = (0..3u8)
        .map(|n| store.put_media(&[n; 2000], "image/png").unwrap().hash)
        .collect();
    let palace = store.create_palace("Haus").unwrap();
    let room = store
        .add_room(&palace.id, "Küche", RoomKind::Blank, None)
        .unwrap();
    for (n, picture) in pictures.iter().chain([&pictures[0]]).enumerate() {
        store
            .add_locus(&room.id, locus(&format!("Ort {n}"), Some(picture.clone())))
            .unwrap();
    }
    store.kv_set("review", "settings", b"first").unwrap();
    store.kv_set("review", "settings", b"second").unwrap();
    for n in 0..40u8 {
        store
            .kv_set("review", &format!("cards/{n}"), &[n; 50_000])
            .unwrap();
    }
    // Over 128 KiB sealed (only newer servers take it), and over 512 KiB (none does).
    store.kv_set("review", "large", &[7; 70_000]).unwrap();
    store.kv_set("review", "huge", &[8; 300_000]).unwrap();
    (store, 3)
}

#[test]
fn a_push_sends_each_operation_once_in_batches_the_server_takes() {
    for knows_missing in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let (store, pictures) = profile(dir.path());
        let (state, cloud) = cloud(knows_missing);
        let report = cloud_http::run(SyncFlow::new(state.clone(), &store, dek()).unwrap()).unwrap();
        let logged = store.last_op_seq().unwrap() as usize;
        assert_eq!(report.compacted, 1, "the first value set twice");
        let refused = if knows_missing { 1 } else { 2 };
        assert_eq!(report.too_large, refused);
        assert_eq!(report.pushed, logged - 1 - refused);
        assert_eq!(report.media_uploaded, pictures);
        assert_eq!(store.sync_cursor("push").unwrap(), logged as i64);
        let cloud = cloud.lock().unwrap();
        assert_eq!(cloud.ops.len(), logged - 1 - refused);
        let ids: HashSet<&str> = cloud
            .ops
            .iter()
            .map(|(_, _, op)| op["op_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), cloud.ops.len(), "no operation twice");
        let pushes = cloud
            .requests
            .iter()
            .filter(|(method, path, _)| method == "POST" && path == "/v1/sync/ops")
            .count();
        assert!(pushes >= 2, "5 MB of values need more than one request");
        let puts = cloud.requests.iter().filter(|(m, ..)| m == "PUT").count();
        assert_eq!(puts, pictures, "each picture once, also the one used twice");
        let heads = cloud.requests.iter().filter(|(m, ..)| m == "HEAD").count();
        if knows_missing {
            assert_eq!(cloud.asked, [cloud.asked[0].clone()]);
            assert_eq!(
                cloud.asked[0].len(),
                pictures,
                "asked once, without repeats"
            );
            assert_eq!(heads, 0);
        } else {
            assert_eq!(heads, pictures, "an older server is asked file by file");
        }
    }
}

#[test]
fn another_device_receives_everything_and_learns_what_changed() {
    let dir = tempfile::tempdir().unwrap();
    let (store, pictures) = profile(dir.path());
    let (state, cloud) = cloud(true);
    cloud_http::run(SyncFlow::new(state.clone(), &store, dek()).unwrap()).unwrap();

    let other_dir = tempfile::tempdir().unwrap();
    let other = Store::open(other_dir.path()).unwrap();
    let report = cloud_http::run(SyncFlow::new(state.clone(), &other, dek()).unwrap()).unwrap();
    assert_eq!(report.media_downloaded, pictures, "each picture once");
    assert_eq!(report.applied.applied, report.pulled);
    assert_eq!(report.received.palaces.len(), 1);
    let palace = &report.received.palaces[0];
    assert_eq!(
        (palace.name.as_deref(), palace.kind),
        (Some("Haus"), ChangeKind::New)
    );
    assert!(report.received.other > 40);
    let gets = cloud
        .lock()
        .unwrap()
        .requests
        .iter()
        .filter(|(m, p, _)| m == "GET" && p.starts_with("/v1/media/"))
        .count();
    assert_eq!(gets, pictures);

    // A deletion on the first device reaches the other one with the palace's name.
    let id = store.find_palace("Haus").unwrap().unwrap().id;
    store.delete_palace(&id).unwrap();
    cloud_http::run(SyncFlow::new(state.clone(), &store, dek()).unwrap()).unwrap();
    let report = cloud_http::run(SyncFlow::new(state, &other, dek()).unwrap()).unwrap();
    assert_eq!(report.received.palaces.len(), 1);
    let palace = &report.received.palaces[0];
    assert_eq!(
        (palace.name.as_deref(), palace.kind),
        (Some("Haus"), ChangeKind::Deleted)
    );
    assert!(other.palaces().unwrap().is_empty());
}

#[test]
fn a_pending_palace_deletion_waits_for_its_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let palace = store.create_palace("Alt").unwrap();
    store
        .set_sync_cursor("push", store.last_op_seq().unwrap())
        .unwrap();
    store.delete_palace(&palace.id).unwrap();
    let (state, cloud) = cloud(true);
    let options = SyncOptions {
        confirmed: Some(Vec::new()),
        automatic: true,
    };
    let unlock = crate::cloud_sync::Unlock::Key(Box::new(
        Dek::from_bytes(dek().to_bytes().as_ref()).unwrap(),
    ));
    let error =
        cloud_http::run(UnlockSync::start(state.clone(), &store, unlock, options)).unwrap_err();
    match crate::cloud_sync::hold_of(&error) {
        Some(SyncHold::Confirm(palaces)) => {
            assert_eq!(palaces.len(), 1);
            assert_eq!(palaces[0].name.as_deref(), Some("Alt"));
        }
        other => panic!("no confirmation asked: {other:?}"),
    }
    assert!(
        cloud.lock().unwrap().requests.is_empty(),
        "nothing was sent"
    );
}

#[test]
fn a_batch_the_server_finds_too_large_is_sent_in_halves() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = profile(dir.path());
    let (state, cloud) = cloud(true);
    cloud.lock().unwrap().push_limit = 1_000_000;
    let report = cloud_http::run(SyncFlow::new(state, &store, dek()).unwrap()).unwrap();
    let logged = store.last_op_seq().unwrap() as usize;
    assert_eq!(report.pushed, logged - 2);
    assert_eq!(store.sync_cursor("push").unwrap(), logged as i64);
    let cloud = cloud.lock().unwrap();
    let ids: HashSet<&str> = cloud
        .ops
        .iter()
        .map(|(_, _, op)| op["op_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), logged - 2, "every operation arrived once");
}

#[test]
fn a_pending_deletion_of_the_game_scores_is_asked_first_and_kept_only_while_unanswered() {
    for (status, outcome, pending) in [
        (0, cloud::RankingsDeletion::Deleted, false),
        (405, cloud::RankingsDeletion::ServerTooOld, false),
        (503, cloud::RankingsDeletion::Pending, true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let (mut state, cloud) = cloud(true);
        cloud.lock().unwrap().game_status = status;
        state.game_delete_pending = true;
        let report = cloud_http::run(SyncFlow::new(state.clone(), &store, dek()).unwrap()).unwrap();
        assert_eq!(report.rankings, Some(outcome), "{status}");
        let first = cloud.lock().unwrap().requests[0].clone();
        assert_eq!(
            (first.0.as_str(), first.1.as_str()),
            ("DELETE", "/v1/game/data")
        );
        crate::cloud_sync::keep(&mut state, &report);
        assert_eq!(state.game_delete_pending, pending, "{status}");
        // Nothing pending: the sync does not ask.
        if !pending {
            let report =
                cloud_http::run(SyncFlow::new(state.clone(), &store, dek()).unwrap()).unwrap();
            assert_eq!(report.rankings, None);
            let asked = cloud
                .lock()
                .unwrap()
                .requests
                .iter()
                .filter(|(_, path, _)| path == "/v1/game/data")
                .count();
            assert_eq!(asked, 1);
        }
    }
}
