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
    Account, EntitlementReply, Friend, FriendInvite, KeyBundle, KeyUpload, Login, NameReport,
    Register, Session, SessionKind, SubscriptionState,
};
use locora_cloud_api::entitlement::{Access, Claims, Plan, parse_public_key, verify};
use locora_crypto::Dek;
use serde::{Deserialize, Serialize};

use crate::cloud_http::{self, Body, Call, Flow, Method, Reply, Step, json};

/// Public half of the entitlement signing key (base64). The private key only
/// exists on the Locora Cloud server.
pub const ENTITLEMENT_PUBLIC_KEY: &str = "m81i2VY47/UqknfNqtgzoff6NOdN9491c2ZkSj26SLg=";
/// The Locora Cloud server used when no other address is given. The app's sign-in form
/// shows the same address as its default.
pub const DEFAULT_SERVER: &str = "https://api.locora.cloud";
const FILE: &str = "cloud.json";
/// Seconds before an ordinary call gives up, and before a media transfer does.
const TIMEOUT_SECS: u64 = 20;
const MEDIA_TIMEOUT_SECS: u64 = 1800;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudState {
    pub server: String,
    pub token: String,
    pub account: Account,
    #[serde(default)]
    pub sync_generation: i64,
    /// Last signed entitlement token the server issued.
    pub entitlement: Option<String>,
    /// Unix seconds of the last sync that finished on this device.
    #[serde(default)]
    pub last_sync_at: Option<i64>,
    /// Whether this device syncs on its own; its key is then kept in the system's secure store
    /// (or the browser's), never in this file.
    #[serde(default)]
    pub auto_sync: bool,
    /// Unix seconds when a sync of this device last sent the stored key bundle back with the key
    /// proof (`cloud_sync`), so a bundle stored by an older app gets its proof.
    #[serde(default)]
    pub key_proof_sent_at: Option<i64>,
}

/// What became of the account's game scores on the server (the rankings) when the game progress
/// was deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RankingsDeletion {
    /// No account on this device: nothing on the server was touched.
    NotSignedIn,
    Deleted,
    /// The server does not know the request yet: the entries stay until it is updated (asked
    /// again at every start and sync until then).
    ServerTooOld,
    /// No answer: asked again at the next start, status check and sync.
    Pending,
    /// The session ended: asked again once the same account signs in again.
    SignInAgain,
}

impl RankingsDeletion {
    /// Whether the server still has to be asked.
    pub fn pending(self) -> bool {
        matches!(self, Self::Pending | Self::SignInAgain | Self::ServerTooOld)
    }
}

/// Accounts whose game scores the server still has to delete. Kept apart from the session
/// (`cloud.json`), which signing in again replaces, so the request outlives an expired session
/// and does not depend on sync access.
const RANKINGS_PENDING: &str = "game-rankings-pending.json";

fn pending_accounts(data: &Path) -> Result<Vec<String>> {
    match fs::read_to_string(data.join(RANKINGS_PENDING)) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err).context("Ausstehende Löschung der Ranglisten lesen"),
    }
}

/// Whether the game scores of `account` still wait to be deleted on the server.
pub fn rankings_pending(data: &Path, account: &str) -> Result<bool> {
    Ok(pending_accounts(data)?.iter().any(|id| id == account))
}

/// Notes whether the game scores of `account` still wait to be deleted on the server.
pub fn set_rankings_pending(data: &Path, account: &str, pending: bool) -> Result<()> {
    let mut accounts = pending_accounts(data)?;
    let had = accounts.iter().any(|id| id == account);
    if had == pending {
        return Ok(());
    }
    accounts.retain(|id| id != account);
    if pending {
        accounts.push(account.to_owned());
    }
    fs::create_dir_all(data)?;
    fs::write(
        data.join(RANKINGS_PENDING),
        serde_json::to_string(&accounts)?,
    )
    .context("Ausstehende Löschung der Ranglisten speichern")
}

