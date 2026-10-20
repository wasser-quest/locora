//! Device-local start view and palace thumbnail choices.
use std::collections::HashMap;

const CORE_KEY: &str = "locora-start-in-core-v1";
const COVERS_KEY: &str = "locora-palace-covers-v1";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn in_core() -> bool {
    storage()
        .and_then(|store| store.get_item(CORE_KEY).ok().flatten())
        .as_deref()
        == Some("true")
}

pub fn set_in_core(value: bool) {
    if let Some(store) = storage() {
        let _ = store.set_item(CORE_KEY, if value { "true" } else { "false" });
    }
}

pub fn covers() -> HashMap<String, String> {
    storage()
        .and_then(|store| store.get_item(COVERS_KEY).ok().flatten())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

pub fn set_cover(palace: &str, room: &str) {
    let mut choices = covers();
    choices.insert(palace.to_owned(), room.to_owned());
    if let (Some(store), Ok(json)) = (storage(), serde_json::to_string(&choices)) {
        let _ = store.set_item(COVERS_KEY, &json);
    }
}
