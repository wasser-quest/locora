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
    /// Whether the address was confirmed with the link from the confirmation e-mail. Older
    /// servers never send it.
    #[serde(default)]
    pub email_verified: bool,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Friend {
    pub display_name: String,
    /// `true` once both sides accepted.
    pub accepted: bool,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}
