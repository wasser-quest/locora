//! Sync of the local operation log with Locora Cloud.
//!
//! Push: local operations after the push cursor are sent in batches. Media that
//! an operation refers to is uploaded first, so no other device ever receives a
//! reference it cannot resolve. Pull: operations of other devices after the pull
//! cursor are fetched, their media downloaded and verified, then applied in
//! order. The pull cursor acknowledges durable receipt; pending operations are
//! retried independently, so one bad operation cannot block later pages.

use std::fs;

use anyhow::{Context, Result, anyhow, bail};
use locora_cloud_api::api::{PulledOp, PushOps, PushReply, WireOp};
use locora_store::{ApplyReport, Op, PullRetry, RemoteOp, Store, StoreError, media_refs};
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
    /// Newly quarantined operations, retained locally with payload and reason.
    pub quarantined: Vec<String>,
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

// Decode each operation separately so a malformed wire item can be quarantined.
#[derive(serde::Deserialize)]
struct PullPage {
    ops: Vec<serde_json::Value>,
    cursor: i64,
}

fn pull(state: &CloudState, store: &Store, report: &mut SyncReport) -> Result<()> {
    let pending = store.pending_pull()?;
    let mut after = store.sync_cursor("pull")?;
    loop {
        let page: PullPage = cloud::get(
            &state.server,
            &format!("/v1/sync/ops?after={after}&limit={BATCH}"),
            &state.token,
        )?;
        if page.cursor < after || (!page.ops.is_empty() && page.cursor == after) {
            bail!("Die Cloud hat keinen fortschreitenden Sync-Cursor geliefert");
        }
        let done = page.ops.is_empty();
        let ops: Vec<_> = page
            .ops
            .into_iter()
            .filter(|op| {
                op.get("device").and_then(serde_json::Value::as_str) != Some(store.node_id())
            })
            .collect();
        let entries = store.queue_pull_page(page.cursor, &ops)?;
        report.pulled += entries.len();
        for entry in entries {
            apply_entry(state, store, entry, report)?;
        }
        after = page.cursor;
        if done {
            break;
        }
    }
    for entry in pending {
        apply_entry(state, store, entry, report)?;
    }
    Ok(())
}

fn apply_entry(
    state: &CloudState,
    store: &Store,
    entry: PullRetry,
    report: &mut SyncReport,
) -> Result<()> {
    let pulled: PulledOp = match serde_json::from_str(&entry.operation) {
        Ok(op) => op,
        Err(error) => {
            record_failure(
                store,
                entry.id,
                format!("Ungültige Operation: {error}"),
                true,
                report,
            )?;
            return Ok(());
        }
    };
    let op = remote_pulled(pulled);
    for hash in media_refs(&op) {
        if store.media(&hash)?.is_none() {
            if let Err(error) = download_media(state, store, &hash) {
                let permanent = matches!(
                    error.downcast_ref::<StoreError>(),
                    Some(StoreError::Invalid(_))
                );
                record_failure(
                    store,
                    entry.id,
                    format!("{} {}: {error}", op.entity, op.entity_id),
                    permanent,
                    report,
                )?;
                return Ok(());
            }
            report.media_downloaded += 1;
        }
    }
    let attempt = store.apply_pull(entry.id, &op)?;
    report.applied.applied += attempt.applied.applied;
    report.applied.stale += attempt.applied.stale;
    report.applied.missing += attempt.applied.missing;
    report.applied.failed.extend(attempt.applied.failed);
    report.quarantined.extend(attempt.quarantined);
    Ok(())
}

fn record_failure(
    store: &Store,
    id: i64,
    reason: String,
    permanent: bool,
    report: &mut SyncReport,
) -> Result<()> {
    report
        .quarantined
        .extend(store.fail_pull(id, &reason, permanent)?);
    report.applied.failed.push(reason);
    Ok(())
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
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(StoreError::Invalid(format!("Ungültige Medien-Kennung: {hash}")).into());
    }
    let (mime, bytes) = cloud::get_media(state, hash)?;
    let actual: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != hash {
        return Err(StoreError::Invalid(format!(
            "Das Medium {hash} passt nicht zu seiner Kennung"
        ))
        .into());
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
#[path = "cloud_sync_tests.rs"]
mod tests;
