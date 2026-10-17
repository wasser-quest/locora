//! Sync of the local operation log with Locora Cloud, end to end encrypted.
//!
//! Every operation and every media file is sealed with the account key before it
//! leaves the device (see `locora-crypto`). The server stores ciphertext under keyed
//! ids and cannot read any of it.
//!
//! Push: local operations after the push cursor are sealed and sent in batches.
//! Media that an operation refers to is uploaded first, so no other device receives a
//! reference it cannot resolve. Pull: pages are queued durably first; only after all
//! pages arrived does the pull cursor acknowledge receipt. Queued operations are
//! opened, their media downloaded and checked, then applied in order; failed
//! operations are retried independently and quarantined, so one bad operation cannot
//! block later pages.

use std::fs;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use locora_cloud_api::api::{EncryptedOp, PulledOp, PushOps, PushReply};
use locora_crypto::{Dek, sha256_hex};
use locora_store::{ApplyReport, Op, PullRetry, RemoteOp, Store, media_refs};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub cloud_expires_at: Option<i64>,
}

/// The plaintext of one sealed operation. Never leaves the device unsealed.
#[derive(Debug, Serialize, Deserialize)]
struct Plain {
    hlc: String,
    entity: String,
    entity_id: String,
    op: String,
    data: Option<Value>,
}

pub fn sync(state: &CloudState, store: &Store, dek: &Dek) -> Result<SyncReport> {
    if cloud::access(state, cloud::now_secs()).is_none() {
        bail!("Keine gültige Freischaltung: `locora cloud refresh` ausführen oder das Abo prüfen");
    }
    let mut report = SyncReport::default();
    push(state, store, dek, &mut report)?;
    pull(state, store, dek, &mut report)?;
    report.cloud_expires_at = Some(cloud::complete_sync(state)?.expires_at);
    Ok(report)
}

