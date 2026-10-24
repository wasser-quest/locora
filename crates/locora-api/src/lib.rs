//! Types shared by the Locora host and every plugin.
//!
//! Events are plain serde structs with a fixed topic. They travel across the
//! WebAssembly boundary encoded with [`postcard`], so both sides only ever see
//! typed Rust values.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Plugin API version implemented by this crate.
pub const API_VERSION: &str = "0.7";

/// A typed event with a stable topic name.
pub trait Event: Serialize + DeserializeOwned {
    /// Topic the event is published on, namespaced by the owning plugin.
    const TOPIC: &'static str;
}

/// A request/response interface one plugin offers to the host and others.
///
/// The name carries a version (`review.due@1`); a breaking change gets a new
/// name so old and new callers can coexist.
pub trait Service {
    /// Versioned service name, namespaced by the providing plugin.
    const NAME: &'static str;
    type Request: Serialize + DeserializeOwned;
    type Response: Serialize + DeserializeOwned;
}

/// A typed, synchronous filter chain driven by the native host.
pub trait Hook {
    const NAME: &'static str;
    type Value: Serialize + DeserializeOwned;
}

pub mod journey;
pub mod palaces;
pub mod storage;

/// Encode an event payload.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(value)
}

/// Decode an event payload.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, postcard::Error> {
    postcard::from_bytes(bytes)
}

/// Well-known permissions a plugin can request.
pub mod permission {
    /// Plugin-private key-value storage.
    pub const STORAGE: &str = "storage";
    /// Read palaces, stages and loci.
    pub const PALACES_READ: &str = "palaces:read";
    /// Create and modify palaces, stages and loci.
    pub const PALACES_WRITE: &str = "palaces:write";
    /// Send and schedule notifications.
    pub const NOTIFY: &str = "notify";
    /// Highlight a named app control during an interactive tour.
    pub const UI_HIGHLIGHT: &str = "ui:highlight";
}

/// Events published by the host itself.
pub mod host {
    use super::*;

    /// All plugins are loaded and activated.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Started;

    impl Event for Started {
        const TOPIC: &'static str = "host.started";
    }
}

/// UI contributions published by plugins and rendered by the host.
pub mod ui {
    use super::*;

    /// Show or update the publishing plugin's tile on the home screen.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Tile {
        pub title: String,
        pub text: String,
    }

    impl Event for Tile {
        const TOPIC: &'static str = "ui.tile";
    }

    /// Most blocks one plugin may declare, and most pages.
    pub const MAX_BLOCKS: usize = 24;
    pub const MAX_PAGES: usize = 8;

    /// Ids of blocks, pages and removals: 1 to 48 lowercase letters, digits, `-` or `_`.
    /// The app builds its own addresses from them, so nothing else is accepted.
    pub fn valid_id(id: &str) -> bool {
        (1..=48).contains(&id.len())
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    }

    /// A card the user can place on any page of the app (`ui.block`). Declaring the same id
    /// again replaces the card. The app shows the title and text; plugins send no markup.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Block {
        pub id: String,
        pub title: String,
        pub text: String,
    }

    impl Event for Block {
        const TOPIC: &'static str = "ui.block";
    }

    /// A page the user can add in one step (`ui.page`). `blocks` are this plugin's block ids,
    /// placed on the new page in this order. Declaring the same id again replaces the page.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Page {
        pub id: String,
        pub name: String,
        pub blocks: Vec<String>,
    }

    impl Event for Page {
        const TOPIC: &'static str = "ui.page";
    }

    /// Remove a block or a page of the publishing plugin by id (`ui.remove`).
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Remove {
        pub id: String,
    }

    impl Event for Remove {
        const TOPIC: &'static str = "ui.remove";
    }

    /// A short, declarative tour step. The app locates `target` by its stable
    /// element ID; no HTML or CSS selector is accepted from a plugin.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct HighlightRequest {
        pub target: String,
        pub title: String,
        pub text: String,
        pub step: u8,
        pub total: u8,
    }

    pub struct Highlight;

    impl Service for Highlight {
        const NAME: &'static str = "ui.highlight@1";
        type Request = HighlightRequest;
        type Response = bool;
    }
}

