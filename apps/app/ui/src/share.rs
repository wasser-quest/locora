//! Sharing an achievement, a finished journey or the weekly goal as a picture with a short text,
//! the platform's way (`ShareWays`): the system's share sheet where the browser takes files, else
//! the picture is saved or downloaded and the text copied. The picture is made on this device
//! (share_card.rs); nothing is sent to Locora Cloud.
//!
//! A card never shows an e-mail address, a locus or a palace's content. The person's display name
//! and a palace's name are on it only when the person turns them on for this share; the choice is
//! not remembered.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_CLOUD_STATUS, CMD_SHARE_ART, CMD_SHARE_SEND, CMD_SHARE_WAYS, CloudStatus, Media,
    ShareArtArgs, ShareDone, ShareSendArgs, ShareWay, ShareWays, UiAssets,
};
use wasm_bindgen::JsCast;

use crate::i18n::{t, tf};
use crate::modal::Modal;
use crate::tauri::{NoArgs, call};

thread_local! {
    static WAYS: Cell<Option<ShareWays>> = const { Cell::new(None) };
    /// The graphics drawn for cards, by hash (base64 PNG).
    static ART: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    /// The element the share dialog gives the focus back to.
    static OPENER: RefCell<Option<web_sys::HtmlElement>> = const { RefCell::new(None) };
}

/// How sharing works here; a device that cannot tell saves the picture and copies the text.
pub async fn ways() -> ShareWays {
    if let Some(ways) = WAYS.with(Cell::get) {
        return ways;
    }
    let ways = call::<ShareWays>(CMD_SHARE_WAYS, NoArgs {})
        .await
        .unwrap_or(ShareWays {
            card: ShareWay::Save,
            text_sheet: false,
        });
    WAYS.with(|cell| cell.set(Some(ways)));
    ways
}

/// Copies `text` to the clipboard and says whether it worked. The copy starts at once, before the
/// future is awaited, so it runs inside the click that asked for it (browsers allow it only there).
pub fn copy_text(text: &str) -> impl std::future::Future<Output = bool> + use<> {
    let started = web_sys::window()
        .and_then(|window| js_sys::Reflect::get(&window.navigator(), &"clipboard".into()).ok())
        .filter(|clipboard| !clipboard.is_undefined())
        .and_then(|clipboard| {
            let write = js_sys::Reflect::get(&clipboard, &"writeText".into()).ok()?;
            let write: js_sys::Function = write.dyn_into().ok()?;
            let promise = write.call1(&clipboard, &text.into()).ok()?;
            promise.dyn_into::<js_sys::Promise>().ok()
        });
    async move {
        match started {
            Some(promise) => wasm_bindgen_futures::JsFuture::from(promise).await.is_ok(),
            None => false,
        }
    }
}

/// What a card shows. Never an e-mail address, a palace's content or a locus.
#[derive(Clone, Debug, PartialEq)]
pub enum CardKind {
    /// An achievement: a journey trophy (`game-*`), a palace medal (`palace-<level>`, with its
    /// palace's name) or one of the app's. `day` is `YYYY-MM-DD` when known.
    Achievement {
        id: String,
        palace: Option<String>,
        day: Option<String>,
    },
    /// A finished journey; `palace` when it walked one palace only.
    Journey {
        correct: u32,
        asked: u32,
        xp: Option<u64>,
        palace: Option<String>,
    },
    /// The weekly goal, reached: learning days this week and weeks in a row.
    Week { days: u32, streak: u32 },
}

impl CardKind {
    /// An achievement from its id as a celebration or a journey has it: a palace medal comes as
    /// `palace-<level>@<palace name>`.
    pub fn achievement(id: &str, day: Option<String>) -> Self {
        let (id, palace) = match id.split_once('@') {
            Some((medal, palace)) => (medal.to_owned(), Some(palace.to_owned())),
            None => (id.to_owned(), None),
        };
        Self::Achievement { id, palace, day }
    }
}

/// What the person chose for this share (not remembered).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Choice {
    pub show_name: bool,
    pub name: String,
    pub show_palace: bool,
}