/// After asking the server: keeps the request for `state`'s account while it is pending.
pub fn note_rankings(data: &Path, state: &CloudState, outcome: RankingsDeletion) -> Result<()> {
    set_rankings_pending(data, &state.account.id, outcome.pending())
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

/// Whether the account is a guest (every account until it pays) whose trial is still running.
/// A guest gets no signed entitlement; the server checks the trial on every request.
fn trial_running(account: &Account, now: u64) -> bool {
    account.subscription != SubscriptionState::None
        && account.plan == Some(Plan::Trial)
        && account.period_end.is_some_and(|end| end > now)
}

/// Whether the server issues an entitlement for this account: only a paid plan has one.
pub fn wants_entitlement(account: &Account) -> bool {
    account.subscription != SubscriptionState::None && account.plan != Some(Plan::Trial)
}

/// Whether this device may sync now: with a current entitlement (paid, also offline) or with a
/// guest's running trial.
pub fn can_sync(state: &CloudState, now: u64) -> bool {
    access(state, now).is_some() || trial_running(&state.account, now)
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
pub(crate) fn call(
    method: Method,
    server: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Body>,
) -> Result<Call> {
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
        last_sync_at: None,
        auto_sync: false,
        key_proof_sent_at: None,
    }
}

/// A failing entitlement keeps the account signed in: access stays off until a later refresh.
fn with_entitlement(mut state: CloudState, reply: Reply) -> CloudState {
    state.entitlement = json::<EntitlementReply>(reply)
        .ok()
        .map(|reply| reply.token);
    state
}

pub(crate) fn entitlement_call(state: &CloudState) -> Result<Call> {
    call(
        Method::Post,
        &state.server,
        "/v1/entitlement",
        Some(&state.token),
        Some(json_body(&serde_json::Value::Null)?),
    )
}

pub(crate) fn me_call(state: &CloudState) -> Result<Call> {
    call(
        Method::Get,
        &state.server,
        "/v1/me",
        Some(&state.token),
        None,
    )
}

pub fn keys_call(state: &CloudState) -> Result<Call> {
    call(
        Method::Get,
        &state.server,
        "/v1/keys",
        Some(&state.token),
        None,
    )
}

/// The wrapped keys, or `None` (a 404) before the encryption was set up.
pub fn keys_reply(reply: Reply) -> Result<Option<KeyBundle>> {
    if reply.status == 404 {
        return Ok(None);
    }
    json(reply).map(Some)
}

/// Stores the keys of a new account. Every bundle goes with the key proof of the key it wraps
/// (`KeyUpload`): a server that has it replaces the bundle later only for a holder of that key.
/// An older server ignores the proof.
pub fn create_keys_call(state: &CloudState, bundle: &KeyBundle, dek: &Dek) -> Result<Call> {
    keys_upload_call(Method::Post, state, bundle, dek)
}

/// Replaces the stored bundle (a new passphrase after a recovery), with the key proof. Refused
/// (403, with the server's message) when the server has another key's proof.
pub fn replace_keys_call(state: &CloudState, bundle: &KeyBundle, dek: &Dek) -> Result<Call> {
    keys_upload_call(Method::Put, state, bundle, dek)
}

fn keys_upload_call(
    method: Method,
    state: &CloudState,
    bundle: &KeyBundle,
    dek: &Dek,
) -> Result<Call> {
    call(
        method,
        &state.server,
        "/v1/keys",
        Some(&state.token),
        Some(json_body(&KeyUpload::new(bundle.clone(), dek))?),
    )
}

pub fn retention_call(state: &CloudState) -> Result<Call> {
    call(
        Method::Get,
        &state.server,
        "/v1/sync/retention",
        Some(&state.token),
        None,
    )
}

pub fn rebuild_call(state: &CloudState, generation: i64) -> Result<Call> {
    let body = serde_json::json!({ "generation": generation });
    call(
        Method::Post,
        &state.server,
        "/v1/sync/rebuild",
        Some(&state.token),
        Some(json_body(&body)?),
    )
}

/// Deletes the account's synced operations and media on the server; the reply is the new
/// generation (see `adopt_generation`).
pub fn erase_call(state: &CloudState) -> Result<Call> {
    call(
        Method::Delete,
        &state.server,
        "/v1/sync/data",
        Some(&state.token),
        None,
    )
}

/// Deletes the account's game scores on the server (what the rankings count).
pub fn erase_game_call(state: &CloudState) -> Result<Call> {
    call(
        Method::Delete,
        &state.server,
        "/v1/game/data",
        Some(&state.token),
        None,
    )
}

/// The answer to `erase_game_call`. Never an error: the local deletion does not depend on it.
pub fn erase_game_reply(reply: Reply) -> Result<RankingsDeletion> {
    Ok(match reply.status {
        200..=299 => RankingsDeletion::Deleted,
        // A server from before this request.
        404 | 405 => RankingsDeletion::ServerTooOld,
        401 => RankingsDeletion::SignInAgain,
        _ => RankingsDeletion::Pending,
    })
}

/// Deletes the account with all its data on the server, once its password is confirmed.
pub fn delete_account_call(state: &CloudState, password: &str) -> Result<Call> {
    let body = serde_json::json!({ "password": password });
    call(
        Method::Post,
        &state.server,
        "/v1/me/delete",
        Some(&state.token),
        Some(json_body(&body)?),
    )
}

/// Starts over with the server's generation: the next sync sends all local data and reads all of
/// the server's again. Needed after the cloud data was deleted or expired.
/// The answer to `delete_account_call`: a refusal there means the password was wrong.
pub fn delete_account_reply(reply: Reply) -> Result<()> {
    if reply.status == 401 {
        bail!("Das Cloud-Passwort stimmt nicht");
    }
    cloud_http::expect_success(reply)
}

pub fn adopt_generation(
    state: &mut CloudState,
    store: &locora_store::Store,
    generation: i64,
) -> Result<()> {
    store.set_sync_cursor("push", 0)?;
    store.set_sync_cursor("pull", 0)?;
    state.sync_generation = generation;
    Ok(())
}

/// How many operations of other devices wait on the server after this device's pull cursor.
pub fn pending_call(state: &CloudState, store: &locora_store::Store) -> Result<Call> {
    let after = store.sync_cursor("pull")?;
    let device =
        percent_encoding::utf8_percent_encode(store.node_id(), percent_encoding::NON_ALPHANUMERIC);
    call(
        Method::Get,
        &state.server,
        &format!(
            "/v1/sync/pending?after={after}&device={device}&generation={}",
            state.sync_generation
        ),
        Some(&state.token),
        None,
    )
}

/// The count, or `None` when the server cannot tell (an older server, or one that refuses): the
/// summary before a sync then shows only this device's changes.
pub fn pending_reply(reply: Reply) -> Result<Option<u64>> {
    if !reply.is_success() {
        return Ok(None);
    }
    Ok(json::<locora_cloud_api::api::PendingReply>(reply)
        .ok()
        .map(|pending| pending.count))
}

/// Sends the e-mail with the confirmation link again.
pub fn resend_email_call(state: &CloudState) -> Result<Call> {
    call(
        Method::Post,
        &state.server,
        "/v1/email/resend",
        Some(&state.token),
        Some(json_body(&serde_json::Value::Null)?),
    )
}

pub fn resend_email_reply(reply: Reply) -> Result<()> {
    if cloud_http::unsupported(&reply) {
        bail!("Dieser Server verschickt keine Bestätigungs-E-Mails");
    }
    cloud_http::expect_success(reply)
}

/// What an entry of the friend list is. A newer server sends an id with every entry and, with a
/// request this account sent, no name but the time it was sent (`since`); a server of the 0.1.6
/// test builds sent the address asked instead, which is ignored now. An older one sends neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FriendKind {
    Friend,
    /// A request to this account.
    Incoming,
    /// A request (or an invite to an address without an account) this account sent.
    Outgoing,
    /// A request listed by an older server, which does not tell who sent it.
    Open,
}

