//! Sync of the local operation log with Locora Cloud.
//!
//! Push: local operations after the push cursor are sent in batches. Media that
//! an operation refers to is uploaded first, so no other device ever receives a
//! reference it cannot resolve. Pull: operations of other devices after the pull
//! cursor are fetched, their media downloaded and verified, then applied in
//! order. A cursor only moves after the step it describes has succeeded.

use std::fs;

use anyhow::{Context, Result, anyhow, bail};
use locora_cloud_api::api::{PullReply, PulledOp, PushOps, PushReply, WireOp};
use locora_store::{ApplyReport, Op, RemoteOp, Store, media_refs};
use sha2::{Digest, Sha256};

use crate::cloud::{self, CloudState};

/// Operations per request; also the page size when pulling.
pub const BATCH: usize = 500;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub pushed: usize,
    pub media_uploaded: usize,
    pub pulled: usize,
    pub media_downloaded: usize,
    pub applied: ApplyReport,
}

pub fn sync(state: &CloudState, store: &Store) -> Result<SyncReport> {
    if cloud::access(state, cloud::now_secs()).is_none() {
        bail!("Keine gültige Freischaltung: `locora cloud refresh` ausführen oder das Abo prüfen");
    }
    let mut report = SyncReport::default();
    push(state, store, &mut report)?;
    pull(state, store, &mut report)?;
    Ok(report)
}

fn push(state: &CloudState, store: &Store, report: &mut SyncReport) -> Result<()> {
    let mut cursor = store.sync_cursor("push")?;
    loop {
        let batch: Vec<Op> = store.ops_since(cursor)?.into_iter().take(BATCH).collect();
        let Some(last) = batch.last().map(|op| op.seq) else {
            return Ok(());
        };
        for op in &batch {
            for hash in media_refs(&remote(op.clone())) {
                if upload_media_if_missing(state, store, &hash)? {
                    report.media_uploaded += 1;
                }
            }
        }
        let ops = batch.iter().map(wire).collect();
        let _: PushReply = cloud::post(
            &state.server,
            "/v1/sync/ops",
            Some(&state.token),
            &PushOps {
                device: store.node_id().to_owned(),
                ops,
            },
        )?;
        store.set_sync_cursor("push", last)?;
        report.pushed += batch.len();
        cursor = last;
    }
}

fn pull(state: &CloudState, store: &Store, report: &mut SyncReport) -> Result<()> {
    let mut after = store.sync_cursor("pull")?;
    loop {
        let page: PullReply = cloud::get(
            &state.server,
            &format!("/v1/sync/ops?after={after}&limit={BATCH}"),
            &state.token,
        )?;
        if page.ops.is_empty() {
            return Ok(());
        }
        let ops: Vec<RemoteOp> = page
            .ops
            .iter()
            .filter(|op| op.device != store.node_id())
            .map(|op| remote_pulled(op.clone()))
            .collect();
        report.pulled += ops.len();
        for op in &ops {
            for hash in media_refs(op) {
                if store.media(&hash)?.is_none() {
                    download_media(state, store, &hash)?;
                    report.media_downloaded += 1;
                }
            }
        }
        if !ops.is_empty() {
            let applied = store.apply_remote(&ops)?;
            let retry = applied.missing > 0 || !applied.failed.is_empty();
            report.applied.applied += applied.applied;
            report.applied.stale += applied.stale;
            report.applied.missing += applied.missing;
            report.applied.failed.extend(applied.failed);
            if retry {
                // Replay this page on the next sync; already applied ops are stale.
                return Ok(());
            }
        }
        after = page.cursor;
        store.set_sync_cursor("pull", after)?;
    }
}

/// Uploads the media file unless the server has it. Returns whether it uploaded.
fn upload_media_if_missing(state: &CloudState, store: &Store, hash: &str) -> Result<bool> {
    if cloud::media_exists(state, hash)? {
        return Ok(false);
    }
    let mime = store
        .media(hash)?
        .map(|media| media.mime)
        .ok_or_else(|| anyhow!("Medium {hash} ist auf diesem Gerät nicht vorhanden"))?;
    let path = store
        .media_file(hash)
        .ok_or_else(|| anyhow!("Dieser Speicher hat keine Mediendateien"))?;
    let bytes = fs::read(&path).with_context(|| format!("{} lesen", path.display()))?;
    cloud::put_media(state, hash, &mime, bytes)?;
    Ok(true)
}

