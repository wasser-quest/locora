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
//!
//! The sync is a flow (see `cloud_http`): a queue of jobs, where a job either does local
//! work or asks for one call. The desktop runs it with ureq, the browser with fetch.

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::ops::Deref;
#[cfg(feature = "cloud")]
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use locora_cloud_api::api::{EncryptedOp, PulledOp, PushOps};
use locora_crypto::{Dek, sha256_hex};
use locora_store::{ApplyReport, Op, PullRetry, RemoteOp, Store, media_refs};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cloud::{self, CloudState, RetentionStatus};
use crate::cloud_http::{self, Body, Call, Flow, Method, Reply, Step, json};

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
    /// Set when this sync started over with a new generation of the server (its cloud data was
    /// deleted or had expired): the caller saves it in the cloud state.
    pub new_generation: Option<i64>,
}

/// App entry point for the same encrypted sync used by the CLI. The passphrase
/// stays on this device and is only used to unlock the account key for this run.
#[cfg(feature = "cloud")]
pub fn sync_with_passphrase(data: &Path, store: &Store, passphrase: &str) -> Result<SyncReport> {
    let state = cloud::load(data)?.ok_or_else(|| anyhow!("Nicht bei Locora Cloud angemeldet"))?;
    if cloud::access(&state, cloud::now_secs()).is_none() {
        bail!("Keine gültige Cloud-Freischaltung. Abo prüfen und Freischaltung aktualisieren");
    }
    let mut state = state;
    let report = cloud_http::run(UnlockSync::new(state.clone(), store, passphrase.to_owned()))?;
    if let Some(generation) = report.new_generation {
        state.sync_generation = generation;
        cloud::save(data, &state)?;
    }
    Ok(report)
}

#[cfg(feature = "cloud")]
pub fn sync(state: &CloudState, store: &Store, dek: &Dek) -> Result<SyncReport> {
    cloud_http::run(SyncFlow::new(state.clone(), store, dek)?)
}

/// Unlocks the account key with the passphrase (after fetching the wrapped keys), then syncs.
/// `S` is the store, borrowed on the desktop and shared in the browser.
/// Before the sync, the server's generation is checked: after the cloud data was deleted (or had
/// expired, which is restored from this device), the device starts over and sends all its data.
pub struct UnlockSync<S> {
    state: CloudState,
    store: S,
    passphrase: String,
    stage: Stage,
    new_generation: Option<i64>,
    syncing: Option<SyncFlow<S, Box<Dek>>>,
}

enum Stage {
    Keys,
    Retention(Box<Dek>),
    Rebuild(Box<Dek>),
}

impl<S: Clone + Deref<Target = Store>> UnlockSync<S> {
    pub fn new(state: CloudState, store: S, passphrase: String) -> Self {
        Self {
            state,
            store,
            passphrase,
            stage: Stage::Keys,
            new_generation: None,
            syncing: None,
        }
    }

    /// Adopts the server's generation when it differs, then runs the sync itself.
    fn start(&mut self, dek: Box<Dek>, status: RetentionStatus) -> Result<Step<SyncReport>> {
        if status.generation != self.state.sync_generation {
            cloud::adopt_generation(&mut self.state, &self.store, status.generation)?;
            self.new_generation = Some(status.generation);
        }
        let mut flow = SyncFlow::new(self.state.clone(), self.store.clone(), dek)?;
        let first = flow.step(None)?;
        self.syncing = Some(flow);
        Ok(self.finish(first))
    }

    fn finish(&self, step: Step<SyncReport>) -> Step<SyncReport> {
        match step {
            Step::Done(mut report) => {
                report.new_generation = self.new_generation;
                Step::Done(report)
            }
            call => call,
        }
    }
}

