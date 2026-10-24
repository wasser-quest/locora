//! The cloud client in the browser. The flows of `locora_runtime::cloud` ask for their HTTP calls;
//! this file keeps the flow that waits for a reply, and the page sends each call with `fetch` and
//! hands the reply back (`cloud_reply`). Nothing here blocks: the page only waits for `fetch`.
//!
//! A command answers `{"call": …}` while a call is needed, and `{"done": …}` with its result.

use std::cell::RefCell;
use std::collections::VecDeque;

use anyhow::{Result, anyhow, bail};
use locora_app_api::{
    CloudConnect, CloudKeyCreated, CloudKeySetup, CloudStatus, GameResetReport, Social,
};
use locora_app_core::{cloud_view, social};
use locora_cloud_api::api::KeyBundle;
use locora_runtime::cloud::{self, CloudState, Connect, Refresh, RetentionStatus};
use locora_runtime::cloud_http::{
    Body, Call, Exchange, Flow, Method, Reply, Step, failure, json as cloud_http_json,
};
use serde_json::{Value, json};

/// A flow waiting for the replies to its calls: how many it asked for, and those that came.
struct Waiting {
    flow: Box<dyn Flow<Output = Value>>,
    /// `None` for a single call (`step`), `Some(n)` for calls side by side (`step_many`).
    expected: Option<usize>,
    replies: Vec<Reply>,
}

thread_local! {
    /// The flow that waits for the reply to its last call.
    static WAITING: RefCell<Option<Waiting>> = const { RefCell::new(None) };
    /// The bodies of the last calls, read by the page with `cloud_body`, one per call in order.
    static OUTBOUND: RefCell<VecDeque<Vec<u8>>> = const { RefCell::new(VecDeque::new()) };
}

