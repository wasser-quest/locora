//! Locora Cloud on the device: the signed-in session and the offline entitlement.
//!
//! The session token is stored in `cloud.json` inside the profile (mode 0600).
//! The entitlement is checked locally with the public key built into the app,
//! so starting the app never needs the network.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use locora_cloud_api::api::{
    Account, Challenge, CreateChallenge, EntitlementReply, ErrorBody, FinishReply, Friend,
    FriendRequest, GameAnswer, GameState, JourneyStarted, KeyBundle, Login, Ranking, Register,
    Session, StartJourney, SubscriptionState, XpState,
};
use locora_cloud_api::entitlement::{Access, Claims, parse_public_key, verify};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Public half of the entitlement signing key (base64). The private key only
/// exists on the Locora Cloud server.
pub const ENTITLEMENT_PUBLIC_KEY: &str = "I625aQ0M16ixtCYAAo2UP0M6CstD+LcPsHQcK9aPKuI=";
const FILE: &str = "cloud.json";

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

#[derive(Debug, Clone, Deserialize)]
pub struct RetentionStatus {
    pub state: String,
    pub last_successful_sync_at: i64,
    pub expires_at: i64,
    pub generation: i64,
}

pub fn retention(state: &CloudState) -> Result<RetentionStatus> {
    get(&state.server, "/v1/sync/retention", &state.token)
}

pub(crate) fn complete_sync(state: &CloudState) -> Result<RetentionStatus> {
    post(
        &state.server,
        &format!("/v1/sync/complete?generation={}", state.sync_generation),
        Some(&state.token),
        &serde_json::Value::Null,
    )
}