impl<S: Clone + Deref<Target = Store>> Flow for UnlockSync<S> {
    type Output = SyncReport;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<SyncReport>> {
        if let Some(flow) = self.syncing.as_mut() {
            let step = flow.step(reply)?;
            return Ok(self.finish(step));
        }
        let Some(reply) = reply else {
            return Ok(Step::Call(cloud::keys_call(&self.state)?));
        };
        match std::mem::replace(&mut self.stage, Stage::Keys) {
            Stage::Keys => {
                let Some(bundle) = cloud::keys_reply(reply)? else {
                    bail!("Verschlüsselung fehlt. In den Einstellungen einrichten");
                };
                let dek = Box::new(locora_crypto::unlock(&bundle, &self.passphrase)?);
                self.stage = Stage::Retention(dek);
                Ok(Step::Call(cloud::retention_call(&self.state)?))
            }
            Stage::Retention(dek) => {
                let status: RetentionStatus = json(reply)?;
                if status.state == "expired" {
                    // Expired data is restored from this device.
                    let call = cloud::rebuild_call(&self.state, status.generation)?;
                    self.stage = Stage::Rebuild(dek);
                    return Ok(Step::Call(call));
                }
                self.start(dek, status)
            }
            Stage::Rebuild(dek) => {
                let status: RetentionStatus = json(reply)?;
                self.start(dek, status)
            }
        }
    }
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

/// One page of pulled operations, decoded loosely so a malformed wire item can be
/// quarantined instead of stalling the whole sync.
#[derive(serde::Deserialize)]
struct PullPage {
    ops: Vec<serde_json::Value>,
    cursor: i64,
}

/// One unit of work of the sync.
enum Job {
    /// The next batch of local operations, or on to the pull when there are none.
    Push,
    /// The next page of remote operations after this cursor.
    Pull {
        after: i64,
    },
    /// Whether the server holds the media file of this hash, and uploads it if not.
    CheckMedia(String),
    UploadMedia(String),
    SendBatch {
        ops: Vec<Op>,
        last: i64,
    },
    /// Opens one queued operation and works out which media it needs.
    Apply(PullRetry),
    FetchMedia {
        hash: String,
        entry: i64,
    },
    Commit {
        entry: i64,
        op: RemoteOp,
    },
    Complete,
}

/// What the reply to the last call is for.
enum Awaiting {
    Exists(String),
    Uploaded,
    Batch { last: i64, count: usize },
    Page { after: i64 },
    Fetched { hash: String, entry: i64 },
    Completed,
}

/// The sync as steps. `S` and `K` hold the store and the account key: borrowed on the desktop,
/// shared (`Arc`) in the browser, where the flow outlives one call.
pub struct SyncFlow<S, K> {
    store: S,
    dek: K,
    state: CloudState,
    jobs: VecDeque<Job>,
    awaiting: Option<Awaiting>,
    /// The queued pull entries, applied after the last page.
    pending: Vec<PullRetry>,
    /// Whether the sync ends by telling the cloud (the pull alone in the tests does not).
    complete: bool,
    /// Entries whose media could not be fetched: their remaining jobs are skipped.
    failed: HashSet<i64>,
    report: SyncReport,
}

impl<S: Deref<Target = Store>, K: Deref<Target = Dek>> SyncFlow<S, K> {
    /// Refuses to start without a valid entitlement, as the sync always did.
    pub fn new(state: CloudState, store: S, dek: K) -> Result<Self> {
        if cloud::access(&state, cloud::now_secs()).is_none() {
            bail!(
                "Keine gültige Freischaltung: `locora cloud refresh` ausführen oder das Abo prüfen"
            );
        }
        Ok(Self::unchecked(state, store, dek))
    }

    fn unchecked(state: CloudState, store: S, dek: K) -> Self {
        let mut jobs = VecDeque::new();
        jobs.push_back(Job::Push);
        Self {
            store,
            dek,
            state,
            jobs,
            awaiting: None,
            pending: Vec::new(),
            complete: true,
            failed: HashSet::new(),
            report: SyncReport::default(),
        }
    }

    /// The pull alone: no push, and no completion at the end (the page tests use this).
    #[cfg(test)]
    pub(crate) fn pull_only(state: CloudState, store: S, dek: K) -> Result<Self> {
        let mut flow = Self::unchecked(state, store, dek);
        flow.jobs.clear();
        flow.complete = false;
        flow.pending = flow.store.pending_pull()?;
        let after = flow.store.sync_cursor("pull")?;
        flow.jobs.push_back(Job::Pull { after });
        Ok(flow)
    }

    /// Runs local jobs until one needs a call, or the queue is empty.
    fn advance(&mut self) -> Result<Step<SyncReport>> {
        while let Some(job) = self.jobs.pop_front() {
            if let Some((call, awaiting)) = self.start(job)? {
                self.awaiting = Some(awaiting);
                return Ok(Step::Call(call));
            }
        }
        Ok(Step::Done(self.report.clone()))
    }