/// Runs a command that may need the network. A new command drops a flow that was still waiting.
pub fn command(name: &str, args: &Value) -> Result<Value> {
    // The worker that derives the key has no profile: this one works without.
    if name == "cloud_unlock" {
        return crate::cloud_auto::unlock(args);
    }
    WAITING.with(|cell| cell.borrow_mut().take());
    OUTBOUND.with(|cell| cell.borrow_mut().clear());
    let data = crate::with_backend(|backend| Ok(backend.profile.data_dir.clone()))?;
    if let Some(flow) = crate::cloud_auto::command(name, args, &data)? {
        return match flow {
            crate::cloud_auto::Run::Done(value) => Ok(json!({ "done": value })),
            crate::cloud_auto::Run::Flow(flow) => drive(flow, None),
        };
    }
    match name {
        // A deletion of the game scores that still waits for the server is asked again here
        // (the app reads the status at the start, after signing in and on the settings).
        "cloud_status" => {
            let state = cloud::load(&data)?;
            match state {
                Some(state) if cloud::rankings_pending(&data, &state.account.id)? => {
                    let call = cloud::erase_game_call(&state)?;
                    drive(
                        Box::new(Map::new(
                            Exchange::new(call, cloud::erase_game_reply),
                            move |outcome: cloud::RankingsDeletion| {
                                cloud::note_rankings(&data, &state, outcome)?;
                                Ok(json!(status(Some(&state))))
                            },
                        )),
                        None,
                    )
                }
                state => Ok(json!({ "done": status(state.as_ref()) })),
            }
        }
        "cloud_refresh" => {
            let state = saved(&data)?;
            drive(
                Box::new(Map::new(Refresh::new(state), move |state: CloudState| {
                    cloud::save(&data, &state)?;
                    Ok(json!(status(Some(&state))))
                })),
                None,
            )
        }
        "cloud_logout" => {
            let Some(state) = cloud::load(&data)? else {
                return Ok(json!({ "done": Value::Null }));
            };
            // The server is told best effort: the local account is forgotten either way.
            let call = cloud::logout_call(&state)?;
            drive(
                Box::new(Map::new(Exchange::new(call, |_| Ok(())), move |()| {
                    cloud::remove(&data)?;
                    Ok(Value::Null)
                })),
                None,
            )
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
            drive(
                Box::new(Map::new(flow, move |state: CloudState| {
                    cloud::save(&data, &state)?;
                    Ok(json!(status(Some(&state))))
                })),
                None,
            )
        }
        "cloud_keys_status" => {
            let state = saved(&data)?;
            let call = cloud::keys_call(&state)?;
            drive(
                Box::new(Map::new(
                    Exchange::new(call, cloud::keys_reply),
                    |keys: Option<KeyBundle>| Ok(json!(keys.is_some())),
                )),
                None,
            )
        }
        "cloud_keys_setup" => {
            let request: CloudKeySetup = serde_json::from_value(request_of(args))?;
            if !(12..=1024).contains(&request.passphrase.chars().count()) {
                bail!("Verschlüsselungs-Passwort braucht 12 bis 1024 Zeichen");
            }
            let state = saved(&data)?;
            drive(Box::new(KeysSetup::new(state, request.passphrase)), None)
        }
        "cloud_sync" => crate::cloud_auto::sync(args, data),
        // Friends, invite links, rankings and reports: the arguments are checked by the flows
        // (locora_app_core::social) before anything is sent. The page's cloud lane runs one
        // command at a time, so the friends page waits while an auto-sync runs.
        "cloud_friends" => social(&data, social::friends),
        "cloud_friend_ask" => social(&data, |state| social::ask(state, text(args, "email"))),
        "cloud_friend_remove" => social(&data, |state| social::remove(state, text(args, "id"))),
        "cloud_ranking" => {
            let friends = args.get("friends").and_then(Value::as_bool).unwrap_or(true);
            social(&data, |state| social::ranking(state, friends))
        }
        "cloud_report" => {
            let reason = args.get("reason").and_then(Value::as_str);
            social(&data, |state| {
                social::report(state, text(args, "reference"), reason)
            })
        }
        "cloud_invite_create" => social(&data, social::create_invite),
        "cloud_invite_revoke" => social(&data, |state| {
            social::revoke_invite(state, text(args, "code"))
        }),
        "cloud_invite_preview" => social(&data, |state| {
            social::preview_invite(state, text(args, "code"))
        }),
        "cloud_invite_accept" => social(&data, |state| {
            social::accept_invite(state, text(args, "code"))
        }),
        // Deleted here first; the rankings on the server are asked for when an account is
        // signed in, and asked again (see `cloud_status`) until the server has deleted them.
        "game_reset" => {
            crate::with_backend(|backend| backend.profile.reset_game().map(drop))?;
            let report = |outcome| {
                json!(GameResetReport {
                    rankings: cloud_view::rankings(outcome),
                })
            };
            let Some(state) = cloud::load(&data)? else {
                return Ok(json!({ "done": report(cloud::RankingsDeletion::NotSignedIn) }));
            };
            let call = cloud::erase_game_call(&state)?;
            drive(
                Box::new(Map::new(
                    Exchange::new(call, cloud::erase_game_reply),
                    move |outcome: cloud::RankingsDeletion| {
                        cloud::note_rankings(&data, &state, outcome)?;
                        Ok(report(outcome))
                    },
                )),
                None,
            )
        }
        "cloud_erase" => {
            let state = saved(&data)?;
            let call = cloud::erase_call(&state)?;
            drive(
                Box::new(Map::new(
                    Exchange::new(call, cloud_http_json),
                    move |remote: RetentionStatus| {
                        let mut state = saved(&data)?;
                        let store =
                            crate::with_backend(|backend| Ok(backend.profile.store.clone()))?;
                        cloud::adopt_generation(&mut state, &store, remote.generation)?;
                        // The page forgets the kept key as well; auto-sync asks again.
                        state.auto_sync = false;
                        cloud::save(&data, &state)?;
                        Ok(Value::Null)
                    },
                )),
                None,
            )
        }
        "cloud_delete_account" => {
            let password = args
                .get("password")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if password.is_empty() || password.chars().count() > 256 {
                bail!("Cloud-Passwort eingeben");
            }
            let state = saved(&data)?;
            let call = cloud::delete_account_call(&state, &password)?;
            drive(
                Box::new(Map::new(
                    Exchange::new(call, cloud::delete_account_reply),
                    move |()| {
                        cloud::remove(&data)?;
                        Ok(Value::Null)
                    },
                )),
                None,
            )
        }
        other => bail!("unknown cloud command {other}"),
    }
}

