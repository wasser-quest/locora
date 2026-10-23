//! The sync itself as a flow (see `cloud_http`): a queue of jobs, where a job either does local
//! work or asks for calls. The desktop runs it with ureq, the browser with fetch.
//!
//! Push: local operations after the push cursor are sealed and sent in batches of up to 1000
//! operations and about 3.5 MB (the server takes 4 MiB per request). Operations that later local
//! operations replace are left out (see `locora_store::superseded`). The media an operation refers
//! to are uploaded before its batch, so no device receives a reference it cannot resolve; one
//! request asks which of them the server lacks (an older server is asked with one HEAD each).
//!
//! Pull: pages are queued durably first; only after all pages arrived does the pull cursor
//! acknowledge receipt. Queued operations are opened, the media they need are downloaded (each
//! once, a few at a time) and checked, then they are applied in order; failed operations are
//! retried independently and quarantined, so one bad operation cannot block later pages.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::ops::Deref;

use anyhow::{Context, Result, anyhow, bail};
use locora_cloud_api::api::{EncryptedOp, MediaMissing, MediaQuery, PulledOp, PushOps};
use locora_crypto::{Dek, sha256_hex};
use locora_store::{PullRetry, RemoteOp, Store, change_kind, media_refs};

use crate::cloud::{self, CloudState, RetentionStatus};
use crate::cloud_http::{self, Body, Call, Flow, Method, PARALLEL, Reply, Step, json};
use crate::cloud_sync::{SyncReport, open, remote, seal};

/// Operations per push (the server's limit).
pub const PUSH_OPS: usize = 1000;
/// Bytes of sealed operations per push: under 4 MiB per request, which older servers take (newer
/// ones take 8 MiB for a push).
pub const PUSH_BYTES: usize = 3_500_000;
/// The largest sealed operation the server stores.
pub const MAX_SEALED_OP: usize = 512 * 1024;
/// The largest sealed operation older servers store: a larger one is sent alone, so a refusal
/// (and its leaving out) concerns only that one.
pub const OLD_SEALED_OP: usize = 128 * 1024;
/// Operations per page when pulling (the server's limit).
pub const PULL_PAGE: usize = 1000;
/// Operations read from the log at a time while a batch is filled.
const READ_PAGE: usize = 200;
/// Media ids per question to the server.
const MEDIA_QUERY: usize = 1000;
/// Uploads sent side by side together hold at most this many bytes.
const UPLOAD_GROUP_BYTES: u64 = 64 * 1024 * 1024;

/// One page of pulled operations, decoded loosely so a malformed wire item can be
/// quarantined instead of stalling the whole sync.
#[derive(serde::Deserialize)]
struct PullPage {
    ops: Vec<serde_json::Value>,
    cursor: i64,
}

/// One unit of work of the sync.
enum Job {
    /// Asks the server again to delete the account's game scores (see `game_delete_pending`).
    DeleteGame,
    /// The next batch of local operations, or on to the pull when there are none.
    Push,
    /// The next page of remote operations after this cursor.
    Pull {
        after: i64,
    },
    /// Which of these media the server lacks; those are uploaded.
    FindMedia(Vec<String>),
    /// The same question to an older server, one HEAD each (at most `PARALLEL`).
    HeadMedia(Vec<String>),
    Upload(Vec<String>),
    /// Sealed operations with their sequence numbers; `last` is where the push cursor goes after
    /// them (past left-out operations too).
    SendBatch(Batch),
    /// Opens queued operations and works out which media they need.
    Prepare(Vec<PullRetry>),
    Fetch(Vec<String>),
    Commit {
        entry: i64,
        op: RemoteOp,
    },
    Complete,
}

#[derive(Clone)]
struct Batch {
    ops: Vec<EncryptedOp>,
    seqs: Vec<i64>,
    last: i64,
    /// Whether it holds one operation larger than older servers take.
    oversized: bool,
}

impl Batch {
    /// Two halves, for a server that refused the whole as too large.
    fn split(mut self) -> (Batch, Batch) {
        let middle = self.ops.len() / 2;
        let ops = self.ops.split_off(middle);
        let seqs = self.seqs.split_off(middle);
        let first = Batch {
            last: *self
                .seqs
                .last()
                .expect("a batch to split has two operations"),
            ops: self.ops,
            seqs: self.seqs,
            oversized: false,
        };
        let second = Batch {
            ops,
            seqs,
            last: self.last,
            oversized: false,
        };
        (first, second)
    }
}

