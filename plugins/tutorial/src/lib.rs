//! A small, persistent app tour. The plugin chooses the step; the host checks
//! and forwards declarative highlights to the native UI.
use locora_sdk::api::tutorial::{Tour, TourAction};
use locora_sdk::api::ui::{Highlight, HighlightRequest};
use locora_sdk::prelude::*;

#[locora_plugin(name = "App-Tour", permissions = [storage, ui_highlight])]
struct Tutorial;

const STEPS: &[(&str, &str, &str)] = &[
    (
        "tour-home-media",
        "Ein Bild wird zum Palast",
        "Wähle ein Bild oder Video. Locora legt den Palast an und öffnet direkt den Editor.",
    ),
    (
        "tour-home-import",
        "Vorhandene Paläste mitnehmen",
        "Hier importierst du eine .locora-Datei oder eine locora.toml-Quelle.",
    ),
    (
        "tour-home-settings",
        "Lernen nach deinem Rhythmus",
        "Stelle Tageslimit und Behaltensrate ein und sieh dir deine Lernstatistik an.",
    ),
    (
        "tour-palace-route",
        "Deine Route",
        "Im Tutorial-Palast startest du hier den Weg durch die Orte. Die App-Tour öffnet dafür den Beispielpalast.",
    ),
    (
        "tour-editor-toggle",
        "Orte bearbeiten",
        "Aktiviere Bearbeiten, um Orte im Raum zu setzen und ihre Inhalte zu ändern.",
    ),
    (
        "tour-journey-prompt",
        "Am Ort erinnern",
        "Die Journey fragt einen Ort nach dem anderen ab. Zeige die Antwort und bewerte danach, wie gut du sie wusstest.",
    ),
];

fn show(ctx: &mut Ctx, index: u8) -> Result<bool> {
    let Some((target, title, text)) = STEPS.get(index as usize) else {
        return Ok(false);
    };
    ctx.call_host::<Highlight>(&HighlightRequest {
        target: (*target).into(),
        title: (*title).into(),
        text: (*text).into(),
        step: index + 1,
        total: STEPS.len() as u8,
    })
}

impl Plugin for Tutorial {
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.provide::<Tour>(|ctx, action| {
            let index: u8 = ctx.store().get("tour_step")?.unwrap_or(STEPS.len() as u8);
            let next = match action {
                TourAction::Start => 0,
                TourAction::Resume => index,
                TourAction::Previous => index.saturating_sub(1),
                TourAction::Next => index.saturating_add(1).min(STEPS.len() as u8),
                TourAction::Skip => STEPS.len() as u8,
            };
            ctx.store().set("tour_step", &next)?;
            show(ctx, next)
        })
    }
}