/// The reply the page got for the last call. Status 0 means the request never reached the server.
/// After calls side by side the page hands the replies over one by one, in the order of the calls;
/// all but the last answer `{"wait": true}`.
pub fn reply(status: u32, body: Vec<u8>) -> Result<Value> {
    let mut waiting = WAITING
        .with(|cell| cell.borrow_mut().take())
        .ok_or_else(|| anyhow!("no cloud request is waiting for a reply"))?;
    let status = u16::try_from(status).unwrap_or(0);
    let reply = Reply { status, body };
    match waiting.expected {
        None => drive(waiting.flow, Some(reply)),
        Some(expected) => {
            waiting.replies.push(reply);
            if waiting.replies.len() < expected {
                WAITING.with(|cell| *cell.borrow_mut() = Some(waiting));
                return Ok(json!({ "wait": true }));
            }
            let step = waiting.flow.step_many(waiting.replies)?;
            resume(waiting.flow, step)
        }
    }
}

/// The body of the next call, empty when it had none.
pub fn body() -> Vec<u8> {
    OUTBOUND.with(|cell| cell.borrow_mut().pop_front().unwrap_or_default())
}

/// Runs a flow until it needs calls (which the page sends) or is done.
fn drive(mut flow: Box<dyn Flow<Output = Value>>, reply: Option<Reply>) -> Result<Value> {
    let step = flow.step(reply)?;
    resume(flow, step)
}

