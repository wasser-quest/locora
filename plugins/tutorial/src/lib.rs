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
        "Dein eigener Gedächtnispalast",
        "Ein Gedächtnispalast ist ein Ort, den du gut kennst, zum Beispiel deine Wohnung. Normalerweise wählst du hier ein Foto davon. Jetzt baut Simo dir einen Beispiel-Palast: Schau zu und tippe auf „Weiter“. Du darfst dabei alles anklicken und ausprobieren.",
    ),
    (
        "tour-demo-room",
        "Aus einem Bild wird ein Raum",
        "Simo hat ein Bild gewählt: eine Eingangshalle. Daraus ist der Palast „Simos Beispiel“ mit einem Raum geworden. Mit „+ Raum aus Bild oder Video“ kommen weitere Räume dazu.",
    ),
    (
        "tour-editor-adding",
        "„Ort setzen“ einschalten",
        "Damit ein Tipp auf das Bild einen Ort setzt, schaltest du „Ort setzen“ ein. Simo hat es für dich eingeschaltet.",
    ),
    (
        "tour-editor-stage",
        "Auf das Bild tippen",
        "Simo hat auf die Tür getippt. Dort ist jetzt Ort 1, der runde Punkt mit der Nummer. Die Nummern sind die Reihenfolge, in der du später durch den Palast gehst.",
    ),
    (
        "tour-editor-place",
        "Feld „Ort“: Wo ist es?",
        "Hier steht, wo der Ort ist: „Tür“. Das ist später die Frage.",
    ),
    (
        "tour-editor-content",
        "Feld „Inhalt“: Was merkst du dir?",
        "Hier steht, was du dir an dieser Stelle merken willst: „Schlüssel“. Das ist später die Antwort.",
    ),
    (
        "tour-editor-note",
        "Feld „Merkhilfe“ (freiwillig)",
        "Ein verrücktes Bild im Kopf hilft beim Merken: „Ein riesiger goldener Schlüssel steckt quer in der Tür.“ Locora zeigt es dir nach der Antwort. Das Feld darf auch leer bleiben.",
    ),
    (
        "tour-editor-answer",
        "Antwortart: darf so bleiben",
        "„Recall“ passt fast immer: Du erinnerst dich, deckst auf und sagst ehrlich, wie gut es ging. Bei „Guess“ tippst du die Antwort ein, bei „Quiz“ wählst du aus mehreren.",
    ),
    (
        "tour-palace-route",
        "Durch den Palast gehen",
        "Fertig gebaut, und alles ist schon gespeichert. Mit „Route ablaufen“ gehst du deinen Palast Ort für Ort ab, in der Reihenfolge der Nummern.",
    ),
    (
        "tour-journey-prompt",
        "Am Ort erinnern",
        "Locora zeigt dir den Ort: „Tür“. Stell dir die Tür vor. Was liegt dort?",
    ),
    (
        "tour-journey-reveal",
        "Aufdecken",
        "Wenn du es weißt (oder nicht), tippe auf „Aufdecken“ oder drücke die Leertaste. Dann siehst du die Antwort.",
    ),
    (
        "tour-journey-grade",
        "Ehrlich bewerten",
        "Sag, wie gut du es wusstest: „Nochmal“ heißt gar nicht, „Leicht“ heißt sofort. Was du schlecht wusstest, fragt Locora bald wieder. Danach geht es zum nächsten Ort.",
    ),
    (
        "tour-home-media",
        "Jetzt du!",
        "Mach ein Foto von deiner Wohnung und wähle es hier. Dann baust du deinen eigenen Palast, genau wie Simo. Simos Beispiel findest du bei deinen Palästen: behalten oder löschen.",
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