/// The picture's lines.
#[derive(Clone, Debug, PartialEq)]
pub struct CardText {
    pub heading: String,
    pub title: String,
    pub detail: Option<String>,
    pub footer: String,
}

/// Characters of a name on a card (as many as a display name has).
const NAME_MAX: usize = 32;

/// The palace a card may name, if the person turns it on.
pub fn palace_of(kind: &CardKind) -> Option<&str> {
    match kind {
        CardKind::Achievement { palace, .. } | CardKind::Journey { palace, .. } => {
            palace.as_deref().filter(|name| !name.trim().is_empty())
        }
        CardKind::Week { .. } => None,
    }
}

/// The level of a palace medal (`palace-30` → `30`).
fn level(id: &str) -> Option<&str> {
    id.strip_prefix("palace-")
}

/// The card's name in the choice of cards.
pub fn label(kind: &CardKind) -> String {
    match kind {
        CardKind::Journey { .. } => t("Diese Journey").to_owned(),
        CardKind::Week { .. } => t("Wochenziel").to_owned(),
        CardKind::Achievement { .. } => card_text(kind, &Choice::default(), "").title,
    }
}

/// The picture's lines; `date` is the localized day (empty: none).
pub fn card_text(kind: &CardKind, choice: &Choice, date: &str) -> CardText {
    let palace = palace_of(kind).filter(|_| choice.show_palace);
    let (heading, title, detail) = match kind {
        CardKind::Achievement { id, .. } => match level(id) {
            Some(level) => {
                let level = crate::achievements::palace_level_title(level).unwrap_or_default();
                let title = match palace {
                    Some(palace) => format!("{level} · {palace}"),
                    None => level.to_owned(),
                };
                (t("Palastmedaille"), title, None)
            }
            None => {
                let title = crate::achievements::describe(id)
                    .map(|(title, _)| title)
                    .unwrap_or_default();
                let heading = if id.starts_with("game-") {
                    t("Journey-Trophäe")
                } else {
                    t("Erfolg")
                };
                (heading, title, None)
            }
        },
        CardKind::Journey {
            correct, asked, xp, ..
        } => {
            let mut parts = Vec::new();
            if let Some(xp) = xp {
                parts.push(tf("+{0} XP", &[xp]));
            }
            if let Some(palace) = palace {
                parts.push(tf("Palast „{0}“", &[&palace]));
            }
            (
                t("Journey geschafft"),
                tf("{0} von {1} gewusst", &[correct, asked]),
                (!parts.is_empty()).then(|| parts.join(" · ")),
            )
        }
        CardKind::Week { days, streak } => (
            t("Wochenziel erreicht"),
            if *streak >= 2 {
                tf("{0} Wochen in Folge", &[streak])
            } else if *days == 1 {
                t("An 1 Tag gelernt").to_owned()
            } else {
                tf("An {0} Tagen gelernt", &[days])
            },
            None,
        ),
    };
    let name: String = choice.name.trim().chars().take(NAME_MAX).collect();
    let footer = [
        date.to_owned(),
        if choice.show_name {
            name
        } else {
            String::new()
        },
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    CardText {
        heading: heading.to_owned(),
        title,
        detail,
        footer,
    }
}

/// The message that goes with the picture. The display name is never in it: the person sends it
/// themselves.
pub fn share_text(kind: &CardKind, choice: &Choice) -> String {
    let palace = palace_of(kind).filter(|_| choice.show_palace);
    match kind {
        CardKind::Achievement { id, .. } => match level(id) {
            Some(level) => {
                let level = crate::achievements::palace_level_title(level).unwrap_or_default();
                match palace {
                    Some(palace) => tf(
                        "Mein Gedächtnispalast „{1}“ hat in Locora die Medaille „{0}“ bekommen.",
                        &[&level, &palace],
                    ),
                    None => tf(
                        "Einer meiner Gedächtnispaläste hat in Locora die Medaille „{0}“ bekommen.",
                        &[&level],
                    ),
                }
            }
            None => {
                let title = crate::achievements::describe(id)
                    .map(|(title, _)| title)
                    .unwrap_or_default();
                if id.starts_with("game-") {
                    tf(
                        "Ich habe in Locora die Journey-Trophäe „{0}“ bekommen.",
                        &[&title],
                    )
                } else {
                    tf("Ich habe in Locora den Erfolg „{0}“ geschafft.", &[&title])
                }
            }
        },
        CardKind::Journey {
            correct, asked, xp, ..
        } => {
            let done = match xp {
                Some(xp) => tf(
                    "Journey in Locora geschafft: {0} von {1} gewusst, +{2} XP.",
                    &[correct, asked, xp],
                ),
                None => tf(
                    "Journey in Locora geschafft: {0} von {1} gewusst.",
                    &[correct, asked],
                ),
            };
            match palace {
                Some(palace) => format!("{done} {}", tf("Im Palast „{0}“.", &[&palace])),
                None => done,
            }
        }
        CardKind::Week { days, streak } => {
            if *streak >= 2 {
                tf(
                    "Mein Wochenziel in Locora: {0} Wochen in Folge erreicht.",
                    &[streak],
                )
            } else if *days == 1 {
                t("Wochenziel in Locora erreicht: an 1 Tag gelernt.").to_owned()
            } else {
                tf(
                    "Wochenziel in Locora erreicht: an {0} Tagen gelernt.",
                    &[days],
                )
            }
        }
    }
}

/// The picture described for a screen reader.
pub fn alt_text(text: &CardText) -> String {
    [
        Some(text.heading.as_str()),
        Some(text.title.as_str()),
        text.detail.as_deref(),
        Some(text.footer.as_str()),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" – ")
}

/// The picture's file name: never a palace's name.
pub fn file_name(kind: &CardKind) -> String {
    match kind {
        CardKind::Achievement { id, .. } => {
            let id: String = id
                .split('@')
                .next()
                .unwrap_or_default()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            format!("locora-{}.png", id.to_ascii_lowercase())
        }
        CardKind::Journey { .. } => "locora-journey.png".into(),
        CardKind::Week { .. } => "locora-week.png".into(),
    }
}

/// The graphic on the card: the medal, the journey's „geschafft“, the week's series.
pub fn art(kind: &CardKind, art: &UiAssets) -> Option<Media> {
    match kind {
        CardKind::Achievement { id, .. } => crate::achievements::medal_for(art, id, true),
        CardKind::Journey { .. } => Some(art.done.clone()),
        CardKind::Week { .. } => Some(art.streak.clone()),
    }
}

/// Today as `YYYY-MM-DD` (local), for an achievement celebrated now.
pub fn today() -> String {
    let now = js_sys::Date::new_0();
    format!(
        "{:04}-{:02}-{:02}",
        now.get_full_year(),
        now.get_month() + 1,
        now.get_date()
    )
}

/// A day as the person reads it: `YYYY-MM-DD` read as local noon (no shift across time zones),
/// today without one.
fn date_text(day: Option<&str>) -> String {
    let date = match day {
        Some(day) => {
            let mut parts = day.split('-').filter_map(|part| part.parse::<i32>().ok());
            match (parts.next(), parts.next(), parts.next()) {
                (Some(year), Some(month), Some(day)) => {
                    js_sys::Date::new_with_year_month_day_hr_min_sec(
                        u32::try_from(year).unwrap_or(1970),
                        month - 1,
                        day,
                        12,
                        0,
                        0,
                    )
                }
                _ => return String::new(),
            }
        }
        None => js_sys::Date::new_0(),
    };
    let options = js_sys::Object::new();
    for (key, value) in [("year", "numeric"), ("month", "long"), ("day", "numeric")] {
        let _ = js_sys::Reflect::set(&options, &key.into(), &value.into());
    }
    date.to_locale_date_string(crate::i18n::current().code(), &options)
        .into()
}

/// The cards the dialog offers: one, or a few to choose from (the end of a journey).
#[derive(Clone, Debug, PartialEq)]
pub struct ShareRequest {
    pub cards: Vec<CardKind>,
}

/// The share dialog's request; `None` while it is closed. Provided by `App`.
#[derive(Clone, Copy)]
pub struct Sharing(pub RwSignal<Option<ShareRequest>>);

/// Opens the share dialog with `cards`; `opener` gets the focus back when it closes.
pub fn open(cards: Vec<CardKind>, opener: Option<web_sys::HtmlElement>) {
    if cards.is_empty() {
        return;
    }
    if let Some(Sharing(request)) = use_context::<Sharing>() {
        OPENER.with(|cell| *cell.borrow_mut() = opener);
        request.set(Some(ShareRequest { cards }));
    }
}

/// The graphic `media` as a PNG for the card (base64), drawn once per run.
async fn art_png(media: &Media) -> Result<String, String> {
    if let Some(png) = ART.with(|art| art.borrow().get(&media.hash).cloned()) {
        return Ok(png);
    }
    let png = call::<String>(
        CMD_SHARE_ART,
        ShareArtArgs {
            hash: media.hash.clone(),
        },
    )
    .await?;
    ART.with(|art| art.borrow_mut().insert(media.hash.clone(), png.clone()));
    Ok(png)
}

/// The share dialog, mounted once in `App`.
#[component]
pub fn ShareDialog() -> impl IntoView {
    let Sharing(request) = expect_context::<Sharing>();
    move || {
        request.get().map(|request| {
            let opener = OPENER.with(|cell| cell.borrow_mut().take());
            view! { <ShareCard cards=request.cards opener=opener /> }
        })
    }
}

/// What the dialog says after an action.
#[derive(Clone, PartialEq)]
enum Said {
    Done(String),
    Problem(String),
}

#[component]
fn ShareCard(cards: Vec<CardKind>, opener: Option<web_sys::HtmlElement>) -> impl IntoView {
    let Sharing(request) = expect_context::<Sharing>();
    let assets = crate::art();
    let first = cards[0].clone();
    let several = cards.len() > 1;
    let labels: Vec<String> = cards.iter().map(label).collect();
    let cards = StoredValue::new(cards);
    let chosen = RwSignal::new(0_usize);
    let choice = RwSignal::new(Choice::default());
    // Something was chosen or typed: a tap beside the dialog does not throw it away.
    let touched = RwSignal::new(false);
    let ways = RwSignal::new(None::<ShareWays>);
    let picture = RwSignal::new(None::<Result<String, String>>);
    let drawing = RwSignal::new(true);
    let said = RwSignal::new(None::<Said>);
    let busy = RwSignal::new(false);
    spawn_local(async move {
        ways.try_set(Some(self::ways().await));
        // The own display name, to put on the card when the person turns it on.
        if let Ok(status) = call::<CloudStatus>(CMD_CLOUD_STATUS, NoArgs {}).await
            && let Some(name) = status.display_name
        {
            choice.try_update(|choice| {
                if choice.name.is_empty() {
                    choice.name = name;
                }
            });
        }
    });
    let kind = Memo::new(move |_| {
        let index = chosen.get();
        cards.with_value(|cards| cards.get(index).cloned().unwrap_or_else(|| first.clone()))
    });
    let text = Memo::new(move |_| share_text(&kind.get(), &choice.get()));
    let card = Memo::new(move |_| {
        let kind = kind.get();
        let date = match &kind {
            // An achievement from before the sync has no known day: no date then.
            CardKind::Achievement { day: None, .. } => String::new(),
            CardKind::Achievement { day: Some(day), .. } => date_text(Some(day)),
            _ => date_text(None),
        };
        card_text(&kind, &choice.get(), &date)
    });
    // The picture is drawn again 150 ms after the last change of a choice.
    let generation = StoredValue::new(0_u32);
    Effect::new(move |_| {
        let text = card.get();
        let media = assets.with(|assets| {
            assets
                .as_ref()
                .and_then(|assets| art(&kind.get(), assets).map(|art| (art, assets.logo.clone())))
        });
        let now = generation.get_value().wrapping_add(1);
        generation.set_value(now);
        drawing.set(true);
        set_timeout(
            move || {
                if generation.try_get_value() != Some(now) {
                    return;
                }
                spawn_local(async move {
                    let result = match media {
                        Some((media, logo)) => match art_png(&media).await {
                            Ok(png) => {
                                let logo = art_png(&logo).await.ok();
                                crate::share_card::render(&text, &png, logo.as_deref()).await
                            }
                            Err(error) => Err(error),
                        },
                        None => Err(String::new()),
                    };
                    if generation.try_get_value() == Some(now) {
                        picture.try_set(Some(
                            result.map_err(|_| {
                                t("Das Bild konnte nicht erstellt werden.").to_owned()
                            }),
                        ));
                        drawing.try_set(false);
                    }
                });
            },
            std::time::Duration::from_millis(150),
        );
    });
    let close = Callback::new(move |()| request.set(None));
    let copied_note = |copied: bool| {
        if copied {
            t("Der Text ist in der Zwischenablage.")
        } else {
            t("Kopieren nicht möglich. Bitte den Text oben markieren und kopieren.")
        }
    };
    let send = move |_| {
        let (Some(ways), Some(Ok(url))) = (ways.get_untracked(), picture.get_untracked()) else {
            return;
        };
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        said.set(None);
        let text = text.get_untracked();
        // Started in the click: browsers allow the clipboard only there.
        let copied = (ways.card != ShareWay::Sheet).then(|| copy_text(&text));
        let args = ShareSendArgs {
            name: file_name(&kind.get_untracked()),
            png: url.split_once(',').map(|(_, png)| png.to_owned()),
            text,
            title: "Locora".into(),
        };
        spawn_local(async move {
            let copied = match copied {
                Some(copied) => copied.await,
                None => false,
            };
            let note = match call::<ShareDone>(CMD_SHARE_SEND, args).await {
                Ok(ShareDone::Shared) => Some(Said::Done(t("Geteilt.").into())),
                Ok(ShareDone::Saved) => Some(Said::Done(format!(
                    "{} {}",
                    t("Bild gespeichert."),
                    copied_note(copied)
                ))),
                Ok(ShareDone::Downloaded) => Some(Said::Done(format!(
                    "{} {}",
                    t("Bild heruntergeladen."),
                    copied_note(copied)
                ))),
                Ok(ShareDone::Cancelled | ShareDone::Copy) => None,
                Err(error) => Some(Said::Problem(error)),
            };
            said.try_set(note);
            busy.try_set(false);
        });
    };
    let copy = move |_| {
        let copied = copy_text(&text.get_untracked());
        spawn_local(async move {
            said.try_set(Some(if copied.await {
                Said::Done(t("Kopiert.").into())
            } else {
                Said::Problem(
                    t("Kopieren nicht möglich. Bitte den Text oben markieren und kopieren.").into(),
                )
            }));
        });
    };
    let palace = move || palace_of(&kind.get()).map(str::to_owned);
    view! {
        <Modal labelled="share-title" class="share-dialog" opener=opener on_close=close
            dirty=Signal::derive(move || touched.get())>
            <div class="modal-head">
                <h2 id="share-title" tabindex="-1">{t("Teilen")}</h2>
                <button type="button" class="icon" aria-label=t("Schließen") title=t("Schließen")
                    on:click=move |_| close.run(())>"✕"</button>
            </div>
            {several.then(|| if labels.len() <= 4 {
                view! {
                    <fieldset class="share-cards">
                        <legend>{t("Was möchtest du teilen?")}</legend>
                        {labels.into_iter().enumerate().map(|(index, label)| view! {
                            <label class="input check">
                                <input type="radio" name="share-card" prop:checked=move || chosen.get() == index
                                    on:change=move |_| {
                                        chosen.set(index);
                                        touched.set(true);
                                    } />
                                {label}
                            </label>
                        }).collect_view()}
                    </fieldset>
                }.into_any()
            } else {
                view! {
                    <label class="input">
                        <span>{t("Was möchtest du teilen?")}</span>
                        <select on:change=move |event| {
                            if let Ok(index) = event_target_value(&event).parse::<usize>() {
                                chosen.set(index);
                                touched.set(true);
                            }
                        }>
                            {labels.into_iter().enumerate().map(|(index, label)| view! {
                                <option value=index.to_string() prop:selected=move || chosen.get() == index>{label}</option>
                            }).collect_view()}
                        </select>
                    </label>
                }.into_any()
            })}
            <label class="input check">
                <input type="checkbox" prop:checked=move || choice.with(|c| c.show_name)
                    on:change=move |event| {
                        let on = event_target_checked(&event);
                        choice.update(|c| c.show_name = on);
                        touched.set(true);
                    } />
                {t("Meinen Namen zeigen")}
            </label>
            <Show when=move || choice.with(|c| c.show_name)>
                <label class="input keep-in-view">
                    <span>{t("Name auf dem Bild")}</span>
                    <input type="text" maxlength="32" prop:value=move || choice.with(|c| c.name.clone())
                        on:input=move |event| {
                            let name = event_target_value(&event);
                            choice.update(|c| c.name = name);
                            touched.set(true);
                        }
                        on:focus=move |event| crate::keep_in_view(event_target(&event)) />
                </label>
            </Show>
            {move || palace().map(|palace| view! {
                <label class="input check">
                    <input type="checkbox" prop:checked=move || choice.with(|c| c.show_palace)
                        on:change=move |event| {
                            let on = event_target_checked(&event);
                            choice.update(|c| c.show_palace = on);
                            touched.set(true);
                        } />
                    {tf("Palastnamen „{0}“ zeigen", &[&palace])}
                </label>
            })}
            <div class="share-preview">
                {move || match picture.get() {
                    Some(Ok(url)) => view! { <img src=url alt=move || alt_text(&card.get()) /> }.into_any(),
                    Some(Err(error)) => view! { <p class="notes" role="alert">{error}</p> }.into_any(),
                    None => view! { <p class="progress">{t("Bild wird erstellt …")}</p> }.into_any(),
                }}
            </div>
            <label class="input">
                <span>{t("Text zum Bild")}</span>
                <textarea rows="3" readonly prop:value=move || text.get()></textarea>
            </label>
            <div class="share-actions modal-foot">
                <button type="button" class="primary"
                    disabled=move || drawing.get() || busy.get() || !matches!(picture.get(), Some(Ok(_)))
                    on:click=send>
                    {move || match ways.get().map(|ways| ways.card) {
                        Some(ShareWay::Sheet) => t("Teilen…"),
                        Some(ShareWay::Download) => t("Bild herunterladen"),
                        _ => t("Bild speichern…"),
                    }}
                </button>
                <button type="button" on:click=copy>{t("Text kopieren")}</button>
            </div>
            <div class="friends-said" aria-live="polite">
                {move || drawing.get().then(|| view! { <p class="progress">{t("Bild wird erstellt …")}</p> })}
                {move || said.get().map(|said| match said {
                    Said::Done(text) => view! { <p class="status">{text}</p> }.into_any(),
                    Said::Problem(text) => view! { <p class="notes" role="alert">{text}</p> }.into_any(),
                })}
            </div>
            <p class="notes">{t("Das Bild entsteht auf diesem Gerät. Dein Name und Palastnamen stehen nur darauf, wenn du sie oben einschaltest.")}</p>
        </Modal>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palace_medal() -> CardKind {
        CardKind::achievement("palace-30@Küche", Some("2026-10-20".into()))
    }

    fn journey(xp: Option<u64>) -> CardKind {
        CardKind::Journey {
            correct: 7,
            asked: 9,
            xp,
            palace: Some("Küche".into()),
        }
    }

    #[test]
    fn a_card_shows_no_palace_or_name_unless_chosen() {
        crate::i18n::set_current(crate::i18n::Lang::De);
        let off = Choice {
            show_name: false,
            name: "Lea".into(),
            show_palace: false,
        };
        for kind in [palace_medal(), journey(Some(12))] {
            let card = card_text(&kind, &off, "20. Oktober 2026");
            let all = [
                format!("{card:?}"),
                share_text(&kind, &off),
                alt_text(&card),
                file_name(&kind),
            ];
            for text in &all {
                assert!(!text.contains("Küche") && !text.contains("Lea"), "{text}");
                assert!(!text.contains('@'), "{text}");
            }
            let palace = Choice {
                show_palace: true,
                ..off.clone()
            };
            let card = card_text(&kind, &palace, "");
            assert!(format!("{card:?}").contains("Küche"));
            assert!(share_text(&kind, &palace).contains("Küche"));
            assert!(!file_name(&kind).contains("che"), "{}", file_name(&kind));
            let named = Choice {
                show_name: true,
                ..off.clone()
            };
            let card = card_text(&kind, &named, "20. Oktober 2026");
            assert_eq!(card.footer, "20. Oktober 2026 · Lea");
            assert!(!card.title.contains("Lea"));
            assert!(!share_text(&kind, &named).contains("Lea"));
        }
        assert_eq!(file_name(&palace_medal()), "locora-palace-30.png");
        assert_eq!(
            file_name(&CardKind::achievement("game-seven-day-streak", None)),
            "locora-game-seven-day-streak.png"
        );
        assert_eq!(card_text(&palace_medal(), &off, "").title, "Gefestigt");
        assert_eq!(
            card_text(&palace_medal(), &off, "").heading,
            "Palastmedaille"
        );
        // A long name is cut to what a display name may hold.
        let long = Choice {
            show_name: true,
            name: "x".repeat(80),
            show_palace: false,
        };
        assert_eq!(
            card_text(&journey(None), &long, "").footer.chars().count(),
            32
        );
    }

    #[test]
    fn share_texts_read_in_the_singular_where_one() {
        let off = Choice::default();
        crate::i18n::set_current(crate::i18n::Lang::En);
        let week = |days, streak| share_text(&CardKind::Week { days, streak }, &off);
        assert_eq!(
            week(1, 1),
            "Weekly goal reached in Locora: learned on 1 day."
        );
        assert_eq!(
            week(3, 0),
            "Weekly goal reached in Locora: learned on 3 days."
        );
        assert_eq!(
            week(4, 5),
            "My weekly goal in Locora: reached 5 weeks in a row."
        );
        assert_eq!(
            card_text(&CardKind::Week { days: 1, streak: 1 }, &off, "").title,
            "Learned on 1 day"
        );
        assert_eq!(
            share_text(&journey(Some(12)), &off),
            "Journey done in Locora: 7 of 9 recalled, +12 XP."
        );
        assert_eq!(
            share_text(&journey(None), &off),
            "Journey done in Locora: 7 of 9 recalled."
        );
        assert_eq!(
            share_text(
                &journey(None),
                &Choice {
                    show_palace: true,
                    ..off.clone()
                }
            ),
            "Journey done in Locora: 7 of 9 recalled. In the palace “Küche”."
        );
        assert_eq!(
            share_text(&CardKind::achievement("game-first-journey", None), &off),
            "I earned the journey trophy “First journey” in Locora."
        );
        assert_eq!(
            card_text(&journey(Some(12)), &off, "").detail.as_deref(),
            Some("+12 XP")
        );
        crate::i18n::set_current(crate::i18n::Lang::De);
        assert_eq!(
            week(2, 2),
            "Mein Wochenziel in Locora: 2 Wochen in Folge erreicht."
        );
        assert_eq!(
            week(1, 0),
            "Wochenziel in Locora erreicht: an 1 Tag gelernt."
        );
        crate::i18n::set_current(crate::i18n::Lang::Fr);
        let french = share_text(&journey(None), &off);
        assert!(french.contains("\u{202f}:"), "{french}");
        crate::i18n::set_current(crate::i18n::Lang::De);
    }
}