pub mod tutorial {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum TourAction {
        Start,
        Resume,
        Previous,
        Next,
        Skip,
    }

    pub struct Tour;

    impl Service for Tour {
        const NAME: &'static str = "tutorial.tour@1";
        type Request = TourAction;
        type Response = bool;
    }
}

/// Events published by the `core` plugin.
pub mod core {
    use super::*;

    /// Filter the points awarded for a journey answer. No scoring rules are implied.
    pub struct JourneyScore;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Score {
        pub palace_id: String,
        pub locus_id: String,
        pub points: u64,
    }

    impl Hook for JourneyScore {
        const NAME: &'static str = "core.journey.score";
        type Value = Score;
    }

    /// A new palace was created.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct PalaceCreated {
        pub palace_id: String,
        pub name: String,
    }

    impl Event for PalaceCreated {
        const TOPIC: &'static str = "core.palace.created";
    }

    /// How a locus was answered.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Grade {
        Again,
        Hard,
        Good,
        Easy,
    }

    /// A locus was answered during a journey.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct LocusAnswered {
        pub palace_id: String,
        pub locus_id: String,
        pub correct: bool,
        pub grade: Grade,
        pub elapsed_ms: u64,
        /// When the answer was given, in seconds since the Unix epoch.
        pub answered_at: u64,
    }

    impl Event for LocusAnswered {
        const TOPIC: &'static str = "core.journey.locus_answered";
    }

    /// A journey begins for these palaces. Sent after a route was planned.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct JourneyStarted {
        pub palace_ids: Vec<String>,
    }

    impl Event for JourneyStarted {
        const TOPIC: &'static str = "core.journey.started";
    }

    /// Answer with the profile's local learning day attached by the runtime.
    /// This additive event leaves the established LocusAnswered ABI intact.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct LearningDayAnswer {
        pub palace_id: String,
        pub correct: bool,
        pub answered_at: u64,
        pub day_start: u64,
    }

    impl Event for LearningDayAnswer {
        const TOPIC: &'static str = "core.journey.learning_day_answer";
    }

    /// Whether an answered locus was due for review when it was answered.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Timing {
        /// Its review was due (or no review plugin schedules it).
        Due,
        /// Never answered before: a new locus.
        New,
        /// Answered before its review was due (practice).
        Early,
        /// Answered before on the same learning day, e.g. again after „Again“ (practice: a locus
        /// counts once per learning day).
        Repeat,
    }

    /// An answer with what the runtime knows about it: the review state before the answer,
    /// the local learning day and who answered (this device, the current game epoch).
    /// Published before [`LocusAnswered`], so the card state is the one before this answer.
    /// A new type and topic, so the layouts of the older answer events stay intact.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ReviewedAnswer {
        pub palace_id: String,
        pub locus_id: String,
        pub kind: crate::journey::AnswerKind,
        pub grade: Grade,
        pub correct: bool,
        pub timing: Timing,
        /// Seconds since the Unix epoch.
        pub answered_at: u64,
        /// Start of the learning day, seconds since the Unix epoch.
        pub day_start: u64,
        /// The local date of the learning day, `YYYY-MM-DD`.
        pub day: String,
        /// The game epoch (changes when the game progress is deleted), `0` before any reset.
        pub epoch: String,
        /// This device's id in the operation log.
        pub device: String,
    }

    impl Event for ReviewedAnswer {
        const TOPIC: &'static str = "core.journey.reviewed";
    }

    /// A journey was finished.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct JourneyCompleted {
        pub palace_id: String,
        pub correct: u32,
        pub total: u32,
        pub duration_secs: u64,
    }

    impl Event for JourneyCompleted {
        const TOPIC: &'static str = "core.journey.completed";
    }

    /// A palace and everything in it was deleted.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct PalaceDeleted {
        pub palace_id: String,
    }

    impl Event for PalaceDeleted {
        const TOPIC: &'static str = "core.palace.deleted";
    }

    /// A locus was deleted.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct LocusDeleted {
        pub palace_id: String,
        pub locus_id: String,
    }

    impl Event for LocusDeleted {
        const TOPIC: &'static str = "core.locus.deleted";
    }
}

