//! What the Leptos UI and the Tauri backend exchange (JSON over Tauri IPC).
//! Command names are the `CMD_*` constants; arguments and results are the
//! types below. Text meant for people is German and ready to show.

use serde::{Deserialize, Serialize};

pub use locora_api::journey::{Answer as EditorAnswer, AnswerKind as EditorAnswerKind, QuizOption};

pub const CMD_EDITOR_CHANGE: &str = "editor_change";
pub const CMD_EDITOR_PICK_PICTURE: &str = "editor_pick_picture";
/// Picks a sound clip for a locus (argument: `{ target: PictureTarget }`); `None` when cancelled.
pub const CMD_EDITOR_PICK_AUDIO: &str = "editor_pick_audio";

/// Exact scope only; file paths never come from the WebView.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PictureTarget {
    pub palace_id: String,
    pub room_id: String,
    pub locus_id: String,
}
pub const CMD_HOME: &str = "home";
pub const CMD_HOME_PICK_MEDIA: &str = "home_pick_media";
/// One more room for a palace from a picture or video the user chooses; `None` when cancelled.
pub const CMD_EDITOR_PICK_ROOM: &str = "editor_pick_room";
pub const CMD_PALACE: &str = "palace";
pub const CMD_TUTORIAL: &str = "tutorial";
/// The language a built-in palace is built in (`de`, `en`, `fr`, `ja`, `zh`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageArgs {
    pub lang: String,
}
pub const CMD_FORMULA_GUIDE: &str = "formula_guide";
pub const CMD_TOUR: &str = "tour";
/// Simo's example palace for the app tour (`LanguageArgs`); returns its id.
pub const CMD_TOUR_DEMO: &str = "tour_demo";
pub use locora_api::tutorial::TourAction;
pub use locora_api::ui::HighlightRequest;
pub const CMD_NOTIFICATIONS: &str = "notifications";
pub const CMD_NOTIFICATION_SHOWN: &str = "notification_shown";
pub const CMD_JOURNEY_START: &str = "journey_start";
pub const CMD_GAMIFICATION_STATUS: &str = "gamification_status";
pub const CMD_ACTIVITY: &str = "activity";
pub const CMD_ACHIEVEMENTS: &str = "achievements";
/// Unlock an achievement the UI earns (`AchievementArgs`): a lesson of „Mnemotechnik lernen“
/// (`learn-<lesson>`) or the finished app tour (`tour-done`). Returns whether it was new.
pub const CMD_ACHIEVEMENT_UNLOCK: &str = "achievement_unlock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AchievementArgs {
    pub id: String,
}

/// The lessons of „Mnemotechnik lernen“ in order; each one's achievement is `learn-<id>`.
pub const LEARN_LESSONS: [&str; 17] = [
    "basics",
    "visualize",
    "exaggerate",
    "motion",
    "senses",
    "link",
    "abstract",
    "numbers",
    "palace",
    "loci",
    "route",
    "review",
    "reuse",
    "mistakes",
    "history",
    "research",
    "sport",
];

/// Achievements the UI may unlock itself: the lessons and the finished tour. The others (the
/// built-in palaces, all lessons, the first own palace) follow from what the backend sees.
pub fn unlockable_by_ui(id: &str) -> bool {
    id == "tour-done"
        || id
            .strip_prefix("learn-")
            .is_some_and(|lesson| LEARN_LESSONS.contains(&lesson))
}
pub use locora_api::gamification::{
    Earned, Goal, PalaceMedal, SessionView, StatusV2Response as GamificationStatus, Today, Week,
};

/// Arguments of `CMD_GAMIFICATION_STATUS`: `full` adds the palace medals (each palace's strength
/// is worked out, so the journey bar leaves it out).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameStatusArgs {
    #[serde(default)]
    pub full: bool,
}

/// Deletes the game progress on this device and, through the sync, on all others (no
/// arguments); returns a `GameResetReport`.
pub const CMD_GAME_RESET: &str = "game_reset";
/// Sets the weekly goal from this week on (`WeekGoalArgs`); returns the goal.
pub const CMD_GAME_WEEK_GOAL: &str = "game_week_goal";
/// A one-time hint was dismissed (`GameSeenArgs`), on every device.
pub const CMD_GAME_SEEN: &str = "game_seen";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeekGoalArgs {
    pub days: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameSeenArgs {
    pub hint: String,
}

/// What became of the account's game scores on the Locora Cloud server (the rankings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RankingsDeletion {
    /// This device is not signed in: nothing on the server was touched.
    NotSignedIn,
    Deleted,
    /// The server does not know the request yet; the entries stay until it is updated.
    ServerTooOld,
    /// Not reachable: asked again at the next sync.
    Pending,
    /// The session ended: asked again after signing in.
    SignInAgain,
}

/// The game progress was deleted; this is what happened on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameResetReport {
    pub rankings: RankingsDeletion,
}
pub const CMD_JOURNEY_REVEAL: &str = "journey_reveal";
pub const CMD_JOURNEY_ANSWER: &str = "journey_answer";
pub const CMD_JOURNEY_INPUTS: &str = "journey_inputs";
pub const CMD_UI_ASSETS: &str = "ui_assets";
pub const CMD_IMPORT_PICK: &str = "import_pick";

