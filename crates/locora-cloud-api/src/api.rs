//! JSON bodies of the Locora Cloud API (`/v1`). Times are Unix seconds unless
//! a field says otherwise (`at_ms` is milliseconds within one journey).

use serde::{Deserialize, Serialize};

use crate::entitlement::Plan;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Register {
    pub email: String,
    pub password: String,
    pub display_name: String,
    /// A guest keeps its data for six months with the access of a trial; a paid
    /// subscription makes it permanent. Older clients never send it.
    #[serde(default)]
    pub guest: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Login {
    pub email: String,
    pub password: String,
    /// The kind of session to start; older clients never send it and get an app session. Older
    /// servers ignore it and start an app session for every sign-in.
    #[serde(default)]
    pub session: SessionKind,
}

/// How long a session lasts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    /// The apps and the CLI: 90 days.
    #[default]
    App,
    /// The account site in a browser: at most 12 hours, and it ends after 30 minutes without a
    /// request (both adjustable on the server).
    Web,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasswordChange {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Opaque bearer token; shown once, stored hashed on the server.
    pub token: String,
    pub account: Account,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionState {
    None,
    Active,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub subscription: SubscriptionState,
    pub plan: Option<Plan>,
    /// Unix seconds when the paid period ends.
    pub period_end: Option<u64>,
    /// Whether the address was confirmed with the link from the confirmation e-mail; `None`
    /// from older servers, which do not confirm addresses.
    #[serde(default)]
    pub email_verified: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitlementReply {
    /// Signed token from [`crate::entitlement::sign`].
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendRequest {
    pub email: String,
}

/// One entry of `GET /v1/friends`, or the answer of `POST /v1/friends`.
///
/// The list holds accepted friends (with their names), requests to this account (with the
/// sender's name, no `email`), and requests this account sent, among them requests to an address
/// without a confirmed account (invites): those without a name (`display_name` is empty until the
/// other side accepts) and with the address that was asked (`email`). So a request not accepted
/// that has an `email` is one this account sent, one without is one it received. Older servers
/// send neither `id` nor `email`, and the names of both sides of a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Friend {
    pub display_name: String,
    /// `true` once both sides accepted.
    pub accepted: bool,
    /// Opaque and stable for the friendship, request or invite: `DELETE /v1/friends/{id}`
    /// removes a friend (for both sides), withdraws a request or declines one. Not in the answer
    /// to a request that is still waiting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The address asked, for a request this account sent that is still waiting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateChallenge {
    pub name: String,
    pub ends_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Challenge {
    pub id: String,
    pub name: String,
    pub ends_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankingEntry {
    pub display_name: String,
    pub xp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ranking {
    pub entries: Vec<RankingEntry>,
}

/// One sealed operation. The server sees only the keyed id and the ciphertext
/// (base64, see `locora-crypto`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedOp {
    /// 32 hex characters, a keyed id from `Dek::op_id`.
    pub op_id: String,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushOps {
    /// Node id of the device that produced the operations (not secret).
    pub device: String,
    pub ops: Vec<EncryptedOp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushReply {
    /// Operations stored by this request; duplicates are ignored.
    pub stored: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PulledOp {
    /// Server sequence number; pass the largest one back as `after`.
    pub seq: i64,
    pub device: String,
    #[serde(flatten)]
    pub op: EncryptedOp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullReply {
    pub ops: Vec<PulledOp>,
    pub cursor: i64,
}

/// Live journey: the server starts it and times every answer on its own clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartJourney {
    /// Client-made id, 1 to 64 letters, digits or dashes.
    pub journey_id: String,
    /// Challenge the journey counts for, if any.
    pub challenge_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JourneyStarted {
    pub journey_id: String,
    /// Server clock when the journey started, Unix milliseconds.
    pub started_at_ms: i64,
}

/// One answer. `at_ms` is measured from the journey start on the device;
/// the server rejects times that it cannot reconcile with its own clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameAnswer {
    /// 1 for the first answer of the journey, then 2, 3, ...
    pub seq: u32,
    pub correct: bool,
    pub at_ms: u64,
}

/// What the server knows about a journey after an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameState {
    pub answered: u32,
    pub correct: u32,
    pub combo: u32,
    pub max_combo: u32,
    pub hearts: u8,
}

/// Result of a finished journey, computed by the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishReply {
    pub correct: u32,
    pub total: u32,
    pub max_combo: u32,
    pub xp: u64,
    /// All XP of the account after this journey.
    pub xp_total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct XpState {
    pub xp_total: u64,
    pub journeys_finished: u64,
    pub journeys_open: u64,
}

/// `POST /v1/media/missing`: which of these media ids the server does not have yet, so a sync
/// uploads only those (at most 1000 ids per request).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaQuery {
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaMissing {
    pub missing: Vec<String>,
}

/// `GET /v1/sync/pending?after=<cursor>&device=<node>`: how many operations other devices
/// stored after the cursor; shown before a sync, without pulling them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingReply {
    pub count: u64,
}

/// `POST /v1/email/verify`: the token from the confirmation link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyEmail {
    pub token: String,
}

/// `POST /v1/password/forgot`: always answers 204, whether the address has an account or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasswordForgot {
    pub email: String,
}

/// `POST /v1/password/reset`: the token from the reset link and the new password. Signs out
/// every device; the encryption password and the cloud data stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasswordReset {
    pub token: String,
    pub new_password: String,
}

/// `GET /v1/me/usage`: what the account uses on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub media_bytes: u64,
    pub media_quota: u64,
    pub ops: u64,
}

pub use locora_crypto::KeyBundle;

/// `POST /v1/keys` and `PUT /v1/keys`: the bundle's fields, and next to them the key proof
/// (`Dek::key_proof`, base64 like the bundle). The server keeps only the proof's SHA-256 and
/// refuses a replacement of the bundle without it once it has one (403); it never returns it.
/// Older servers ignore the field. `GET /v1/keys` answers a plain [`KeyBundle`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyUpload {
    #[serde(flatten)]
    pub bundle: KeyBundle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_proof: Option<String>,
}

impl KeyUpload {
    /// The bundle with the proof of the key it wraps.
    pub fn new(bundle: KeyBundle, dek: &locora_crypto::Dek) -> Self {
        use base64::Engine;
        Self {
            bundle,
            key_proof: Some(
                base64::engine::general_purpose::STANDARD.encode(dek.key_proof().as_ref()),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_sign_in_without_a_kind_is_one_of_an_app() {
        let older: Login =
            serde_json::from_value(json!({"email": "a@example.com", "password": "pw"})).unwrap();
        assert_eq!(older.session, SessionKind::App);
        let web: Login = serde_json::from_value(
            json!({"email": "a@example.com", "password": "pw", "session": "web"}),
        )
        .unwrap();
        assert_eq!(web.session, SessionKind::Web);
        assert_eq!(serde_json::to_value(&web).unwrap()["session"], json!("web"));
        assert!(
            serde_json::from_value::<Login>(
                json!({"email": "a@example.com", "password": "pw", "session": "tv"})
            )
            .is_err()
        );
    }

    #[test]
    fn friends_of_older_servers_and_of_newer_ones_read_alike() {
        let older: Friend =
            serde_json::from_value(json!({"display_name": "Ada", "accepted": true})).unwrap();
        assert_eq!((older.id, older.email), (None, None));
        let waiting = Friend {
            display_name: String::new(),
            accepted: false,
            id: Some("fr_1".into()),
            email: Some("ada@example.com".into()),
        };
        let value = serde_json::to_value(&waiting).unwrap();
        assert_eq!(
            value,
            json!({"display_name": "", "accepted": false, "id": "fr_1", "email": "ada@example.com"})
        );
        assert_eq!(serde_json::from_value::<Friend>(value).unwrap(), waiting);
        // What a 0.1.5 client reads: unknown fields are ignored.
        #[derive(Deserialize)]
        struct OlderFriend {
            display_name: String,
            accepted: bool,
        }
        let read: OlderFriend = serde_json::to_value(&waiting)
            .and_then(serde_json::from_value)
            .unwrap();
        assert_eq!((read.display_name.as_str(), read.accepted), ("", false));
        let answer = Friend {
            display_name: String::new(),
            accepted: false,
            id: None,
            email: Some("ada@example.com".into()),
        };
        assert!(serde_json::to_value(&answer).unwrap().get("id").is_none());
    }

    #[test]
    fn the_key_proof_travels_next_to_the_bundle_fields() {
        let account = locora_crypto::create("ein langes geheimes passwort").unwrap();
        let upload = KeyUpload::new(account.bundle.clone(), &account.dek);
        let value = serde_json::to_value(&upload).unwrap();
        for field in ["v", "salt", "wrapped_dek", "recovery_wrapped", "key_proof"] {
            assert!(value.get(field).is_some(), "{field}");
        }
        use base64::Engine;
        let proof = base64::engine::general_purpose::STANDARD
            .decode(value["key_proof"].as_str().unwrap())
            .unwrap();
        assert_eq!(proof, account.dek.key_proof().as_ref());
        // An older server reads the body as a plain bundle and ignores the proof.
        let plain: KeyBundle = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(plain, account.bundle);
        assert_eq!(serde_json::from_value::<KeyUpload>(value).unwrap(), upload);
        // An older client sends the bundle alone.
        let older: KeyUpload =
            serde_json::from_value(serde_json::to_value(&account.bundle).unwrap()).unwrap();
        assert_eq!(older.bundle, account.bundle);
        assert_eq!(older.key_proof, None);
        assert!(
            serde_json::to_value(&older)
                .unwrap()
                .get("key_proof")
                .is_none()
        );
    }
}