/// Local, offline progress supplied by the gamification plugin.
pub mod gamification {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Trophy {
        FirstJourney,
        PerfectJourney,
        HundredXp,
        SevenDayStreak,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct TrophyUnlocked {
        pub trophy: Trophy,
    }

    impl Event for TrophyUnlocked {
        const TOPIC: &'static str = "gamification.trophy.unlocked";
    }

    pub struct Status;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct StatusRequest {
        /// Start of the current learning day, calculated by the native runtime.
        pub day_start: u64,
        /// The runtime's local-time streak (zero without review history).
        pub streak: u32,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct StatusResponse {
        pub xp: u64,
        pub combo: u32,
        pub max_combo: u32,
        pub hearts: u8,
        pub trophies: Vec<Trophy>,
        pub daily_correct: u32,
        pub daily_goal: u32,
        pub streak: u32,
    }

    impl Service for Status {
        const NAME: &'static str = "gamification.status@1";
        type Request = StatusRequest;
        type Response = StatusResponse;
    }

    impl Trophy {
        /// The stable id of a trophy (also used by `status@2`).
        pub fn id(self) -> &'static str {
            match self {
                Trophy::FirstJourney => "first-journey",
                Trophy::PerfectJourney => "perfect-journey",
                Trophy::HundredXp => "hundred-xp",
                Trophy::SevenDayStreak => "seven-day-streak",
            }
        }
    }

    /// Strength of one palace from its review cards: how many of its loci hold for at least
    /// 7, 30 and 180 days (FSRS stability). Computed by the runtime, which counts a locus only
    /// with a right answer on a later learning day than its first, and only the stability that
    /// the first answer of each learning day built: answers in one sitting earn no medal.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct PalaceStrength {
        pub palace_id: String,
        pub loci: u32,
        pub hold_7: u32,
        pub hold_30: u32,
        pub hold_180: u32,
    }

    /// Progress derived from the synced facts of every device (`gamification.status@2`).
    pub struct StatusV2;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct StatusV2Request {
        /// The local date of the learning day, `YYYY-MM-DD`.
        pub day: String,
        pub day_start: u64,
        /// The game epoch (see `ReviewedAnswer`) and this device's id.
        pub epoch: String,
        pub device: String,
        /// Reviews due on this learning day that were not reviewed yet today, over all palaces.
        pub due_open: u32,
        /// Loci answered earlier today that are back for practice now (after „Again“); they are
        /// not part of the day's goal.
        pub repeat_open: u32,
        /// Whether any locus has a review schedule (a day with nothing due counts only then).
        pub has_cards: bool,
        /// Strength per palace; `None` when the caller does not need palace medals.
        pub palaces: Option<Vec<PalaceStrength>>,
        /// Now, seconds since the Unix epoch (orders what devices saw of the schedule).
        pub at: u64,
        /// The next learning day (`YYYY-MM-DD`) on which a review is due as the schedule stands
        /// (today while reviews are open); `None` without any card. Days before it have nothing
        /// due, so the weekly goal can tell them without the app being opened.
        pub next_due: Option<String>,
        /// The learning day the current epoch began (the day the progress was deleted): reviews
        /// before the deletion are gone, so it is never taken for a day with nothing due.
        pub epoch_day: Option<String>,
    }