/// The app's own animations (assets/ui).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiAssets {
    /// Journey feedback: right, again, journey done.
    pub right: Media,
    pub again: Media,
    pub done: Media,
    /// Journey progress symbols.
    pub combo: Media,
    pub trophy: Media,
    pub streak: Media,
    /// Room kinds (tabs): no picture, picture, video.
    pub room_blank: Media,
    pub room_image: Media,
    pub room_video: Media,
    /// The Locora logo (loading) and Simo (resting, nothing due).
    pub logo: Media,
    pub simo: Media,
    /// Achievement medals: coloured once unlocked (clickable), grey while locked.
    pub medal_tutorial: Media,
    pub medal_tutorial_locked: Media,
    pub medal_formulas: Media,
    pub medal_formulas_locked: Media,
    /// Medals of the other achievements by id (lessons, `learn-all`, `tour-done`,
    /// `first-palace`): coloured once unlocked, grey while locked.
    #[serde(default)]
    pub medals: std::collections::BTreeMap<String, MedalArt>,
    /// Simo's friends who teach the lessons, by name (`mina`, `cami`, `bruno`, `hafiza`).
    #[serde(default)]
    pub characters: std::collections::BTreeMap<String, Media>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MedalArt {
    pub unlocked: Media,
    pub locked: Media,
}
pub const CMD_EXPORT_PICK: &str = "export_pick";
pub const CMD_DUMP_PICK: &str = "dump_pick";
/// Save a short text (the cloud recovery key) as a file the person picks; `None` when cancelled.
pub const CMD_SAVE_TEXT_PICK: &str = "save_text_pick";

/// After importing a `.locora` file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportResult {
    pub palaces: Vec<ImportedPalace>,
    /// Loci whose review schedule came along; `None` when the file had none.
    pub restored: Option<u32>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedPalace {
    pub id: String,
    pub name: String,
    /// Differs from `name` when the name was taken.
    pub original_name: String,
}

pub const CMD_SETTINGS: &str = "settings";
pub const CMD_SETTINGS_SET: &str = "settings_set";
pub const CMD_SETTINGS_OPTIMIZE: &str = "settings_optimize";
/// The formulas of a palace: its tables, the shared tables and the bindings of its loci.
pub const CMD_WORKBOOK: &str = "workbook";
/// One change to a table cell or a binding; the reply is the workbook as it now reads.
pub const CMD_WORKBOOK_CHANGE: &str = "workbook_change";
/// The value of a formula with the palace's tables, without saving anything.
pub const CMD_WORKBOOK_CALC: &str = "workbook_calc";
pub const CMD_CLOUD_STATUS: &str = "cloud_status";
pub const CMD_CLOUD_SYNC: &str = "cloud_sync";
pub const CMD_CLOUD_CONNECT: &str = "cloud_connect";
pub const CMD_CLOUD_KEYS_STATUS: &str = "cloud_keys_status";
pub const CMD_CLOUD_KEYS_SETUP: &str = "cloud_keys_setup";
pub const CMD_CLOUD_LOGOUT: &str = "cloud_logout";
pub const CMD_CLOUD_REFRESH: &str = "cloud_refresh";
/// Deletes the synced data on the server; the devices keep theirs.
pub const CMD_CLOUD_ERASE: &str = "cloud_erase";
/// Deletes the cloud account (its password confirms it) and signs this device out.
pub const CMD_CLOUD_DELETE_ACCOUNT: &str = "cloud_delete_account";
/// What the next sync sends, by palace, and how many changes wait on the server (`CloudPending`).
pub const CMD_CLOUD_PENDING: &str = "cloud_pending";
/// Turns auto-sync on with the encryption password (`{ passphrase }`); the key is kept in the
/// device's secure store. Answers `CloudStatus`.
pub const CMD_CLOUD_AUTO_ENABLE: &str = "cloud_auto_enable";
/// Forgets the kept key and turns auto-sync off. Answers `CloudStatus`.
pub const CMD_CLOUD_AUTO_DISABLE: &str = "cloud_auto_disable";
/// What the auto-sync driver needs to decide whether to sync now (`CloudAutoState`).
pub const CMD_CLOUD_AUTO_STATE: &str = "cloud_auto_state";
/// A sync with the kept key (`{ confirmed: [palace ids] }`); answers `CloudAutoOutcome`.
pub const CMD_CLOUD_AUTO_SYNC: &str = "cloud_auto_sync";
/// Sends the e-mail with the confirmation link again.
pub const CMD_CLOUD_EMAIL_RESEND: &str = "cloud_email_resend";
/// The page where a forgotten cloud password is reset.
pub const CLOUD_FORGOT_URL: &str = "https://web.locora.cloud/forgot";
/// The friends page: friends, requests and this account's open invite links
/// (`Social<FriendsList>`).
pub const CMD_CLOUD_FRIENDS: &str = "cloud_friends";
/// Asks someone by e-mail address (`FriendAskArgs`); asking someone who asked first accepts
/// (`Social<FriendAsked>`).
pub const CMD_CLOUD_FRIEND_ASK: &str = "cloud_friend_ask";
/// Ends a friendship, withdraws a request or declines one (`FriendIdArgs`, `Social<()>`).
pub const CMD_CLOUD_FRIEND_REMOVE: &str = "cloud_friend_remove";
/// The ranking of the friends or of everyone (`RankingArgs`, `Social<RankingList>`).
pub const CMD_CLOUD_RANKING: &str = "cloud_ranking";
/// Reports a name as offensive (`ReportArgs`, `Social<Reported>`).
pub const CMD_CLOUD_REPORT: &str = "cloud_report";
/// A new invite link (`Social<InviteLink>`).
pub const CMD_CLOUD_INVITE_CREATE: &str = "cloud_invite_create";
/// Revokes an open invite link (`InviteArgs` with its code, `Social<()>`).
pub const CMD_CLOUD_INVITE_REVOKE: &str = "cloud_invite_revoke";
/// Who invites with a pasted link or code (`InviteArgs`, `Social<InviteFrom>`).
pub const CMD_CLOUD_INVITE_PREVIEW: &str = "cloud_invite_preview";
/// Accepts an invite (`InviteArgs` with the previewed code, `Social<FriendEntry>`).
pub const CMD_CLOUD_INVITE_ACCEPT: &str = "cloud_invite_accept";
/// One of the app's own graphics (`ShareArtArgs`) as a PNG for a share card, base64.
pub const CMD_SHARE_ART: &str = "share_art";
/// How sharing works on this device (`ShareWays`).
pub const CMD_SHARE_WAYS: &str = "share_ways";
/// Shares a picture with its text, or a text alone (`ShareSendArgs`, `ShareDone`).
pub const CMD_SHARE_SEND: &str = "share_send";

