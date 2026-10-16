//! Types shared by the Locora host and every plugin.
//!
//! Events are plain serde structs with a fixed topic. They travel across the
//! WebAssembly boundary encoded with [`postcard`], so both sides only ever see
//! typed Rust values.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Plugin API version implemented by this crate.
pub const API_VERSION: &str = "0.5";

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