/// Downloads a media file and stores it only if its SHA-256 is the one asked for.
fn download_media(state: &CloudState, store: &Store, hash: &str) -> Result<()> {
    let (mime, bytes) = cloud::get_media(state, hash)?;
    let actual: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != hash {
        bail!("Das Medium {hash} passt nicht zu seiner Kennung");
    }
    store.put_media(&bytes, &mime)?;
    Ok(())
}

fn remote(op: Op) -> RemoteOp {
    RemoteOp {
        hlc: op.hlc,
        entity: op.entity,
        entity_id: op.entity_id,
        op: op.op,
        data: op.data,
    }
}

fn remote_pulled(op: PulledOp) -> RemoteOp {
    RemoteOp {
        hlc: op.op.hlc,
        entity: op.op.entity,
        entity_id: op.op.entity_id,
        op: op.op.op,
        data: op.op.data,
    }
}

fn wire(op: &Op) -> WireOp {
    WireOp {
        hlc: op.hlc.clone(),
        entity: op.entity.clone(),
        entity_id: op.entity_id.clone(),
        op: op.op.clone(),
        data: op.data.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_cloud_api::api::{Account, SubscriptionState};
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    fn pages(pages: Vec<PullReply>) -> (CloudState, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            pages
                .into_iter()
                .map(|page| {
                    let (mut socket, _) = listener.accept().unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
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
                    let body = serde_json::to_string(&page).unwrap();
                    write!(
                        socket,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .unwrap();
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

    #[test]
    fn missing_and_failed_ops_keep_the_page_available_for_retry() {
        for missing in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open(dir.path()).unwrap();
            let op = PulledOp {
                seq: 8,
                device: "remote".into(),
                op: WireOp {
                    hlc: "0000000000000001-000000-remote".into(),
                    entity: "room".into(),
                    entity_id: "room".into(),
                    op: "create".into(),
                    data: Some(
                        serde_json::json!({"palace_id": "palace", "name": "Room", "kind": "blank", "position": 0}),
                    ),
                },
            };
            let mut retry = op.clone();
            if !missing {
                retry.op.data = Some(serde_json::json!({"kind": "invalid"}));
            }
            store.set_sync_cursor("pull", 7).unwrap();
            let page = PullReply {
                ops: vec![retry],
                cursor: 8,
            };
            let (state, worker) = pages(vec![page.clone(), page]);
            for _ in 0..2 {
                let mut report = SyncReport::default();
                pull(&state, &store, &mut report).unwrap();
                assert_eq!(store.sync_cursor("pull").unwrap(), 7);
                if missing {
                    assert_eq!(report.applied.missing, 1);
                } else {
                    assert_eq!(report.applied.failed.len(), 1);
                }
            }
            for request in worker.join().unwrap() {
                assert!(request.contains("after=7&"), "{request}");
            }
            if missing {
                let parent = RemoteOp {
                    hlc: "0000000000000000-000000-remote".into(),
                    entity: "palace".into(),
                    entity_id: "palace".into(),
                    op: "create".into(),
                    data: Some(serde_json::json!({"name": "Palace"})),
                };
                assert_eq!(store.apply_remote(&[parent]).unwrap().applied, 1);
                let (state, worker) = pages(vec![
                    PullReply {
                        ops: vec![op],
                        cursor: 8,
                    },
                    PullReply {
                        ops: vec![],
                        cursor: 8,
                    },
                ]);
                let mut report = SyncReport::default();
                pull(&state, &store, &mut report).unwrap();
                assert_eq!(report.applied.applied, 1);
                assert_eq!(store.sync_cursor("pull").unwrap(), 8);
                let requests = worker.join().unwrap();
                assert!(requests[0].contains("after=7&"));
                assert!(requests[1].contains("after=8&"));
            }
        }
    }
}
