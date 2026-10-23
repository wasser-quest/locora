use super::*;
use crate::cloud_http;
use locora_cloud_api::api::{Account, SubscriptionState};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

fn responses(replies: Vec<(u16, Value)>) -> (CloudState, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        replies
            .into_iter()
            .map(|(status, reply)| {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut reader = BufReader::new(&mut socket);
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                }
                let body = reply.to_string();
                write!(socket, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                request
            })
            .collect()
    });
    (
        CloudState {
            server,
            token: "synthetic-test-token".into(),
            sync_generation: 0,
            entitlement: None,
            last_sync_at: None,
            auto_sync: false,
            game_delete_pending: false,
            account: Account {
                id: "local".into(),
                email: "test@example.invalid".into(),
                display_name: "Test".into(),
                subscription: SubscriptionState::None,
                plan: None,
                period_end: None,
                email_verified: None,
            },
        },
        worker,
    )
}

/// The pull of the tests: the pages of the script, applied as the sync applies them.
fn pull(state: &CloudState, store: &Store, dek: &Dek, report: &mut SyncReport) -> Result<()> {
    *report = cloud_http::run(SyncFlow::pull_only(state.clone(), store, dek)?)?;
    Ok(())
}

fn dek() -> &'static Dek {
    static KEY: OnceLock<Dek> = OnceLock::new();
    KEY.get_or_init(|| {
        locora_crypto::create("test passphrase for sync")
            .unwrap()
            .dek
    })
}

fn op(seq: i64, entity: &str, id: &str, action: &str, data: Value) -> Value {
    let hlc = format!("{seq:016}-000000-remote");
    let op_id = dek().op_id(&hlc);
    let plain = Plain {
        hlc,
        entity: entity.into(),
        entity_id: id.into(),
        op: action.into(),
        data: Some(data),
    };
    let sealed = dek().seal_op("local", &op_id, &serde_json::to_vec(&plain).unwrap());
    json!({"seq": seq, "device": "remote", "op_id": op_id,
        "payload": STANDARD.encode(sealed)})
}

fn room() -> Value {
    op(
        8,
        "room",
        "room",
        "create",
        json!({"palace_id": "palace", "name": "Room", "kind": "blank", "position": 0}),
    )
}

fn page(cursor: i64, ops: Vec<Value>) -> (u16, Value) {
    (200, json!({"cursor": cursor, "ops": ops}))
}

#[test]
fn missing_op_does_not_block_later_parent_and_recovers_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store.set_sync_cursor("pull", 7).unwrap();
    let parent = op(9, "palace", "palace", "create", json!({"name": "Palace"}));
    let (state, worker) = responses(vec![
        page(8, vec![room()]),
        page(9, vec![parent]),
        page(9, vec![]),
    ]);
    let mut report = SyncReport::default();
    pull(&state, &store, dek(), &mut report).unwrap();
    assert_eq!(store.sync_cursor("pull").unwrap(), 9);
    assert_eq!(report.pulled, 2);
    assert_eq!(report.applied.applied, 1);
    assert_eq!(report.applied.missing, 1);
    assert_eq!(store.pending_pull().unwrap()[0].attempts, 1);
    for (request, after) in worker.join().unwrap().iter().zip([7, 8, 9]) {
        assert!(request.contains(&format!("after={after}&")), "{request}");
    }
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let (state, worker) = responses(vec![page(9, vec![])]);
    let mut report = SyncReport::default();
    pull(&state, &store, dek(), &mut report).unwrap();
    assert_eq!(report.applied.applied, 1);
    assert_eq!(report.pulled, 0);
    assert!(store.pending_pull().unwrap().is_empty());
    assert_eq!(store.palace("Palace").unwrap().rooms.len(), 1);
    assert!(worker.join().unwrap()[0].contains("after=9&"));
}