pub fn begin_rebuild(state: &CloudState, generation: i64) -> Result<RetentionStatus> {
    post(
        &state.server,
        "/v1/sync/rebuild",
        Some(&state.token),
        &serde_json::json!({"generation": generation}),
    )
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

pub fn register(
    server: &str,
    email: &str,
    password: &str,
    display_name: &str,
) -> Result<CloudState> {
    let server = check_server(server)?;
    let body = Register {
        email: email.to_owned(),
        password: password.to_owned(),
        display_name: display_name.to_owned(),
    };
    let session: Session = post(&server, "/v1/accounts", None, &body)?;
    finish_login(server, session)
}

pub fn login(server: &str, email: &str, password: &str) -> Result<CloudState> {
    let server = check_server(server)?;
    let body = Login {
        email: email.to_owned(),
        password: password.to_owned(),
    };
    let session: Session = post(&server, "/v1/sessions", None, &body)?;
    finish_login(server, session)
}

/// Changes the signed-in account password. The server invalidates this and all
/// other sessions; callers must remove the local session after success.
pub fn change_password(state: &CloudState, current: &str, new: &str) -> Result<()> {
    let body = locora_cloud_api::api::PasswordChange {
        current_password: current.to_owned(),
        new_password: new.to_owned(),
    };
    let response = send(
        &state.server,
        ureq::http::Method::PUT,
        "/v1/me/password",
        Some(&state.token),
        Some(&body),
    )?;
    read::<serde_json::Value>(response)?;
    Ok(())
}

fn finish_login(server: String, session: Session) -> Result<CloudState> {
    let mut state = CloudState {
        server,
        token: session.token,
        account: session.account,
        sync_generation: 0,
        entitlement: None,
    };
    if state.account.subscription != SubscriptionState::None {
        // A valid account session must survive a transient entitlement failure.
        // Access stays disabled until a later successful refresh.
        let _ = refresh(&mut state);
    }
    Ok(state)
}

/// Asks the server for a new entitlement. Without a subscription the old token is dropped.
pub fn refresh(state: &mut CloudState) -> Result<()> {
    match post::<EntitlementReply, _>(
        &state.server,
        "/v1/entitlement",
        Some(&state.token),
        &serde_json::Value::Null,
    ) {
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
    state.account = get(&state.server, "/v1/me", &state.token)?;
    Ok(())
}

/// All-time ranking of this account's world, or of its friends.
pub fn ranking(state: &CloudState, friends: bool) -> Result<Ranking> {
    let scope = if friends { "friends" } else { "global" };
    get(
        &state.server,
        &format!("/v1/ranking?scope={scope}"),
        &state.token,
    )
}

pub fn friends(state: &CloudState) -> Result<Vec<Friend>> {
    get(&state.server, "/v1/friends", &state.token)
}

pub fn request_friend(state: &CloudState, email: &str) -> Result<Friend> {
    let body = FriendRequest {
        email: email.to_owned(),
    };
    post(&state.server, "/v1/friends", Some(&state.token), &body)
}

pub fn create_challenge(state: &CloudState, name: &str, ends_at: u64) -> Result<Challenge> {
    let body = CreateChallenge {
        name: name.to_owned(),
        ends_at,
    };
    post(&state.server, "/v1/challenges", Some(&state.token), &body)
}

pub fn join_challenge(state: &CloudState, id: &str) -> Result<Challenge> {
    let id = percent_encoding::utf8_percent_encode(id, percent_encoding::NON_ALPHANUMERIC);
    post(
        &state.server,
        &format!("/v1/challenges/{id}/join"),
        Some(&state.token),
        &serde_json::Value::Null,
    )
}

pub fn challenge_ranking(state: &CloudState, id: &str) -> Result<Ranking> {
    let id = percent_encoding::utf8_percent_encode(id, percent_encoding::NON_ALPHANUMERIC);
    get(
        &state.server,
        &format!("/v1/challenges/{id}/ranking"),
        &state.token,
    )
}

/// The wrapped account keys, or `None` before the encryption was set up.
pub fn get_keys(state: &CloudState) -> Result<Option<KeyBundle>> {
    let response = send(
        &state.server,
        ureq::http::Method::GET,
        "/v1/keys",
        Some(&state.token),
        None::<&()>,
    )?;
    if response.status().as_u16() == 404 {
        return Ok(None);
    }
    read(response).map(Some)
}

pub fn create_keys(state: &CloudState, bundle: &KeyBundle) -> Result<()> {
    let _: serde_json::Value = post(&state.server, "/v1/keys", Some(&state.token), bundle)?;
    Ok(())
}

pub fn replace_keys(state: &CloudState, bundle: &KeyBundle) -> Result<()> {
    let _: serde_json::Value = put(&state.server, "/v1/keys", &state.token, bundle)?;
    Ok(())
}

/// Starts a journey on the server; its clock starts now.
pub fn start_journey(
    state: &CloudState,
    journey_id: &str,
    challenge: Option<&str>,
) -> Result<JourneyStarted> {
    let body = StartJourney {
        journey_id: journey_id.to_owned(),
        challenge_id: challenge.map(str::to_owned),
    };
    post(
        &state.server,
        "/v1/game/journeys",
        Some(&state.token),
        &body,
    )
}

/// Reports one answer. `at_ms` is measured from the same start as the server's clock.
pub fn answer(state: &CloudState, journey_id: &str, answer: &GameAnswer) -> Result<GameState> {
    post(
        &state.server,
        &format!("/v1/game/journeys/{journey_id}/answers"),
        Some(&state.token),
        answer,
    )
}

/// Ends a journey; the server computes its XP from the answers it recorded.
pub fn finish_journey(state: &CloudState, journey_id: &str) -> Result<FinishReply> {
    post(
        &state.server,
        &format!("/v1/game/journeys/{journey_id}/finish"),
        Some(&state.token),
        &serde_json::Value::Null,
    )
}

/// The XP of this account, as the server counts it.
pub fn xp(state: &CloudState) -> Result<XpState> {
    get(&state.server, "/v1/game/xp", &state.token)
}

/// Ends the server session (best effort) and forgets the local one.
pub fn logout(data: &Path, state: &CloudState) -> Result<()> {
    let _ = send(
        &state.server,
        ureq::http::Method::DELETE,
        "/v1/sessions",
        Some(&state.token),
        None::<&()>,
    );
    remove(data)
}

pub(crate) fn get<T: DeserializeOwned>(server: &str, path: &str, token: &str) -> Result<T> {
    let response = send(
        server,
        ureq::http::Method::GET,
        path,
        Some(token),
        None::<&()>,
    )?;
    read(response)
}

pub(crate) fn put<T: DeserializeOwned, B: Serialize>(
    server: &str,
    path: &str,
    token: &str,
    body: &B,
) -> Result<T> {
    let response = send(
        server,
        ureq::http::Method::PUT,
        path,
        Some(token),
        Some(body),
    )?;
    read(response)
}

pub(crate) fn post<T: DeserializeOwned, B: Serialize>(
    server: &str,
    path: &str,
    token: Option<&str>,
    body: &B,
) -> Result<T> {
    let response = send(server, ureq::http::Method::POST, path, token, Some(body))?;
    read(response)
}

/// Largest response body accepted from the server (one media file or one page of operations).
const MAX_BODY: u64 = 96 * 1024 * 1024;

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into()
}

fn send<B: Serialize>(
    server: &str,
    method: ureq::http::Method,
    path: &str,
    token: Option<&str>,
    body: Option<&B>,
) -> Result<ureq::http::Response<ureq::Body>> {
    let server = check_server(server)?;
    let url = format!("{server}{path}");
    let mut request = ureq::http::Request::builder().method(method).uri(&url);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = match body {
        Some(body) => {
            let payload = serde_json::to_vec(body)?;
            let request = request
                .header("content-type", "application/json")
                .body(payload)?;
            agent(Duration::from_secs(20)).run(request)
        }
        None => agent(Duration::from_secs(20)).run(request.body(Vec::new())?),
    };
    response.with_context(|| format!("Locora Cloud unter {server} nicht erreichbar"))
}

/// Whether the server already holds the media file with this keyed id.
pub(crate) fn media_exists(state: &CloudState, id: &str) -> Result<bool> {
    let response = send(
        &state.server,
        ureq::http::Method::HEAD,
        &format!("/v1/media/{id}?generation={}", state.sync_generation),
        Some(&state.token),
        None::<&()>,
    )?;
    match response.status().as_u16() {
        200 => Ok(true),
        404 => Ok(false),
        other => bail!("Locora Cloud antwortet mit {other}"),
    }
}

/// Uploads one sealed media file under its keyed id.
pub(crate) fn put_media(state: &CloudState, id: &str, sealed: Vec<u8>) -> Result<()> {
    let server = check_server(&state.server)?;
    let request = ureq::http::Request::builder()
        .method(ureq::http::Method::PUT)
        .uri(format!(
            "{}/v1/media/{id}?generation={}",
            server, state.sync_generation
        ))
        .header("authorization", format!("Bearer {}", state.token))
        .header("content-type", "application/octet-stream")
        .body(sealed)?;
    let response = agent(Duration::from_secs(300))
        .run(request)
        .with_context(|| "Medium konnte nicht hochgeladen werden".to_string())?;
    read::<serde_json::Value>(response).map(|_| ())
}

/// The sealed bytes of a media file, by its keyed id.
pub(crate) fn get_media(state: &CloudState, id: &str) -> Result<Vec<u8>> {
    let mut response = send(
        &state.server,
        ureq::http::Method::GET,
        &format!("/v1/media/{id}?generation={}", state.sync_generation),
        Some(&state.token),
        None::<&()>,
    )?;
    if !response.status().is_success() {
        return read::<serde_json::Value>(response).map(|_| Vec::new());
    }
    Ok(response
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_vec()?)
}

fn read<T: DeserializeOwned>(mut response: ureq::http::Response<ureq::Body>) -> Result<T> {
    let status = response.status();
    let text = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_string()
        .unwrap_or_default();
    if status.is_success() {
        if text.is_empty() {
            return Ok(serde_json::from_str("null")?);
        }
        return serde_json::from_str(&text).context("Antwort von Locora Cloud ist ungültig");
    }
    match serde_json::from_str::<ErrorBody>(&text) {
        Ok(body) => bail!("{}", body.error),
        Err(_) => bail!("Locora Cloud antwortet mit {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::thread;

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
}