    /// Today's goal: the due reviews.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Goal {
        /// Due reviews are open.
        Open,
        /// Every due review is done.
        AllDone,
        /// Nothing was due: a free day.
        Free,
        /// A long backlog: enough for today (see [`ENOUGH`]).
        Enough,
    }

    /// Due reviews after which a backlog counts as enough for one day.
    pub const ENOUGH: u32 = 50;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Today {
        pub due_done: u32,
        pub due_open: u32,
        pub new_done: u32,
        /// Loci back for practice today (see `StatusV2Request::repeat_open`), not in the goal.
        pub repeat_open: u32,
        pub goal: Goal,
    }

    /// The weekly rhythm: learning days this ISO week against the chosen goal. A learning day
    /// has a due or new answer on some device; a day on which nothing was due lowers the week's
    /// target instead (to at least one learning day). A week with no learning day whose free
    /// days alone cover the goal neither counts nor breaks a run.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Week {
        pub days: u32,
        /// The chosen goal (learning days per week).
        pub goal: u8,
        /// Days of this week so far on which nothing was due (and nothing was learned).
        pub free: u32,
        /// Learning days this week needs: the goal less the free days, at least one.
        pub target: u32,
        /// Weeks in a row that reached their target (the current week counts once reached, and
        /// does not break the run before it ends).
        pub streak: u32,
        pub best: u32,
        /// The previous week did not reach its goal (and some week did before).
        pub missed_last: bool,
    }

    /// An earned trophy and the learning day it was earned on (`None` for trophies from
    /// before the sync, whose day is not known).
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Earned {
        pub id: String,
        pub day: Option<String>,
    }

    /// One medal level of one palace: `level` is 7, 30 or 180 days.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct PalaceMedal {
        pub palace_id: String,
        pub level: u16,
        pub loci: u32,
        /// Loci that hold for `level` days or more now.
        pub hold: u32,
        /// The learning day it was first earned on any device.
        pub unlocked: Option<String>,
    }

    /// This journey on this device (kept in memory only).
    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct SessionView {
        pub xp: u64,
        pub due: u32,
        pub new: u32,
        pub early: u32,
        /// Loci answered again on the same learning day (practice).
        pub repeat: u32,
        /// Right Quiz and Guess answers in a row.
        pub combo: u32,
        /// What this device's own answers and journeys earned since the journey began, in the
        /// order earned: trophies as `game-<id>`, palace medals as `palace-<level>@<palace id>`.
        /// Progress that arrives through a sync is never in here.
        pub unlocked: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct StatusV2Response {
        pub xp: u64,
        pub today: Today,
        pub week: Week,
        pub learning_days: u32,
        pub trophies: Vec<Earned>,
        /// Trophies that can still be earned.
        pub locked: Vec<String>,
        pub palace_medals: Vec<PalaceMedal>,
        pub session: Option<SessionView>,
        /// The one-time explanation was dismissed (on any device).
        pub intro_seen: bool,
        /// Progress from before the sync was taken over (the XP rules changed for this person).
        pub legacy: bool,
    }

    impl Service for StatusV2 {
        const NAME: &'static str = "gamification.status@2";
        type Request = StatusV2Request;
        type Response = StatusV2Response;
    }

    /// A one-time hint was dismissed (`gamification.seen@1`).
    pub struct Seen;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct SeenRequest {
        pub hint: String,
    }

    impl Service for Seen {
        const NAME: &'static str = "gamification.seen@1";
        type Request = SeenRequest;
        type Response = bool;
    }

    /// Takes over this device's progress from before the sync, once (`gamification.migrate@1`).
    /// `days` are the local dates of the learning days in the review log. Answers whether
    /// anything was written.
    pub struct Migrate;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct MigrateRequest {
        pub device: String,
        pub days: Vec<String>,
        /// The progress blob this device kept for itself and never synced (the web app kept the
        /// game in the browser): postcard bytes as the plugin wrote them. It is added to the
        /// shared progress instead of being merged with it.
        pub own: Option<Vec<u8>>,
    }

    impl Service for Migrate {
        const NAME: &'static str = "gamification.migrate@1";
        type Request = MigrateRequest;
        type Response = bool;
    }

    /// The game progress was deleted: `epoch` is the new epoch (`gamification.reset@1`).
    /// Answers how many stored facts were emptied.
    pub struct Reset;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ResetRequest {
        pub epoch: String,
        pub device: String,
    }

    impl Service for Reset {
        const NAME: &'static str = "gamification.reset@1";
        type Request = ResetRequest;
        type Response = u32;
    }

    /// Learning days per week the learner aims for, from `week` (ISO `YYYY-Www`) on
    /// (`gamification.week_goal@1`).
    pub struct WeekGoal;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct WeekGoalRequest {
        pub days: u8,
        pub week: String,
    }

    impl Service for WeekGoal {
        const NAME: &'static str = "gamification.week_goal@1";
        type Request = WeekGoalRequest;
        type Response = u8;
    }
}