#[test]
fn pending_retry_waits_for_successful_pages_and_a_later_parent() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let entry = store.queue_pull_page(8, &[room()]).unwrap().remove(0);
    for _ in 0..2 {
        assert!(
            store
                .fail_pull(entry.id, "missing parent", false)
                .unwrap()
                .is_none()
        );
    }
    let other = op(9, "palace", "other", "create", json!({"name": "Other"}));
    let (state, worker) = responses(vec![
        page(9, vec![other]),
        (503, json!({"error": "temporarily unavailable"})),
    ]);
    let mut report = SyncReport::default();
    assert!(pull(&state, &store, dek(), &mut report).is_err());
    assert_eq!(store.sync_cursor("pull").unwrap(), 9);
    assert!(report.quarantined.is_empty());
    let pending = store.pending_pull().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, entry.id);
    assert_eq!(pending[0].attempts, 2);
    assert_eq!(worker.join().unwrap().len(), 2);

    let parent = op(10, "palace", "palace", "create", json!({"name": "Palace"}));
    let (state, worker) = responses(vec![page(10, vec![parent]), page(10, vec![])]);
    let mut report = SyncReport::default();
    pull(&state, &store, dek(), &mut report).unwrap();
    assert_eq!(report.applied.applied, 2);
    assert_eq!(report.applied.missing, 0);
    assert!(report.quarantined.is_empty());
    assert!(store.pending_pull().unwrap().is_empty());
    assert_eq!(store.palace("Palace").unwrap().rooms.len(), 1);
    assert_eq!(worker.join().unwrap().len(), 2);
}

#[test]
fn invalid_wire_and_payload_are_quarantined_without_blocking_later_pages() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let mut malformed = room();
    malformed["op_id"] = json!(42);
    let invalid = op(9, "palace", "invalid", "create", json!({"name": ""}));
    let invalid_locus = op(10, "locus", "locus", "create", json!({"id": "locus"}));
    let invalid_update = op(11, "locus", "unknown", "update", json!({"x": "bad"}));
    let mut invalid_room = room();
    invalid_room["seq"] = json!(12);
    invalid_room["payload"] = json!("not base64");
    let valid = op(13, "palace", "palace", "create", json!({"name": "Palace"}));
    let (state, worker) = responses(vec![
        page(
            12,
            vec![
                malformed,
                invalid,
                invalid_locus,
                invalid_update,
                invalid_room,
            ],
        ),
        page(13, vec![valid]),
        page(13, vec![]),
    ]);
    let mut report = SyncReport::default();
    pull(&state, &store, dek(), &mut report).unwrap();
    assert_eq!(store.sync_cursor("pull").unwrap(), 13);
    assert_eq!(report.quarantined.len(), 5);
    assert_eq!(report.applied.failed.len(), 5);
    assert_eq!(report.applied.applied, 1);
    assert!(store.pending_pull().unwrap().is_empty());
    worker.join().unwrap();

    let (state, worker) = responses(vec![page(13, vec![])]);
    let mut report = SyncReport::default();
    pull(&state, &store, dek(), &mut report).unwrap();
    assert!(report.quarantined.is_empty());
    assert!(report.applied.failed.is_empty());
    worker.join().unwrap();
}

#[test]
fn missing_and_media_failures_are_quarantined_after_three_attempts() {
    for media_failure in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let operation = if media_failure {
            op(
                8,
                "palace",
                "palace",
                "update",
                json!({"marker": "a".repeat(64)}),
            )
        } else {
            room()
        };
        for attempt in 1..=4 {
            let mut replies = vec![page(
                8,
                if attempt == 1 {
                    vec![operation.clone()]
                } else {
                    vec![]
                },
            )];
            if media_failure && attempt <= 3 {
                replies.push((503, json!({"error": "temporarily unavailable"})));
            }
            if attempt == 1 {
                replies.push(page(8, vec![]));
            }
            let (state, worker) = responses(replies);
            let mut report = SyncReport::default();
            pull(&state, &store, dek(), &mut report).unwrap();
            assert_eq!(store.sync_cursor("pull").unwrap(), 8);
            assert_eq!(report.quarantined.len(), usize::from(attempt == 3));
            let pending = store.pending_pull().unwrap();
            if attempt < 3 {
                assert_eq!(pending.len(), 1);
                assert_eq!(pending[0].attempts, attempt);
            } else {
                assert!(pending.is_empty());
            }
            worker.join().unwrap();
            drop(store);
            store = Store::open(dir.path()).unwrap();
        }
    }
}