/// The kind of an entry of `GET /v1/friends`.
pub fn friend_kind(friend: &Friend) -> FriendKind {
    match (friend.accepted, friend.since, &friend.id) {
        (true, ..) => FriendKind::Friend,
        (false, Some(_), _) => FriendKind::Outgoing,
        // A request received always shows its sender's name.
        (false, None, Some(_)) if friend.display_name.is_empty() => FriendKind::Outgoing,
        (false, None, Some(_)) => FriendKind::Incoming,
        (false, None, None) => FriendKind::Open,
    }
}

/// Removes a friend (for both sides), withdraws a request or an invite this account sent, or
/// declines a request to it: `id` is the one `GET /v1/friends` lists with the entry.
pub fn remove_friend_call(state: &CloudState, id: &str) -> Result<Call> {
    let id = percent_encoding::utf8_percent_encode(id, percent_encoding::NON_ALPHANUMERIC);
    call(
        Method::Delete,
        &state.server,
        &format!("/v1/friends/{id}"),
        Some(&state.token),
        None,
    )
}

/// The answer to `remove_friend_call`. An older server has no such route: it answers 404 with its
/// general "Nicht gefunden" or with no message at all (a newer one names the friendship it did not
/// find), or 405.
pub fn remove_friend_reply(reply: Reply) -> Result<()> {
    if cloud_http::no_route(&reply) {
        bail!(
            "Dieser Server kann Freundschaften und Anfragen noch nicht entfernen; das geht, sobald er aktualisiert ist"
        );
    }
    cloud_http::expect_success(reply)
}

/// What an older server's refusal of the reports means.
const NO_REPORTS: &str =
    "Dieser Server nimmt noch keine Meldungen von Namen an; das geht, sobald er aktualisiert ist";
/// What an older server's refusal of the invite links means.
const NO_INVITES: &str =
    "Dieser Server kennt noch keine Einladungslinks; das geht, sobald er aktualisiert ist";

/// Reports the name shown under `reference`: a ranking entry's `ref`, or a friend list entry's
/// `id`. The server answers the same for every well-formed reference, known or not.
pub fn report_name_call(state: &CloudState, reference: &str, reason: Option<&str>) -> Result<Call> {
    let body = NameReport {
        reference: reference.trim().to_owned(),
        reason: reason
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(str::to_owned),
    };
    call(
        Method::Post,
        &state.server,
        "/v1/reports",
        Some(&state.token),
        Some(json_body(&body)?),
    )
}

/// The answer to `report_name_call`: a newer server never answers 404 or 405 there.
pub fn report_name_reply(reply: Reply) -> Result<()> {
    if cloud_http::unsupported(&reply) {
        bail!(NO_REPORTS);
    }
    cloud_http::expect_success(reply)
}

/// A new invite link of this account (one use, 14 days).
pub fn create_invite_call(state: &CloudState) -> Result<Call> {
    call(
        Method::Post,
        &state.server,
        "/v1/friends/invites",
        Some(&state.token),
        Some(json_body(&serde_json::Value::Null)?),
    )
}

/// The answer to `create_invite_call`. An older server takes the path for a friendship's id and
/// answers 405 (or 404, older still).
pub fn create_invite_reply(reply: Reply) -> Result<FriendInvite> {
    if cloud_http::unsupported(&reply) {
        bail!(NO_INVITES);
    }
    json(reply)
}

/// The open invite links of this account, oldest first.
pub fn invites_call(state: &CloudState) -> Result<Call> {
    call(
        Method::Get,
        &state.server,
        "/v1/friends/invites",
        Some(&state.token),
        None,
    )
}

pub fn invites_reply(reply: Reply) -> Result<Vec<FriendInvite>> {
    if cloud_http::unsupported(&reply) {
        bail!(NO_INVITES);
    }
    json(reply)
}

/// The path of one invite code; `code` as `parse_invite_code` returns it.
fn invite_path(code: &str, rest: &str) -> String {
    let code = percent_encoding::utf8_percent_encode(code, percent_encoding::NON_ALPHANUMERIC);
    format!("/v1/friends/invites/{code}{rest}")
}

/// Accepts another account's invite: both are friends then, and the code is used up.
pub fn accept_invite_call(state: &CloudState, code: &str) -> Result<Call> {
    call(
        Method::Post,
        &state.server,
        &invite_path(code, "/accept"),
        Some(&state.token),
        Some(json_body(&serde_json::Value::Null)?),
    )
}

