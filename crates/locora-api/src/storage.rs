//! Host services around plugin storage.
use super::*;

/// Lists the calling plugin's own keys that changed after `after_rev`, in the order they
/// changed (`storage.list@1`, reserved for the host; needs the `storage` permission). Changes
/// that a sync applied are listed like local ones. A key written with empty bytes is listed with
/// an empty value. Pass the returned `rev` as `after_rev` next time; `more` says the page was
/// full and another call returns the rest.
pub struct List;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListRequest {
    /// Only keys that start with this (at most [`MAX_PREFIX`] bytes).
    pub prefix: String,
    pub after_rev: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListEntry {
    pub key: String,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListResponse {
    pub entries: Vec<ListEntry>,
    /// The revision of the last change looked at.
    pub rev: u64,
    pub more: bool,
}

impl Service for List {
    const NAME: &'static str = "storage.list@1";
    type Request = ListRequest;
    type Response = ListResponse;
}

/// Longest prefix a plugin may ask for.
pub const MAX_PREFIX: usize = 200;
/// Most entries in one reply.
pub const PAGE: usize = 5000;