/// Services of the `notify` plugin: local notifications. Every plugin sends
/// its notifications through `notify`, so quiet hours, muting and bundling
/// live in one place.
pub mod notify {
    use super::*;

    /// One notification, as shown to the user. `plugin` is filled in by the
    /// host: a sender cannot claim another identity.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Message {
        /// Id of the plugin that sent the notification.
        pub plugin: String,
        /// Channel, e.g. `review.due` or `gamification.streak`.
        pub channel: String,
        /// `0` = informational, `1` = time-sensitive, `2` = urgent.
        pub priority: u8,
        pub title: String,
        pub body: String,
        /// What a tap runs, e.g. `review.start@Planeten`; empty for none.
        pub action: String,
        /// Seconds since the Unix epoch when the message was shown.
        pub shown_at: u64,
    }

    /// A notification waiting to be shown (the plugin's queue).
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Queued {
        pub channel: String,
        pub priority: u8,
        pub title: String,
        pub body: String,
        pub action: String,
        /// Queued at, in seconds since the Unix epoch.
        pub queued_at: u64,
    }

    /// Show a notification now (subject to quiet hours, muting, the daily
    /// per-channel limit and bundling).
    pub struct Send;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct SendRequest {
        /// Host-provided queue time in seconds since the Unix epoch.
        pub now: u64,
        pub channel: String,
        pub priority: u8,
        pub title: String,
        pub body: String,
        /// What a tap runs, e.g. `review.start@Planeten`; empty for none.
        pub action: String,
    }

    impl Service for Send {
        const NAME: &'static str = "notify.send@1";
        type Request = SendRequest;
        /// Whether the message was queued.
        type Response = bool;
    }

    /// One planned notification.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Plan {
        /// Once, at a Unix time in seconds.
        At { at: u64 },
        /// Every day at `seconds` past midnight local time (0–86 399).
        Daily { seconds: u64 },
    }

    /// Schedule a notification under `key`: the same key replaces, `None`
    /// cancels. Answers the stored schedules, sorted by key.
    pub struct Schedule;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ScheduleRequest {
        pub key: String,
        pub channel: String,
        pub priority: u8,
        pub title: String,
        pub body: String,
        pub action: String,
        /// `None` cancels the schedule with this key.
        pub plan: Option<Plan>,
    }

    impl Service for Schedule {
        const NAME: &'static str = "notify.schedule@1";
        type Request = ScheduleRequest;
        type Response = Vec<ScheduleEntry>;
    }

    /// A stored schedule, as answered by [`Schedule`].
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ScheduleEntry {
        pub key: String,
        pub channel: String,
        pub priority: u8,
        pub title: String,
        pub body: String,
        pub action: String,
        pub plan: Plan,
    }

    /// Everything that became due and may now be shown (after quiet hours,
    /// muting and the daily limit). The host calls this at start and whenever
    /// the app is used; the plugin cannot keep time itself.
    pub struct Due;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct DueRequest {
        /// Now, in seconds since the Unix epoch.
        pub now: u64,
        /// Seconds past local midnight (daily plans fire at or after this).
        pub day_seconds: u32,
    }

    impl Service for Due {
        const NAME: &'static str = "notify.due@1";
        type Request = DueRequest;
        /// Messages that may be shown now, oldest first, bundled per channel.
        type Response = Vec<Queued>;
    }

    /// A notification was shown to the user (host → plugin, so the queue can
    /// drop it and the history can keep it). Also the event other plugins
    /// may log.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Displayed {
        /// The message as shown; the host fills in `plugin` and `shown_at`.
        pub message: Message,
    }

    impl Event for Displayed {
        const TOPIC: &'static str = "notify.displayed";
    }

    /// Quiet hours and muted channels.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub struct Settings {
        /// Muted channels (exact names), sorted.
        pub muted: Vec<String>,
        /// Quiet hours as seconds past local midnight; equal means none.
        pub quiet_start: u32,
        pub quiet_end: u32,
    }

    /// Read (empty request) or change the settings; answers the new ones.
    pub struct Configure;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub struct ConfigureRequest {
        /// Mute this channel in addition to the current list.
        pub mute: Option<String>,
        /// Replace the muted list entirely; applied after `mute`.
        pub muted: Option<Vec<String>>,
        /// Quiet hours as `HH:MM-HH:MM` local time; `Some("")` clears.
        pub quiet: Option<String>,
    }

    impl Service for Configure {
        const NAME: &'static str = "notify.configure@1";
        type Request = ConfigureRequest;
        type Response = Settings;
    }

    /// The notification history (newest first), for the bell in the app and
    /// `locora notify ls`.
    pub struct History;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct HistoryRequest {
        /// At most this many entries.
        pub limit: u32,
    }

    impl Service for History {
        const NAME: &'static str = "notify.history@1";
        type Request = HistoryRequest;
        type Response = Vec<Message>;
    }

    /// Parse `HH:MM` into seconds past midnight; `Err` keeps the message.
    pub fn parse_hhmm(value: &str) -> Result<u32, String> {
        let (h, m) = value
            .split_once(':')
            .ok_or_else(|| format!("Ungültige Zeit {value}: HH:MM erwartet"))?;
        let h: u32 = h
            .parse()
            .map_err(|_| format!("Ungültige Stunde in {value}"))?;
        let m: u32 = m
            .parse()
            .map_err(|_| format!("Ungültige Minute in {value}"))?;
        if h > 23 || m > 59 {
            return Err(format!("Ungültige Zeit {value}"));
        }
        Ok(h * 3600 + m * 60)
    }

    impl Settings {
        /// Whether `day_seconds` (past local midnight) falls in quiet hours.
        /// Equal start and end mean no quiet hours; ranges may wrap midnight.
        pub fn is_quiet(&self, day_seconds: u32) -> bool {
            if self.quiet_start == self.quiet_end {
                return false;
            }
            if self.quiet_start < self.quiet_end {
                (self.quiet_start..self.quiet_end).contains(&day_seconds)
            } else {
                // Wraps midnight, e.g. 22:00–08:00.
                day_seconds >= self.quiet_start || day_seconds < self.quiet_end
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn quiet_hours_wrap_midnight() {
            let night = Settings {
                quiet_start: 22 * 3600,
                quiet_end: 8 * 3600,
                ..Default::default()
            };
            assert!(night.is_quiet(23 * 3600));
            assert!(night.is_quiet(3 * 3600));
            assert!(!night.is_quiet(12 * 3600));
            let day = Settings {
                quiet_start: 8 * 3600,
                quiet_end: 17 * 3600,
                ..Default::default()
            };
            assert!(day.is_quiet(8 * 3600));
            assert!(!day.is_quiet(7 * 3600));
            assert!(!Settings::default().is_quiet(0));
            // A range that is the whole day counts as none too (start == end).
            assert!(!Settings::default().is_quiet(86_399));
        }

        #[test]
        fn hhmm_parses() {
            assert_eq!(parse_hhmm("18:00"), Ok(18 * 3600));
            assert_eq!(parse_hhmm("00:00"), Ok(0));
            assert!(parse_hhmm("24:00").is_err());
            assert!(parse_hhmm("8").is_err());
            assert!(parse_hhmm("ab:cd").is_err());
            assert!(parse_hhmm("8:60").is_err());
        }
    }
}

/// Services of the `review` plugin (spaced repetition).
pub mod review {
    use super::*;

    /// Scheduling state of one locus.
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub struct CardState {
        /// Days until recall probability drops to 90 %.
        pub stability: f64,
        /// 1 (easy) … 10 (hard).
        pub difficulty: f64,
        /// Last review, seconds since the Unix epoch.
        pub last_review: u64,
        /// Next review, seconds since the Unix epoch.
        pub due: u64,
        pub reps: u32,
        pub lapses: u32,
    }

    /// A scheduled locus.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ScheduledLocus {
        pub palace_id: String,
        pub locus_id: String,
        pub card: CardState,
    }

    /// Scheduled loci (all, or one palace's), with whether each is due at `now`.
    /// Loci that were never answered are not listed: they are new.
    pub struct Schedule;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ScheduleRequest {
        pub palace_id: Option<String>,
    }

    impl Service for Schedule {
        const NAME: &'static str = "review.schedule@1";
        type Request = ScheduleRequest;
        type Response = Vec<ScheduledLocus>;
    }

    /// Restore cards (e.g. from an imported `.locora` with progress).
    /// Existing cards of the same loci are replaced; the answer is the number restored.
    pub struct Restore;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct RestoreRequest {
        pub palace_id: String,
        /// (locus id, card)
        pub cards: Vec<(String, CardState)>,
    }

    impl Service for Restore {
        const NAME: &'static str = "review.restore@1";
        type Request = RestoreRequest;
        type Response = u32;
    }

    /// One answer in the review log.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct LogEntry {
        pub palace_id: String,
        pub locus_id: String,
        pub grade: crate::core::Grade,
        /// Seconds since the Unix epoch.
        pub answered_at: u64,
        pub elapsed_ms: u64,
    }

    /// The review log (all palaces or one), oldest answer first per palace.
    pub struct Log;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct LogRequest {
        pub palace_id: Option<String>,
    }

    impl Service for Log {
        const NAME: &'static str = "review.log@1";
        type Request = LogRequest;
        type Response = Vec<LogEntry>;
    }

    /// Scheduler settings of the review plugin.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Settings {
        /// Personal FSRS weights; `None` = the defaults.
        pub weights: Option<Vec<f64>>,
        /// Target probability of remembering at review time (0.7–0.99).
        pub desired_retention: f64,
    }

    /// Read (empty request) or change the scheduler settings; answers the new settings.
    pub struct Configure;

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct ConfigureRequest {
        /// `None` = keep, `Some(None)` = back to defaults, `Some(Some(w))` = use `w`.
        pub weights: Option<Option<Vec<f64>>>,
        pub desired_retention: Option<f64>,
    }

    impl Service for Configure {
        const NAME: &'static str = "review.configure@1";
        type Request = ConfigureRequest;
        type Response = Settings;
    }

    impl CardState {
        /// Whether the numbers are usable for scheduling.
        pub fn is_valid(&self) -> bool {
            self.stability.is_finite()
                && self.stability > 0.0
                && self.difficulty.is_finite()
                && (1.0..=10.0).contains(&self.difficulty)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::core::JourneyCompleted;
    use super::*;

    #[test]
    fn events_round_trip() {
        let ev = JourneyCompleted {
            palace_id: "p1".into(),
            correct: 9,
            total: 10,
            duration_secs: 125,
        };
        let bytes = encode(&ev).unwrap();
        assert_eq!(decode::<JourneyCompleted>(&bytes).unwrap(), ev);
    }
}