/// What the replies to the last calls are for.
enum Awaiting {
    GameDeleted,
    Missing(Vec<String>),
    Heads(Vec<String>),
    Uploaded(Vec<String>),
    Batch(Batch),
    Page { after: i64 },
    Fetched(Vec<String>),
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
    /// Operations of the outbox that later ones replace (worked out once per sync).
    skip: Option<HashSet<i64>>,
    /// Media the server has (asked or uploaded in this sync).
    present: HashSet<String>,
    /// Whether the server answers `POST /v1/media/missing` (older ones do not).
    media_query: bool,
    /// The queued entries that wait for a download.
    needs: HashMap<String, Vec<i64>>,
    /// Entries whose media could not be fetched: their commit is skipped.
    failed: HashSet<i64>,
    /// Names of palaces taken before a received delete removed them.
    names: HashMap<String, String>,
    report: SyncReport,
}

impl<S: Deref<Target = Store>, K: Deref<Target = Dek>> SyncFlow<S, K> {
    /// Refuses to start without a valid entitlement, as the sync always did.
    pub fn new(state: CloudState, store: S, dek: K) -> Result<Self> {
        if !cloud::can_sync(&state, cloud::now_secs()) {
            bail!(
                "Keine gültige Freischaltung: `locora cloud refresh` ausführen oder das Abo prüfen"
            );
        }
        Ok(Self::unchecked(state, store, dek))
    }