/// What a Locora Cloud friends command answers: its result, or why there is none. The UI words
/// each state itself, so no server message about subscriptions, routes or sessions ever reaches
/// the screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum Social<T> {
    Ready(T),
    /// No account on this device (no network was used).
    SignedOut,
    /// The session ended (401): sign in again in the settings.
    SignInAgain,
    /// The account has no access to this (402). Neutral: never says how to get it.
    NotForAccount,
    /// The server does not know this request yet (404/405 of an older server).
    ServerTooOld,
}

/// A friend, or the sender of a request to this account. `id` answers it
/// (`CMD_CLOUD_FRIEND_REMOVE`) and is the reference of the name for `CMD_CLOUD_REPORT`; an older
/// server sends none. `name` is safe to show (as text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendEntry {
    pub id: Option<String>,
    pub name: String,
}

/// A request (or an invite to an address) this account sent: never a name or an address, only
/// when (Unix seconds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentRequest {
    pub id: Option<String>,
    pub since: Option<u64>,
}

/// This account's open invite links as the friends page got them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum InviteList {
    /// The links, oldest first.
    Ready(Vec<InviteLink>),
    /// The server has no invite links yet.
    ServerTooOld,
    /// The list could not be read (the message); the rest of the page stays.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendsList {
    /// Whether the account may use friends and rankings. Without (402) the lists are empty and
    /// only the open invite links are there, to be revoked.
    pub access: bool,
    /// Requests to this account, first: they wait for an answer.
    pub incoming: Vec<FriendEntry>,
    pub friends: Vec<FriendEntry>,
    /// Oldest first.
    pub sent: Vec<SentRequest>,
    /// Requests an older server lists without saying who sent them.
    pub open: usize,
    pub invites: InviteList,
}

/// An invite link. `url` is the account site's page for it (`https://…/einladung#<code>`),
/// `None` when the server sent something else: the code alone is shown then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteLink {
    pub code: String,
    pub url: Option<String>,
    pub expires_at: u64,
}

/// Who invites with a pasted code, before accepting. `name` is safe to show; `reference` reports
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteFrom {
    pub code: String,
    pub name: String,
    pub expires_at: u64,
    pub reference: Option<String>,
}

/// The answer to asking someone: `accepted` (with the name) when both are friends now; a
/// request that waits names nobody.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendAsked {
    pub accepted: bool,
    pub name: Option<String>,
}

/// One row of a ranking. `place` is shared by equal points (1, 1, 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankingRow {
    pub place: u32,
    pub name: String,
    pub points: u64,
    /// For `CMD_CLOUD_REPORT`; `None` from an older server.
    pub reference: Option<String>,
    /// This account's own entry: only in the friends ranking (which always holds the account),
    /// and only when no friend has the same name. Never in the ranking of everyone, where a name
    /// tells nobody apart.
    pub own: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankingList {
    pub entries: Vec<RankingRow>,
    /// No entry carries this account's name: it is not in this ranking.
    pub without_you: bool,
}

/// A name was reported. `counts_now` is false while this account's address is unconfirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reported {
    pub counts_now: bool,
}

/// How a picture is shared here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareWay {
    /// The system's share sheet takes the picture with its text.
    Sheet,
    /// Saved with a dialog; the text is copied.
    Save,
    /// Downloaded; the text is copied.
    Download,
}

/// How sharing works here: a picture with its text (`card`), and whether a text alone (an invite
/// link) can go to the system's share sheet (else it is copied).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareWays {
    pub card: ShareWay,
    pub text_sheet: bool,
}

/// What became of a share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareDone {
    Shared,
    Saved,
    Downloaded,
    Cancelled,
    /// No sheet for a text here: the UI copies it.
    Copy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendAskArgs {
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendIdArgs {
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankingArgs {
    pub friends: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportArgs {
    pub reference: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteArgs {
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareArtArgs {
    pub hash: String,
}

/// A picture (`png`, base64 without a data URL's prefix) with its text, or a text alone; `name`
/// is the picture's file name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareSendArgs {
    pub name: String,
    pub png: Option<String>,
    pub text: String,
    pub title: String,
}

/// What a sync does to a palace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudChangeKind {
    New,
    Changed,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudPalaceChange {
    pub id: String,
    /// `None` when this device never knew the palace's name.
    pub name: Option<String>,
    pub kind: CloudChangeKind,
    pub changes: usize,
}

/// Changes grouped by palace; `other` counts reviews, journeys and plugin data.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudChanges {
    pub total: usize,
    pub palaces: Vec<CloudPalaceChange>,
    pub other: usize,
}

/// Before a sync: what this device sends, and how many changes of other devices wait on the
/// server (`None` when the server cannot tell).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudPending {
    pub local: CloudChanges,
    pub remote: Option<u64>,
}

/// For the auto-sync driver: whether it is on, and the newest local change (to wait until the
/// person pauses).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudAutoState {
    pub enabled: bool,
    /// Sequence number of the newest local change.
    pub local_seq: i64,
    /// Whether local changes wait to be sent.
    pub pending: bool,
    pub last_sync_at: Option<i64>,
}

/// The result of an automatic sync. Exactly one of `result`, `confirm` (palace deletions that
/// wait for a yes), `paused` (a person must act: sign in again, sync by hand) or `error` is set,
/// unless auto-sync is off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudAutoOutcome {
    pub enabled: bool,
    pub result: Option<CloudSyncResult>,
    pub confirm: Vec<CloudPalaceChange>,
    pub paused: Option<String>,
    pub error: Option<String>,
    /// The server asked to wait (429): try again much later.
    pub rate_limited: bool,
}

