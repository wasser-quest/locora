//! Sync of the local operation log with Locora Cloud, end to end encrypted.
//!
//! Every operation and every media file is sealed with the account key before it
//! leaves the device (see `locora-crypto`). The server stores ciphertext under keyed
//! ids and cannot read any of it. The sync itself is `sync_flow::SyncFlow`; this file
//! unlocks the account key, checks the server's state first, and seals and opens operations.

use std::ops::Deref;
#[cfg(feature = "cloud")]
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use locora_cloud_api::api::{EncryptedOp, PulledOp};
use locora_crypto::Dek;
use locora_store::{ApplyReport, ChangeSummary, Op, PalaceChange, RemoteOp, Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::cloud::{self, CloudState, RetentionStatus};
use crate::cloud_http::{Flow, Reply, Step, json};
pub use crate::sync_flow::SyncFlow;

/// Operations per request; also the page size when pulling.
pub const BATCH: usize = crate::sync_flow::PUSH_OPS;

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
    /// Local operations left out because later ones replace them.
    pub compacted: usize,
    /// Local operations too large for the server; they stay on this device.
    pub too_large: usize,
    /// What the operations from other devices changed here.
    pub received: ChangeSummary,
}

/// How a sync may run.
#[derive(Debug, Clone, Default)]
pub struct SyncOptions {
    /// The palace deletions this sync may send. `None` sends any (the CLI); with `Some`, the sync
    /// does not start while another deletion waits (`SyncHold::Confirm`), so the app can ask first.
    pub confirmed: Option<Vec<String>>,
    /// Started by the app on its own: instead of starting over with a new generation of the
    /// server, or restoring expired cloud data, it stops (`SyncHold::Rebuild`) for a person to decide.
    pub automatic: bool,
}

/// Why a sync did not start; the app shows it and waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncHold {
    /// These palaces are deleted locally, and the deletion would reach the other devices.
    Confirm(Vec<PalaceChange>),
    /// The cloud data was deleted or has expired: a sync by hand sends everything again.
    Rebuild,
}

impl std::fmt::Display for SyncHold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncHold::Confirm(_) => {
                f.write_str("Das Löschen eines Palasts muss erst bestätigt werden")
            }
            SyncHold::Rebuild => f.write_str(
                "Die Cloud-Daten werden neu aufgebaut. Bitte einmal von Hand synchronisieren",
            ),
        }
    }
}

impl std::error::Error for SyncHold {}

/// The hold behind an error, if the sync stopped for one.
pub fn hold_of(error: &anyhow::Error) -> Option<&SyncHold> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<SyncHold>())
}

/// Palace deletions in the outbox that are not among `confirmed`.
pub fn unconfirmed_deletions(store: &Store, confirmed: &[String]) -> Result<Vec<PalaceChange>> {
    let pending = store.pending_changes(store.sync_cursor("push")?)?;
    Ok(pending
        .deleted()
        .filter(|palace| !confirmed.contains(&palace.id))
        .cloned()
        .collect())
}

/// App entry point for the same encrypted sync used by the CLI. The passphrase
/// stays on this device and is only used to unlock the account key for this run.
#[cfg(feature = "cloud")]
pub fn sync_with_passphrase(data: &Path, store: &Store, passphrase: &str) -> Result<SyncReport> {
    sync_with(
        data,
        store,
        Unlock::Passphrase(Zeroizing::new(passphrase.to_owned())),
        SyncOptions::default(),
    )
}

/// The sync with an unlocked key (`Unlock::Key`, kept by this device) or a passphrase.
#[cfg(feature = "cloud")]
pub fn sync_with(
    data: &Path,
    store: &Store,
    unlock: Unlock,
    options: SyncOptions,
) -> Result<SyncReport> {
    let state = cloud::load(data)?.ok_or_else(|| anyhow!("Nicht bei Locora Cloud angemeldet"))?;
    if !cloud::can_sync(&state, cloud::now_secs()) {
        bail!("Keine gültige Cloud-Freischaltung. Abo prüfen und Freischaltung aktualisieren");
    }
    let report = crate::cloud_http::run(UnlockSync::start(state, store, unlock, options))?;
    // Read again: the session may have changed while the sync ran.
    if let Some(mut state) = cloud::load(data)? {
        if let Some(generation) = report.new_generation {
            state.sync_generation = generation;
        }
        state.last_sync_at = Some(cloud::now_secs() as i64);
        cloud::save(data, &state)?;
    }
    Ok(report)
}

#[cfg(feature = "cloud")]
pub fn sync(state: &CloudState, store: &Store, dek: &Dek) -> Result<SyncReport> {
    crate::cloud_http::run(SyncFlow::new(state.clone(), store, dek)?)
}

/// How the account key is opened for a sync.
pub enum Unlock {
    /// With the encryption password (Argon2, after fetching the wrapped keys).
    Passphrase(Zeroizing<String>),
    /// The key this device keeps (auto-sync), or one derived elsewhere (a browser worker).
    Key(Box<Dek>),
}

/// Unlocks the account key (with the passphrase, after fetching the wrapped keys), then syncs.
/// `S` is the store, borrowed on the desktop and shared in the browser.
/// Before the sync, the server's generation is checked: after the cloud data was deleted (or had
/// expired, which is restored from this device), the device starts over and sends all its data.
pub struct UnlockSync<S> {
    state: CloudState,
    store: S,
    options: SyncOptions,
    stage: Stage,
    new_generation: Option<i64>,
    syncing: Option<SyncFlow<S, Box<Dek>>>,
}

