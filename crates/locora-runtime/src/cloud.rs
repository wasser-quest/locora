//! Locora Cloud on the device: the signed-in session and the offline entitlement.
//!
//! The session token is stored in `cloud.json` inside the profile (mode 0600).
//! The entitlement is checked locally with the public key built into the app,
//! so starting the app never needs the network.
//!
//! The requests of the cloud client are flows (see `cloud_http`): data that asks for a call and
//! takes its reply. The desktop runs them with the blocking wrappers at the end of this file; the
//! browser runs the same flows with `fetch`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use locora_cloud_api::api::{
    Account, EntitlementReply, KeyBundle, Login, Register, Session, SubscriptionState,
};
use locora_cloud_api::entitlement::{Access, Claims, parse_public_key, verify};
use serde::{Deserialize, Serialize};

use crate::cloud_http::{self, Body, Call, Flow, Method, Reply, Step, json};

/// Public half of the entitlement signing key (base64). The private key only
/// exists on the Locora Cloud server.
pub const ENTITLEMENT_PUBLIC_KEY: &str = "I625aQ0M16ixtCYAAo2UP0M6CstD+LcPsHQcK9aPKuI=";
/// The Locora Cloud server used when no other address is given. The app's sign-in form
/// shows the same address as its default.
pub const DEFAULT_SERVER: &str = "https://api.locora.cloud";
const FILE: &str = "cloud.json";
/// Seconds before an ordinary call gives up, and before a media transfer does.
const TIMEOUT_SECS: u64 = 20;
const MEDIA_TIMEOUT_SECS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudState {
    pub server: String,
    pub token: String,
    pub account: Account,
    #[serde(default)]
    pub sync_generation: i64,
    /// Last signed entitlement token the server issued.
    pub entitlement: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetentionStatus {
    pub state: String,
    pub last_successful_sync_at: i64,
    pub expires_at: i64,
    pub generation: i64,
}

pub fn state_path(data: &Path) -> PathBuf {
    data.join(FILE)
}

pub fn load(data: &Path) -> Result<Option<CloudState>> {
    let path = state_path(data);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(
            serde_json::from_str(&text).context("cloud.json ist beschädigt")?,
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("{} lesen", path.display())),
    }
}