/// Credentials go directly to the Tauri backend; the UI never persists them.
#[derive(Clone, Serialize, Deserialize)]
pub struct CloudConnect {
    pub server: String,
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
    pub register: bool,
    /// With `register`: a guest account (data kept six months, trial access).
    #[serde(default)]
    pub guest: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CloudKeySetup {
    pub passphrase: String,
}

/// Returned once after the wrapped account key was stored on the server.
#[derive(Clone, Serialize, Deserialize)]
pub struct CloudKeyCreated {
    pub recovery_key: String,
    pub stored: bool,
    pub warning: Option<String>,
}

/// Cloud account saved in this profile; status is available without a network call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudStatus {
    pub account: Option<String>,
    pub server: Option<String>,
    pub access: bool,
    /// Whether the e-mail address is confirmed; `None` from servers that do not confirm.
    #[serde(default)]
    pub email_verified: Option<bool>,
    #[serde(default)]
    pub auto_sync: bool,
    /// Unix seconds of the last finished sync on this device.
    #[serde(default)]
    pub last_sync_at: Option<i64>,
    /// This account's own display name (to put on a share card when the person chooses).
    #[serde(default)]
    pub display_name: Option<String>,
}

impl CloudStatus {
    pub fn signed_out() -> Self {
        Self {
            account: None,
            server: None,
            access: false,
            email_verified: None,
            auto_sync: false,
            last_sync_at: None,
            display_name: None,
        }
    }
}

/// Result of one encrypted, manually started sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudSyncResult {
    pub pushed: usize,
    pub media_uploaded: usize,
    pub pulled: usize,
    pub media_downloaded: usize,
    pub applied: usize,
    pub stale: usize,
    pub missing: usize,
    pub failed: usize,
    pub quarantined: usize,
    pub cloud_expires_at: Option<i64>,
    /// What other devices changed here.
    #[serde(default)]
    pub received: CloudChanges,
    /// Changes not sent because later ones replace them.
    #[serde(default)]
    pub compacted: usize,
    /// Changes too large for the cloud; they stay on this device.
    #[serde(default)]
    pub too_large: usize,
}
/// The system's offset from UTC in minutes, sent by the UI at every start.
pub const CMD_SET_UTC_OFFSET: &str = "set_utc_offset";

/// Argument of `CMD_SET_UTC_OFFSET` (sent as `{ "offset": … }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UtcOffset {
    /// Minutes east of UTC, e.g. 120 for summer time in Germany.
    pub minutes: i32,
}

/// Settings and learning statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsView {
    pub new_per_day: u32,
    pub max_new_per_day: u32,
    /// Target retention 0.7–0.99; `None` without the review plugin.
    pub retention: Option<f64>,
    /// Personal FSRS weights (from `locora optimize`) are in use.
    pub personal_weights: bool,
    pub stats: Option<StatsView>,
    #[serde(default)]
    pub reminders: RemindersView,
    /// Reviews at least a day apart (what fitting weights learns from) and
    /// how many it needs at least.
    #[serde(default)]
    pub usable_reviews: u32,
    #[serde(default)]
    pub needed_reviews: u32,
    /// Fitting (again) is worth it now.
    #[serde(default)]
    pub optimize_hint: bool,
    /// Learning days begin at this full hour, local time.
    #[serde(default)]
    pub day_start_hour: u8,
    /// „4:00 (UTC+2)“
    #[serde(default)]
    pub day_start: String,
}

/// After fitting personal FSRS weights in the app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptimizeDone {
    pub message: String,
    pub view: SettingsView,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StatsView {
    pub answers: u32,
    pub answered_today: u32,
    pub new_today: u32,
    /// Answers of the last 30 days, and how many were not „Nochmal“.
    pub recent: u32,
    pub recent_known: u32,
    pub streak: u32,
    pub scheduled: u32,
    pub due: u32,
    /// Learning days and weeks in a row with the weekly goal reached (on every device);
    /// `None` without the gamification plugin, where `streak` is shown instead.
    #[serde(default)]
    pub learning_days: Option<u32>,
    #[serde(default)]
    pub week_streak: Option<u32>,
}

/// A built-in achievement. It lives apart from the palaces, so it stays when they are removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AchievementView {
    pub id: String,
    pub unlocked: bool,
    /// The learning day it was earned (`YYYY-MM-DD`), when known.
    #[serde(default)]
    pub day: Option<String>,
    /// A palace medal's palace (its name).
    #[serde(default)]
    pub palace: Option<String>,
    /// A palace medal's progress: loci that hold long enough, of all loci.
    #[serde(default)]
    pub progress: Option<(u32, u32)>,
}

/// Answers on one learning day, for the activity overview. `day` is the start of that day in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityDay {
    pub day: u64,
    pub answers: u32,
}

/// Change settings; `None` keeps a value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SettingsChange {
    pub new_per_day: Option<u32>,
    /// As typed: `0,9`, `0.85`, `90 %`.
    pub retention: Option<String>,
    #[serde(default)]
    pub day_start_hour: Option<u8>,
    #[serde(default)]
    pub reminders: Option<RemindersChange>,
}

/// Daily reminders as typed: times „HH:MM“, local time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemindersChange {
    pub enabled: bool,
    pub start: String,
    pub end: String,
    pub every: u8,
}

/// Daily reminders as the settings screen shows them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RemindersView {
    pub enabled: bool,
    pub start: String,
    pub end: String,
    pub every: u8,
    /// The day's reminder times, „HH:MM“, oldest first; empty when off.
    pub times: Vec<String>,
}
pub const CMD_JOURNEY_ACTION: &str = "journey_action";

