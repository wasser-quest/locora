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
pub const API_VERSION: &str = "0.1";

/// A typed event with a stable topic name.
pub trait Event: Serialize + DeserializeOwned {
    /// Topic the event is published on, namespaced by the owning plugin.
    const TOPIC: &'static str;
}

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
