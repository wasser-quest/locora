//! Device-local choices for how route markers appear while learning.
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const KEY: &str = "locora-locus-display-v1";
const LEADS_KEY: &str = "locora-locus-video-leads-v1";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub previous: u8,
    pub next: u8,
    pub numbers: bool,
    pub hide_revealed: bool,
    /// Keep the revealed pictures of earlier loci on the stage, in place of their markers.
    pub keep_previous: bool,
    /// Grade a recall right after revealing it, and move on after an answer.
    pub quick_next: bool,
    pub video_lead: f64,
    /// Reverse journeys: how far from the locus a click may land, as a share of the stage width.
    pub reverse_radius: f64,
    /// Show the XP, combo and hearts strip in journeys.
    pub gamification: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            // Neighbouring loci and the earlier pictures show by default; the numbers stay off,
            // so one has to recall what the locus was at that place. quick_next stays off:
            // it grades a reveal by itself.
            previous: 3,
            next: 3,
            numbers: false,
            hide_revealed: true,
            keep_previous: true,
            quick_next: false,
            video_lead: 0.0,
            reverse_radius: 0.1,
            gamification: true,
        }
    }
}

#[derive(Clone, Copy)]
pub struct DisplaySettings(pub RwSignal<Preferences>);

#[derive(Clone, Copy)]
pub struct LeadOverrides(pub RwSignal<HashMap<String, f64>>);

pub fn signal() -> RwSignal<Preferences> {
    expect_context::<DisplaySettings>().0
}

pub fn leads() -> RwSignal<HashMap<String, f64>> {
    expect_context::<LeadOverrides>().0
}

pub fn load_leads() -> HashMap<String, f64> {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(LEADS_KEY).ok().flatten())
        .and_then(|value| serde_json::from_str::<HashMap<String, f64>>(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|(id, lead)| !id.is_empty() && lead.is_finite() && (0.0..=30.0).contains(lead))
        .collect()
}

pub fn save_leads(leads: &HashMap<String, f64>) {
    if let Some(storage) =
        web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    {
        if let Ok(json) = serde_json::to_string(leads) {
            let _ = storage.set_item(LEADS_KEY, &json);
        }
    }
}

pub fn load() -> Preferences {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(KEY).ok().flatten())
        .and_then(|value| serde_json::from_str::<Preferences>(&value).ok())
        .map(|value| Preferences {
            previous: value.previous.min(10),
            next: value.next.min(10),
            numbers: value.numbers,
            hide_revealed: value.hide_revealed,
            keep_previous: value.keep_previous,
            quick_next: value.quick_next,
            gamification: value.gamification,
            reverse_radius: if value.reverse_radius.is_finite() {
                value.reverse_radius.clamp(0.02, 0.3)
            } else {
                0.1
            },
            video_lead: if value.video_lead.is_finite() {
                value.video_lead.clamp(0.0, 30.0)
            } else {
                0.0
            },
        })
        .unwrap_or_default()
}

pub fn save(value: Preferences) {
    if let Some(storage) =
        web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    {
        if let Ok(json) = serde_json::to_string(&value) {
            let _ = storage.set_item(KEY, &json);
        }
    }
}

pub fn visible(offset: i32, previous: u8, next: u8) -> bool {
    offset >= -(previous as i32) && offset <= next as i32
}

pub fn at_video_time(locus_time: Option<f64>, playhead: f64, lead: f64) -> bool {
    locus_time.is_none_or(|time| {
        time.is_finite()
            && time >= 0.0
            && playhead >= (time - lead).max(0.0)
            && playhead <= time + 0.5
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_window_and_video_lead() {
        assert!(visible(-2, 2, 1));
        assert!(visible(1, 2, 1));
        assert!(!visible(2, 2, 1));
        assert!(!at_video_time(Some(10.0), 8.9, 1.0));
        assert!(at_video_time(Some(10.0), 9.0, 1.0));
        assert!(!at_video_time(Some(10.0), 10.6, 1.0));
    }
}
