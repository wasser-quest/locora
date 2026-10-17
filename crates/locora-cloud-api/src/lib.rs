//! Shared types between the Locora app and Locora Cloud: signed entitlements
//! and the JSON request and response bodies of the cloud API.

pub mod api;
pub mod entitlement;

pub use entitlement::{Access, Claims, EntitlementError, OFFLINE_GRACE_SECS, Plan};