    fn unchecked(state: CloudState, store: S, dek: K) -> Self {
        let mut jobs = VecDeque::new();
        if state.game_delete_pending {
            jobs.push_back(Job::DeleteGame);
        }
        jobs.push_back(Job::Push);
        Self {
            store,
            dek,
            state,
            jobs,
            awaiting: None,
            pending: Vec::new(),
            complete: true,
            skip: None,
            present: HashSet::new(),
            media_query: true,
            needs: HashMap::new(),
            failed: HashSet::new(),
            names: HashMap::new(),
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

    /// Runs local jobs until one needs calls, or the queue is empty.
    fn advance(&mut self) -> Result<Step<SyncReport>> {
        while let Some(job) = self.jobs.pop_front() {
            if let Some((mut calls, awaiting)) = self.start(job)? {
                self.awaiting = Some(awaiting);
                return Ok(if calls.len() == 1 {
                    Step::Call(calls.remove(0))
                } else {
                    Step::Calls(calls)
                });
            }
        }
        let mut received = std::mem::take(&mut self.report.received);
        self.store.name_palaces(&mut received, &self.names)?;
        self.report.received = received;
        Ok(Step::Done(self.report.clone()))
    }

    fn call(&self, method: Method, path: &str, body: Option<Body>) -> Result<Call> {
        cloud::call(
            method,
            &self.state.server,
            path,
            Some(&self.state.token),
            body,
        )
    }

    /// Starts a job: local work is done here (None); calls are returned with what their replies mean.
    fn start(&mut self, job: Job) -> Result<Option<(Vec<Call>, Awaiting)>> {
        match job {
            Job::DeleteGame => {
                let call = cloud::erase_game_call(&self.state)?;
                Ok(Some((vec![call], Awaiting::GameDeleted)))
            }
            Job::Push => self.push_next().map(|()| None),
            Job::Pull { after } => {
                let path = format!(
                    "/v1/sync/ops?after={after}&limit={PULL_PAGE}&generation={}",
                    self.state.sync_generation
                );
                let call = self.call(Method::Get, &path, None)?;
                Ok(Some((vec![call], Awaiting::Page { after })))
            }
            Job::FindMedia(mut hashes) => {
                if !self.media_query {
                    self.queue_heads(hashes);
                    return Ok(None);
                }
                if hashes.len() > MEDIA_QUERY {
                    let rest = hashes.split_off(MEDIA_QUERY);
                    self.jobs.push_front(Job::FindMedia(rest));
                }
                let body = MediaQuery {
                    ids: hashes.iter().map(|hash| self.dek.media_id(hash)).collect(),
                };
                let path = format!(
                    "/v1/media/missing?generation={}",
                    self.state.sync_generation
                );
                let call = self.call(Method::Post, &path, Some(cloud::json_body(&body)?))?;
                Ok(Some((vec![call], Awaiting::Missing(hashes))))
            }
            Job::HeadMedia(hashes) => {
                let calls = hashes
                    .iter()
                    .map(|hash| {
                        let id = self.dek.media_id(hash);
                        cloud::media_call(&self.state, Method::Head, &id, None)
                    })
                    .collect::<Result<_>>()?;
                Ok(Some((calls, Awaiting::Heads(hashes))))
            }
            Job::Upload(hashes) => {
                let calls = hashes
                    .iter()
                    .map(|hash| self.upload_call(hash))
                    .collect::<Result<_>>()?;
                Ok(Some((calls, Awaiting::Uploaded(hashes))))
            }
            Job::SendBatch(batch) => {
                let body = PushOps {
                    device: self.store.node_id().to_owned(),
                    ops: batch.ops.clone(),
                };
                let path = format!("/v1/sync/ops?generation={}", self.state.sync_generation);
                let call = self.call(Method::Post, &path, Some(cloud::json_body(&body)?))?;
                Ok(Some((vec![call], Awaiting::Batch(batch))))
            }
            Job::Prepare(entries) => self.prepare(entries).map(|()| None),
            Job::Fetch(hashes) => {
                let calls = hashes
                    .iter()
                    .map(|hash| {
                        let id = self.dek.media_id(hash);
                        cloud::media_call(&self.state, Method::Get, &id, None)
                    })
                    .collect::<Result<_>>()?;
                Ok(Some((calls, Awaiting::Fetched(hashes))))
            }
            Job::Commit { entry, op } => self.commit(entry, op).map(|()| None),
            Job::Complete => {
                let call = cloud::complete_sync_call(&self.state)?;
                Ok(Some((vec![call], Awaiting::Completed)))
            }
        }
    }

    /// The next batch, as many operations as the server takes in one request. Its media are
    /// checked (and uploaded) first, then the batch is sent.
    fn push_next(&mut self) -> Result<()> {
        let cursor = self.store.sync_cursor("push")?;
        if self.skip.is_none() {
            let skip = self.store.superseded_after(cursor)?;
            self.report.compacted += skip.len();
            self.skip = Some(skip);
        }
        let skip = self.skip.as_ref().expect("set above");
        let account = self.state.account.id.clone();
        let mut ops = Vec::new();
        let mut seqs = Vec::new();
        let mut bytes = 0;
        let mut last = cursor;
        let mut media = Vec::new();
        let mut oversized = false;
        'read: loop {
            let page = self.store.ops_after(last, READ_PAGE)?;
            if page.is_empty() {
                break;
            }
            for op in page {
                if skip.contains(&op.seq) {
                    last = op.seq;
                    continue;
                }
                let (sealed, size) = seal(&self.dek, &account, &op)?;
                if size > MAX_SEALED_OP {
                    // No server takes it; it stays on this device.
                    self.report.too_large += 1;
                    last = op.seq;
                    continue;
                }
                let large = size > OLD_SEALED_OP;
                let wire = sealed.payload.len() + sealed.op_id.len() + 32;
                if !ops.is_empty()
                    && (large || oversized || ops.len() == PUSH_OPS || bytes + wire > PUSH_BYTES)
                {
                    break 'read;
                }
                oversized = large;
                bytes += wire;
                last = op.seq;
                seqs.push(op.seq);
                media.extend(media_refs(&remote(op)));
                ops.push(sealed);
            }
        }
        if ops.is_empty() {
            if last > cursor {
                // Only left-out operations remained: nothing to send for them.
                self.store.set_sync_cursor("push", last)?;
            }
            // On to the pull; its pending entries are applied at its end.
            self.pending = self.store.pending_pull()?;
            let after = self.store.sync_cursor("pull")?;
            self.jobs.push_back(Job::Pull { after });
            return Ok(());
        }
        self.jobs.push_front(Job::Push);
        self.jobs.push_front(Job::SendBatch(Batch {
            ops,
            seqs,
            last,
            oversized,
        }));
        let mut seen = HashSet::new();
        media.retain(|hash| !self.present.contains(hash) && seen.insert(hash.clone()));
        if !media.is_empty() {
            self.jobs.push_front(Job::FindMedia(media));
        }
        Ok(())
    }

    fn queue_heads(&mut self, hashes: Vec<String>) {
        for group in hashes.chunks(PARALLEL).rev() {
            self.jobs.push_front(Job::HeadMedia(group.to_vec()));
        }
    }

    /// Uploads in groups of at most `PARALLEL` files and `UPLOAD_GROUP_BYTES` (a large file goes alone).
    fn queue_uploads(&mut self, hashes: Vec<String>) -> Result<()> {
        let mut groups: Vec<Vec<String>> = Vec::new();
        let mut size = 0;
        for hash in hashes {
            let bytes = self.store.media(&hash)?.map_or(0, |media| media.size);
            match groups.last_mut() {
                Some(group) if group.len() < PARALLEL && size + bytes <= UPLOAD_GROUP_BYTES => {
                    size += bytes;
                    group.push(hash);
                }
                _ => {
                    size = bytes;
                    groups.push(vec![hash]);
                }
            }
        }
        for group in groups.into_iter().rev() {
            self.jobs.push_front(Job::Upload(group));
        }
        Ok(())
    }

    fn upload_call(&self, hash: &str) -> Result<Call> {
        let id = self.dek.media_id(hash);
        let media = self
            .store
            .media(hash)?
            .ok_or_else(|| anyhow!("Medium {hash} ist auf diesem Gerät nicht vorhanden"))?;
        let path = self
            .store
            .media_file(hash)
            .ok_or_else(|| anyhow!("Dieser Speicher hat keine Mediendateien"))?;
        let bytes = fs::read(&path).with_context(|| format!("{} lesen", path.display()))?;
        let sealed = self.dek.seal_media(&id, &media.mime, &bytes);
        cloud::media_call(&self.state, Method::Put, &id, Some(Body::Bytes(sealed)))
    }

    /// Opens queued entries, queues the downloads they need (each file once), then their commits.
    fn prepare(&mut self, entries: Vec<PullRetry>) -> Result<()> {
        let mut commits = Vec::new();
        let mut wanted = Vec::new();
        for entry in entries {
            let pulled: PulledOp = match serde_json::from_str(&entry.operation) {
                Ok(op) => op,
                Err(error) => {
                    self.record_failure(entry.id, format!("Ungültige Operation: {error}"), true)?;
                    continue;
                }
            };
            let op = match open(&self.dek, &self.state.account.id, &pulled) {
                Ok(op) => op,
                Err(error) => {
                    self.record_failure(entry.id, error.to_string(), true)?;
                    continue;
                }
            };
            for hash in media_refs(&op) {
                if self.store.media(&hash)?.is_some() {
                    continue;
                }
                let waiting = self.needs.entry(hash.clone()).or_default();
                if waiting.is_empty() {
                    wanted.push(hash);
                }
                waiting.push(entry.id);
            }
            commits.push(Job::Commit {
                entry: entry.id,
                op,
            });
        }
        for job in commits.into_iter().rev() {
            self.jobs.push_front(job);
        }
        for group in wanted.chunks(PARALLEL).rev() {
            self.jobs.push_front(Job::Fetch(group.to_vec()));
        }
        Ok(())
    }

    /// Applies one received operation and notes what it changed.
    fn commit(&mut self, entry: i64, op: RemoteOp) -> Result<()> {
        if self.failed.contains(&entry) {
            return Ok(());
        }
        let palace = self.store.palace_of(&op)?;
        if op.entity == "palace" && op.op == "delete" {
            let name = match self.store.palace_name(&op.entity_id)? {
                Some(name) => Some(name),
                None => op
                    .data
                    .as_ref()
                    .and_then(|data| data.get("name"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
            };
            if let Some(name) = name {
                self.names.insert(op.entity_id.clone(), name);
            }
        }
        let attempt = self.store.apply_pull(entry, &op)?;
        if attempt.applied.applied > 0 {
            self.report
                .received
                .add(palace.as_deref(), change_kind(&op.entity, &op.op));
        }
        self.report.applied.applied += attempt.applied.applied;
        self.report.applied.stale += attempt.applied.stale;
        self.report.applied.missing += attempt.applied.missing;
        self.report.applied.failed.extend(attempt.applied.failed);
        self.report.quarantined.extend(attempt.quarantined);
        Ok(())
    }

    fn record_failure(&mut self, id: i64, reason: String, permanent: bool) -> Result<()> {
        self.report
            .quarantined
            .extend(self.store.fail_pull(id, &reason, permanent)?);
        self.report.applied.failed.push(reason);
        Ok(())
    }

    /// Takes the replies to the calls the flow asked for last, and continues the queue.
    fn answer(&mut self, awaiting: Awaiting, replies: Vec<Reply>) -> Result<()> {
        let one = |replies: Vec<Reply>| expect(replies, 1).map(|mut replies| replies.remove(0));
        match awaiting {
            // Whatever the answer, the sync goes on; the caller keeps what is still pending.
            Awaiting::GameDeleted => {
                self.report.rankings = Some(cloud::erase_game_reply(one(replies)?)?);
            }
            Awaiting::Missing(hashes) => {
                let reply = one(replies)?;
                if cloud_http::unsupported(&reply) {
                    // An older server: asked file by file instead.
                    self.media_query = false;
                    self.queue_heads(hashes);
                    return Ok(());
                }
                let missing: HashSet<String> =
                    json::<MediaMissing>(reply)?.missing.into_iter().collect();
                let mut upload = Vec::new();
                for hash in hashes {
                    if missing.contains(&self.dek.media_id(&hash)) {
                        upload.push(hash);
                    } else {
                        self.present.insert(hash);
                    }
                }
                self.queue_uploads(upload)?;
            }
            Awaiting::Heads(hashes) => {
                let replies = expect(replies, hashes.len())?;
                let mut upload = Vec::new();
                for (hash, reply) in hashes.into_iter().zip(replies) {
                    if cloud::exists_reply(reply)? {
                        self.present.insert(hash);
                    } else {
                        upload.push(hash);
                    }
                }
                self.queue_uploads(upload)?;
            }
            Awaiting::Uploaded(hashes) => {
                let replies = expect(replies, hashes.len())?;
                for (hash, reply) in hashes.into_iter().zip(replies) {
                    cloud_http::expect_success(reply)?;
                    self.report.media_uploaded += 1;
                    self.present.insert(hash);
                }
            }
            Awaiting::Batch(batch) => {
                let reply = one(replies)?;
                // Too large for this server: the halves go one after the other. A single
                // operation it refuses (an older server answers 400 for one over its limit)
                // stays on this device.
                let refused = reply.status == 413 || (reply.status == 400 && batch.oversized);
                if refused && batch.ops.len() > 1 {
                    let (first, second) = batch.split();
                    self.jobs.push_front(Job::SendBatch(second));
                    self.jobs.push_front(Job::SendBatch(first));
                    return Ok(());
                }
                if refused {
                    self.report.too_large += 1;
                    self.store.set_sync_cursor("push", batch.last)?;
                    return Ok(());
                }
                cloud_http::expect_success(reply)?;
                self.store.set_sync_cursor("push", batch.last)?;
                self.report.pushed += batch.ops.len();
            }
            Awaiting::Page { after } => {
                let reply = one(replies)?;
                self.page(after, reply)?;
            }
            Awaiting::Fetched(hashes) => {
                let replies = expect(replies, hashes.len())?;
                for (hash, reply) in hashes.into_iter().zip(replies) {
                    let waiting = self.needs.remove(&hash).unwrap_or_default();
                    if let Err(error) = self.fetched(&hash, reply) {
                        for entry in waiting {
                            if self.failed.insert(entry) {
                                self.record_failure(entry, error.to_string(), false)?;
                            }
                        }
                    }
                }
            }
            Awaiting::Completed => {
                let retention: RetentionStatus = json(one(replies)?)?;
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
        let mut next = Vec::new();
        if !entries.is_empty() {
            next.push(Job::Prepare(entries));
        }
        if done {
            let pending = std::mem::take(&mut self.pending);
            if !pending.is_empty() {
                next.push(Job::Prepare(pending));
            }
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

/// The replies to calls sent side by side: exactly one per call.
fn expect(replies: Vec<Reply>, count: usize) -> Result<Vec<Reply>> {
    if replies.len() != count {
        bail!("the sync got {} replies for {count} calls", replies.len());
    }
    Ok(replies)
}

impl<S: Deref<Target = Store>, K: Deref<Target = Dek>> Flow for SyncFlow<S, K> {
    type Output = SyncReport;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<SyncReport>> {
        if let Some(awaiting) = self.awaiting.take() {
            let reply =
                reply.ok_or_else(|| anyhow!("the sync needs the reply to its last call"))?;
            self.answer(awaiting, vec![reply])?;
        }
        self.advance()
    }

    fn step_many(&mut self, replies: Vec<Reply>) -> Result<Step<SyncReport>> {
        let awaiting = self
            .awaiting
            .take()
            .ok_or_else(|| anyhow!("the sync did not ask for these replies"))?;
        self.answer(awaiting, replies)?;
        self.advance()
    }
}

#[cfg(test)]
#[path = "sync_flow_tests.rs"]
mod tests;