/// The new friend. A code that does not work gets the server's 404 with its message; an older
/// server answers its general 404 instead.
pub fn accept_invite_reply(reply: Reply) -> Result<Friend> {
    if cloud_http::no_route(&reply) {
        bail!(NO_INVITES);
    }
    json(reply)
}

/// Revokes an open invite link of this account.
pub fn revoke_invite_call(state: &CloudState, code: &str) -> Result<Call> {
    call(
        Method::Delete,
        &state.server,
        &invite_path(code, ""),
        Some(&state.token),
        None,
    )
}

pub fn revoke_invite_reply(reply: Reply) -> Result<()> {
    if cloud_http::no_route(&reply) {
        bail!(NO_INVITES);
    }
    cloud_http::expect_success(reply)
}

pub fn logout_call(state: &CloudState) -> Result<Call> {
    call(
        Method::Delete,
        &state.server,
        "/v1/sessions",
        Some(&state.token),
        None,
    )
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
pub(crate) fn media_call(
    state: &CloudState,
    method: Method,
    id: &str,
    body: Option<Body>,
) -> Result<Call> {
    let mut call = call(
        method,
        &state.server,
        &format!("/v1/media/{id}?generation={}", state.sync_generation),
        Some(&state.token),
        body,
    )?;
    // Uploads and downloads move whole files (up to 500 MiB); a check only asks.
    if matches!(method, Method::Put | Method::Get) {
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
    if reply.is_success() {
        Ok(reply.body)
    } else {
        Err(cloud_http::failure(&reply))
    }
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
    pub fn sign_up(
        server: &str,
        email: &str,
        password: &str,
        display_name: &str,
        guest: bool,
    ) -> Result<Self> {
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
            session: SessionKind::App,
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
        if !wants_entitlement(&state.account) {
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

/// Reads the account and, with a paid subscription, its entitlement again. Without one (also as a
/// guest) the old entitlement is dropped. A failing entitlement is an error, and the caller keeps the old state.
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
                if !wants_entitlement(&self.state.account) {
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
        Challenge, CreateChallenge, FinishReply, FriendRequest, GameAnswer, GameState,
        JourneyStarted, PasswordChange, Ranking, StartJourney, XpState,
    };

    pub(crate) fn get<T: serde::de::DeserializeOwned>(
        server: &str,
        path: &str,
        token: &str,
    ) -> Result<T> {
        run(Exchange::new(
            call(Method::Get, server, path, Some(token), None)?,
            json::<T>,
        ))
    }

    pub(crate) fn put<T: serde::de::DeserializeOwned, B: Serialize>(
        server: &str,
        path: &str,
        token: &str,
        body: &B,
    ) -> Result<T> {
        run(Exchange::new(
            call(
                Method::Put,
                server,
                path,
                Some(token),
                Some(json_body(body)?),
            )?,
            json::<T>,
        ))
    }

    pub(crate) fn post<T: serde::de::DeserializeOwned, B: Serialize>(
        server: &str,
        path: &str,
        token: Option<&str>,
        body: &B,
    ) -> Result<T> {
        run(Exchange::new(
            call(Method::Post, server, path, token, Some(json_body(body)?))?,
            json::<T>,
        ))
    }

    pub fn register(
        server: &str,
        email: &str,
        password: &str,
        display_name: &str,
        guest: bool,
    ) -> Result<CloudState> {
        run(Connect::sign_up(
            server,
            email,
            password,
            display_name,
            guest,
        )?)
    }

    pub fn login(server: &str, email: &str, password: &str) -> Result<CloudState> {
        run(Connect::sign_in(server, email, password)?)
    }

    /// Finishes a session that was read elsewhere (its entitlement is read here).
    #[cfg(test)]
    pub(crate) fn finish_login(server: String, session: Session) -> Result<CloudState> {
        let mut state = state_from_session(server, session);
        if wants_entitlement(&state.account) {
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
        match run(Exchange::new(
            entitlement_call(state)?,
            json::<EntitlementReply>,
        )) {
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
        post(
            &state.server,
            "/v1/sync/rebuild",
            Some(&state.token),
            &serde_json::json!({"generation": generation}),
        )
    }

    /// Deletes the synced data on the server; the reply carries the new generation.
    pub fn erase(state: &CloudState) -> Result<RetentionStatus> {
        run(Exchange::new(erase_call(state)?, json::<RetentionStatus>))
    }

    /// Asks the server to delete the account's game scores; a network failure is `Pending`.
    pub fn erase_game(state: &CloudState) -> RankingsDeletion {
        let call = match erase_game_call(state) {
            Ok(call) => call,
            Err(_) => return RankingsDeletion::Pending,
        };
        run(Exchange::new(call, erase_game_reply)).unwrap_or(RankingsDeletion::Pending)
    }

    /// After the game progress was deleted on this device: asks the server to delete the
    /// rankings too when an account is signed in, and keeps the request (for this account)
    /// while the server has not done it.
    pub fn delete_rankings(data: &Path) -> Result<RankingsDeletion> {
        let Some(state) = load(data)? else {
            return Ok(RankingsDeletion::NotSignedIn);
        };
        let outcome = erase_game(&state);
        note_rankings(data, &state, outcome)?;
        Ok(outcome)
    }

    /// Asks the server again to delete the game scores when a request waits for the signed-in
    /// account: at the start, after signing in, on a status check and before a sync, with or
    /// without sync access. `None` when nothing waits.
    pub fn retry_rankings(data: &Path) -> Result<Option<RankingsDeletion>> {
        let Some(state) = load(data)? else {
            return Ok(None);
        };
        if !rankings_pending(data, &state.account.id)? {
            return Ok(None);
        }
        let outcome = erase_game(&state);
        note_rankings(data, &state, outcome)?;
        Ok(Some(outcome))
    }

    /// Deletes the account on the server; the caller then removes the local session.
    pub fn delete_account(state: &CloudState, password: &str) -> Result<()> {
        run(Exchange::new(
            delete_account_call(state, password)?,
            delete_account_reply,
        ))
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

    /// Removes a friend, withdraws a request or declines one (see `remove_friend_call`).
    pub fn remove_friend(state: &CloudState, id: &str) -> Result<()> {
        run(Exchange::new(
            remove_friend_call(state, id)?,
            remove_friend_reply,
        ))
    }

    /// Reports the name shown under `reference` (see `report_name_call`).
    pub fn report_name(state: &CloudState, reference: &str, reason: Option<&str>) -> Result<()> {
        run(Exchange::new(
            report_name_call(state, reference, reason)?,
            report_name_reply,
        ))
    }

    /// A new invite link of this account.
    pub fn create_invite(state: &CloudState) -> Result<FriendInvite> {
        run(Exchange::new(
            create_invite_call(state)?,
            create_invite_reply,
        ))
    }

    /// The open invite links of this account.
    pub fn invites(state: &CloudState) -> Result<Vec<FriendInvite>> {
        run(Exchange::new(invites_call(state)?, invites_reply))
    }

    /// Accepts the invite with `code`; the answer is the new friend.
    pub fn accept_invite(state: &CloudState, code: &str) -> Result<Friend> {
        run(Exchange::new(
            accept_invite_call(state, code)?,
            accept_invite_reply,
        ))
    }

    /// Revokes the open invite link with `code`.
    pub fn revoke_invite(state: &CloudState, code: &str) -> Result<()> {
        run(Exchange::new(
            revoke_invite_call(state, code)?,
            revoke_invite_reply,
        ))
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
        run(Exchange::new(keys_call(state)?, keys_reply))
    }

    /// Stores the keys of a new account, with the key proof of `dek`.
    pub fn create_keys(state: &CloudState, bundle: &KeyBundle, dek: &Dek) -> Result<()> {
        run(Exchange::new(
            create_keys_call(state, bundle, dek)?,
            ignore_reply,
        ))
    }

    /// Replaces the stored bundle with one that wraps the same key `dek`, with its key proof.
    pub fn replace_keys(state: &CloudState, bundle: &KeyBundle, dek: &Dek) -> Result<()> {
        run(Exchange::new(
            replace_keys_call(state, bundle, dek)?,
            ignore_reply,
        ))
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

    /// Operations of other devices waiting on the server; `None` when the server cannot tell.
    pub fn pending(state: &CloudState, store: &locora_store::Store) -> Result<Option<u64>> {
        // A summary must not fail because the server is away.
        Ok(run(Exchange::new(pending_call(state, store)?, pending_reply)).unwrap_or(None))
    }

    /// Sends the confirmation e-mail again.
    pub fn resend_email(state: &CloudState) -> Result<()> {
        run(Exchange::new(resend_email_call(state)?, resend_email_reply))
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

    use std::io::{BufRead, BufReader, Read};
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
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse().unwrap();
                }
            }
            // Read the body too: closing with unread data resets the connection on Windows.
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            socket.write_all(response.as_bytes()).unwrap();
            request
        });
        let state = CloudState {
            server,
            token: "synthetic-test-token".into(),
            sync_generation: 0,
            entitlement: None,
            last_sync_at: None,
            auto_sync: false,
            key_proof_sent_at: None,
            account: Account {
                id: "local".into(),
                email: "test@example.invalid".into(),
                display_name: "Test".into(),
                subscription: SubscriptionState::None,
                plan: None,
                period_end: None,
                email_verified: None,
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

    fn status(code: u16, reason: &str) -> String {
        format!("HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    }

    #[test]
    fn deleting_the_game_scores_asks_the_server_with_the_session() {
        let (state, worker) = fixture(status(204, "No Content"));
        assert_eq!(erase_game(&state), RankingsDeletion::Deleted);
        assert_eq!(worker.join().unwrap(), "DELETE /v1/game/data HTTP/1.1\r\n");
        let call = erase_game_call(&state).unwrap();
        assert_eq!(call.method, Method::Delete);
        assert_eq!(call.token.as_deref(), Some("synthetic-test-token"));
        for (code, reason, outcome) in [
            (404, "Not Found", RankingsDeletion::ServerTooOld),
            (405, "Method Not Allowed", RankingsDeletion::ServerTooOld),
            (401, "Unauthorized", RankingsDeletion::SignInAgain),
            (500, "Internal Server Error", RankingsDeletion::Pending),
        ] {
            let (state, worker) = fixture(status(code, reason));
            assert_eq!(erase_game(&state), outcome, "{code}");
            worker.join().unwrap();
        }
        // Nobody listens: asked again later.
        let mut state = state;
        state.server = "http://127.0.0.1:9".into();
        assert_eq!(erase_game(&state), RankingsDeletion::Pending);
        assert!(RankingsDeletion::Pending.pending() && RankingsDeletion::SignInAgain.pending());
        assert!(
            RankingsDeletion::ServerTooOld.pending(),
            "asked again until it is updated"
        );
        assert!(!RankingsDeletion::Deleted.pending() && !RankingsDeletion::NotSignedIn.pending());
    }

    #[test]
    fn the_rankings_request_is_kept_only_while_it_is_pending() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            delete_rankings(dir.path()).unwrap(),
            RankingsDeletion::NotSignedIn
        );
        assert_eq!(retry_rankings(dir.path()).unwrap(), None);
        let (state, worker) = fixture(status(503, "Service Unavailable"));
        save(dir.path(), &state).unwrap();
        assert_eq!(
            delete_rankings(dir.path()).unwrap(),
            RankingsDeletion::Pending
        );
        worker.join().unwrap();
        assert!(rankings_pending(dir.path(), &state.account.id).unwrap());
        let (state, worker) = fixture(status(204, "No Content"));
        save(dir.path(), &state).unwrap();
        assert_eq!(
            retry_rankings(dir.path()).unwrap(),
            Some(RankingsDeletion::Deleted)
        );
        worker.join().unwrap();
        assert!(!rankings_pending(dir.path(), &state.account.id).unwrap());
        // Nothing waits: nothing is asked (no server listens here).
        assert_eq!(retry_rankings(dir.path()).unwrap(), None);
    }

    #[test]
    fn a_pending_rankings_deletion_outlives_signing_in_again_and_needs_no_sync_access() {
        let dir = tempfile::tempdir().unwrap();
        // The session had ended: the server answered 401.
        let (state, worker) = fixture(status(401, "Unauthorized"));
        save(dir.path(), &state).unwrap();
        assert_eq!(
            delete_rankings(dir.path()).unwrap(),
            RankingsDeletion::SignInAgain
        );
        worker.join().unwrap();
        // Signing in again writes a new session for the same account (without sync access:
        // no entitlement, no trial).
        let (mut fresh, worker) = fixture(status(204, "No Content"));
        fresh.entitlement = None;
        fresh.account.id = state.account.id.clone();
        remove(dir.path()).unwrap();
        save(dir.path(), &fresh).unwrap();
        assert!(!can_sync(&fresh, now_secs()));
        assert_eq!(
            retry_rankings(dir.path()).unwrap(),
            Some(RankingsDeletion::Deleted)
        );
        assert_eq!(worker.join().unwrap(), "DELETE /v1/game/data HTTP/1.1\r\n");
        // Another account signed in later is not asked about the first one's scores.
        let (mut other, _) = fixture(status(204, "No Content"));
        other.account.id = "someone-else".into();
        set_rankings_pending(dir.path(), &state.account.id, true).unwrap();
        save(dir.path(), &other).unwrap();
        assert_eq!(retry_rankings(dir.path()).unwrap(), None);
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

    fn bundle() -> KeyBundle {
        KeyBundle {
            v: 1,
            salt: "c2FsdA==".into(),
            wrapped_dek: "d3JhcHBlZA==".into(),
            recovery_wrapped: "cmVjb3Zlcnk=".into(),
        }
    }

    #[test]
    fn every_bundle_sent_carries_the_proof_of_its_key() {
        let state = CloudState {
            server: "https://cloud.example".into(),
            token: "t".into(),
            sync_generation: 0,
            entitlement: None,
            last_sync_at: None,
            auto_sync: false,
            key_proof_sent_at: None,
            account: Account {
                id: "a".into(),
                email: "e@x.invalid".into(),
                display_name: "D".into(),
                subscription: SubscriptionState::None,
                plan: None,
                period_end: None,
                email_verified: None,
            },
        };
        let dek = Dek::from_bytes(&[7; 32]).unwrap();
        let calls = [
            (
                create_keys_call(&state, &bundle(), &dek).unwrap(),
                Method::Post,
            ),
            (
                replace_keys_call(&state, &bundle(), &dek).unwrap(),
                Method::Put,
            ),
        ];
        for (call, method) in calls {
            assert_eq!(call.method, method);
            assert_eq!(call.url, format!("{}/v1/keys", state.server));
            let Some(Body::Json(body)) = &call.body else {
                panic!("a JSON body")
            };
            let upload: KeyUpload = serde_json::from_slice(body).unwrap();
            assert_eq!(upload, KeyUpload::new(bundle(), &dek));
            assert!(upload.key_proof.is_some());
            // An older server reads the same body as the bundle alone.
            assert_eq!(serde_json::from_slice::<KeyBundle>(body).unwrap(), bundle());
        }
        let other = Dek::from_bytes(&[8; 32]).unwrap();
        let Some(Body::Json(body)) = replace_keys_call(&state, &bundle(), &other).unwrap().body
        else {
            panic!("a JSON body")
        };
        let upload: KeyUpload = serde_json::from_slice(&body).unwrap();
        assert_ne!(
            upload.key_proof,
            KeyUpload::new(bundle(), &dek).key_proof,
            "the proof is the key's"
        );
    }

    #[test]
    fn a_replacement_refused_for_its_proof_shows_the_servers_words() {
        let body = r#"{"error":"Der Schlüssel konnte nicht bestätigt werden; die Schlüsselsicherung bleibt, wie sie war. Bitte die App aktualisieren und es erneut versuchen"}"#;
        let (state, worker) = fixture(format!(
            "HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ));
        let dek = Dek::from_bytes(&[7; 32]).unwrap();
        let error = replace_keys(&state, &bundle(), &dek).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("Der Schlüssel konnte nicht bestätigt werden"),
            "{error:#}"
        );
        assert_eq!(cloud_http::status_of(&error), Some(403));
        assert_eq!(worker.join().unwrap(), "PUT /v1/keys HTTP/1.1\r\n");

        let (state, worker) =
            fixture("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
        replace_keys(&state, &bundle(), &dek).unwrap();
        assert_eq!(worker.join().unwrap(), "PUT /v1/keys HTTP/1.1\r\n");
    }

    #[test]
    fn removing_a_friend_tells_an_older_server_from_an_unknown_id() {
        let reply = |status: u16, error: &str| Reply {
            status,
            body: serde_json::json!({ "error": error })
                .to_string()
                .into_bytes(),
        };
        let older = remove_friend_reply(reply(404, "Nicht gefunden")).unwrap_err();
        assert!(
            older
                .to_string()
                .starts_with("Dieser Server kann Freundschaften"),
            "{older:#}"
        );
        let older = remove_friend_reply(reply(405, "Diese Methode ist hier nicht erlaubt"));
        assert!(
            older
                .unwrap_err()
                .to_string()
                .starts_with("Dieser Server kann")
        );
        // Older still: no error message at all.
        let older = remove_friend_reply(Reply {
            status: 404,
            body: Vec::new(),
        });
        assert!(
            older
                .unwrap_err()
                .to_string()
                .starts_with("Dieser Server kann")
        );
        let unknown = "Diese Freundschaft oder Anfrage gibt es nicht";
        assert_eq!(
            remove_friend_reply(reply(404, unknown))
                .unwrap_err()
                .to_string(),
            unknown
        );
        remove_friend_reply(Reply {
            status: 204,
            body: Vec::new(),
        })
        .unwrap();

        let (state, worker) = fixture(
            "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        );
        remove_friend(&state, "fr_0a/../x?y").unwrap();
        assert_eq!(
            worker.join().unwrap(),
            "DELETE /v1/friends/fr%5F0a%2F%2E%2E%2Fx%3Fy HTTP/1.1\r\n"
        );
    }

    #[test]
    fn reports_and_invite_links_tell_an_older_server_from_a_refusal() {
        let reply = |status: u16, body: &str| Reply {
            status,
            body: body.as_bytes().to_vec(),
        };
        let general = r#"{"error":"Nicht gefunden"}"#;
        let method = r#"{"error":"Diese Methode ist hier nicht erlaubt"}"#;
        for older in [reply(404, general), reply(405, method), reply(404, "")] {
            let error = report_name_reply(older.clone()).unwrap_err().to_string();
            assert!(
                error.starts_with("Dieser Server nimmt noch keine Meldungen"),
                "{error}"
            );
            for error in [
                create_invite_reply(older.clone()).unwrap_err(),
                invites_reply(older.clone()).unwrap_err(),
                accept_invite_reply(older.clone()).unwrap_err(),
                revoke_invite_reply(older).unwrap_err(),
            ] {
                assert!(
                    error
                        .to_string()
                        .starts_with("Dieser Server kennt noch keine Einladungslinks"),
                    "{error}"
                );
            }
        }
        // A newer server's own refusals keep their words.
        let gone = "Diese Einladung gibt es nicht (mehr): Sie ist abgelaufen, wurde schon angenommen oder zurückgezogen.";
        let body = serde_json::json!({ "error": gone }).to_string();
        assert_eq!(
            accept_invite_reply(reply(404, &body))
                .unwrap_err()
                .to_string(),
            gone
        );
        assert_eq!(
            revoke_invite_reply(reply(404, &body))
                .unwrap_err()
                .to_string(),
            gone
        );
        let own = r#"{"error":"Das ist deine eigene Einladung."}"#;
        let error = accept_invite_reply(reply(400, own)).unwrap_err();
        assert_eq!(
            (error.to_string().as_str(), cloud_http::status_of(&error)),
            ("Das ist deine eigene Einladung.", Some(400))
        );
        let bad = r#"{"error":"Das ist kein Verweis auf einen Namen"}"#;
        assert_eq!(
            report_name_reply(reply(400, bad)).unwrap_err().to_string(),
            "Das ist kein Verweis auf einen Namen"
        );
        report_name_reply(reply(204, "")).unwrap();
        revoke_invite_reply(reply(204, "")).unwrap();
        let friend = accept_invite_reply(reply(
            200,
            r#"{"display_name":"Ada","accepted":true,"id":"fr_1"}"#,
        ))
        .unwrap();
        assert_eq!(
            (friend.display_name.as_str(), friend.accepted),
            ("Ada", true)
        );
        let invite = create_invite_reply(reply(
            201,
            r#"{"code":"ABCD-EFGH-JKLM-NPQR","url":"https://web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR","expires_at":5}"#,
        ))
        .unwrap();
        assert_eq!(
            (invite.code.as_str(), invite.expires_at),
            ("ABCD-EFGH-JKLM-NPQR", 5)
        );
        assert!(invites_reply(reply(200, "[]")).unwrap().is_empty());
    }

    #[test]
    fn a_report_and_an_invite_code_go_where_they_belong() {
        let (state, worker) = fixture(
            "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        );
        let call = report_name_call(&state, " nr_0a ", Some("  ")).unwrap();
        assert_eq!(
            (call.method, call.url.ends_with("/v1/reports")),
            (Method::Post, true)
        );
        assert_eq!(call.body, Some(Body::Json(br#"{"ref":"nr_0a"}"#.to_vec())));
        let call = report_name_call(&state, "fr_1", Some(" beleidigend ")).unwrap();
        assert_eq!(
            call.body,
            Some(Body::Json(
                br#"{"ref":"fr_1","reason":"beleidigend"}"#.to_vec()
            ))
        );
        report_name(&state, "fr_1", None).unwrap();
        assert_eq!(worker.join().unwrap(), "POST /v1/reports HTTP/1.1\r\n");
        let (state, worker) = fixture(
            "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        );
        revoke_invite(&state, "ABCD-EFGH/../x").unwrap();
        assert_eq!(
            worker.join().unwrap(),
            "DELETE /v1/friends/invites/ABCD%2DEFGH%2F%2E%2E%2Fx HTTP/1.1\r\n"
        );
        let call = accept_invite_call(&state, "ABCD-EFGH-JKLM-NPQR").unwrap();
        assert!(
            call.url
                .ends_with("/v1/friends/invites/ABCD%2DEFGH%2DJKLM%2DNPQR/accept"),
            "{}",
            call.url
        );
        assert_eq!(
            (
                create_invite_call(&state).unwrap().method,
                invites_call(&state).unwrap().method
            ),
            (Method::Post, Method::Get)
        );
    }

    #[test]
    fn friend_kinds_follow_the_list() {
        let entry = |name: &str, accepted: bool, since: Option<u64>, id: Option<&str>| Friend {
            display_name: name.into(),
            accepted,
            id: id.map(str::to_owned),
            since,
        };
        assert_eq!(
            friend_kind(&entry("Ada", true, None, Some("f1"))),
            FriendKind::Friend
        );
        assert_eq!(
            friend_kind(&entry("Ada", true, None, None)),
            FriendKind::Friend
        );
        assert_eq!(
            friend_kind(&entry("Ben", false, None, Some("f2"))),
            FriendKind::Incoming
        );
        assert_eq!(
            friend_kind(&entry("", false, Some(9), Some("f3"))),
            FriendKind::Outgoing
        );
        // A server of the 0.1.6 test builds: no `since`, no name, an id.
        assert_eq!(
            friend_kind(&entry("", false, None, Some("f4"))),
            FriendKind::Outgoing
        );
        assert_eq!(
            friend_kind(&entry("Cleo", false, None, None)),
            FriendKind::Open
        );
    }

    #[test]
    fn a_guest_syncs_during_its_trial_without_an_entitlement() {
        let mut account = Account {
            id: "guest".into(),
            email: "guest@example.invalid".into(),
            display_name: "Gast".into(),
            subscription: SubscriptionState::Active,
            plan: Some(Plan::Trial),
            period_end: Some(2_000),
            email_verified: None,
        };
        assert!(
            !wants_entitlement(&account),
            "the server issues none for a trial"
        );
        let mut state = CloudState {
            server: "https://cloud.example".into(),
            token: "synthetic-test-token".into(),
            sync_generation: 0,
            entitlement: None,
            last_sync_at: None,
            auto_sync: false,
            key_proof_sent_at: None,
            account: account.clone(),
        };
        assert!(can_sync(&state, 1_000));
        assert!(!can_sync(&state, 2_000), "not after the trial ended");
        account.plan = Some(Plan::Monthly);
        assert!(wants_entitlement(&account));
        state.account = account;
        assert!(
            !can_sync(&state, 1_000),
            "a paid plan needs its signed entitlement"
        );
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
        let mut flow =
            Connect::sign_in("https://cloud.example", "e@x.invalid", "password one").unwrap();
        assert!(matches!(flow.step(None), Ok(Step::Call(_))));
        let state = match flow.step(Some(Reply {
            status: 200,
            body: session("none").into_bytes(),
        })) {
            Ok(Step::Done(state)) => state,
            _ => panic!("no subscription: the session is the whole answer"),
        };
        assert!(state.entitlement.is_none());

        let mut flow =
            Connect::sign_in("https://cloud.example", "e@x.invalid", "password one").unwrap();
        let _ = flow.step(None);
        let next = flow.step(Some(Reply {
            status: 200,
            body: session("active").into_bytes(),
        }));
        let Ok(Step::Call(call)) = next else {
            panic!("an active subscription asks for the entitlement")
        };
        assert!(call.url.ends_with("/v1/entitlement"));
        let done = flow.step(Some(Reply {
            status: 503,
            body: Vec::new(),
        }));
        assert!(matches!(done, Ok(Step::Done(state)) if state.entitlement.is_none()));
    }

    #[test]
    fn a_media_upload_waits_longer_than_a_sign_in() {
        let state = CloudState {
            server: "https://cloud.example".into(),
            token: "t".into(),
            sync_generation: 3,
            entitlement: None,
            last_sync_at: None,
            auto_sync: false,
            key_proof_sent_at: None,
            account: Account {
                id: "a".into(),
                email: "e@x.invalid".into(),
                display_name: "D".into(),
                subscription: SubscriptionState::None,
                plan: None,
                period_end: None,
                email_verified: None,
            },
        };
        let upload = media_call(&state, Method::Put, "abc", Some(Body::Bytes(vec![1]))).unwrap();
        assert_eq!(upload.timeout_secs, MEDIA_TIMEOUT_SECS);
        assert_eq!(
            upload.url,
            "https://cloud.example/v1/media/abc?generation=3"
        );
        assert_eq!(
            media_call(&state, Method::Get, "abc", None)
                .unwrap()
                .timeout_secs,
            MEDIA_TIMEOUT_SECS
        );
        assert_eq!(
            media_call(&state, Method::Head, "abc", None)
                .unwrap()
                .timeout_secs,
            TIMEOUT_SECS
        );
    }
}