    /// Starts a job: local work is done here (None), a call is returned with what its reply means.
    fn start(&mut self, job: Job) -> Result<Option<(Call, Awaiting)>> {
        match job {
            Job::Push => self.push_next(),
            Job::Pull { after } => {
                let call = cloud::call(
                    Method::Get,
                    &self.state.server,
                    &format!(
                        "/v1/sync/ops?after={after}&limit={BATCH}&generation={}",
                        self.state.sync_generation
                    ),
                    Some(&self.state.token),
                    None,
                )?;
                Ok(Some((call, Awaiting::Page { after })))
            }
            Job::CheckMedia(hash) => {
                let id = self.dek.media_id(&hash);
                let call = cloud::media_call(&self.state, Method::Head, &id, None)?;
                Ok(Some((call, Awaiting::Exists(hash))))
            }
            Job::UploadMedia(hash) => {
                let id = self.dek.media_id(&hash);
                let media = self
                    .store
                    .media(&hash)?
                    .ok_or_else(|| anyhow!("Medium {hash} ist auf diesem Gerät nicht vorhanden"))?;
                let path = self
                    .store
                    .media_file(&hash)
                    .ok_or_else(|| anyhow!("Dieser Speicher hat keine Mediendateien"))?;
                let bytes = fs::read(&path).with_context(|| format!("{} lesen", path.display()))?;
                let sealed = self.dek.seal_media(&id, &media.mime, &bytes);
                let call =
                    cloud::media_call(&self.state, Method::Put, &id, Some(Body::Bytes(sealed)))?;
                Ok(Some((call, Awaiting::Uploaded)))
            }
            Job::SendBatch { ops, last } => {
                let account = self.state.account.id.clone();
                let sealed = ops
                    .iter()
                    .map(|op| seal(&self.dek, &account, op))
                    .collect::<Result<Vec<_>>>()?;
                let count = ops.len();
                let body = PushOps {
                    device: self.store.node_id().to_owned(),
                    ops: sealed,
                };
                let call = cloud::call(
                    Method::Post,
                    &self.state.server,
                    &format!("/v1/sync/ops?generation={}", self.state.sync_generation),
                    Some(&self.state.token),
                    Some(cloud::json_body(&body)?),
                )?;
                Ok(Some((call, Awaiting::Batch { last, count })))
            }
            Job::Apply(entry) => {
                self.apply(entry)?;
                Ok(None)
            }
            Job::FetchMedia { hash, entry } => {
                if self.failed.contains(&entry) {
                    return Ok(None);
                }
                let id = self.dek.media_id(&hash);
                let call = cloud::media_call(&self.state, Method::Get, &id, None)?;
                Ok(Some((call, Awaiting::Fetched { hash, entry })))
            }
            Job::Commit { entry, op } => {
                if !self.failed.contains(&entry) {
                    let attempt = self.store.apply_pull(entry, &op)?;
                    self.report.applied.applied += attempt.applied.applied;
                    self.report.applied.stale += attempt.applied.stale;
                    self.report.applied.missing += attempt.applied.missing;
                    self.report.applied.failed.extend(attempt.applied.failed);
                    self.report.quarantined.extend(attempt.quarantined);
                }
                Ok(None)
            }
            Job::Complete => {
                let call = cloud::complete_sync_call(&self.state)?;
                Ok(Some((call, Awaiting::Completed)))
            }
        }
    }

    /// The next batch: the media of its operations are checked first, then the batch is sent.
    fn push_next(&mut self) -> Result<Option<(Call, Awaiting)>> {
        let cursor = self.store.sync_cursor("push")?;
        let batch: Vec<Op> = self
            .store
            .ops_since(cursor)?
            .into_iter()
            .take(BATCH)
            .collect();
        let Some(last) = batch.last().map(|op| op.seq) else {
            // Nothing to push: the pull starts, and its pending entries are applied at its end.
            self.pending = self.store.pending_pull()?;
            let after = self.store.sync_cursor("pull")?;
            self.jobs.push_back(Job::Pull { after });
            return Ok(None);
        };
        self.jobs.push_front(Job::Push);
        self.jobs.push_front(Job::SendBatch {
            ops: batch.clone(),
            last,
        });
        for op in batch.iter().rev() {
            for hash in media_refs(&remote(op.clone())).into_iter().rev() {
                self.jobs.push_front(Job::CheckMedia(hash));
            }
        }
        Ok(None)
    }

