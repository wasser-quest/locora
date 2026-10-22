//! The browser's side of the summary before a sync, auto-sync and the confirmation e-mail.
//!
//! The page keeps the auto-sync key itself: wrapped by a WebCrypto key that cannot be exported,
//! in IndexedDB (see apps/web/shell/src/cloud.mjs). It hands the key to `cloud_sync` for each
//! sync; this core never stores it. The Argon2 derivation runs in a worker (`cloud_unlock`), so
//! the page stays responsive while the password is checked.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use locora_app_api::CloudPending;
use locora_app_core::cloud_view;
use locora_cloud_api::api::KeyBundle;
use locora_crypto::Dek;
use locora_runtime::cloud::{self, CloudState};
use locora_runtime::cloud_http::{Exchange, Flow, Reply, Step};
use locora_runtime::cloud_sync::{SyncOptions, SyncReport, Unlock, UnlockSync};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::cloud::{Map, status};

/// A command's result: at once, or a flow that needs the network.
pub enum Run {
    Done(Value),
    Flow(Box<dyn Flow<Output = Value>>),
}

fn saved(data: &Path) -> Result<CloudState> {
    cloud::load(data)?.ok_or_else(|| anyhow!("Kein Cloud-Konto angemeldet"))
}

fn store() -> Result<std::sync::Arc<locora_store::Store>> {
    crate::with_backend(|backend| Ok(backend.profile.store.clone()))
}

/// The commands of this file; `None` for the others.
pub fn command(name: &str, args: &Value, data: &Path) -> Result<Option<Run>> {
    Ok(Some(match name {
        "cloud_pending" => {
            let store = store()?;
            let local = cloud_view::changes(&store.pending_changes(store.sync_cursor("push")?)?);
            match cloud::load(data)? {
                Some(state) => {
                    let call = cloud::pending_call(&state, &store)?;
                    Run::Flow(Box::new(Map::new(
                        Exchange::new(call, cloud::pending_reply),
                        move |remote: Option<u64>| {
                            Ok(json!(CloudPending {
                                local: local.clone(),
                                remote,
                            }))
                        },
                    )))
                }
                None => Run::Done(json!(CloudPending {
                    local,
                    remote: None
                })),
            }
        }
        "cloud_auto_state" => {
            let state = cloud::load(data)?;
            Run::Done(json!(cloud_view::auto_state(state.as_ref(), &*store()?)?))
        }
        // The page turns the flag on once it has kept the key, and off when it forgot it.
        "cloud_auto_set" => {
            let mut state = saved(data)?;
            state.auto_sync = args
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            cloud::save(data, &state)?;
            Run::Done(json!(status(Some(&state))))
        }
        "cloud_keys_bundle" => {
            let call = cloud::keys_call(&saved(data)?)?;
            Run::Flow(Box::new(Map::new(
                Exchange::new(call, cloud::keys_reply),
                |bundle: Option<KeyBundle>| match bundle {
                    Some(bundle) => Ok(json!(bundle)),
                    None => bail!("Verschlüsselung fehlt. In den Einstellungen einrichten"),
                },
            )))
        }
        "cloud_email_resend" => {
            let call = cloud::resend_email_call(&saved(data)?)?;
            Run::Flow(Box::new(Map::new(
                Exchange::new(call, cloud::resend_email_reply),
                |()| Ok(Value::Null),
            )))
        }
        _ => return Ok(None),
    }))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Result<Zeroizing<Vec<u8>>> {
    if text.len() != 64 || !text.is_ascii() {
        bail!("Der Schlüssel ist ungültig");
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16))
        .collect::<std::result::Result<Vec<u8>, _>>()
        .map(Zeroizing::new)
        .map_err(|_| anyhow!("Der Schlüssel ist ungültig"))
}

fn passphrase(args: &Value) -> Result<Zeroizing<String>> {
    let passphrase = Zeroizing::new(
        args.get("passphrase")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    );
    if !(12..=1024).contains(&passphrase.chars().count()) {
        bail!("Verschlüsselungs-Passwort braucht 12 bis 1024 Zeichen");
    }
    Ok(passphrase)
}

