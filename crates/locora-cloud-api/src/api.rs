//! JSON bodies of the Locora Cloud API (`/v1`). Times are Unix seconds unless
//! a field says otherwise (`at_ms` is milliseconds within one journey).

use serde::{Deserialize, Serialize};

use crate::entitlement::Plan;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Register {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Login {
    pub email: String,
    pub password: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerDto {
    pub correct: bool,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitJourney {
    /// Client-generated id of this journey. Sending it twice stores it once.
    pub journey_id: String,
    /// Challenge the score counts for; `None` for the global ranking.
    pub challenge_id: Option<String>,
    /// Palace name shown in rankings and used to group a journey.
    pub palace: String,
    pub answers: Vec<AnswerDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreReply {
    pub correct: u32,
    pub total: u32,
    pub max_combo: u32,
    pub xp: u64,
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

/// One change from the local operation log, as sent to the cloud.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireOp {
    pub hlc: String,
    pub entity: String,
    pub entity_id: String,
    pub op: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushOps {
    /// Node id of the device that produced the operations.
    pub device: String,
    pub ops: Vec<WireOp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushReply {
    /// Operations stored by this request; duplicates are ignored.
    pub stored: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PulledOp {
    /// Server sequence number; pass the largest one back as `after`.
    pub seq: i64,
    pub device: String,
    #[serde(flatten)]
    pub op: WireOp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullReply {
    pub ops: Vec<PulledOp>,
    pub cursor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}