    /// Opens one queued entry and queues the media it needs, then its commit.
    fn apply(&mut self, entry: PullRetry) -> Result<()> {
        let pulled: PulledOp = match serde_json::from_str(&entry.operation) {
            Ok(op) => op,
            Err(error) => {
                return self.record_failure(
                    entry.id,
                    format!("Ungültige Operation: {error}"),
                    true,
                );
            }
        };
        let op = match open(&self.dek, &self.state.account.id, &pulled) {
            Ok(op) => op,
            Err(error) => return self.record_failure(entry.id, error.to_string(), true),
        };
        let mut missing = Vec::new();
        for hash in media_refs(&op) {
            if self.store.media(&hash)?.is_none() {
                missing.push(hash);
            }
        }
        // Front of the queue, in order: the fetches, then the commit that applies the operation.
        self.jobs.push_front(Job::Commit {
            entry: entry.id,
            op,
        });
        for hash in missing.into_iter().rev() {
            self.jobs.push_front(Job::FetchMedia {
                hash,
                entry: entry.id,
            });
        }
        Ok(())
    }

    fn record_failure(&mut self, id: i64, reason: String, permanent: bool) -> Result<()> {
        self.report
            .quarantined
            .extend(self.store.fail_pull(id, &reason, permanent)?);
        self.report.applied.failed.push(reason);
        Ok(())
    }

    /// Takes the reply to the call the flow asked for last, and continues the queue.
    fn answer(&mut self, awaiting: Awaiting, reply: Reply) -> Result<()> {
        match awaiting {
            Awaiting::Exists(hash) => {
                if !cloud::exists_reply(reply)? {
                    self.jobs.push_front(Job::UploadMedia(hash));
                }
            }
            Awaiting::Uploaded => {
                cloud_http::expect_success(reply)?;
                self.report.media_uploaded += 1;
            }
            Awaiting::Batch { last, count } => {
                cloud_http::expect_success(reply)?;
                self.store.set_sync_cursor("push", last)?;
                self.report.pushed += count;
            }
            Awaiting::Page { after } => self.page(after, reply)?,
            Awaiting::Fetched { hash, entry } => {
                if let Err(error) = self.fetched(&hash, reply) {
                    self.failed.insert(entry);
                    self.record_failure(entry, error.to_string(), false)?;
                }
            }
            Awaiting::Completed => {
                let retention: RetentionStatus = json(reply)?;
                self.report.cloud_expires_at = Some(retention.expires_at);
            }
        }
        Ok(())
    }

    /// A page of remote operations: its entries are queued durably, then applied; the next page
    /// follows, or the pending entries and the completion when the page was the last.
    fn page(&mut self, after: i64, reply: Reply) -> Result<()> {
        let page: PullPage = json(reply)?;
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
                op.get("device").and_then(serde_json::Value::as_str) != Some(self.store.node_id())
            })
            .collect();
        let entries = self.store.queue_pull_page(page.cursor, &ops)?;
        self.report.pulled += entries.len();
        let mut next: Vec<Job> = entries.into_iter().map(Job::Apply).collect();
        if done {
            next.extend(
                std::mem::take(&mut self.pending)
                    .into_iter()
                    .map(Job::Apply),
            );
            if self.complete {
                next.push(Job::Complete);
            }
        } else {
            next.push(Job::Pull { after: page.cursor });
        }
        for job in next.into_iter().rev() {
            self.jobs.push_front(job);
        }
        Ok(())
    }

    /// Opens a downloaded media file, checks it against its hash, and stores it.
    fn fetched(&mut self, hash: &str, reply: Reply) -> Result<()> {
        let sealed = cloud::media_reply(reply)?;
        let id = self.dek.media_id(hash);
        let (mime, bytes) = self
            .dek
            .open_media(&id, &sealed)
            .map_err(|_| anyhow!("Das Medium {hash} lässt sich nicht öffnen"))?;
        if sha256_hex(&bytes) != hash {
            bail!("Das Medium {hash} passt nicht zu seiner Kennung");
        }
        self.store.put_media(&bytes, &mime)?;
        self.report.media_downloaded += 1;
        Ok(())
    }
}

impl<S: Deref<Target = Store>, K: Deref<Target = Dek>> Flow for SyncFlow<S, K> {
    type Output = SyncReport;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<SyncReport>> {
        if let Some(awaiting) = self.awaiting.take() {
            let reply =
                reply.ok_or_else(|| anyhow!("the sync needs the reply to its last call"))?;
            self.answer(awaiting, reply)?;
        }
        self.advance()
    }
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