/// `{ bundle, passphrase }` → `{ key }` (hex): the Argon2 step alone, for the page's worker.
pub fn unlock(args: &Value) -> Result<Value> {
    let bundle: KeyBundle = serde_json::from_value(args.get("bundle").cloned().unwrap_or_default())
        .map_err(|_| anyhow!("Schlüsseldaten sind ungültig"))?;
    let passphrase = passphrase(args)?;
    let dek = locora_crypto::unlock(&bundle, &passphrase)?;
    Ok(json!({ "done": { "key": hex(dek.to_bytes().as_ref()) } }))
}

/// The sync, with the password (`passphrase`) or a key the page derived or kept (`key`, hex).
/// `confirmed` lists the palace deletions the person agreed to; with `auto` the answer is a
/// `CloudAutoOutcome` (errors included) instead of a `CloudSyncResult`.
pub fn sync(args: &Value, data: PathBuf) -> Result<Value> {
    let automatic = args.get("auto").and_then(Value::as_bool).unwrap_or(false);
    let confirmed: Vec<String> = args
        .get("confirmed")
        .and_then(|list| serde_json::from_value(list.clone()).ok())
        .unwrap_or_default();
    let unlock = match args.get("key").and_then(Value::as_str) {
        Some(key) => Unlock::Key(Box::new(Dek::from_bytes(&unhex(key)?)?)),
        None => Unlock::Passphrase(passphrase(args)?),
    };
    let state = saved(&data)?;
    if !cloud::can_sync(&state, cloud::now_secs()) {
        bail!("Keine gültige Cloud-Freischaltung. Abo prüfen und Freischaltung aktualisieren");
    }
    let options = SyncOptions {
        confirmed: Some(confirmed),
        automatic,
    };
    let flow = UnlockSync::start(state, store()?, unlock, options);
    if automatic {
        return drive(Box::new(Auto { flow, data }));
    }
    drive(Box::new(Map::new(flow, move |report: SyncReport| {
        finished(&data, &report)?;
        Ok(json!(cloud_view::sync_result(&report)))
    })))
}

fn drive(mut flow: Box<dyn Flow<Output = Value>>) -> Result<Value> {
    let step = flow.step(None)?;
    crate::cloud::resume(flow, step)
}

/// Keeps what a finished sync means for the next one: a new generation, and when it ran.
fn finished(data: &Path, report: &SyncReport) -> Result<()> {
    let mut state = saved(data)?;
    if let Some(generation) = report.new_generation {
        state.sync_generation = generation;
    }
    state.last_sync_at = Some(cloud::now_secs() as i64);
    cloud::save(data, &state)
}

/// An automatic sync: whatever happens becomes a `CloudAutoOutcome`, so the page can tell a pause
/// or a deletion to confirm from an error to retry.
struct Auto<F> {
    flow: F,
    data: PathBuf,
}

impl<F: Flow<Output = SyncReport>> Auto<F> {
    fn outcome(&self, step: Result<Step<SyncReport>>) -> Result<Step<Value>> {
        Ok(match step {
            Ok(Step::Call(call)) => Step::Call(call),
            Ok(Step::Calls(calls)) => Step::Calls(calls),
            Ok(Step::Done(report)) => {
                finished(&self.data, &report)?;
                Step::Done(json!(cloud_view::auto_outcome(Ok(report))))
            }
            Err(error) => Step::Done(json!(cloud_view::auto_outcome(Err(error)))),
        })
    }
}

impl<F: Flow<Output = SyncReport>> Flow for Auto<F> {
    type Output = Value;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<Value>> {
        let step = self.flow.step(reply);
        self.outcome(step)
    }

    fn step_many(&mut self, replies: Vec<Reply>) -> Result<Step<Value>> {
        let step = self.flow.step_many(replies);
        self.outcome(step)
    }
}
