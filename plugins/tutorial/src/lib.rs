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
        "Wähle ein Bild oder Video. Locora legt daraus einen Palast mit einem Raum an und öffnet direkt den Editor.",
    ),
    (
        "tour-editor-toggle",
        "Bearbeiten einschalten",
        "Aktiviere „Bearbeiten“, damit du Orte im Bild setzen und verändern kannst.",
    ),
    (
        "tour-editor-stage",
        "Orte setzen",
        "Klicke auf das Bild dort, wo etwas liegen soll. Jeder Klick setzt einen Ort, den du dir merken willst.",
    ),
    (
        "tour-editor-content",
        "Inhalt eintragen",
        "Klicke einen Ort an. Im Feld Ort beschreibst du den Platz, im Feld Inhalt das, was du dir merken willst – zum Beispiel „Haustür“ und „Schlüssel“.",
    ),
    (
        "tour-editor-answer",
        "Antwortart wählen",
        "Wähle den Ort und stelle die Antwortart ein. Recall zeigt den Ort, und du bewertest dich selbst. Guess fragt eine Eingabe ab, Quiz bietet mehrere Antworten an.",
    ),
    (
        "tour-palace-route",
        "Deine Route",
        "Mit „Route ablaufen“ gehst du alle Orte nacheinander durch. Die App fragt sie ab und merkt sich, was du wusstest.",
    ),
    (
        "tour-journey-prompt",
        "Am Ort erinnern",
        "Die Journey fragt einen Ort nach dem anderen ab. Zeige die Antwort und bewerte danach, wie gut du sie wusstest.",
    ),
    (
        "tour-home-settings",
        "Lernen nach deinem Rhythmus",
        "Stelle Tageslimit und Behaltensrate ein und sieh dir deine Lernstatistik an.",
    ),
    (
        "tour-home-import",
        "Vorhandene Paläste mitnehmen",
        "Hier importierst du eine .locora-Datei oder eine locora.toml-Quelle.",
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