/// Custom URI scheme for media: `<scheme>://localhost/<kind>/<sha256>`
/// (`http://<scheme>.localhost/…` on Windows and Android; the UI asks Tauri's
/// `convertFileSrc` for the right form). Kinds: `media` (bytes as stored) and
/// `sply` (a `.sply` as interactive SVG with the Splinely runtime).
pub const SCHEME: &str = "locora";

/// The start screen.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Home {
    pub palaces: Vec<PalaceCard>,
    /// Due loci across all palaces.
    pub due: u32,
    /// New loci that today's limit still allows.
    pub fresh: u32,
    /// Is the tutorial palace there?
    pub tutorial: bool,
    /// Is the separate formula guide palace there?
    #[serde(default)]
    pub formula_guide: bool,
    /// Problems while opening the profile (plugins that did not load).
    pub notes: Vec<String>,
    /// The latest tile of every loaded plugin (`ui.tile`).
    #[serde(default)]
    pub tiles: Vec<TileView>,
    /// No palaces yet: Simo welcomes and points to the tutorial.
    #[serde(default)]
    pub welcome: Option<Media>,
    /// Cards plugins declared (`ui.block`), in plugin order.
    #[serde(default)]
    pub blocks: Vec<PluginBlock>,
    /// Pages plugins suggest (`ui.page`), in plugin order.
    #[serde(default)]
    pub pages: Vec<PluginPage>,
}

/// A card a plugin declared. `id` is unique within the plugin; the app addresses it as
/// `plugin/id`. Titles and texts may be translation templates, as for tiles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginBlock {
    pub plugin: String,
    pub id: String,
    pub title: String,
    pub text: String,
}

/// A page a plugin suggests: `blocks` are block ids of the same plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginPage {
    pub plugin: String,
    pub id: String,
    pub name: String,
    pub blocks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TileView {
    pub plugin: String,
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub icon: Option<Media>,
}

/// A notification to show now. `action` names what a tap runs,
/// e.g. `review.start@Planeten` (start a journey through that palace).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationView {
    pub channel: String,
    /// `0` = informational, `1` = time-sensitive, `2` = urgent.
    pub priority: u8,
    pub title: String,
    pub body: String,
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PalaceCard {
    pub id: String,
    pub name: String,
    /// `tutorial` or `formulas` for the built-in palaces, in any language.
    #[serde(default)]
    pub built_in: Option<String>,
    pub rooms: u32,
    pub loci: u32,
    pub due: u32,
    pub fresh: u32,
    /// The first room's backdrop, as a thumbnail.
    pub cover: Option<Media>,
    /// Rooms with a backdrop that can be selected as the card thumbnail.
    #[serde(default)]
    pub previews: Vec<RoomPreview>,
    /// How many loci are consolidated; `None` without the review plugin.
    #[serde(default)]
    pub strength: Option<Strength>,
}

/// A palace's loci, and how many of them are consolidated (hold for 30 days or more).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Strength {
    pub loci: u32,
    pub solid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomPreview {
    pub id: String,
    pub name: String,
    pub media: Media,
}

/// A stored file the UI can show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Media {
    pub hash: String,
    pub kind: MediaKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Video,
    /// A Splinely animation, shown through the `sply` URL kind.
    Sply,
    /// A sound clip (MP3, M4A, WAV or Ogg).
    Audio,
}

impl Media {
    /// Path below the custom scheme, e.g. `sply/<hash>`.
    pub fn path(&self) -> String {
        match self.kind {
            MediaKind::Sply => format!("sply/{}", self.hash),
            MediaKind::Image | MediaKind::Video | MediaKind::Audio => {
                format!("media/{}", self.hash)
            }
        }
    }
}

/// A palace with its rooms, for the room view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PalaceView {
    pub id: String,
    pub name: String,
    /// Locus marker (`.sply`, see assets/STIL.md 8b); the default when none.
    pub marker: Media,
    pub rooms: Vec<RoomView>,
    /// How many loci are consolidated; `None` without the review plugin.
    #[serde(default)]
    pub strength: Option<Strength>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomView {
    pub video: bool,
    pub id: String,
    pub name: String,
    pub backdrop: Option<Media>,
    pub loci: Vec<LocusView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocusView {
    pub answer: EditorAnswer,
    /// Keep the raw reference even if media cannot currently be displayed.
    pub image: Option<String>,
    pub bound: bool,
    /// Formula-bound fields (`place`, `item`, `x`, `y`, `visible`, `active`);
    /// only these stay read-only in the editor.
    #[serde(default)]
    pub bound_fields: Vec<String>,
    /// Picture width as a fraction of the room width.
    pub image_size: f64,
    /// Zoom on the locus while asking, 1 (whole room) to 4.
    #[serde(default = "whole_room")]
    pub zoom: f64,
    pub id: String,
    /// Video position in seconds; absent in non-video rooms.
    pub time: Option<f64>,
    /// Route number across the palace, from 1.
    pub number: u32,
    pub place: String,
    pub item: String,
    /// Position 0..1 from the left / top.
    pub x: f64,
    pub y: f64,
    pub status: Status,
    /// Belongs to the answer: shown only once the locus is revealed.
    pub picture: Option<Media>,
    pub note: String,
    /// The locus sound clip (hash); played with the answer.
    #[serde(default)]
    pub audio: Option<String>,
}

impl LocusView {
    pub fn is_bound(&self, field: &str) -> bool {
        self.bound_fields.iter().any(|f| f == field)
    }
}

/// Full content replacement; geometry is deliberately patched separately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocusContent {
    pub place: String,
    pub item: String,
    pub note: String,
    pub image: Option<String>,
    pub image_size: f64,
    #[serde(default = "whole_room")]
    pub zoom: f64,
    pub answer: EditorAnswer,
    /// The locus sound clip (hash); `None` removes it.
    #[serde(default)]
    pub audio: Option<String>,
}

