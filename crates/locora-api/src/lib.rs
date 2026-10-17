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
pub const API_VERSION: &str = "0.6";

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