fn push(state: &CloudState, store: &Store, dek: &Dek, report: &mut SyncReport) -> Result<()> {
    let account = &state.account.id;
    let mut cursor = store.sync_cursor("push")?;
    loop {
        let batch: Vec<Op> = store.ops_since(cursor)?.into_iter().take(BATCH).collect();
        let Some(last) = batch.last().map(|op| op.seq) else {
            return Ok(());
        };
        for op in &batch {
            for hash in media_refs(&remote(op.clone())) {
                if upload_media_if_missing(state, store, dek, &hash)? {
                    report.media_uploaded += 1;
                }
            }
        }
        let ops = batch
            .iter()
            .map(|op| seal(dek, account, op))
            .collect::<Result<Vec<_>>>()?;
        let _: PushReply = cloud::post(
            &state.server,
            &format!("/v1/sync/ops?generation={}", state.sync_generation),
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

/// One page of pulled operations, decoded loosely so a malformed wire item can be
/// quarantined instead of stalling the whole sync.
#[derive(serde::Deserialize)]
struct PullPage {
    ops: Vec<serde_json::Value>,
    cursor: i64,
}

fn pull(state: &CloudState, store: &Store, dek: &Dek, report: &mut SyncReport) -> Result<()> {
    let account = &state.account.id;
    let pending = store.pending_pull()?;
    let mut after = store.sync_cursor("pull")?;
    loop {
        let page: PullPage = cloud::get(
            &state.server,
            &format!(
                "/v1/sync/ops?after={after}&limit={BATCH}&generation={}",
                state.sync_generation
            ),
            &state.token,
        )?;
        if page.cursor < after || (!page.ops.is_empty() && page.cursor == after) {
            bail!("Die Cloud hat keinen fortschreitenden Sync-Cursor geliefert");
        }
        let done = page.ops.is_empty();
        // The server is trusted to page correctly, not with the plaintext of the
        // operations; each entry is opened and checked on its own.
        let ops: Vec<serde_json::Value> = page
            .ops
            .into_iter()
            .filter(|op| {
                op.get("device").and_then(serde_json::Value::as_str) != Some(store.node_id())
            })
            .collect();
        let entries = store.queue_pull_page(page.cursor, &ops)?;
        report.pulled += entries.len();
        for entry in entries {
            apply_entry(state, store, dek, entry, report)?;
        }
        after = page.cursor;
        if done {
            break;
        }
    }
    for entry in pending {
        apply_entry(state, store, dek, entry, report)?;
    }
    Ok(())
}

fn apply_entry(
    state: &CloudState,
    store: &Store,
    dek: &Dek,
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
    let op = match open(dek, &state.account.id, &pulled) {
        Ok(op) => op,
        Err(error) => {
            record_failure(store, entry.id, error.to_string(), true, report)?;
            return Ok(());
        }
    };
    for hash in media_refs(&op) {
        if store.media(&hash)?.is_none() {
            if let Err(error) = download_media(state, store, dek, &hash) {
                record_failure(store, entry.id, error.to_string(), false, report)?;
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

/// Seals a local operation. Its id is keyed to its timestamp, so it cannot be linked to content.
fn seal(dek: &Dek, account: &str, op: &Op) -> Result<EncryptedOp> {
    let plain = Plain {
        hlc: op.hlc.clone(),
        entity: op.entity.clone(),
        entity_id: op.entity_id.clone(),
        op: op.op.clone(),
        data: op.data.clone(),
    };
    let id = dek.op_id(&op.hlc);
    let sealed = dek.seal_op(account, &id, &serde_json::to_vec(&plain)?);
    Ok(EncryptedOp {
        op_id: id,
        payload: STANDARD.encode(sealed),
    })
}

/// Opens a pulled operation and checks that its id belongs to its timestamp.
fn open(dek: &Dek, account: &str, pulled: &PulledOp) -> Result<RemoteOp> {
    let sealed = STANDARD
        .decode(&pulled.op.payload)
        .context("Eine Operation ist kein gültiges Base64")?;
    let plain = dek
        .open_op(account, &pulled.op.op_id, &sealed)
        .context("Eine Operation lässt sich nicht öffnen (falscher Schlüssel oder beschädigt)")?;
    let plain: Plain = serde_json::from_slice(&plain).context("Eine Operation ist beschädigt")?;
    if dek.op_id(&plain.hlc) != pulled.op.op_id {
        bail!("Eine Operation passt nicht zu ihrer Kennung");
    }
    Ok(RemoteOp {
        hlc: plain.hlc,
        entity: plain.entity,
        entity_id: plain.entity_id,
        op: plain.op,
        data: plain.data,
    })
}

/// Uploads the sealed media file unless the server has it. Returns whether it uploaded.
fn upload_media_if_missing(
    state: &CloudState,
    store: &Store,
    dek: &Dek,
    hash: &str,
) -> Result<bool> {
    let id = dek.media_id(hash);
    if cloud::media_exists(state, &id)? {
        return Ok(false);
    }
    let media = store
        .media(hash)?
        .ok_or_else(|| anyhow!("Medium {hash} ist auf diesem Gerät nicht vorhanden"))?;
    let path = store
        .media_file(hash)
        .ok_or_else(|| anyhow!("Dieser Speicher hat keine Mediendateien"))?;
    let bytes = fs::read(&path).with_context(|| format!("{} lesen", path.display()))?;
    let sealed = dek.seal_media(&id, &media.mime, &bytes);
    cloud::put_media(state, &id, sealed)?;
    Ok(true)
}

/// Downloads and opens a media file. Stored only if its content hash is the one asked for.
fn download_media(state: &CloudState, store: &Store, dek: &Dek, hash: &str) -> Result<()> {
    let id = dek.media_id(hash);
    let sealed = cloud::get_media(state, &id)?;
    let (mime, bytes) = dek
        .open_media(&id, &sealed)
        .map_err(|_| anyhow!("Das Medium {hash} lässt sich nicht öffnen"))?;
    if sha256_hex(&bytes) != hash {
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
#[cfg(test)]
#[path = "cloud_sync_tests.rs"]
mod tests;
