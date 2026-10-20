//! The cloud client in the browser. The flows of `locora_runtime::cloud` ask for their HTTP calls;
//! this file keeps the flow that waits for a reply, and the page sends each call with `fetch` and
//! hands the reply back (`cloud_reply`). Nothing here blocks: the page only waits for `fetch`.
//!
//! A command answers `{"call": …}` while a call is needed, and `{"done": …}` with its result.

use std::cell::RefCell;

use anyhow::{Result, anyhow, bail};
use locora_app_api::{CloudConnect, CloudKeyCreated, CloudKeySetup, CloudStatus, CloudSyncResult};
use locora_cloud_api::api::KeyBundle;
use locora_runtime::cloud::{self, CloudState, Connect, Refresh, RetentionStatus};
use locora_runtime::cloud_http::{Body, Call, Exchange, Flow, Method, Reply, Step, failure, json as cloud_http_json};
use locora_runtime::cloud_sync::{SyncReport, UnlockSync};
use serde_json::{Value, json};

thread_local! {
    /// The flow that waits for the reply to its last call.
    static WAITING: RefCell<Option<Box<dyn Flow<Output = Value>>>> = const { RefCell::new(None) };
    /// The body of the last call, read by the page with `cloud_body`.
    static OUTBOUND: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Runs a command that may need the network. A new command drops a flow that was still waiting.
pub fn command(name: &str, args: &Value) -> Result<Value> {
    WAITING.with(|cell| cell.borrow_mut().take());
    let data = crate::with_backend(|backend| Ok(backend.profile.data_dir.clone()))?;
    match name {
        "cloud_status" => {
            let state = cloud::load(&data)?;
            Ok(json!({ "done": status(state.as_ref()) }))
        }
        "cloud_refresh" => {
            let state = saved(&data)?;
            drive(Box::new(Map::new(Refresh::new(state), move |state: CloudState| {
                cloud::save(&data, &state)?;
                Ok(json!(status(Some(&state))))
            })), None)
        }
        "cloud_logout" => {
            let Some(state) = cloud::load(&data)? else {
                return Ok(json!({ "done": Value::Null }));
            };
            // The server is told best effort: the local account is forgotten either way.
            let call = cloud::logout_call(&state)?;
            drive(Box::new(Map::new(Exchange::new(call, |_| Ok(())), move |()| {
                cloud::remove(&data)?;
                Ok(Value::Null)
            })), None)
        }
        "cloud_connect" => {
            let request: CloudConnect = serde_json::from_value(request_of(args))?;
            validate_connect(&request)?;
            if cloud::load(&data)?.is_some() {
                bail!("Auf diesem Gerät ist bereits ein Cloud-Konto angemeldet");
            }
            let flow = if request.register {
                Connect::sign_up(
                    &request.server,
                    request.email.trim(),
                    &request.password,
                    request.display_name.as_deref().unwrap_or("").trim(),
                    request.guest,
                )?
            } else {
                Connect::sign_in(&request.server, request.email.trim(), &request.password)?
            };
            drive(Box::new(Map::new(flow, move |state: CloudState| {
                cloud::save(&data, &state)?;
                Ok(json!(status(Some(&state))))
            })), None)
        }
        "cloud_keys_status" => {
            let state = saved(&data)?;
            let call = cloud::keys_call(&state)?;
            drive(Box::new(Map::new(Exchange::new(call, cloud::keys_reply), |keys: Option<KeyBundle>| {
                Ok(json!(keys.is_some()))
            })), None)
        }
        "cloud_keys_setup" => {
            let request: CloudKeySetup = serde_json::from_value(request_of(args))?;
            if !(12..=1024).contains(&request.passphrase.chars().count()) {
                bail!("Verschlüsselungs-Passwort braucht 12 bis 1024 Zeichen");
            }
            let state = saved(&data)?;
            drive(Box::new(KeysSetup::new(state, request.passphrase)), None)
        }
        "cloud_sync" => {
            let passphrase = args.get("passphrase").and_then(Value::as_str).unwrap_or_default().to_owned();
            if !(12..=1024).contains(&passphrase.chars().count()) {
                bail!("Verschlüsselungs-Passwort braucht 12 bis 1024 Zeichen");
            }
            let state = saved(&data)?;
            if cloud::access(&state, cloud::now_secs()).is_none() {
                bail!("Keine gültige Cloud-Freischaltung. Abo prüfen und Freischaltung aktualisieren");
            }
            let store = crate::with_backend(|backend| Ok(backend.profile.store.clone()))?;
            drive(Box::new(Map::new(UnlockSync::new(state, store, passphrase), move |report: SyncReport| {
                // A new generation of the server (deleted or expired cloud data) is kept for the next sync.
                if let Some(generation) = report.new_generation {
                    let mut state = saved(&data)?;
                    state.sync_generation = generation;
                    cloud::save(&data, &state)?;
                }
                sync_result(report)
            })), None)
        }
        "cloud_erase" => {
            let state = saved(&data)?;
            let call = cloud::erase_call(&state)?;
            drive(Box::new(Map::new(Exchange::new(call, cloud_http_json), move |remote: RetentionStatus| {
                let mut state = saved(&data)?;
                let store = crate::with_backend(|backend| Ok(backend.profile.store.clone()))?;
                cloud::adopt_generation(&mut state, &store, remote.generation)?;
                cloud::save(&data, &state)?;
                Ok(Value::Null)
            })), None)
        }
        "cloud_delete_account" => {
            let password = args.get("password").and_then(Value::as_str).unwrap_or_default().to_owned();
            if password.is_empty() || password.chars().count() > 256 {
                bail!("Cloud-Passwort eingeben");
            }
            let state = saved(&data)?;
            let call = cloud::delete_account_call(&state, &password)?;
            drive(Box::new(Map::new(Exchange::new(call, cloud::delete_account_reply), move |()| {
                cloud::remove(&data)?;
                Ok(Value::Null)
            })), None)
        }
        other => bail!("unknown cloud command {other}"),
    }
}

/// The reply the page got for the last call. Status 0 means the request never reached the server.
pub fn reply(status: u32, body: Vec<u8>) -> Result<Value> {
    let flow = WAITING
        .with(|cell| cell.borrow_mut().take())
        .ok_or_else(|| anyhow!("no cloud request is waiting for a reply"))?;
    let status = u16::try_from(status).unwrap_or(0);
    drive(flow, Some(Reply { status, body }))
}

/// The body of the last call, empty when it had none.
pub fn body() -> Vec<u8> {
    OUTBOUND.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

/// Runs a flow until it needs a call (which the page sends) or is done.
fn drive(mut flow: Box<dyn Flow<Output = Value>>, reply: Option<Reply>) -> Result<Value> {
    match flow.step(reply)? {
        Step::Call(call) => {
            let description = describe(&call);
            WAITING.with(|cell| *cell.borrow_mut() = Some(flow));
            Ok(json!({ "call": description }))
        }
        Step::Done(value) => Ok(json!({ "done": value })),
    }
}

/// The call as the page sends it. The body stays in the core until `body` reads it.
fn describe(call: &Call) -> Value {
    let (kind, bytes) = match &call.body {
        Some(Body::Json(bytes)) => ("json", bytes.clone()),
        Some(Body::Bytes(bytes)) => ("bytes", bytes.clone()),
        None => ("none", Vec::new()),
    };
    let length = bytes.len();
    OUTBOUND.with(|cell| *cell.borrow_mut() = bytes);
    json!({
        "method": match call.method {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
        },
        "url": call.url,
        "token": call.token,
        "body": kind,
        "length": length,
        "timeout": call.timeout_secs,
    })
}

fn saved(data: &std::path::Path) -> Result<CloudState> {
    cloud::load(data)?.ok_or_else(|| anyhow!("Kein Cloud-Konto angemeldet"))
}

fn status(state: Option<&CloudState>) -> CloudStatus {
    match state {
        Some(state) => CloudStatus {
            account: Some(state.account.email.clone()),
            server: Some(state.server.clone()),
            access: cloud::access(state, cloud::now_secs()).is_some(),
        },
        None => CloudStatus {
            account: None,
            server: None,
            access: false,
        },
    }
}

fn sync_result(report: SyncReport) -> Result<Value> {
    Ok(json!(CloudSyncResult {
        pushed: report.pushed,
        media_uploaded: report.media_uploaded,
        pulled: report.pulled,
        media_downloaded: report.media_downloaded,
        applied: report.applied.applied,
        stale: report.applied.stale,
        missing: report.applied.missing,
        failed: report.applied.failed.len(),
        quarantined: report.quarantined.len(),
        cloud_expires_at: report.cloud_expires_at,
    }))
}

/// The arguments of a command the UI sends as `{ request: … }`, the shape of the desktop app's
/// commands; a bare object is taken as it is.
fn request_of(args: &Value) -> Value {
    match args.get("request") {
        Some(request) if request.is_object() => request.clone(),
        _ => args.clone(),
    }
}

/// The same checks the desktop app makes before it contacts the server.
fn validate_connect(request: &CloudConnect) -> Result<()> {
    if request.server.len() > 2048 {
        bail!("Serveradresse ist zu lang");
    }
    cloud::check_server(&request.server)?;
    let email = request.email.trim();
    if email.is_empty() || email.len() > 254 || !email.contains('@') {
        bail!("Eine gültige E-Mail-Adresse eingeben");
    }
    let length = request.password.chars().count();
    if !(10..=256).contains(&length) {
        bail!("Cloud-Passwort braucht 10 bis 256 Zeichen");
    }
    if request.register {
        let name = request.display_name.as_deref().unwrap_or("").trim();
        if name.is_empty() || name.chars().count() > 32 {
            bail!("Anzeigename braucht 1 bis 32 Zeichen");
        }
    }
    Ok(())
}

/// A flow whose result goes through `convert` (which may also save what the result means).
struct Map<F, G> {
    flow: F,
    convert: G,
}

impl<F, G> Map<F, G> {
    fn new(flow: F, convert: G) -> Self {
        Self { flow, convert }
    }
}

impl<F: Flow, G: Fn(F::Output) -> Result<Value>> Flow for Map<F, G> {
    type Output = Value;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<Value>> {
        match self.flow.step(reply)? {
            Step::Call(call) => Ok(Step::Call(call)),
            Step::Done(output) => Ok(Step::Done((self.convert)(output)?)),
        }
    }
}

enum KeysStage {
    Fetch,
    Store { bundle: KeyBundle, recovery_key: String },
    Confirm { bundle: KeyBundle, recovery_key: String, error: String },
}

/// Sets the account keys up: it checks that none exist, creates them here, and stores them. If the
/// answer to the store is lost, it asks whether the keys arrived. The recovery key is returned
/// either way, so a stored key cannot be lost with it.
struct KeysSetup {
    state: CloudState,
    passphrase: String,
    stage: KeysStage,
}

impl KeysSetup {
    fn new(state: CloudState, passphrase: String) -> Self {
        Self {
            state,
            passphrase,
            stage: KeysStage::Fetch,
        }
    }

    fn created(stored: bool, recovery_key: String, warning: Option<String>) -> Value {
        json!(CloudKeyCreated {
            recovery_key,
            stored,
            warning,
        })
    }
}

impl Flow for KeysSetup {
    type Output = Value;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<Value>> {
        let stage = std::mem::replace(&mut self.stage, KeysStage::Fetch);
        match (stage, reply) {
            (KeysStage::Fetch, None) => Ok(Step::Call(cloud::keys_call(&self.state)?)),
            (KeysStage::Fetch, Some(reply)) => {
                if cloud::keys_reply(reply)?.is_some() {
                    bail!("Die Verschlüsselung ist für dieses Konto bereits eingerichtet");
                }
                let account = locora_crypto::create(&self.passphrase)?;
                let call = cloud::create_keys_call(&self.state, &account.bundle)?;
                self.stage = KeysStage::Store {
                    bundle: account.bundle,
                    recovery_key: account.recovery_key,
                };
                Ok(Step::Call(call))
            }
            (KeysStage::Store { bundle, recovery_key }, Some(reply)) => {
                if reply.is_success() {
                    return Ok(Step::Done(Self::created(true, recovery_key, None)));
                }
                let error = failure(&reply).to_string();
                self.stage = KeysStage::Confirm { bundle, recovery_key, error };
                Ok(Step::Call(cloud::keys_call(&self.state)?))
            }
            (KeysStage::Confirm { bundle, recovery_key, error }, Some(reply)) => match cloud::keys_reply(reply) {
                Ok(Some(remote)) if remote == bundle => Ok(Step::Done(Self::created(true, recovery_key, None))),
                Ok(Some(_)) => bail!(
                    "Ein anderes Gerät hat die Verschlüsselung inzwischen eingerichtet. Diesen neu erzeugten Schlüssel nicht verwenden."
                ),
                _ => Ok(Step::Done(Self::created(
                    false,
                    recovery_key,
                    Some(format!(
                        "Der Server hat die Schlüsseleinrichtung nicht bestätigt: {error}. Wiederherstellungsschlüssel sichern und den Status erneut prüfen."
                    )),
                ))),
            },
            (_, None) => bail!("the cloud did not answer"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ui_wraps_its_request_and_both_shapes_are_read() {
        let fields = serde_json::json!({ "server": "https://api.example.com", "email": "a@example.com",
            "password": "long enough password", "register": true });
        let wrapped: CloudConnect = serde_json::from_value(request_of(&serde_json::json!({ "request": fields.clone() }))).unwrap();
        let bare: CloudConnect = serde_json::from_value(request_of(&fields)).unwrap();
        assert_eq!(wrapped.server, "https://api.example.com");
        assert_eq!(bare.server, wrapped.server);
    }

    #[test]
    fn a_connect_request_is_checked_before_the_network() {
        let mut request = CloudConnect {
            server: "http://example.com".into(),
            email: "a@example.com".into(),
            password: "long enough password".into(),
            display_name: None,
            register: false,
            guest: false,
        };
        assert!(validate_connect(&request).is_err());
        request.server = "https://example.com".into();
        assert!(validate_connect(&request).is_ok());
        request.password = "short".into();
        assert!(validate_connect(&request).is_err());
        request.password = "long enough password".into();
        request.register = true;
        assert!(validate_connect(&request).is_err());
        request.display_name = Some("Lernende".into());
        assert!(validate_connect(&request).is_ok());
    }
}
