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
            report.applied.applied += applied.applied;
            report.applied.stale += applied.stale;
            report.applied.missing += applied.missing;
            report.applied.failed.extend(applied.failed);
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