/// Zoom when none is given: the whole room.
pub fn whole_room() -> f64 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorChange {
    pub palace_id: String,
    pub room_id: String,
    pub action: EditorAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum EditorAction {
    Create {
        x: f64,
        y: f64,
    },
    CreateVideo {
        time: f64,
    },
    CreateTimed {
        time: f64,
        x: f64,
        y: f64,
    },
    Paste {
        items: Vec<String>,
    },
    Save {
        id: String,
        content: LocusContent,
    },
    Move {
        id: String,
        x: f64,
        y: f64,
    },
    Retime {
        id: String,
        time: f64,
    },
    Resize {
        id: String,
        image_size: f64,
    },
    Delete {
        id: String,
    },
    /// Backend-issued, scoped, single-use structural undo/redo capability.
    Restore {
        receipt: String,
    },
}

pub const MAX_PASTE_ITEMS: usize = 100;
pub const MAX_PASTE_ITEM_BYTES: usize = 500;
pub const MAX_PASTE_BYTES: usize = 64 * 1024;

/// Validate again at the IPC boundary; the UI is not trusted.
pub fn validate_paste_items(items: &[String]) -> Result<(), &'static str> {
    if items.is_empty() || items.len() > MAX_PASTE_ITEMS {
        return Err("Bitte 1 bis 100 Begriffe einfügen");
    }
    if items.iter().any(|item| {
        item.trim().is_empty() || item.len() > MAX_PASTE_ITEM_BYTES || item.contains(['\n', '\r'])
    }) {
        return Err("Jeder Begriff braucht eine nichtleere Zeile mit höchstens 500 Bytes");
    }
    Ok(())
}

/// Serpentine route, inset from the edges. Call with index < count, count > 0.
pub fn paste_route_position(index: usize, count: usize) -> (f64, f64) {
    let columns = (count as f64).sqrt().ceil() as usize;
    let rows = count.div_ceil(columns);
    let row = index / columns;
    let column = if row.is_multiple_of(2) {
        index % columns
    } else {
        columns - 1 - index % columns
    };
    let axis = |n: usize, total: usize| {
        if total == 1 {
            0.5
        } else {
            0.1 + 0.8 * n as f64 / (total - 1) as f64
        }
    };
    (axis(column, columns), axis(row, rows))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorUpdated {
    pub palace: PalaceView,
    pub selected: Option<String>,
    /// Reciprocal receipt after a structural mutation or restore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
}

/// Marker states; the names are the marker's state machine events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Neu,
    Faellig,
    Gewusst,
}

impl Status {
    pub fn event(self) -> &'static str {
        match self {
            Status::Neu => "neu",
            Status::Faellig => "faellig",
            Status::Gewusst => "gewusst",
        }
    }
}

/// Start a journey through one palace or (without `palace_id`) everything due.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JourneyStart {
    pub palace_id: Option<String>,
    pub due_only: bool,
    pub reverse: bool,
    pub numbers: bool,
    pub shuffle: bool,
    /// Values for workbook inputs (`Inputs` table), asked before the start.
    #[serde(default)]
    pub inputs: Vec<InputValue>,
    /// Quiz: how many answers each question shows in all (the correct ones always count);
    /// `None` shows four.
    #[serde(default)]
    pub quiz_answers: Option<u8>,
    /// Sudden death: the journey ends at the first wrong answer.
    #[serde(default)]
    pub sudden_death: bool,
    /// Seconds per question. A question left unanswered in time counts as wrong;
    /// `None` means no limit.
    #[serde(default)]
    pub time_limit: Option<u8>,
}

/// A workbook input a journey asks for before it starts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputField {
    pub palace_id: String,
    pub palace: String,
    pub key: String,
    /// `text`, `number`, `boolean`, …
    pub kind: String,
    /// The default as text.
    pub default: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputValue {
    pub palace_id: String,
    pub key: String,
    pub value: String,
}

/// After running a workbook action (`!name`) on the current question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionDone {
    /// The action asks to show the answer.
    pub reveal: bool,
    /// The question again, with fields recomputed.
    pub question: Question,
    /// The answer side, when it is (or becomes) visible.
    pub revealed: Option<Revealed>,
    /// After grading: the next question, recomputed (actions change the run
    /// values later stops use). `None` at the end of the journey.
    #[serde(default)]
    pub next: Option<Question>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnswerKind {
    Recall,
    Guess,
    Quiz,
}

/// One question of a journey.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    /// 1-based position and length of the journey.
    pub n: u32,
    pub total: u32,
    pub palace: String,
    pub room: String,
    /// Video position for this locus, in seconds.
    pub time: Option<f64>,
    /// At the first zero-second video stop, show all loci placed at the start.
    #[serde(default)]
    pub video_preview: Vec<VideoPreviewItem>,
    pub backdrop: Option<Media>,
    pub marker: Media,
    /// Route markers in the same room, relative to this question.
    #[serde(default)]
    pub nearby: Vec<NearbyLocus>,
    pub x: f64,
    pub y: f64,
    /// The stage zooms in on the locus by this factor (1 = whole room).
    #[serde(default = "whole_room")]
    pub zoom: f64,
    pub prompt: String,
    pub kind: AnswerKind,
    /// Quiz: the options shown, in order; answer with their positions.
    pub options: Vec<String>,
    /// Reverse journeys ask with the picture, which then is the question.
    pub picture: Option<Media>,
    /// Workbook actions of this palace, run with `journey_action`.
    #[serde(default)]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NearbyLocus {
    pub id: String,
    pub offset: i32,
    pub number: u32,
    pub x: f64,
    pub y: f64,
    pub time: Option<f64>,
    /// The locus picture, shown on the stage once the locus is revealed.
    pub picture: Option<Media>,
    /// Picture width as a fraction of the room width.
    pub image_size: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoPreviewItem {
    pub place: String,
    pub item: String,
}