pub(crate) fn resume(flow: Box<dyn Flow<Output = Value>>, step: Step<Value>) -> Result<Value> {
    OUTBOUND.with(|cell| cell.borrow_mut().clear());
    match step {
        Step::Call(call) => {
            let description = describe(&call);
            WAITING.with(|cell| {
                *cell.borrow_mut() = Some(Waiting {
                    flow,
                    expected: None,
                    replies: Vec::new(),
                })
            });
            Ok(json!({ "call": description }))
        }
        Step::Calls(calls) => {
            let descriptions: Vec<Value> = calls.iter().map(describe).collect();
            WAITING.with(|cell| {
                *cell.borrow_mut() = Some(Waiting {
                    flow,
                    expected: Some(calls.len()),
                    replies: Vec::new(),
                })
            });
            Ok(json!({ "calls": descriptions }))
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
    OUTBOUND.with(|cell| cell.borrow_mut().push_back(bytes));
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

/// A friends command: `SignedOut` without an account (and without the network), else its flow,
/// whose result goes to the page as JSON.
fn social<F>(data: &std::path::Path, make: impl FnOnce(&CloudState) -> Result<F>) -> Result<Value>
where
    F: Flow + 'static,
    F::Output: serde::Serialize,
{
    let Some(state) = cloud::load(data)? else {
        return Ok(json!({ "done": Social::<()>::SignedOut }));
    };
    drive(
        Box::new(Map::new(make(&state)?, |output: F::Output| {
            Ok(json!(output))
        })),
        None,
    )
}

/// A text argument of the UI; a missing one is empty (and refused by the flow).
fn text<'a>(args: &'a Value, name: &str) -> &'a str {
    args.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn saved(data: &std::path::Path) -> Result<CloudState> {
    cloud::load(data)?.ok_or_else(|| anyhow!("Kein Cloud-Konto angemeldet"))
}

pub(crate) fn status(state: Option<&CloudState>) -> CloudStatus {
    locora_app_core::cloud_view::status(state)
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
pub(crate) struct Map<F, G> {
    flow: F,
    convert: G,
}

impl<F, G> Map<F, G> {
    pub(crate) fn new(flow: F, convert: G) -> Self {
        Self { flow, convert }
    }
}

impl<F: Flow, G: Fn(F::Output) -> Result<Value>> Flow for Map<F, G> {
    type Output = Value;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<Value>> {
        let step = self.flow.step(reply)?;
        self.convert(step)
    }

    fn step_many(&mut self, replies: Vec<Reply>) -> Result<Step<Value>> {
        let step = self.flow.step_many(replies)?;
        self.convert(step)
    }
}

impl<F: Flow, G: Fn(F::Output) -> Result<Value>> Map<F, G> {
    fn convert(&self, step: Step<F::Output>) -> Result<Step<Value>> {
        match step {
            Step::Call(call) => Ok(Step::Call(call)),
            Step::Calls(calls) => Ok(Step::Calls(calls)),
            Step::Done(output) => Ok(Step::Done((self.convert)(output)?)),
        }
    }
}

enum KeysStage {
    Fetch,
    Store {
        bundle: KeyBundle,
        recovery_key: String,
    },
    Confirm {
        bundle: KeyBundle,
        recovery_key: String,
        error: String,
    },
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
                let call = cloud::create_keys_call(&self.state, &account.bundle, &account.dek)?;
                self.stage = KeysStage::Store {
                    bundle: account.bundle,
                    recovery_key: account.recovery_key,
                };
                Ok(Step::Call(call))
            }
            (
                KeysStage::Store {
                    bundle,
                    recovery_key,
                },
                Some(reply),
            ) => {
                if reply.is_success() {
                    return Ok(Step::Done(Self::created(true, recovery_key, None)));
                }
                let error = failure(&reply).to_string();
                self.stage = KeysStage::Confirm {
                    bundle,
                    recovery_key,
                    error,
                };
                Ok(Step::Call(cloud::keys_call(&self.state)?))
            }
            (
                KeysStage::Confirm {
                    bundle,
                    recovery_key,
                    error,
                },
                Some(reply),
            ) => match cloud::keys_reply(reply) {
                Ok(Some(remote)) if remote == bundle => {
                    Ok(Step::Done(Self::created(true, recovery_key, None)))
                }
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
        let wrapped: CloudConnect = serde_json::from_value(request_of(
            &serde_json::json!({ "request": fields.clone() }),
        ))
        .unwrap();
        let bare: CloudConnect = serde_json::from_value(request_of(&fields)).unwrap();
        assert_eq!(wrapped.server, "https://api.example.com");
        assert_eq!(bare.server, wrapped.server);
    }

    #[test]
    fn the_keys_of_a_new_account_go_with_the_proof_of_their_key() {
        use locora_cloud_api::api::{Account, KeyUpload, SubscriptionState};
        let state = CloudState {
            server: "https://api.example.com".into(),
            token: "t".into(),
            account: Account {
                id: "a".into(),
                email: "a@example.com".into(),
                display_name: "A".into(),
                subscription: SubscriptionState::None,
                plan: None,
                period_end: None,
                email_verified: None,
            },
            sync_generation: 0,
            entitlement: None,
            last_sync_at: None,
            auto_sync: false,
            key_proof_sent_at: None,
        };
        let passphrase = "ein langes geheimes passwort";
        let mut flow = KeysSetup::new(state, passphrase.into());
        let Ok(Step::Call(fetch)) = flow.step(None) else {
            panic!("it asks for the keys first")
        };
        assert_eq!(
            (fetch.method, fetch.url.ends_with("/v1/keys")),
            (Method::Get, true)
        );
        let none = Reply {
            status: 404,
            body: r#"{"error":"Noch keine Verschlüsselung eingerichtet"}"#.into(),
        };
        let Ok(Step::Call(store)) = flow.step(Some(none)) else {
            panic!("then it stores new ones")
        };
        assert_eq!(store.method, Method::Post);
        let Some(Body::Json(body)) = &store.body else {
            panic!("a JSON body")
        };
        let upload: KeyUpload = serde_json::from_slice(body).unwrap();
        let dek = locora_crypto::unlock(&upload.bundle, passphrase).unwrap();
        assert_eq!(upload, KeyUpload::new(upload.bundle.clone(), &dek));
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