pub fn save(data: &Path, state: &CloudState) -> Result<()> {
    fs::create_dir_all(data)?;
    let path = state_path(data);
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .with_context(|| format!("{} schreiben", path.display()))?;
    file.write_all(serde_json::to_string_pretty(state)?.as_bytes())?;
    // An existing file keeps its old mode when reopened, so set it every time.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn remove(data: &Path) -> Result<()> {
    match fs::remove_file(state_path(data)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Only HTTPS servers, or plain HTTP on the local machine for development.
pub fn check_server(server: &str) -> Result<String> {
    let parsed = url::Url::parse(server.trim()).context("Ungültige Cloud-Serveradresse")?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.host().is_none()
    {
        bail!("Cloud-Serveradresse darf keine Zugangsdaten, Abfrage oder Fragment enthalten");
    }
    let local = match parsed.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if parsed.scheme() != "https" && !(parsed.scheme() == "http" && local) {
        bail!("Der Server muss https:// verwenden (http nur für localhost)")
    }
    Ok(parsed.as_str().trim_end_matches('/').to_owned())
}

/// The entitlement when it is current or inside the offline grace period.
pub fn access(state: &CloudState, now: u64) -> Option<(Claims, Access)> {
    let key = parse_public_key(ENTITLEMENT_PUBLIC_KEY).ok()?;
    verify(state.entitlement.as_deref()?, &key, now).ok()
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The request for `path` on `server`. Only a checked server address is called.
pub(crate) fn call(method: Method, server: &str, path: &str, token: Option<&str>, body: Option<Body>) -> Result<Call> {
    Ok(Call {
        method,
        url: format!("{}{path}", check_server(server)?),
        token: token.map(str::to_owned),
        body,
        timeout_secs: TIMEOUT_SECS,
    })
}

pub(crate) fn json_body<B: Serialize>(body: &B) -> Result<Body> {
    Ok(Body::Json(serde_json::to_vec(body)?))
}

/// The state of a new session. The entitlement is read separately.
fn state_from_session(server: String, session: Session) -> CloudState {
    CloudState {
        server,
        token: session.token,
        account: session.account,
        sync_generation: 0,
        entitlement: None,
    }
}

/// A failing entitlement keeps the account signed in: access stays off until a later refresh.
fn with_entitlement(mut state: CloudState, reply: Reply) -> CloudState {
    state.entitlement = json::<EntitlementReply>(reply).ok().map(|reply| reply.token);
    state
}

pub(crate) fn entitlement_call(state: &CloudState) -> Result<Call> {
    call(Method::Post, &state.server, "/v1/entitlement", Some(&state.token), Some(json_body(&serde_json::Value::Null)?))
}

pub(crate) fn me_call(state: &CloudState) -> Result<Call> {
    call(Method::Get, &state.server, "/v1/me", Some(&state.token), None)
}

pub fn keys_call(state: &CloudState) -> Result<Call> {
    call(Method::Get, &state.server, "/v1/keys", Some(&state.token), None)
}

/// The wrapped keys, or `None` (a 404) before the encryption was set up.
pub fn keys_reply(reply: Reply) -> Result<Option<KeyBundle>> {
    if reply.status == 404 {
        return Ok(None);
    }
    json(reply).map(Some)
}

pub fn create_keys_call(state: &CloudState, bundle: &KeyBundle) -> Result<Call> {
    call(Method::Post, &state.server, "/v1/keys", Some(&state.token), Some(json_body(bundle)?))
}

pub fn logout_call(state: &CloudState) -> Result<Call> {
    call(Method::Delete, &state.server, "/v1/sessions", Some(&state.token), None)
}

pub(crate) fn complete_sync_call(state: &CloudState) -> Result<Call> {
    call(
        Method::Post,
        &state.server,
        &format!("/v1/sync/complete?generation={}", state.sync_generation),
        Some(&state.token),
        Some(json_body(&serde_json::Value::Null)?),
    )
}

/// A call on one media file of the account's storage (the id is keyed, see `locora-crypto`).
pub(crate) fn media_call(state: &CloudState, method: Method, id: &str, body: Option<Body>) -> Result<Call> {
    let mut call = call(
        method,
        &state.server,
        &format!("/v1/media/{id}?generation={}", state.sync_generation),
        Some(&state.token),
        body,
    )?;
    if method == Method::Put {
        call.timeout_secs = MEDIA_TIMEOUT_SECS;
    }
    Ok(call)
}

/// Whether the server holds the media file: 200 for yes, 404 for no.
pub(crate) fn exists_reply(reply: Reply) -> Result<bool> {
    match reply.status {
        200 => Ok(true),
        404 => Ok(false),
        other => bail!("Locora Cloud antwortet mit {other}"),
    }
}

/// The bytes of a media file, or the server's message.
pub(crate) fn media_reply(reply: Reply) -> Result<Vec<u8>> {
    if reply.is_success() { Ok(reply.body) } else { Err(cloud_http::failure(&reply)) }
}

/// A reply whose body is not needed; only a failure is an error.
pub fn ignore_reply(reply: Reply) -> Result<()> {
    json::<serde_json::Value>(reply).map(|_| ())
}

/// Signs an account in or up, and reads its entitlement when there is a subscription. The session
/// comes first. A failing entitlement keeps the account signed in.
pub struct Connect {
    server: String,
    session: Option<Call>,
    awaiting_entitlement: Option<CloudState>,
}

impl Connect {
    pub fn sign_up(server: &str, email: &str, password: &str, display_name: &str, guest: bool) -> Result<Self> {
        let body = Register {
            email: email.to_owned(),
            password: password.to_owned(),
            display_name: display_name.to_owned(),
            guest,
        };
        Self::new(server, "/v1/accounts", &body)
    }

    pub fn sign_in(server: &str, email: &str, password: &str) -> Result<Self> {
        let body = Login {
            email: email.to_owned(),
            password: password.to_owned(),
        };
        Self::new(server, "/v1/sessions", &body)
    }

    fn new<B: Serialize>(server: &str, path: &str, body: &B) -> Result<Self> {
        let server = check_server(server)?;
        let session = call(Method::Post, &server, path, None, Some(json_body(body)?))?;
        Ok(Self {
            server,
            session: Some(session),
            awaiting_entitlement: None,
        })
    }
}

impl Flow for Connect {
    type Output = CloudState;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<CloudState>> {
        let Some(reply) = reply else {
            return match self.session.take() {
                Some(call) => Ok(Step::Call(call)),
                None => bail!("the session was already requested"),
            };
        };
        if let Some(state) = self.awaiting_entitlement.take() {
            return Ok(Step::Done(with_entitlement(state, reply)));
        }
        let session: Session = json(reply)?;
        let state = state_from_session(self.server.clone(), session);
        if state.account.subscription == SubscriptionState::None {
            return Ok(Step::Done(state));
        }
        let call = entitlement_call(&state)?;
        self.awaiting_entitlement = Some(state);
        Ok(Step::Call(call))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefreshStage {
    Account,
    Entitlement,
}

/// Reads the account and, with a subscription, its entitlement again. Without a subscription the
/// old entitlement is dropped. A failing entitlement is an error, and the caller keeps the old state.
pub struct Refresh {
    state: CloudState,
    stage: RefreshStage,
}

impl Refresh {
    pub fn new(state: CloudState) -> Self {
        Self {
            state,
            stage: RefreshStage::Account,
        }
    }
}

impl Flow for Refresh {
    type Output = CloudState;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<CloudState>> {
        match (self.stage, reply) {
            (RefreshStage::Account, None) => Ok(Step::Call(me_call(&self.state)?)),
            (RefreshStage::Account, Some(reply)) => {
                self.state.account = json(reply)?;
                if self.state.account.subscription == SubscriptionState::None {
                    self.state.entitlement = None;
                    return Ok(Step::Done(self.state.clone()));
                }
                self.stage = RefreshStage::Entitlement;
                Ok(Step::Call(entitlement_call(&self.state)?))
            }
            (RefreshStage::Entitlement, Some(reply)) => {
                self.state.entitlement = Some(json::<EntitlementReply>(reply)?.token);
                Ok(Step::Done(self.state.clone()))
            }
            (RefreshStage::Entitlement, None) => bail!("the entitlement was not requested"),
        }
    }
}

/// The blocking wrappers for the desktop and the CLI: each runs its flow with ureq.
#[cfg(feature = "cloud")]
mod blocking {
    use super::*;
    use crate::cloud_http::{Exchange, expect_success, run};
    use locora_cloud_api::api::{
        Challenge, CreateChallenge, FinishReply, Friend, FriendRequest, GameAnswer, GameState, JourneyStarted,
        PasswordChange, Ranking, StartJourney, XpState,
    };

    pub(crate) fn get<T: serde::de::DeserializeOwned>(server: &str, path: &str, token: &str) -> Result<T> {
        run(Exchange::new(call(Method::Get, server, path, Some(token), None)?, json::<T>))
    }

    pub(crate) fn put<T: serde::de::DeserializeOwned, B: Serialize>(server: &str, path: &str, token: &str, body: &B) -> Result<T> {
        run(Exchange::new(call(Method::Put, server, path, Some(token), Some(json_body(body)?))?, json::<T>))
    }

    pub(crate) fn post<T: serde::de::DeserializeOwned, B: Serialize>(server: &str, path: &str, token: Option<&str>, body: &B) -> Result<T> {
        run(Exchange::new(call(Method::Post, server, path, token, Some(json_body(body)?))?, json::<T>))
    }

    pub fn register(server: &str, email: &str, password: &str, display_name: &str, guest: bool) -> Result<CloudState> {
        run(Connect::sign_up(server, email, password, display_name, guest)?)
    }

    pub fn login(server: &str, email: &str, password: &str) -> Result<CloudState> {
        run(Connect::sign_in(server, email, password)?)
    }

    /// Finishes a session that was read elsewhere (its entitlement is read here).
    #[cfg(test)]
    pub(crate) fn finish_login(server: String, session: Session) -> Result<CloudState> {
        let mut state = state_from_session(server, session);
        if state.account.subscription != SubscriptionState::None {
            // A valid account session must survive a transient entitlement failure.
            let _ = refresh(&mut state);
        }
        Ok(state)
    }

    /// Changes the signed-in account password. The server invalidates this and all
    /// other sessions; callers must remove the local session after success.
    pub fn change_password(state: &CloudState, current: &str, new: &str) -> Result<()> {
        let body = PasswordChange {
            current_password: current.to_owned(),
            new_password: new.to_owned(),
        };
        let _: serde_json::Value = put(&state.server, "/v1/me/password", &state.token, &body)?;
        Ok(())
    }

    /// Asks the server for a new entitlement. Without a subscription the old token is dropped.
    pub fn refresh(state: &mut CloudState) -> Result<()> {
        match run(Exchange::new(entitlement_call(state)?, json::<EntitlementReply>)) {
            Ok(reply) => {
                state.entitlement = Some(reply.token);
                Ok(())
            }
            Err(err) => {
                state.entitlement = None;
                Err(err)
            }
        }
    }

    /// Current account data from the server.
    pub fn me(state: &mut CloudState) -> Result<()> {
        state.account = run(Exchange::new(me_call(state)?, json::<Account>))?;
        Ok(())
    }

    pub fn retention(state: &CloudState) -> Result<RetentionStatus> {
        get(&state.server, "/v1/sync/retention", &state.token)
    }

    pub fn begin_rebuild(state: &CloudState, generation: i64) -> Result<RetentionStatus> {
        post(&state.server, "/v1/sync/rebuild", Some(&state.token), &serde_json::json!({"generation": generation}))
    }

    /// All-time ranking of this account's world, or of its friends.
    pub fn ranking(state: &CloudState, friends: bool) -> Result<Ranking> {
        let scope = if friends { "friends" } else { "global" };
        get(&state.server, &format!("/v1/ranking?scope={scope}"), &state.token)
    }

    pub fn friends(state: &CloudState) -> Result<Vec<Friend>> {
        get(&state.server, "/v1/friends", &state.token)
    }

    pub fn request_friend(state: &CloudState, email: &str) -> Result<Friend> {
        let body = FriendRequest { email: email.to_owned() };
        post(&state.server, "/v1/friends", Some(&state.token), &body)
    }

    pub fn create_challenge(state: &CloudState, name: &str, ends_at: u64) -> Result<Challenge> {
        let body = CreateChallenge { name: name.to_owned(), ends_at };
        post(&state.server, "/v1/challenges", Some(&state.token), &body)
    }

    pub fn join_challenge(state: &CloudState, id: &str) -> Result<Challenge> {
        let id = percent_encoding::utf8_percent_encode(id, percent_encoding::NON_ALPHANUMERIC);
        post(&state.server, &format!("/v1/challenges/{id}/join"), Some(&state.token), &serde_json::Value::Null)
    }

    pub fn challenge_ranking(state: &CloudState, id: &str) -> Result<Ranking> {
        let id = percent_encoding::utf8_percent_encode(id, percent_encoding::NON_ALPHANUMERIC);
        get(&state.server, &format!("/v1/challenges/{id}/ranking"), &state.token)
    }

    /// The wrapped account keys, or `None` before the encryption was set up.
    pub fn get_keys(state: &CloudState) -> Result<Option<KeyBundle>> {
        run(Exchange::new(keys_call(state)?, keys_reply))
    }

    pub fn create_keys(state: &CloudState, bundle: &KeyBundle) -> Result<()> {
        run(Exchange::new(create_keys_call(state, bundle)?, ignore_reply))
    }

    pub fn replace_keys(state: &CloudState, bundle: &KeyBundle) -> Result<()> {
        let _: serde_json::Value = put(&state.server, "/v1/keys", &state.token, bundle)?;
        Ok(())
    }

    /// Starts a journey on the server; its clock starts now.
    pub fn start_journey(state: &CloudState, journey_id: &str, challenge: Option<&str>) -> Result<JourneyStarted> {
        let body = StartJourney {
            journey_id: journey_id.to_owned(),
            challenge_id: challenge.map(str::to_owned),
        };
        post(&state.server, "/v1/game/journeys", Some(&state.token), &body)
    }

    /// Reports one answer. `at_ms` is measured from the same start as the server's clock.
    pub fn answer(state: &CloudState, journey_id: &str, answer: &GameAnswer) -> Result<GameState> {
        post(&state.server, &format!("/v1/game/journeys/{journey_id}/answers"), Some(&state.token), answer)
    }

    /// Ends a journey; the server computes its XP from the answers it recorded.
    pub fn finish_journey(state: &CloudState, journey_id: &str) -> Result<FinishReply> {
        post(&state.server, &format!("/v1/game/journeys/{journey_id}/finish"), Some(&state.token), &serde_json::Value::Null)
    }

    /// The XP of this account, as the server counts it.
    pub fn xp(state: &CloudState) -> Result<XpState> {
        get(&state.server, "/v1/game/xp", &state.token)
    }

    /// Ends the server session (best effort) and forgets the local one.
    pub fn logout(data: &Path, state: &CloudState) -> Result<()> {
        let _ = run(Exchange::new(logout_call(state)?, expect_success));
        remove(data)
    }
}

#[cfg(feature = "cloud")]
pub use blocking::*;

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    fn fixture(response: String) -> (CloudState, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
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
            socket.write_all(response.as_bytes()).unwrap();
            request
        });
        let state = CloudState {
            server,
            token: "synthetic-test-token".into(),
            sync_generation: 0,
            entitlement: None,
            account: Account {
                id: "local".into(),
                email: "test@example.invalid".into(),
                display_name: "Test".into(),
                subscription: SubscriptionState::None,
                plan: None,
                period_end: None,
            },
        };
        (state, worker)
    }

    #[test]
    fn challenge_ids_stay_in_one_path_segment() {
        let id = "../a/b?x=#% Grüße";
        let segment = "%2E%2E%2Fa%2Fb%3Fx%3D%23%25%20Gr%C3%BC%C3%9Fe";
        let (state, worker) = fixture("HTTP/1.1 200 OK\r\nContent-Length: 33\r\nConnection: close\r\n\r\n{\"id\":\"x\",\"name\":\"X\",\"ends_at\":1}".into());
        join_challenge(&state, id).unwrap();
        assert_eq!(
            worker.join().unwrap(),
            format!("POST /v1/challenges/{segment}/join HTTP/1.1\r\n")
        );
        let (state, worker) = fixture(
            "HTTP/1.1 200 OK\r\nContent-Length: 14\r\nConnection: close\r\n\r\n{\"entries\":[]}"
                .into(),
        );
        challenge_ranking(&state, id).unwrap();
        assert_eq!(
            worker.join().unwrap(),
            format!("GET /v1/challenges/{segment}/ranking HTTP/1.1\r\n")
        );
    }

    #[test]
    fn redirects_reach_response_handling() {
        let (state, worker) = fixture("HTTP/1.1 302 Found\r\nLocation: /redirected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
        let error = get::<serde_json::Value>(&state.server, "/original", &state.token).unwrap_err();
        assert!(error.to_string().contains("302"), "{error:#}");
        assert!(worker.join().unwrap().starts_with("GET /original "));
    }

    #[test]
    fn rejected_password_change_is_an_error() {
        let body = r#"{"error":"Falsches Passwort"}"#;
        let response = format!(
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let (state, worker) = fixture(response);
        let error = change_password(&state, "wrong", "new password 12345").unwrap_err();
        assert!(error.to_string().contains("Falsches Passwort"), "{error:#}");
        assert!(worker.join().unwrap().starts_with("PUT /v1/me/password "));

        let (state, worker) = fixture(
            "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        );
        change_password(&state, "old", "new password 12345").unwrap();
        assert!(worker.join().unwrap().starts_with("PUT /v1/me/password "));
    }

    #[test]
    fn successful_login_survives_entitlement_refresh_failure() {
        let (state, worker) = fixture(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .into(),
        );
        let session = Session {
            token: state.token.clone(),
            account: Account {
                subscription: SubscriptionState::Active,
                ..state.account
            },
        };
        let signed_in = finish_login(state.server, session).unwrap();
        assert!(signed_in.entitlement.is_none());
        assert!(worker.join().unwrap().starts_with("POST /v1/entitlement "));
    }

    #[test]
    fn only_https_or_localhost_is_accepted() {
        assert_eq!(
            check_server("https://cloud.example/").unwrap(),
            "https://cloud.example"
        );
        assert_eq!(
            check_server("http://127.0.0.1:8787").unwrap(),
            "http://127.0.0.1:8787"
        );
        assert_eq!(
            check_server("http://localhost").unwrap(),
            "http://localhost"
        );
        assert!(check_server("http://cloud.example").is_err());
        assert!(check_server("http://localhost.evil.example").is_err());
        assert!(check_server("https://user:secret@cloud.example").is_err());
        assert!(check_server("https://cloud.example?token=secret").is_err());
        assert!(check_server("https://cloud.example#fragment").is_err());
        assert!(check_server("https://").is_err());
    }

    #[test]
    fn the_shipped_public_key_parses() {
        assert!(parse_public_key(ENTITLEMENT_PUBLIC_KEY).is_ok());
    }

    #[test]
    fn a_sign_in_then_reads_the_entitlement_only_with_a_subscription() {
        let session = |subscription: &str| {
            format!(
                r#"{{"token":"t","account":{{"id":"a","email":"e@x.invalid","display_name":"D","subscription":"{subscription}","plan":null,"period_end":null}}}}"#
            )
        };
        let mut flow = Connect::sign_in("https://cloud.example", "e@x.invalid", "password one").unwrap();
        assert!(matches!(flow.step(None), Ok(Step::Call(_))));
        let state = match flow.step(Some(Reply { status: 200, body: session("none").into_bytes() })) {
            Ok(Step::Done(state)) => state,
            _ => panic!("no subscription: the session is the whole answer"),
        };
        assert!(state.entitlement.is_none());

        let mut flow = Connect::sign_in("https://cloud.example", "e@x.invalid", "password one").unwrap();
        let _ = flow.step(None);
        let next = flow.step(Some(Reply { status: 200, body: session("active").into_bytes() }));
        let Ok(Step::Call(call)) = next else { panic!("an active subscription asks for the entitlement") };
        assert!(call.url.ends_with("/v1/entitlement"));
        let done = flow.step(Some(Reply { status: 503, body: Vec::new() }));
        assert!(matches!(done, Ok(Step::Done(state)) if state.entitlement.is_none()));
    }

    #[test]
    fn a_media_upload_waits_longer_than_a_sign_in() {
        let state = CloudState {
            server: "https://cloud.example".into(),
            token: "t".into(),
            sync_generation: 3,
            entitlement: None,
            account: Account {
                id: "a".into(),
                email: "e@x.invalid".into(),
                display_name: "D".into(),
                subscription: SubscriptionState::None,
                plan: None,
                period_end: None,
            },
        };
        let upload = media_call(&state, Method::Put, "abc", Some(Body::Bytes(vec![1]))).unwrap();
        assert_eq!(upload.timeout_secs, MEDIA_TIMEOUT_SECS);
        assert_eq!(upload.url, "https://cloud.example/v1/media/abc?generation=3");
        assert_eq!(media_call(&state, Method::Head, "abc", None).unwrap().timeout_secs, TIMEOUT_SECS);
    }
}