/// What the learner did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum Reply {
    /// Self-rating 1 (Nochmal) … 4 (Leicht), after revealing.
    Recall(u8),
    Guess(String),
    /// Positions in `Question::options`.
    Quiz(Vec<u32>),
    /// Revealed without answering (counts as Nochmal).
    Forfeit,
}

/// The answer side of a question: shown on reveal and after answering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Revealed {
    /// Picture width as a fraction of the room width.
    pub image_size: f64,
    pub solution: String,
    pub note: String,
    /// The locus picture; part of the answer (assets/STIL.md 8a).
    pub picture: Option<Media>,
    /// Quiz: positions of the right options among those shown.
    pub right: Vec<u32>,
    /// The locus sound clip; played when the answer is shown.
    #[serde(default)]
    pub audio: Option<Media>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feedback {
    pub correct: bool,
    pub points: u64,
    pub revealed: Revealed,
    pub next: Step,
    /// The run of right answers after this answer; `None` without the gamification plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<GameView>,
}

/// Progress inside the current journey, as the gamification plugin counts it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameView {
    /// Right Quiz and Guess answers in a row; `None` after a recalled answer (recall grades
    /// neither show nor break a run).
    #[serde(default)]
    pub combo: Option<u32>,
}

/// What follows an answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
// Sent once per question as JSON; boxing would only complicate the UI code.
#[allow(clippy::large_enum_variant)]
pub enum Step {
    Question(Question),
    Done(Summary),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub asked: u32,
    pub correct: u32,
    pub points: u64,
    /// Messages from plugins (e.g. learning time).
    pub notes: Vec<String>,
    /// XP this journey earned; `None` without the gamification plugin.
    #[serde(default)]
    pub xp_gained: Option<u64>,
    /// Due reviews and new loci in this journey, and loci practised before they were due.
    #[serde(default)]
    pub due_done: u32,
    #[serde(default)]
    pub new_done: u32,
    #[serde(default)]
    pub early: u32,
    /// Trophies and palace medals earned with this journey (achievement ids, see
    /// `AchievementView`; a palace medal as `palace-<level>@<palace name>`).
    #[serde(default)]
    pub new_trophies: Vec<String>,
    /// Due reviews still open today; `None` without the gamification plugin.
    #[serde(default)]
    pub due_left_today: Option<u32>,
    /// Today's due goal after the journey; `None` without the gamification plugin.
    #[serde(default)]
    pub goal: Option<Goal>,
    /// Loci answered again on the same learning day in this journey (practice, no XP).
    #[serde(default)]
    pub repeat: u32,
    /// Loci answered today that come back later today for practice (after „Again“).
    #[serde(default)]
    pub repeat_later: u32,
    /// The next learning day after today with reviews (`YYYY-MM-DD`) and how many are due then.
    #[serde(default)]
    pub next_due: Option<(String, u32)>,
    /// Learning days from today to `next_due` (1 tomorrow).
    #[serde(default)]
    pub next_due_in: Option<u32>,
    /// When the loci practised early in this journey come back first (`YYYY-MM-DD`), and in how
    /// many learning days.
    #[serde(default)]
    pub early_next: Option<(String, u32)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_paths() {
        let sply = Media {
            hash: "ab".into(),
            kind: MediaKind::Sply,
        };
        assert_eq!(sply.path(), "sply/ab");
        let png = Media {
            hash: "cd".into(),
            kind: MediaKind::Image,
        };
        assert_eq!(png.path(), "media/cd");
    }
}

/// One cell of a workbook table: its column, its type (`text`, `number`, `boolean`, `json`,
/// `formula`, `asset`, `palace`, `room`) and its value as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkbookCell {
    pub column: String,
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkbookRow {
    pub table: String,
    pub key: String,
    pub cells: Vec<WorkbookCell>,
}

/// A formula bound to a locus field, with what it gives now (or why it fails).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkbookBinding {
    pub field: String,
    pub expression: String,
    pub value: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkbookLocus {
    pub id: String,
    pub number: u32,
    pub place: String,
    pub bindings: Vec<WorkbookBinding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkbookView {
    pub palace_id: String,
    /// The palace's own tables, then the tables all palaces share.
    pub rows: Vec<WorkbookRow>,
    pub shared: Vec<WorkbookRow>,
    /// Every locus in route order, with its bindings.
    pub loci: Vec<WorkbookLocus>,
}

/// A change to a palace's workbook. `shared` edits the tables all palaces share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum WorkbookChange {
    SetCell {
        shared: bool,
        table: String,
        key: String,
        column: String,
        kind: String,
        value: String,
    },
    RemoveCell {
        shared: bool,
        table: String,
        key: String,
        column: String,
    },
    Bind {
        locus_id: String,
        field: String,
        expression: String,
    },
    Unbind {
        locus_id: String,
        field: String,
    },
}

// The memory contest (`locora_app_core::competition`): one round runs at a time; the app shows its
// task and collects the recall, and the `competition` plugin keeps what was handed in.