#[test]
fn invalid_ciphertext_is_quarantined_and_own_operations_advance_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let mut invalid = op(
        8,
        "palace",
        "palace",
        "update",
        json!({"marker": "../invalid"}),
    );
    invalid["payload"] = json!("not base64");
    let mut own = room();
    own["seq"] = json!(10);
    own["device"] = json!(store.node_id());
    let (state, worker) = responses(vec![page(10, vec![invalid, own]), page(10, vec![])]);
    let mut report = SyncReport::default();
    pull(&state, &store, dek(), &mut report).unwrap();
    assert_eq!(report.pulled, 1);
    assert_eq!(report.quarantined.len(), 1);
    assert_eq!(report.media_downloaded, 0);
    assert!(store.pending_pull().unwrap().is_empty());
    assert_eq!(store.sync_cursor("pull").unwrap(), 10);
    assert_eq!(worker.join().unwrap().len(), 2);
}

#[test]
fn app_sync_requires_a_saved_account_before_contacting_cloud() {
    let data = tempfile::tempdir().unwrap();
    let store = Store::in_memory().unwrap();
    let error = sync_with_passphrase(data.path(), &store, "unused passphrase")
        .unwrap_err()
        .to_string();
    assert!(error.contains("Nicht bei Locora Cloud angemeldet"));
}

#[test]
fn app_sync_checks_access_before_fetching_keys() {
    let data = tempfile::tempdir().unwrap();
    let store = Store::in_memory().unwrap();
    let state = CloudState {
        server: "http://127.0.0.1:1".into(),
        token: "unused".into(),
        account: locora_cloud_api::api::Account {
            id: "test".into(),
            email: "test@example.invalid".into(),
            display_name: "Test".into(),
            subscription: locora_cloud_api::api::SubscriptionState::None,
            plan: None,
            period_end: None,
            email_verified: None,
        },
        sync_generation: 0,
        entitlement: None,
        last_sync_at: None,
        auto_sync: false,
        game_delete_pending: false,
    };
    cloud::save(data.path(), &state).unwrap();
    let error = sync_with_passphrase(data.path(), &store, "unused passphrase")
        .unwrap_err()
        .to_string();
    assert!(error.contains("Keine gültige Cloud-Freischaltung"));
}

#[test]
fn a_new_generation_of_the_server_starts_the_device_over() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store.set_sync_cursor("push", 5).unwrap();
    store.set_sync_cursor("pull", 9).unwrap();
    let passphrase = "passphrase for the new generation";
    let account = locora_crypto::create(passphrase).unwrap();
    let (mut state, _) = responses(Vec::new());
    state.server = "https://cloud.example".into();
    let mut flow = UnlockSync::new(state, &store, passphrase.into());
    let reply = |body: Value| {
        Some(Reply {
            status: 200,
            body: body.to_string().into_bytes(),
        })
    };
    let url = |step: Step<SyncReport>| match step {
        Step::Call(call) => call.url,
        _ => panic!("the sync ended early"),
    };
    assert!(url(flow.step(None).unwrap()).ends_with("/v1/keys"));
    assert!(url(flow.step(reply(json!(account.bundle))).unwrap()).ends_with("/v1/sync/retention"));
    // Expired data is restored from this device: the rebuild is asked for first.
    let expired =
        json!({"state": "expired", "last_successful_sync_at": 0, "expires_at": 0, "generation": 2});
    assert!(url(flow.step(reply(expired)).unwrap()).ends_with("/v1/sync/rebuild"));
    let rebuilding = json!({"state": "rebuilding", "last_successful_sync_at": 0, "expires_at": 0, "generation": 2});
    // The device adopts the generation before the sync itself (which needs an unlock this test
    // cannot sign, so it stops there).
    let refused = flow
        .step(reply(rebuilding))
        .err()
        .expect("no unlock in this test")
        .to_string();
    assert!(refused.contains("Freischaltung"), "{refused}");
    assert_eq!(flow.new_generation, Some(2));
    assert_eq!(store.sync_cursor("push").unwrap(), 0);
    assert_eq!(store.sync_cursor("pull").unwrap(), 0);
}
