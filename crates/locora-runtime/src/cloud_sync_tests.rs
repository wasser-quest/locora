use super::*;
use locora_cloud_api::api::{Account, SubscriptionState};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
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
            entitlement: None,
            account: Account {
                id: "local".into(),
                email: "test@example.invalid".into(),
                display_name: "Test".into(),
                subscription: SubscriptionState::None,
                plan: None,
                period_end: None,
            },
        },
        worker,
    )
}

fn op(seq: i64, entity: &str, id: &str, action: &str, data: Value) -> Value {
    json!({"seq": seq, "device": "remote", "hlc": format!("{seq:016}-000000-remote"),
        "entity": entity, "entity_id": id, "op": action, "data": data})
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
    pull(&state, &store, &mut report).unwrap();
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
    pull(&state, &store, &mut report).unwrap();
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
    assert!(pull(&state, &store, &mut report).is_err());
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
    pull(&state, &store, &mut report).unwrap();
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
    malformed["hlc"] = json!(42);
    let invalid = op(9, "palace", "invalid", "create", json!({"name": ""}));
    let invalid_locus = op(10, "locus", "locus", "create", json!({"id": "locus"}));
    let invalid_update = op(11, "locus", "unknown", "update", json!({"x": "bad"}));
    let mut invalid_room = room();
    invalid_room["seq"] = json!(12);
    invalid_room["data"]["kind"] = json!("invalid");
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
    pull(&state, &store, &mut report).unwrap();
    assert_eq!(store.sync_cursor("pull").unwrap(), 13);
    assert_eq!(report.quarantined.len(), 5);
    assert_eq!(report.applied.failed.len(), 5);
    assert_eq!(report.applied.applied, 1);
    assert!(store.pending_pull().unwrap().is_empty());
    worker.join().unwrap();

    let (state, worker) = responses(vec![page(13, vec![])]);
    let mut report = SyncReport::default();
    pull(&state, &store, &mut report).unwrap();
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
            pull(&state, &store, &mut report).unwrap();
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
fn invalid_media_is_quarantined_immediately_and_own_operations_advance_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let invalid = op(
        8,
        "palace",
        "palace",
        "update",
        json!({"marker": "../invalid"}),
    );
    let mismatch = op(
        9,
        "palace",
        "palace",
        "update",
        json!({"marker": "a".repeat(64)}),
    );
    let mut own = room();
    own["seq"] = json!(10);
    own["device"] = json!(store.node_id());
    let (state, worker) = responses(vec![
        page(10, vec![invalid, mismatch, own]),
        (200, json!("wrong media bytes")),
        page(10, vec![]),
    ]);
    let mut report = SyncReport::default();
    pull(&state, &store, &mut report).unwrap();
    assert_eq!(report.pulled, 2);
    assert_eq!(report.quarantined.len(), 2);
    assert_eq!(report.media_downloaded, 0);
    assert!(store.pending_pull().unwrap().is_empty());
    assert_eq!(store.sync_cursor("pull").unwrap(), 10);
    assert_eq!(worker.join().unwrap().len(), 3);
}