/// Today's challenge (`CompeteToday`).
pub const CMD_COMPETE_TODAY: &str = "compete_today";
/// Begins a round (`{ start: CompeteStart }`); answers `CompeteStarted`.
pub const CMD_COMPETE_START: &str = "compete_start";
/// Memorizing is done: the content goes, recall begins (`CompeteRecall`).
pub const CMD_COMPETE_RECALL: &str = "compete_recall";
/// Hands the round in (`{ finish: CompeteHandIn }`); answers `CompeteResult`.
pub const CMD_COMPETE_FINISH: &str = "compete_finish";
/// Drops the running round before its result (a daily stays begun: one try per day).
pub const CMD_COMPETE_ABANDON: &str = "compete_abandon";
/// Begins the evening recall of a daily round (`{ evening: CompeteEvening }`); answers
/// `CompeteEveningStarted`.
pub const CMD_COMPETE_EVENING_START: &str = "compete_evening_start";
/// Hands the evening recall in (`{ finish: CompeteHandIn }`); answers `CompeteEveningResult`.
pub const CMD_COMPETE_EVENING_FINISH: &str = "compete_evening_finish";
/// Records, days with a daily challenge and the contest trophies (`CompeteRecords`).
pub const CMD_COMPETE_RECORDS: &str = "compete_records";
/// The rounds handed in, newest first (`{ query: CompeteHistoryQuery }`; `CompeteHistoryEntry`).
pub const CMD_COMPETE_HISTORY: &str = "compete_history";
/// A new challenge code (`{ choice: CompeteChoice }`), with a fresh seed.
pub const CMD_COMPETE_CODE: &str = "compete_code";
/// The palaces whose route a round could be laid on (`{ choice: CompeteChoice }`;
/// `RouteChoice`).
pub const CMD_COMPETE_ROUTES: &str = "compete_routes";
/// The place names of a palace's route, in order (`{ palaceId }`), to walk it before a round.
pub const CMD_COMPETE_ROUTE_WALK: &str = "compete_route_walk";

pub use locora_api::competition::{
    Best, Counted, DayPlan, EveningFinishResponse as CompeteEveningFinished,
    EveningResponse as CompeteEveningView, EveningScore, EveningState, EveningToday,
    FinishResponse as CompeteFinished, Hint, HistoryEntry as CompeteHistoryEntry, Kind,
    LevelRecord, LevelState, LevelToday, RecordLine, RecordsResponse as CompeteRecords,
    TodayResponse as CompeteDay, Trophy as CompeteTrophy,
};
pub use locora_competition::schedule::{PlaceOption, Prep};
pub use locora_competition::{
    Date, Discipline, Info, Lang as ContestLang, Level, Mark, Recall, Score, Spec, Task,
};

/// Today's challenge, and for each of its levels the places a route needs per "per place".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompeteToday {
    pub today: CompeteDay,
    /// Levels 1 to 3 of today's discipline.
    pub routes_needed: Vec<Prep>,
}

/// What a round is: practice, the daily challenge, a challenge code, or an earlier day's daily
/// played as practice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartKind {
    Practice,
    Daily,
    Code,
    Earlier,
}

/// Arguments of `CMD_COMPETE_START`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompeteStart {
    pub kind: StartKind,
    /// Practice only; the daily and an earlier day have their date's discipline, a code its own.
    #[serde(default)]
    pub discipline: Option<Discipline>,
    /// Level 1 when missing; a code has its own.
    #[serde(default)]
    pub level: Option<Level>,
    #[serde(default)]
    pub code: Option<String>,
    /// `Earlier` only: one of the last seven days.
    #[serde(default)]
    pub date: Option<Date>,
    /// Practice and codes only: no time limit (no records, no trophies).
    #[serde(default)]
    pub untimed: bool,
    /// A palace whose route labels the places.
    #[serde(default)]
    pub route: Option<String>,
    /// Things per place (see `PlaceOption`); the discipline's first option when missing.
    #[serde(default)]
    pub per_place: Option<u8>,
    /// The language the shopping list is shown and scored in.
    pub lang: ContestLang,
}

/// The place a task item lies on: its number from 1, and the route's name for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceLabel {
    pub place: u16,
    pub name: Option<String>,
}

/// A round that began: its task, and what the screens show with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompeteStarted {
    pub round_id: String,
    pub kind: Kind,
    pub spec: Spec,
    pub task: Task,
    /// The learning date of the round.
    pub date: Date,
    pub untimed: bool,
    /// Shopping list: the things in the round's language, in task order; empty otherwise.
    pub words: Vec<String>,
    /// One per task item.
    pub places: Vec<PlaceLabel>,
}

/// Memorizing ended: how long it counted and how long recall may take (`None` without a limit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompeteRecall {
    pub memorize_ms: u32,
    pub recall_ms_left: Option<u32>,
}

/// Arguments of `CMD_COMPETE_FINISH` and `CMD_COMPETE_EVENING_FINISH`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompeteHandIn {
    pub recall: Recall,
    /// The language of the person (the evening recall and the result's names are in it).
    pub lang: ContestLang,
}

/// A round's result: the plugin's verdict, and per place what was right.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompeteResult {
    pub finished: CompeteFinished,
    /// The task per place, as people read it: things, digits, card names.
    pub correct: Vec<String>,
    pub memorize_ms: u32,
    pub recall_ms: u32,
    /// Practice and code rounds: the challenge code of the task.
    pub code: Option<String>,
}

/// Arguments of `CMD_COMPETE_EVENING_START`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompeteEvening {
    pub round_id: String,
    #[serde(default)]
    pub per_place: Option<u8>,
}

/// An evening recall that began: its window and the places to fill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompeteEveningStarted {
    pub evening: CompeteEveningView,
    pub places: Vec<PlaceLabel>,
}

/// An evening recall's result, with the morning's task per place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompeteEveningResult {
    pub finished: CompeteEveningFinished,
    pub correct: Vec<String>,
}

/// Arguments of `CMD_COMPETE_HISTORY`: at most `limit` (100 for 0), before `before_ms`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompeteHistoryQuery {
    #[serde(default)]
    pub limit: u32,
    #[serde(default)]
    pub before_ms: Option<u64>,
}

/// A discipline and level, for `CMD_COMPETE_CODE` and `CMD_COMPETE_ROUTES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompeteChoice {
    pub discipline: Discipline,
    pub level: Level,
    /// Routes: things per place; the discipline's first option when missing.
    #[serde(default)]
    pub per_place: Option<u8>,
}

/// A palace a round could be laid on: the places on its route, and whether they are enough.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteChoice {
    pub palace_id: String,
    pub name: String,
    pub places: u32,
    pub fits: bool,
}