enum Stage {
    Start(Unlock),
    Keys(Zeroizing<String>),
    Retention(Box<Dek>),
    Rebuild(Box<Dek>),
    Started,
}

impl<S: Clone + Deref<Target = Store>> UnlockSync<S> {
    pub fn new(state: CloudState, store: S, passphrase: String) -> Self {
        Self::start(
            state,
            store,
            Unlock::Passphrase(Zeroizing::new(passphrase)),
            SyncOptions::default(),
        )
    }

    pub fn start(state: CloudState, store: S, unlock: Unlock, options: SyncOptions) -> Self {
        Self {
            state,
            store,
            options,
            stage: Stage::Start(unlock),
            new_generation: None,
            syncing: None,
        }
    }

    /// Adopts the server's generation when it differs, then runs the sync itself.
    fn begin(&mut self, dek: Box<Dek>, status: RetentionStatus) -> Result<Step<SyncReport>> {
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

    /// Whether this device has synced with the cloud before (a new device just takes over the
    /// server's generation).
    fn synced_before(&self) -> Result<bool> {
        Ok(self.store.sync_cursor("push")? > 0 || self.store.sync_cursor("pull")? > 0)
    }
}

impl<S: Clone + Deref<Target = Store>> Flow for UnlockSync<S> {
    type Output = SyncReport;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<SyncReport>> {
        if let Some(flow) = self.syncing.as_mut() {
            let step = flow.step(reply)?;
            return Ok(self.finish(step));
        }
        match (std::mem::replace(&mut self.stage, Stage::Started), reply) {
            (Stage::Start(unlock), None) => {
                // Nothing leaves this device before a pending palace deletion is confirmed.
                if let Some(confirmed) = &self.options.confirmed {
                    let waiting = unconfirmed_deletions(&self.store, confirmed)?;
                    if !waiting.is_empty() {
                        return Err(SyncHold::Confirm(waiting).into());
                    }
                }
                match unlock {
                    Unlock::Passphrase(passphrase) => {
                        self.stage = Stage::Keys(passphrase);
                        Ok(Step::Call(cloud::keys_call(&self.state)?))
                    }
                    Unlock::Key(dek) => {
                        self.stage = Stage::Retention(dek);
                        Ok(Step::Call(cloud::retention_call(&self.state)?))
                    }
                }
            }
            (Stage::Keys(passphrase), Some(reply)) => {
                let Some(bundle) = cloud::keys_reply(reply)? else {
                    bail!("Verschlüsselung fehlt. In den Einstellungen einrichten");
                };
                let dek = Box::new(locora_crypto::unlock(&bundle, &passphrase)?);
                self.stage = Stage::Retention(dek);
                Ok(Step::Call(cloud::retention_call(&self.state)?))
            }
            (Stage::Retention(dek), Some(reply)) => {
                let status: RetentionStatus = json(reply)?;
                let starts_over = status.state == "expired"
                    || (status.generation != self.state.sync_generation && self.synced_before()?);
                if self.options.automatic && starts_over {
                    return Err(SyncHold::Rebuild.into());
                }
                if status.state == "expired" {
                    // Expired data is restored from this device.
                    let call = cloud::rebuild_call(&self.state, status.generation)?;
                    self.stage = Stage::Rebuild(dek);
                    return Ok(Step::Call(call));
                }
                self.begin(dek, status)
            }
            (Stage::Rebuild(dek), Some(reply)) => {
                let status: RetentionStatus = json(reply)?;
                self.begin(dek, status)
            }
            _ => bail!("the sync got a reply it did not ask for"),
        }
    }

    fn step_many(&mut self, replies: Vec<Reply>) -> Result<Step<SyncReport>> {
        let flow = self
            .syncing
            .as_mut()
            .ok_or_else(|| anyhow!("the sync did not ask for these replies"))?;
        let step = flow.step_many(replies)?;
        Ok(self.finish(step))
    }
}

/// The plaintext of one sealed operation. Never leaves the device unsealed.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Plain {
    pub(crate) hlc: String,
    pub(crate) entity: String,
    pub(crate) entity_id: String,
    pub(crate) op: String,
    pub(crate) data: Option<Value>,
}

/// Seals a local operation, and returns the size of its ciphertext (the server limits it). Its id
/// is keyed to its timestamp, so it cannot be linked to content.
pub(crate) fn seal(dek: &Dek, account: &str, op: &Op) -> Result<(EncryptedOp, usize)> {
    let plain = Plain {
        hlc: op.hlc.clone(),
        entity: op.entity.clone(),
        entity_id: op.entity_id.clone(),
        op: op.op.clone(),
        data: op.data.clone(),
    };
    let id = dek.op_id(&op.hlc);
    let sealed = dek.seal_op(account, &id, &serde_json::to_vec(&plain)?);
    let size = sealed.len();
    Ok((
        EncryptedOp {
            op_id: id,
            payload: STANDARD.encode(sealed),
        },
        size,
    ))
}

/// Opens a pulled operation and checks that its id belongs to its timestamp.
pub(crate) fn open(dek: &Dek, account: &str, pulled: &PulledOp) -> Result<RemoteOp> {
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

pub(crate) fn remote(op: Op) -> RemoteOp {
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
