//! Locora app UI: start screen, palace with rooms and markers, journeys.

mod achievements;
mod activity;
mod audio;
mod auto_sync;
mod cloud_settings;
mod cloud_sync_form;
mod crash;
mod dashboard;
mod dashboard_editor;
mod editor;
mod first_palace;
mod formulas;
mod i18n;
mod i18n_catalog;
mod info;
mod journey;
mod language_choice;
mod learn;
mod learn_content;
mod learning_path;
mod locus_display;
mod look;
mod media_format;
mod palette;
mod settings;
mod sply;
mod startup;
#[cfg(test)]
mod store_rules;
mod tauri;
mod theme;
mod tour_gate;
mod video_frames;

use crate::i18n::{t, tf};
use crate::info::Info;
use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_EDITOR_CHANGE, CMD_FORMULA_GUIDE, CMD_GAMIFICATION_STATUS, CMD_HOME, CMD_PALACE, CMD_TOUR,
    CMD_TOUR_DEMO, CMD_TUTORIAL, CMD_UI_ASSETS, EditorAction, EditorChange, EditorUpdated,
    GamificationStatus, HighlightRequest, Home, JourneyStart, LanguageArgs, LocusContent, Media,
    NotificationView, PalaceCard, PalaceView, TourAction, UiAssets,
};
use serde::Serialize;

use crate::sply::{Shown, Sply, VideoThumb};
use crate::tauri::call;

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Home,
    Core,
    Palace(String),
    Imported(PalaceView),
    Journey(JourneyStart),
    Settings,
    /// „Mnemotechnik lernen“ (learn.rs).
    Learn,
}

pub fn start_screen() -> Screen {
    if startup::in_core() {
        Screen::Core
    } else {
        Screen::Home
    }
}

fn main() {
    console_error_panic_hook::set_once();
    // Before mounting, so the first paint already has the chosen language and palette.
    crash::install();
    i18n::set_current(i18n::load());
    theme::apply(theme::current());
    mount_to_body(Root);
    // The static splash from index.html gives way to the app (it waits for its animation).
    if let Some(window) = web_sys::window() {
        use wasm_bindgen::JsCast;
        if let Ok(hide) = js_sys::Reflect::get(&window, &"hideSplash".into())
            && let Ok(hide) = hide.dyn_into::<js_sys::Function>()
        {
            let _ = hide.call0(&wasm_bindgen::JsValue::NULL);
        }
    }
}

/// The root. It builds the app again when the language changes, so every text is looked up
/// again; screens keep no state a language switch could leave behind.
#[component]
fn Root() -> impl IntoView {
    let language = RwSignal::new(i18n::current());
    provide_context(i18n::Language(language));
    // On the very first start the language comes first; the app starts once it is chosen.
    let chosen = RwSignal::new(i18n::is_saved());
    FIRST_START.with(|first| first.set(!chosen.get_untracked()));
    move || {
        language.get();
        if chosen.get() {
            view! { <App /> }.into_any()
        } else {
            view! { <language_choice::LanguageChoice chosen /> }.into_any()
        }
    }
}

#[derive(Serialize)]
struct TourArgs {
    action: TourAction,
}

fn tour_action(
    active: RwSignal<Option<HighlightRequest>>,
    screen: RwSignal<Screen>,
    busy: RwSignal<bool>,
    error: RwSignal<String>,
    route_ready: RwSignal<bool>,
    _target_ready: RwSignal<bool>,
    action: TourAction,
) {
    if busy.get_untracked() {
        return;
    }
    // The tour never blocks: a step without its target still shows its text.
    if action == TourAction::Next && !route_ready.get_untracked() {
        return;
    }
    busy.set(true);
    // „Fertig“ on the last step completes the tour; closing early does not.
    let completed = action == TourAction::Next
        && active
            .get_untracked()
            .is_some_and(|step| step.step == step.total);
    if completed {
        crate::achievements::unlock("tour-done");
    }
    spawn_local(async move {
        match call::<Option<HighlightRequest>>(CMD_TOUR, TourArgs { action }).await {
            Ok(step) => {
                active.set(step.clone());
                match route_tour_step(step, screen).await {
                    Ok(()) => {
                        error.set(String::new());
                        route_ready.set(true);
                    }
                    Err(_) => {
                        error.set(String::new());
                        route_ready.set(true);
                    }
                }
            }
            Err(message) => error.set(message),
        }
        busy.set(false);
    });
}

/// Every element the tour may highlight.
const TOUR_TARGETS: [&str; 15] = [
    "tour-home-media",
    "tour-home-import",
    "tour-home-settings",
    "tour-demo-room",
    "tour-editor-toggle",
    "tour-editor-adding",
    "tour-editor-stage",
    "tour-editor-place",
    "tour-editor-content",
    "tour-editor-note",
    "tour-editor-answer",
    "tour-palace-route",
    "tour-journey-prompt",
    "tour-journey-reveal",
    "tour-journey-grade",
];

/// The window's height and the part of it the person sees, in CSS pixels: the top and height of
/// the visual viewport (which a pinch zoom or an on-screen keyboard makes smaller) in the window's
/// coordinates, and its zoom. The whole window where the browser has no `visualViewport`.
fn seen_area() -> Option<(f64, f64, f64, f64)> {
    let window = web_sys::window()?;
    let full = window.inner_height().ok()?.as_f64()?;
    let viewport = js_sys::Reflect::get(&window, &"visualViewport".into()).ok();
    let number = |name: &str| {
        viewport
            .as_ref()
            .and_then(|viewport| js_sys::Reflect::get(viewport, &name.into()).ok())
            .and_then(|value| value.as_f64())
    };
    Some(match (number("offsetTop"), number("height")) {
        (Some(top), Some(height)) => (full, top, height, number("scale").unwrap_or(1.0)),
        _ => (full, 0.0, full, 1.0),
    })
}

/// The visible part of the window while an on-screen keyboard covers some of it: its top, its
/// height and how much of the window's bottom is covered, all in CSS pixels. None without a
/// keyboard, while the page is pinch-zoomed (which shrinks the visual viewport too), or where the
/// browser has no `visualViewport`.
fn visible_area() -> Option<(f64, f64, f64)> {
    let (full, top, height, zoom) = seen_area()?;
    // A keyboard takes at least this much. Scrolling with it open moves the visible part (its
    // top), not its height.
    ((zoom - 1.0).abs() < 0.01 && full - height > 120.0).then_some((
        top,
        height,
        (full - top - height).max(0.0),
    ))
}

/// Whether the screen is touched rather than pointed at: there a focused text field brings up an
/// on-screen keyboard.
fn touch_screen() -> bool {
    web_sys::window()
        .and_then(|window| window.match_media("(pointer: coarse)").ok().flatten())
        .is_some_and(|query| query.matches())
}

/// Smooth, unless the system asks for reduced motion.
fn tour_scroll_behavior(window: &web_sys::Window) -> web_sys::ScrollBehavior {
    let reduced = window
        .match_media("(prefers-reduced-motion: reduce)")
        .ok()
        .flatten()
        .is_some_and(|query| query.matches());
    if reduced {
        web_sys::ScrollBehavior::Instant
    } else {
        web_sys::ScrollBehavior::Smooth
    }
}

/// Scrolls the page by `by` CSS pixels (down for a positive value), smoothly unless the system
/// asks for reduced motion. From the current position, so it also redirects a scroll still running.
fn scroll_page_by(window: &web_sys::Window, by: f64) {
    let options = web_sys::ScrollToOptions::new();
    options.set_top(window.scroll_y().unwrap_or(0.0) + by);
    options.set_behavior(tour_scroll_behavior(window));
    window.scroll_to_with_scroll_to_options(&options);
}

/// How far down the visible area the tour puts the top of a highlighted control: about where the
/// eye looks first on a page (its upper third). The larger part below stays free for the panel and
/// the thumbs, and an on-screen keyboard, which comes up from the bottom, leaves the control in view.
const TOUR_TARGET_AT: f64 = 0.3;

/// The part of the window the tour lays out the control and its panel in (top and height in the
/// window's coordinates): what the person sees, or its upper half while `assumed` (a text field
/// has the focus on a touch screen and no keyboard is reported, so one may cover the lower half).
fn tour_area(assumed: bool) -> Option<(f64, f64)> {
    let (_, top, height, _) = seen_area()?;
    Some((top, if assumed { height / 2.0 } else { height }))
}

/// The top of a control `size` tall at its tour place in `area`, leaving out the panel:
/// `TOUR_TARGET_AT` down, or higher where the control would reach below the area.
fn tour_place((top, height): (f64, f64), size: f64) -> f64 {
    (top + height * TOUR_TARGET_AT)
        .min(top + height - 12.0 - size)
        .max(top + 16.0)
}

/// Whether the panel goes to the top for a control `size` tall whose top is at `at` (in the
/// window's coordinates): the panel takes the half of `area` the control is not in.
fn tour_panel_up((top, height): (f64, f64), at: f64, size: f64) -> bool {
    at + size / 2.0 > top + height / 2.0
}

/// The space around a highlighted control and its panel in the tour's area: 16 px above the
/// control's place (or 12 below it), 12 px between the control and the panel, and the panel's own
/// distance from the area's edge (at most 1rem; the safe-area inset is reckoned in CSS).
const TOUR_PANEL_MARGINS: f64 = 44.0;

/// The least height of the tour's panel where it gives way to the highlighted control or to an
/// on-screen keyboard; its text scrolls inside.
const TOUR_PANEL_LEAST: f64 = 120.0;

/// Whether the tour's panel and `rect` (a highlighted control) share a column of the window, so
/// that the panel is above or below the control and not beside it (as in a wide window).
fn tour_panel_shares(panel: &web_sys::Element, rect: &web_sys::DomRect) -> bool {
    let side = panel.get_bounding_client_rect();
    side.left() < rect.right() && rect.left() < side.right()
}

/// Scrolls `framed` (a highlighted control with its label) to its tour place in `tour_area`,
/// clear of the panel above or below it (a panel beside it, in a wide window, leaves it alone): a
/// control too tall for that place above the panel moves up until it fits, and with the panel at
/// the top it goes below the panel. (The panel gives way to a control that would not fit beside it
/// at all: see `room` in `TourOverlay`.) The page scrolls by the distance measured from what the
/// person sees, so it lands the same in every browser, with or without a keyboard or zoom.
fn scroll_to_tour_place(framed: &web_sys::Element, assumed: bool) {
    let (Some(window), Some((top, height))) = (web_sys::window(), tour_area(assumed)) else {
        return;
    };
    let rect = framed.get_bounding_client_rect();
    let size = rect.height();
    // The free part of the area: from its top (or below a panel at the top) to its bottom (or
    // above a panel at the bottom).
    let (mut free_top, mut free_bottom) = (top + 16.0, top + height - 12.0);
    if let Some(panel) = window
        .document()
        .and_then(|document| document.query_selector(".tour-panel").ok().flatten())
    {
        let side = panel.get_bounding_client_rect();
        if tour_panel_shares(&panel, &rect) {
            if panel.class_list().contains("top") {
                free_top = free_top.max(side.bottom() + 12.0);
            } else {
                free_bottom = free_bottom.min(side.top() - 12.0);
            }
        }
    }
    let place = (top + height * TOUR_TARGET_AT)
        .min(free_bottom - size)
        .max(free_top);
    scroll_page_by(&window, rect.top() - place);
}

/// Takes the room below the page (the `tour-room` class) away once the tour has ended: at once
/// where the page is above it, else after the page has glided back to its own end, so the page
/// does not jump. `leaving` counts the ticks since; after 1.5 s the room goes anyway.
fn release_tour_room(
    window: &web_sys::Window,
    document: &web_sys::Document,
    leaving: StoredValue<u8>,
) {
    let (Some(body), Some(root)) = (document.body(), document.document_element()) else {
        return;
    };
    if !body.class_list().contains("tour-room") {
        return;
    }
    let room = document
        .query_selector("main")
        .ok()
        .flatten()
        .and_then(|main| {
            window
                .get_computed_style_with_pseudo_elt(&main, "::after")
                .ok()
                .flatten()
        })
        .and_then(|style| style.get_property_value("height").ok())
        .and_then(|height| height.trim_end_matches("px").parse::<f64>().ok())
        .unwrap_or(0.0);
    let full = window
        .inner_height()
        .ok()
        .and_then(|h| h.as_f64())
        .unwrap_or(0.0);
    let end = (f64::from(root.scroll_height()) - room - full).max(0.0);
    let y = window.scroll_y().unwrap_or(0.0);
    if y <= end + 1.0 || leaving.get_value() >= 10 {
        let _ = body.class_list().remove_1("tour-room");
        leaving.set_value(0);
    } else {
        if leaving.get_value() == 0 {
            scroll_page_by(window, end - y);
        }
        leaving.update_value(|ticks| *ticks += 1);
    }
}

/// Whether the focused element takes typed text (and so opens an on-screen keyboard).
fn text_entry_focused() -> bool {
    let Some(element) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element())
    else {
        return false;
    };
    match element.tag_name().as_str() {
        "TEXTAREA" => true,
        "INPUT" => !matches!(
            element.get_attribute("type").unwrap_or_default().as_str(),
            "checkbox" | "radio" | "button" | "submit" | "range" | "color" | "file"
        ),
        _ => element
            .get_attribute("contenteditable")
            .is_some_and(|value| value != "false"),
    }
}

/// The app tour's current step, for the screens that act on it (Simo's example).
#[derive(Clone, Copy)]
pub struct TourStep(pub RwSignal<Option<HighlightRequest>>);

thread_local! {
    /// The id of Simo's example palace once the tour has made it.
    static TOUR_DEMO: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// True until the app starts after the first start's language choice. A tour step left over
    /// from an earlier install (Android restores app data, browsers keep it) is not resumed then.
    static FIRST_START: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The tour step's target while the tour shows Simo's example palace `palace_id`. Reactive.
pub fn tour_target_in(tour: Option<TourStep>, palace_id: &str) -> Option<String> {
    let target = tour?
        .0
        .with(|step| step.as_ref().map(|step| step.target.clone()))?;
    TOUR_DEMO
        .with(|demo| demo.borrow().as_deref() == Some(palace_id))
        .then_some(target)
}

/// Whether `palace_id` is Simo's example palace from the tour.
pub fn is_tour_demo(palace_id: &str) -> bool {
    TOUR_DEMO.with(|demo| demo.borrow().as_deref() == Some(palace_id))
}

/// What Simo writes into the example's locus: place, content and memory aid.
pub fn tour_demo_locus() -> (&'static str, &'static str, &'static str) {
    (
        t("Tür"),
        t("Schlüssel"),
        t("Ein riesiger goldener Schlüssel steckt quer in der Tür."),
    )
}

/// Simo's example palace, with its locus filled in when the tour jumped past the editor steps
/// (the route needs a locus to walk).
async fn tour_demo(filled: bool) -> Result<String, String> {
    let id = call::<String>(
        CMD_TOUR_DEMO,
        LanguageArgs {
            lang: current_language(),
        },
    )
    .await?;
    TOUR_DEMO.with(|demo| *demo.borrow_mut() = Some(id.clone()));
    if !filled {
        return Ok(id);
    }
    let palace = call::<PalaceView>(CMD_PALACE, IdArgs { id: id.clone() }).await?;
    let Some(room) = palace.rooms.first() else {
        return Ok(id);
    };
    if room.loci.is_empty() {
        #[derive(Serialize)]
        struct Args {
            change: EditorChange,
        }
        let change = |action| Args {
            change: EditorChange {
                palace_id: id.clone(),
                room_id: room.id.clone(),
                action,
            },
        };
        let created = call::<EditorUpdated>(
            CMD_EDITOR_CHANGE,
            // The hall is an animation: a locus there has a time.
            change(EditorAction::CreateTimed {
                time: 0.0,
                x: 0.5,
                y: 0.62,
            }),
        )
        .await?;
        if let Some(locus) = created.selected {
            let (place, item, note) = tour_demo_locus();
            let content = LocusContent {
                place: place.into(),
                item: item.into(),
                note: note.into(),
                image: None,
                // The store's default picture width (a quarter of the room).
                image_size: 0.25,
                zoom: 1.0,
                answer: Default::default(),
                audio: None,
            };
            call::<EditorUpdated>(
                CMD_EDITOR_CHANGE,
                change(EditorAction::Save { id: locus, content }),
            )
            .await?;
        }
    }
    Ok(id)
}

/// Route only the official, known tour targets. An existing tutorial palace is
/// reused so moving between steps never replaces edits or review progress.
async fn route_tour_step(
    step: Option<HighlightRequest>,
    screen: RwSignal<Screen>,
) -> Result<(), String> {
    let Some(step) = step else {
        return Ok(());
    };
    match step.target.as_str() {
        "tour-home-media" | "tour-home-import" | "tour-home-settings" => {
            if !matches!(screen.get_untracked(), Screen::Home) {
                screen.set(Screen::Home);
            }
        }
        // Simo builds his example palace in the editor, step by step.
        "tour-demo-room"
        | "tour-editor-toggle"
        | "tour-editor-adding"
        | "tour-editor-stage"
        | "tour-editor-place"
        | "tour-editor-content"
        | "tour-editor-note"
        | "tour-editor-answer"
        | "tour-palace-route" => {
            let id = tour_demo(step.target == "tour-palace-route").await?;
            if !matches!(screen.get_untracked(), Screen::Palace(ref current) if current == &id) {
                screen.set(Screen::Palace(id));
            }
        }
        // Then he walks its route.
        "tour-journey-prompt" | "tour-journey-reveal" | "tour-journey-grade" => {
            let id = tour_demo(true).await?;
            if !matches!(screen.get_untracked(), Screen::Journey(ref start) if start.palace_id.as_ref() == Some(&id))
            {
                screen.set(Screen::Journey(JourneyStart {
                    palace_id: Some(id),
                    ..Default::default()
                }));
            }
        }
        _ => return Err(t("Unbekanntes Ziel der App-Tour").into()),
    }
    Ok(())
}

#[component]
fn TourOverlay(
    active: RwSignal<Option<HighlightRequest>>,
    screen: RwSignal<Screen>,
    error: RwSignal<String>,
    route_ready: RwSignal<bool>,
) -> impl IntoView {
    let busy = RwSignal::new(false);
    let target_ready = RwSignal::new(false);
    // The spotlight's frame around the highlighted control (with its label), in page coordinates:
    // it scrolls with the page, so it stays on the control while the page moves.
    let spot = RwSignal::new(None::<(f64, f64, f64, f64)>);
    // The part of the window an on-screen keyboard leaves visible (top, height, covered at the
    // bottom). Fixed elements stay put while the keyboard covers them, so the panel follows this.
    let visible = RwSignal::new(None::<(f64, f64, f64)>);
    // A text field has the focus on a touch screen and no keyboard is reported (it is not open yet,
    // or the browser does not say): its keyboard would cover the lower half, so the panel and the
    // control share the upper half, whose height this is.
    let assumed = RwSignal::new(None::<f64>);
    // The panel takes the half of the visible area the highlighted control is not in. With a text
    // field focused and no keyboard reported it goes to the top: a keyboard may cover the bottom.
    let panel_up = RwSignal::new(false);
    // The most the panel may take of the tour's area (before the safe-area inset) so that the
    // highlighted control fits above or below it: the area less the control and the margins around
    // both. A taller panel (long texts, large fonts) scrolls inside. None without a control, or
    // where the panel is beside it.
    let room = RwSignal::new(None::<f64>);
    let focused = StoredValue::new(None::<(u8, String)>);
    // What the control's place was found for: the keyboard's height (None without one), the
    // window's height and whether the control's own field had the focus. A keyboard that opens or
    // closes, a window that a keyboard (or turning the phone) resizes, or the person going back to
    // the field moves the place. Nothing else does: the person's own scrolling and zooming stay.
    let placed_for = StoredValue::new((None::<f64>, 0.0, false));
    // While the tour's own scroll runs: the control's top on the last tick. The panel keeps the
    // side chosen with the place until the control stops (also where the person takes over).
    let gliding = StoredValue::new(None::<f64>);
    // The window's height while no text field has the focus, to tell a keyboard that shrinks it.
    let calm = StoredValue::new(
        web_sys::window()
            .and_then(|window| window.inner_height().ok())
            .and_then(|h| h.as_f64())
            .unwrap_or(0.0),
    );
    let leaving = StoredValue::new(0u8);
    let before = StoredValue::new_local(None::<web_sys::HtmlElement>);
    let highlight = move || {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Some(document) = window.document() else {
            return;
        };
        for id in TOUR_TARGETS {
            if let Some(node) = document.get_element_by_id(id) {
                let _ = node.remove_attribute("data-tour-active");
            }
        }
        let Some(step) = active.get_untracked() else {
            focused.set_value(None);
            gliding.set_value(None);
            spot.set(None);
            if room.get_untracked().is_some() {
                room.set(None);
            }
            target_ready.set(false);
            if let Some(element) = before.get_value() {
                let _ = element.focus();
                before.set_value(None);
            }
            release_tour_room(&window, &document, leaving);
            return;
        };
        if let Some(body) = document.body() {
            let _ = body.class_list().add_1("tour-room");
        }
        leaving.set_value(0);
        if before.get_value().is_none() {
            before.set_value(document.active_element().and_then(|element| {
                wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(element).ok()
            }));
        }
        let target = route_ready
            .get_untracked()
            .then(|| document.get_element_by_id(&step.target))
            .flatten();
        let key = (step.step, step.target);
        let fresh = target.is_some() && focused.get_value().as_ref() != Some(&key);
        if let Some(node) = &target {
            let _ = node.set_attribute("data-tour-active", "true");
            if fresh {
                if let Some(element) = wasm_bindgen::JsCast::dyn_ref::<web_sys::HtmlElement>(node) {
                    // The browser's own scroll to a focused field would undo the place.
                    let options = web_sys::FocusOptions::new();
                    options.set_prevent_scroll(true);
                    let _ = element.focus_with_options(&options);
                }
                focused.set_value(Some(key));
            }
        }
        // The keyboard, after the step's own focus.
        let typing = text_entry_focused();
        let full = window
            .inner_height()
            .ok()
            .and_then(|h| h.as_f64())
            .unwrap_or(0.0);
        if !typing {
            calm.set_value(full);
        }
        let keyboard = visible_area().or_else(|| {
            // A keyboard that shrinks the window (the Android app, Firefox): all of it is in view.
            let (_, top, height, _) = seen_area()?;
            (typing && calm.get_value() - full > 120.0).then_some((top, height, 0.0))
        });
        if visible.get_untracked() != keyboard {
            visible.set(keyboard);
        }
        let guess = (typing && keyboard.is_none() && touch_screen())
            .then(|| tour_area(true).map(|(_, height)| height))
            .flatten();
        if assumed.get_untracked() != guess {
            assumed.set(guess);
        }
        let forced = typing && keyboard.is_none();
        let Some(node) = target else {
            focused.set_value(None);
            gliding.set_value(None);
            if route_ready.get_untracked() {
                spot.set(None);
                if panel_up.get_untracked() != forced {
                    panel_up.set(forced);
                }
                if room.get_untracked().is_some() {
                    room.set(None);
                }
            }
            target_ready.set(route_ready.get_untracked());
            return;
        };
        // A checkbox or field is shown with its label, so its text is in the light too.
        let framed = node.closest("label").ok().flatten().unwrap_or(node);
        let rect = framed.get_bounding_client_rect();
        let (now, size) = (rect.top(), rect.height());
        let area = tour_area(guess.is_some()).unwrap_or((0.0, full));
        let in_field = typing
            && document
                .active_element()
                .is_some_and(|element| framed.contains(Some(&element)));
        let layout = (keyboard.map(|(_, height, _)| height), full, in_field);
        let (was_keyboard, was_full, was_in_field) = placed_for.get_value();
        let moved = was_keyboard.is_some() != layout.0.is_some()
            || (was_keyboard.unwrap_or(0.0) - layout.0.unwrap_or(0.0)).abs() > 120.0
            || (was_full - full).abs() > 120.0
            || (in_field && !was_in_field);
        // The panel leaves the control room above or below it, whatever its text's fonts: the
        // height they share (what a keyboard leaves of the window, or its upper half where one is
        // assumed; a pinch zoom changes nothing) less the control and the margins around both.
        // Set before the place is reached (a frame later), so that the place reckons with it.
        // Before the panel is shown it counts as sharing the control's column.
        let next = document
            .query_selector(".tour-panel")
            .ok()
            .flatten()
            .is_none_or(|panel| tour_panel_shares(&panel, &rect))
            .then(|| {
                let shared = keyboard.map(|(_, height, _)| height).or(guess);
                shared.unwrap_or(full) - size - TOUR_PANEL_MARGINS
            });
        if room.get_untracked() != next {
            room.set(next);
        }
        let up = if fresh || moved {
            placed_for.set_value(layout);
            gliding.set_value(Some(now));
            // The panel's side comes with the place: the half the control will be in once there,
            // or where the page cannot scroll that far.
            let y = window.scroll_y().unwrap_or(0.0);
            let end = document
                .document_element()
                .map_or(0.0, |root| f64::from(root.scroll_height()) - full)
                .max(0.0);
            let at = now - ((y + now - tour_place(area, size)).clamp(0.0, end) - y);
            // The scroll waits a frame, until the panel is on that side and of its height: then
            // the space it leaves is known.
            let assumed = guess.is_some();
            let framed = framed.clone();
            request_animation_frame(move || scroll_to_tour_place(&framed, assumed));
            forced || tour_panel_up(area, at, size)
        } else if let Some(last) = gliding.get_value() {
            gliding.set_value(((now - last).abs() >= 0.5).then_some(now));
            forced || panel_up.get_untracked()
        } else {
            forced || tour_panel_up(area, now, size)
        };
        if panel_up.get_untracked() != up {
            panel_up.set(up);
        }
        // The frame, in page coordinates and within the page's width.
        let width = document
            .document_element()
            .map_or(f64::INFINITY, |root| f64::from(root.client_width()));
        let (left, right) = (
            (rect.left() - 8.0).max(0.0),
            (rect.right() + 8.0).min(width),
        );
        let (x, y) = (
            window.scroll_x().unwrap_or(0.0),
            window.scroll_y().unwrap_or(0.0),
        );
        let next = Some((x + left, y + now - 8.0, right - left, size + 16.0));
        if spot.get_untracked() != next {
            spot.set(next);
        }
        target_ready.set(true);
    };
    // The part of the window the panel shares with the control while a keyboard is open or
    // assumed: the panel takes at most half of it and scrolls inside.
    let shared = move || {
        visible
            .get()
            .map(|(_, height, _)| height)
            .or_else(|| assumed.get())
    };
    // The panel's height: at most half of what a keyboard leaves, and no more than leaves the
    // highlighted control room beside it (less the safe-area inset on the panel's side), but at
    // least `TOUR_PANEL_LEAST`. None (as tall as its text) without either.
    let most = move || {
        let keyboard = shared().map(|height| format!("{}px", height / 2.0 - 16.0));
        let beside = room.get().map(|room| {
            let edge = if panel_up.get() { "top" } else { "bottom" };
            format!("calc({room}px - env(safe-area-inset-{edge}))")
        });
        let most = match (keyboard, beside) {
            (Some(keyboard), Some(beside)) => format!("min({keyboard}, {beside})"),
            (Some(most), None) | (None, Some(most)) => most,
            (None, None) => return None,
        };
        Some(format!("max({TOUR_PANEL_LEAST}px, {most})"))
    };
    highlight();
    let timer = set_interval_with_handle(highlight, std::time::Duration::from_millis(150)).ok();
    on_cleanup(move || {
        if let Some(timer) = timer {
            timer.clear();
        }
        if let Some(body) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.body())
        {
            let _ = body.class_list().remove_1("tour-room");
        }
    });
    view! {
        {move || active.get().is_some().then(|| match spot.get() {
            // A frame over everything with the target as its bright hole; the rest is only dimmed.
            Some((x, y, w, h)) => view! {
                <div class="tour-spot" aria-hidden="true"
                    style:left=format!("{x}px") style:top=format!("{y}px")
                    style:width=format!("{w}px") style:height=format!("{h}px")></div>
            }.into_any(),
            None => view! { <div class="tour-scrim" aria-hidden="true"></div> }.into_any(),
        })}
        <Show when=move || active.get().is_some()>
                <aside class="tour-panel card" role="region" aria-label=t("App-Tour") aria-live="polite"
                    class:top=move || panel_up.get()
                    // With a keyboard open the panel sits in the visible part, above the keyboard
                    // or at the top. It scrolls inside when that part is small, or when the
                    // highlighted control would not fit beside all of it.
                    style:bottom=move || visible.get().filter(|_| !panel_up.get()).map(|(_, _, covered)| format!("{}px", covered + 8.0))
                    style:top=move || visible.get().filter(|_| panel_up.get()).map(|(top, _, _)| format!("calc({}px + env(safe-area-inset-top))", top + 8.0))
                    style:max-height=most
                    style:overflow-y=move || most().map(|_| "auto")>
                    <div class="tour-simo">{move || art().get().map(|assets| view! { <Sply media=assets.simo /> })}</div>
                    <div class="tour-copy">
                        <small>{move || active.get().map(|step| crate::i18n::tf("Schritt {0} von {1}", &[&step.step, &step.total])).unwrap_or_default()}</small>
                        <h2>{move || active.get().map(|step| crate::i18n::translate(&step.title)).unwrap_or_default()}</h2>
                        <p>{move || active.get().map(|step| crate::i18n::translate(&step.text)).unwrap_or_default()}</p>
                        <p class="notes" role="status">{move || {
                            let message = error.get();
                            if !message.is_empty() { message }
                            else if route_ready.get() && !target_ready.get() {
                                t("Tour-Ziel nicht gefunden. Du kannst den Schritt erneut laden oder die Tour schließen.").into()
                            } else { String::new() }
                        }}</p>
                        <div class="row">
                            <button disabled=move || busy.get() on:click=move |_| tour_action(active, screen, busy, error, route_ready, target_ready, TourAction::Skip)>{t("Schließen")}</button>
                            <button disabled=move || busy.get() || active.get().is_none_or(|step| step.step <= 1)
                                on:click=move |_| tour_action(active, screen, busy, error, route_ready, target_ready, TourAction::Previous)>{t("Zurück")}</button>
                            <button class="primary" disabled=move || busy.get()
                                on:click=move |_| tour_action(active, screen, busy, error, route_ready, target_ready, TourAction::Next)>
                                {move || if active.get().is_some_and(|step| step.step == step.total) { t("Fertig") } else { t("Weiter") }}
                            </button>
                        </div>
                    </div>
                </aside>
        </Show>
    }
}

#[component]
fn App() -> impl IntoView {
    let screen = RwSignal::new(start_screen());
    let tour = RwSignal::new(None::<HighlightRequest>);
    provide_context(TourStep(tour));
    let tour_error = RwSignal::new(String::new());
    let tour_route_ready = RwSignal::new(false);
    provide_context(tour_gate::TourControl {
        tour,
        error: tour_error,
        route_ready: tour_route_ready,
        busy: RwSignal::new(false),
        asking: RwSignal::new(false),
    });
    provide_context(achievements::Celebration::new());
    provide_context(cloud_settings::SignedIn(RwSignal::new(None)));
    provide_context(first_palace::Guide(RwSignal::new(first_palace::load())));
    // The first start shows only the language choice and then the welcome page; the tour starts
    // from its button.
    let action = if FIRST_START.with(|first| first.replace(false)) {
        TourAction::Skip
    } else {
        TourAction::Resume
    };
    spawn_local(async move {
        match call::<Option<HighlightRequest>>(CMD_TOUR, TourArgs { action }).await {
            Ok(step) => {
                tour.set(step.clone());
                match route_tour_step(step, screen).await {
                    Ok(()) => tour_route_ready.set(true),
                    Err(message) => tour_error.set(message),
                }
            }
            Err(message) => tour_error.set(message),
        }
    });
    let editing = palette::Editing(RwSignal::new(false));
    provide_context(editing);
    provide_context(locus_display::DisplaySettings(RwSignal::new(
        locus_display::load(),
    )));
    provide_context(locus_display::LeadOverrides(RwSignal::new(
        locus_display::load_leads(),
    )));
    // The app's own animations (assets/ui), loaded once for every screen.
    let art = Art(RwSignal::new(None));
    provide_context(art);
    provide_context(dashboard::Layout(RwSignal::new(dashboard::load())));
    look::apply(&look::load());
    // The start page's layout editor (see dashboard.rs).
    let dashboard_open = RwSignal::new(false);
    provide_context(dashboard::Open(dashboard_open));
    spawn_local(async move {
        if let Ok(a) = call::<UiAssets>(CMD_UI_ASSETS, tauri::NoArgs {}).await {
            art.0.set(Some(a));
        }
    });
    // Learning days follow local time: tell the backend the system's offset
    // (JavaScript reports minutes *west* of UTC).
    // Sent at start, when the window comes back and every 15 minutes, but
    // only when it changed (summer time can switch while the app is open).
    // One request at a time; when it returns, the latest sample wins, so an
    // older answer can never overwrite a newer offset.
    let offset_revision = RwSignal::new(0_u64);
    // Auto-sync runs on every screen; a sync that brought changes reloads the start page.
    auto_sync::start(offset_revision);
    let sent = StoredValue::new(None::<i32>);
    let wanted = StoredValue::new(None::<i32>);
    let sending = StoredValue::new(false);
    let sync_offset = move || {
        let minutes = -(js_sys::Date::new_0().get_timezone_offset() as i32);
        wanted.set_value(Some(minutes));
        if sending.get_value() || sent.get_value() == Some(minutes) {
            return;
        }
        sending.set_value(true);
        spawn_local(async move {
            while let Some(minutes) = wanted.get_value().filter(|m| sent.get_value() != Some(*m)) {
                let offset = locora_app_api::UtcOffset { minutes };
                if call::<()>(locora_app_api::CMD_SET_UTC_OFFSET, OffsetArgs { offset })
                    .await
                    .is_err()
                {
                    break; // tried again at the next focus or interval
                }
                sent.set_value(Some(minutes));
                offset_revision.update(|revision| *revision += 1);
            }
            sending.set_value(false);
        });
    };
    sync_offset();
    let focus = window_event_listener(leptos::ev::focus, move |_| sync_offset());
    let every = set_interval_with_handle(sync_offset, std::time::Duration::from_secs(15 * 60)).ok();
    on_cleanup(move || {
        focus.remove();
        if let Some(handle) = every {
            handle.clear();
        }
    });
    // The last crash, shown once with a prefilled bug report that the person submits.
    let crash_notice = RwSignal::new(crash::take());
    view! {
        <palette::Palette screen=screen editing=editing />
        {move || crash_notice.get().map(|report| {
            let url = crash::issue_url(&report);
            view! {
                <aside class="card notes" role="alert" style="margin:.6rem;">
                    <strong>{t("In der letzten Sitzung ist ein Fehler aufgetreten.")}</strong>
                    <p>{format!("{} · {}", report.message, report.location)}</p>
                    <p>
                        <a href=url target="_blank" rel="noopener">{t("Auf GitHub melden")}</a>
                        " · "
                        <button on:click=move |_| crash_notice.set(None)>{t("Schließen")}</button>
                    </p>
                </aside>
            }
        })}
        <auto_sync::DeletionNotice />
        <Notifications screen=screen />
        <TourOverlay active=tour screen=screen error=tour_error route_ready=tour_route_ready />
        <tour_gate::TourQuestion screen=screen />
        <achievements::CelebrationCard />
        <main>
            // The first own palace: its walk ends with the guide's last word.
            {move || match screen.get() {
                Screen::Journey(JourneyStart { palace_id: Some(id), .. }) => Some(view! {
                    <first_palace::GuidePanel palace_id=id screen=screen only_finished=true />
                }),
                _ => None,
            }}
            {move || match screen.get() {
                Screen::Home => view! {
                    <HomeView screen=screen offset_revision=offset_revision core_only=false/>
                }.into_any(),
                Screen::Core => view! {
                    <HomeView screen=screen offset_revision=offset_revision core_only=true/>
                }.into_any(),

                // A new palace opens like any other, with the editor already on.
                Screen::Imported(palace) => view! { <PalaceScreen id=palace.id screen=screen editing=true/> }.into_any(),
                Screen::Palace(id) => view! { <PalaceScreen id=id screen=screen/> }.into_any(),
                Screen::Journey(start) => {
                    view! { <journey::JourneyView start=start screen=screen/> }.into_any()
                }
                Screen::Settings => view! { <settings::SettingsView screen=screen/> }.into_any(),
                Screen::Learn => view! { <learn::LearnView screen=screen/> }.into_any(),
            }}
        </main>
    }
}

/// The app's own graphics (`UiAssets`), `None` until loaded.
#[derive(Clone, Copy)]
pub struct Art(pub RwSignal<Option<UiAssets>>);

/// The graphics signal from the context (empty outside `App`).
pub fn art() -> RwSignal<Option<UiAssets>> {
    use_context::<Art>().map_or_else(|| RwSignal::new(None), |a| a.0)
}

/// Loading: the animated Locora logo.
#[component]
pub fn Loading() -> impl IntoView {
    let art = art();
    view! {
        <div class="loading" role="status">
            {move || art.get().map(|a| view! { <div class="logo"><Sply media=a.logo /></div> })}
            <p>{t("Lädt …")}</p>
        </div>
    }
}

/// The icon of a room kind: no picture, picture or `.sply`, video.
#[component]
pub fn RoomIcon(backdrop: Option<Media>) -> impl IntoView {
    let art = art();
    let kind = backdrop.map(|m| m.kind);
    move || {
        art.get().map(|a| {
            let media = match kind {
                None => a.room_blank,
                Some(locora_app_api::MediaKind::Video) => a.room_video,
                Some(_) => a.room_image,
            };
            view! { <span class="room-icon"><Sply media=media /></span> }
        })
    }
}

/// Load once into a signal: `None` while loading.
pub fn load<T, F>(future: F) -> RwSignal<Option<Result<T, String>>>
where
    T: Send + Sync + 'static,
    F: std::future::Future<Output = Result<T, String>> + 'static,
{
    let data = RwSignal::new(None);
    spawn_local(async move { data.set(Some(future.await)) });
    data
}

/// „„Planeten“ importiert“, with renamed copies and the review schedule.
fn import_message(result: &locora_app_api::ImportResult) -> String {
    let mut parts: Vec<String> = result
        .palaces
        .iter()
        .map(|p| {
            if p.name == p.original_name {
                tf("„{0}“ importiert", &[&p.name])
            } else {
                tf("„{0}“ importiert als „{1}“", &[&p.original_name, &p.name])
            }
        })
        .collect();
    if let Some(n) = result.restored {
        parts.push(crate::i18n::tf(
            "Wiederholungsstand für {0} Loci übernommen",
            &[&n],
        ));
    }
    parts.extend(
        result
            .notes
            .iter()
            .map(|note| crate::i18n::translate_message(note)),
    );
    parts.join(" · ")
}

fn loci_text(n: u32) -> String {
    if n == 1 {
        t("1 Locus").into()
    } else {
        crate::i18n::tf("{0} Loci", &[&n])
    }
}

/// A tap on a notification runs its action, e.g. `review.start@Planeten`.
fn notification_action(notification: &NotificationView, screen: RwSignal<Screen>) {
    if let Some(palace) = notification.action.strip_prefix("review.start@") {
        screen.set(Screen::Journey(JourneyStart {
            palace_id: Some(palace.to_owned()),
            due_only: true,
            ..Default::default()
        }));
    } else if let Some(palace) = notification.action.strip_prefix("review.open@") {
        screen.set(Screen::Palace(palace.to_owned()));
    }
}

/// Due reminders as small cards on top of everything, from the `notify`
/// plugin. Fetching more acknowledges the ones on screen, so each message
/// shows until it is handled (tap or dismiss).
/// Wide enough for the controls to start open; on a phone they start folded so the room comes first.
fn wide_screen() -> bool {
    web_sys::window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|width| width.as_f64())
        .is_none_or(|width| width > 700.0)
}

#[component]
fn Notifications(screen: RwSignal<Screen>) -> impl IntoView {
    let shown = RwSignal::new(Vec::<NotificationView>::new());
    let busy = StoredValue::new(false);
    let poll = move || {
        if busy.get_value() {
            return;
        }
        busy.set_value(true);
        spawn_local(async move {
            if let Ok(due) =
                call::<Vec<NotificationView>>(locora_app_api::CMD_NOTIFICATIONS, tauri::NoArgs {})
                    .await
            {
                shown.set(due);
            }
            busy.set_value(false);
        });
    };
    poll();
    let every = set_interval_with_handle(poll, std::time::Duration::from_secs(15 * 60)).ok();
    on_cleanup(move || {
        if let Some(handle) = every {
            handle.clear();
        }
    });
    let dismiss = move |notification: NotificationView, seen: bool| {
        shown.update(|list| list.retain(|n| n != &notification));
        if seen {
            spawn_local(async move {
                let _ = call::<()>(
                    locora_app_api::CMD_NOTIFICATION_SHOWN,
                    NotificationArgs { notification },
                )
                .await;
            });
        }
    };
    view! {
        <div class="notifications" role="region" aria-label=t("Erinnerungen")>
            <For
                each=move || shown.get()
                key=|n| (n.channel.clone(), n.title.clone(), n.body.clone())
                let:notification
            >
                {move || {
                    let notification = StoredValue::new(notification.clone());
                    view! {
                        <div class="notification" role="status" data-priority=move || notification.get_value().priority>
                            <button
                                class="tap"
                                on:click=move |_| {
                                    notification_action(&notification.get_value(), screen);
                                    dismiss(notification.get_value().clone(), true);
                                }
                            >
                                {move || {
                                    let n = notification.get_value();
                                    view! {
                                        <strong>{n.title.clone()}</strong>
                                        <span>{n.body.clone()}</span>
                                    }
                                }}
                            </button>
                            <button
                                class="close"
                                aria-label=t("Erinnerung schließen")
                                on:click=move |_| dismiss(notification.get_value().clone(), true)
                            >
                                "✕"
                            </button>
                        </div>
                    }
                }}
            </For>
        </div>
    }
}

#[component]
fn HomeView(
    screen: RwSignal<Screen>,
    offset_revision: RwSignal<u64>,
    core_only: bool,
) -> impl IntoView {
    let home = RwSignal::new(None::<Result<Home, String>>);
    // Loads the start page's data again in place: the screen and its scroll position stay.
    let reload = Callback::new(move |()| {
        spawn_local(async move {
            let result = call::<Home>(CMD_HOME, tauri::NoArgs {}).await;
            home.try_set(Some(result));
        });
    });
    provide_context(HomeReload(reload));
    Effect::new(move |_| {
        let revision = offset_revision.get();
        spawn_local(async move {
            let result = call::<Home>(CMD_HOME, tauri::NoArgs {}).await;
            // Ignore an older load if synchronization triggered a newer one.
            if offset_revision.try_get_untracked() == Some(revision) {
                home.try_set(Some(result));
            }
        });
    });
    let busy = RwSignal::new(false);
    let message = RwSignal::new(String::new());
    let info = RwSignal::new(String::new());
    tauri::on_file_drop(move |drop| {
        if busy.get_untracked() {
            return;
        }
        let ticket = match drop {
            Ok(ticket) => ticket,
            Err(error) => {
                message.set(error);
                return;
            }
        };
        busy.set(true);
        message.set(String::new());
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct DropArgs {
                ticket: String,
            }
            match call::<PalaceView>("home_import_drop", DropArgs { ticket }).await {
                Ok(palace) => screen.set(Screen::Imported(palace)),
                Err(error) => message.set(error),
            }
            busy.set(false);
        });
    });
    let pick_media = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        message.set(String::new());
        spawn_local(async move {
            match call::<Option<PalaceView>>(locora_app_api::CMD_HOME_PICK_MEDIA, tauri::NoArgs {})
                .await
            {
                Ok(Some(palace)) => screen.set(Screen::Imported(palace)),
                Ok(None) => {}
                Err(error) => message.set(error),
            }
            busy.set(false);
        });
    };
    let import = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        message.set(String::new());
        info.set(String::new());
        spawn_local(async move {
            let picked = call::<Option<locora_app_api::ImportResult>>(
                locora_app_api::CMD_IMPORT_PICK,
                tauri::NoArgs {},
            )
            .await;
            match picked {
                Ok(Some(result)) => {
                    info.set(import_message(&result));
                    home.set(Some(call::<Home>(CMD_HOME, tauri::NoArgs {}).await));
                }
                Ok(None) => {}
                Err(error) => message.set(error),
            }
            busy.set(false);
        });
    };
    let start_tutorial = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match call::<String>(
                CMD_TUTORIAL,
                LanguageArgs {
                    lang: current_language(),
                },
            )
            .await
            {
                Ok(id) => screen.set(Screen::Palace(id)),
                Err(e) => {
                    busy.set(false);
                    home.set(Some(Err(e)));
                }
            }
        });
    };
    let start_formula_guide = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match call::<String>(
                CMD_FORMULA_GUIDE,
                LanguageArgs {
                    lang: current_language(),
                },
            )
            .await
            {
                Ok(id) => screen.set(Screen::Palace(id)),
                Err(error) => {
                    busy.set(false);
                    message.set(error);
                }
            }
        });
    };
    let tour = tour_gate::control();
    let dashboard_open = expect_context::<dashboard::Open>().0;
    view! {
        <header class="bar home-bar">
            <h1>{if core_only { "Core" } else { "Locora" }}</h1>
            <div class="home-actions">
                {core_only.then(|| view! {
                    <button class="wide" on:click=move |_| screen.set(Screen::Home)>{t("Startseite")}</button>
                })}
                <button class="primary wide" id="tour-home-media" disabled=move || busy.get() on:click=pick_media>{t("Bild/Video wählen…")}</button>
                <button id="home-learn" on:click=move |_| screen.set(Screen::Learn)>{t("Mnemotechnik lernen")}</button>
                <button disabled=move || tour.busy.get() on:click=move |_| tour_gate::request(screen)>{t("App-Tour")}</button>
                <button id="tour-home-import" disabled=move || busy.get() on:click=import title=t("Paläste aus einer .locora-Datei oder locora.toml")>
                    {t("Importieren…")}
                </button>
                {(!core_only).then(|| view! {
                    <button on:click=move |_| dashboard_open.update(|open| *open = !*open)>{t("Seiten anpassen")}</button>
                })}
                <button class="icon" id="tour-home-settings" title=t("Einstellungen & Statistik") aria-label=t("Einstellungen & Statistik") on:click=move |_| screen.set(Screen::Settings)>
                    "⚙"
                </button>
            </div>
        </header>
        <p class="notes" role="status">{move || message.get()}</p>
        <p class="status" role="status">{move || info.get()}</p>
        {move || match home.get() {
            None => view! { <Loading /> }.into_any(),
            Some(Err(e)) => view! { <p class="notes">{e}</p> }.into_any(),
            Some(Ok(home)) => {
                let waiting = home.due + home.fresh;
                let _tutorial = home.tutorial;
                let _formula_guide = home.formula_guide;
                if let Some(simo) = home.welcome.clone().filter(|_| !core_only) {
                    return view! {
                        <div class="home-stack">
                        <section class="card welcome">
                            <div class="simo">
                                <Sply media=simo />
                            </div>
                            <div class="text">
                                <h2>{t("Willkommen bei Locora")}</h2>
                                <p>{t("Merk dir alles, was du willst – mit Bildern an Orten, die du kennst. So fängst du an:")}</p>
                                <learning_path::LearningPath screen=screen always=true />
                                <button class="link formula-link" disabled=move || busy.get() on:click=start_formula_guide>
                                    {t("Für Fortgeschrittene: Formel-Guide")}
                                </button>
                            </div>
                        </section>
                        <Tiles tiles=home.tiles.clone() />
                        <crate::achievements::Achievements />
                        <crate::activity::Activity />
                        </div>
                        {home
                            .notes
                            .into_iter()
                            .map(|n| view! { <p class="notes">{crate::i18n::translate_message(&n)}</p> })
                            .collect_view()}
                    }
                        .into_any();
                }
                let pages = expect_context::<dashboard::Layout>().0;
                let catalog = dashboard::Catalog {
                    tiles: home.tiles.clone(),
                    blocks: home.blocks.clone(),
                    suggestions: home.pages.clone(),
                };
                let known = dashboard::known(&catalog);
                let page = pages.get().current().clone();
                // The built-in palaces show only where the layout places their blocks.
                let built_ins: Vec<PalaceCard> = home.palaces.iter().filter(|p| p.built_in.is_some()).cloned().collect();
                let mut palaces = Some(home.palaces);
                let mut shown: Vec<AnyView> = Vec::new();
                let mut folded: Vec<AnyView> = Vec::new();
                let mut fold_at = None;
                for entry in dashboard::resolve(&page.entries, &known, page.id == dashboard::START) {
                    // Core only: the palaces, nothing else.
                    let placement = if core_only && entry.id != "palaces" {
                        dashboard::Placement::Hidden
                    } else {
                        entry.placement
                    };
                    let block: Option<AnyView> = match entry.id.as_str() {
                        "due" => Some(
                            view! {
                                <section class="card weiter">
                                    <div class="text">
                                        <div class="count">
                                            {if waiting == 0 {
                                                t("Heute ist nichts fällig").to_owned()
                                            } else {
                                                tf("{0} fällig · {1} neu", &[&home.due, &home.fresh])
                                            }}
                                        </div>
                                        <div class="meta">
                                            {if waiting == 0 {
                                                t("Simo hält die Laterne – morgen geht es weiter.")
                                            } else {
                                                t("Wiederholen, was dein Gedächtnis gerade braucht.")
                                            }}
                                        </div>
                                    </div>
                                    {(waiting == 0)
                                        .then(|| {
                                            let art = art();
                                            move || {
                                                art.get()
                                                    .map(|a| view! { <div class="resting"><Sply media=a.simo /></div> })
                                            }
                                        })}
                                    <button
                                        class="primary"
                                        disabled=move || waiting == 0 || busy.get()
                                        on:click=move |_| {
                                            if busy.get_untracked() { return; }
                                            screen
                                                .set(
                                                    Screen::Journey(JourneyStart {
                                                        due_only: true,
                                                        ..Default::default()
                                                    }),
                                                )
                                        }
                                    >
                                        {t("Weiter ▶")}
                                    </button>
                                </section>
                            }
                            .into_any(),
                        ),
                        "palaces" => {
                            let list: Vec<PalaceCard> = palaces
                                .take()
                                .unwrap_or_default()
                                .into_iter()
                                .filter(|p| p.built_in.is_none())
                                .collect();
                            Some(if list.is_empty() {
                                view! {
                                    <p class="empty">
                                        {t("Noch keine Paläste. Wähle oben ein Bild oder Video, oder nutze die CLI: ")}
                                        <code>"locora new Planeten -- Merkur Venus Erde"</code>
                                    </p>
                                }
                                .into_any()
                            } else {
                                view! {
                                    <section class="palaces">
                                        {list
                                            .into_iter()
                                            .map(|p| view! { <PalaceTile palace=p screen=screen busy=busy/> })
                                            .collect_view()}
                                    </section>
                                }
                                .into_any()
                            })
                        }
                        "tutorial" => match built_ins.iter().find(|p| p.built_in.as_deref() == Some("tutorial")) {
                            Some(palace) => Some(view! { <PalaceTile palace=palace.clone() screen=screen busy=busy/> }.into_any()),
                            None => Some({
                            view! {
                                <section class="card weiter">
                                    <div class="text">
                                        <div class="count">{t("Locora lernen")}</div>
                                        <div class="meta">
                                            {t("Das Tutorial ist selbst ein Gedächtnispalast: 3 Räume, 9 Orte.")}
                                        </div>
                                    </div>
                                    <button disabled=move || busy.get() on:click=start_tutorial>
                                        {t("Tutorial anlegen")}
                                    </button>
                                </section>
                            }
                            .into_any()
                            }),
                        },
                        "formulas" => match built_ins.iter().find(|p| p.built_in.as_deref() == Some("formulas")) {
                            Some(palace) => Some(view! { <PalaceTile palace=palace.clone() screen=screen busy=busy/> }.into_any()),
                            None => Some({
                            view! {
                                <section class="card weiter">
                                    <div class="text">
                                        <div class="count">{t("Formeln verstehen")}</div>
                                        <div class="meta">{t("Variablen, Tabellen und Bindungen in einem eigenen Palast üben.")}</div>
                                    </div>
                                    <button disabled=move || busy.get() on:click=start_formula_guide>
                                        {t("Formel-Guide anlegen")}
                                    </button>
                                </section>
                            }
                            .into_any()
                            }),
                        },
                        "achievements" => Some(view! { <crate::achievements::Achievements /> }.into_any()),
                        "activity" => Some(view! { <crate::activity::Activity /> }.into_any()),
                        other => dashboard::plugin_view(&catalog, other),
                    };
                    let Some(block) = block else {
                        continue;
                    };
                    let block = dashboard::sized(block, entry.size);
                    match placement {
                        dashboard::Placement::Shown => shown.push(block),
                        dashboard::Placement::Folded => {
                            fold_at.get_or_insert(shown.len());
                            folded.push(block);
                        }
                        dashboard::Placement::Hidden => {}
                    }
                }
                if shown.is_empty() && folded.is_empty() {
                    shown.push(
                        view! { <p class="notes">{t("Auf dieser Seite ist noch nichts. Füge Blöcke unter „Seiten anpassen“ hinzu.")}</p> }
                            .into_any(),
                    );
                }
                if !folded.is_empty() {
                    let at = fold_at.unwrap_or(shown.len());
                    let count = folded.len();
                    shown.insert(
                        at,
                        view! {
                            <details class="plugins-fold">
                                <summary>{tf("Plugins ({0})", &[&count])}</summary>
                                {folded}
                            </details>
                        }
                        .into_any(),
                    );
                }
                let editor_catalog = catalog.clone();
                view! {
                    {(!core_only).then(|| view! {
                        <section class="card path-card">
                            <h2>{t("Dein Weg in Locora")}</h2>
                            <learning_path::LearningPath screen=screen />
                        </section>
                    })}
                    <dashboard::Tabs />
                    {move || {
                        dashboard_open
                            .get()
                            .then(|| view! { <dashboard_editor::Editor catalog=editor_catalog.clone() open=dashboard_open/> })
                    }}
                    {shown}
                    {home
                        .notes
                        .into_iter()
                        .map(|n| view! { <p class="notes">{crate::i18n::translate_message(&n)}</p> })
                        .collect_view()}
                }
                    .into_any()
            }
        }}
    }
}

/// Plugin tiles (`ui.tile`), on the welcome screen as well as the overview.
#[component]
fn Tiles(tiles: Vec<locora_app_api::TileView>) -> impl IntoView {
    // The game's tile names XP, the week and the medals; the weekly run goes below, when there
    // is one (never what was lost).
    let run = RwSignal::new(None::<String>);
    if tiles.iter().any(|tile| tile.plugin == "gamification") {
        spawn_local(async move {
            let args = locora_app_api::GameStatusArgs { full: false };
            if let Ok(Some(status)) =
                call::<Option<GamificationStatus>>(CMD_GAMIFICATION_STATUS, args).await
            {
                run.set(journey::week_run_text(&status.week));
            }
        });
    }
    let art = art();
    (!tiles.is_empty()).then(|| {
        view! {
            <section class="tiles" aria-label=t("Plugins")>
                {tiles
                    .into_iter()
                    .map(|t| {
                        let gamification = t.plugin == "gamification";
                        view! {
                            <article class="card tile" title=t.plugin>
                                {t.icon.map(|m| view! { <div class="icon"><Sply media=m /></div> })}
                                <div class="text">
                                    <span class="meta">{crate::i18n::plugin_text(&t.title)}</span>
                                    <span class="count">{crate::i18n::plugin_text(&t.text)}</span>
                                    {gamification.then(|| view! {
                                        <Show when=move || run.with(Option::is_some)>
                                            <span class="tile-streak">
                                                {move || art.get().map(|assets| view! { <span class="game-icon" aria-hidden="true"><Sply media=assets.streak /></span> })}
                                                {move || run.get()}
                                            </span>
                                        </Show>
                                    })}
                                </div>
                            </article>
                        }
                    })
                    .collect_view()}
            </section>
        }
    })
}

/// Reloads the start page's data in place (provided by the home view).
#[derive(Clone, Copy)]
struct HomeReload(Callback<()>);

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DeletePalaceArgs {
    palace_id: String,
}

/// The language the palaces are built in: the app's language.
fn current_language() -> String {
    crate::i18n::current().code().to_owned()
}

#[component]
fn PalaceTile(palace: PalaceCard, screen: RwSignal<Screen>, busy: RwSignal<bool>) -> impl IntoView {
    let id = palace.id.clone();
    let cover_open_id = palace.id.clone();
    let play_id = palace.id.clone();
    // Built-in palaces can be removed; the app tour creates them again.
    let built_in = palace.built_in.is_some();
    let delete_id = palace.id.clone();
    let delete_name = palace.name.clone();
    let status = match (palace.due, palace.fresh) {
        (0, 0) => t("alles gewusst").to_owned(),
        (due, 0) => crate::i18n::tf("{0} fällig", &[&due]),
        (0, fresh) => crate::i18n::tf("{0} neu", &[&fresh]),
        (due, fresh) => crate::i18n::tf("{0} fällig · {1} neu", &[&due, &fresh]),
    };
    let previews = palace.previews.clone();
    let selected = RwSignal::new(startup::covers().get(&palace.id).cloned());
    let palace_for_cover = palace.id.clone();
    let selected_preview = Memo::new(move |_| {
        let choice = selected.get();
        previews
            .iter()
            .find(|preview| choice.as_deref() == Some(&preview.id))
            .or_else(|| previews.first())
            .cloned()
    });
    let previews_for_button = palace.previews.clone();
    let can_change_cover = previews_for_button.len() > 1;
    view! {
        <article class="card palace" aria-disabled=move || busy.get()
            on:click=move |_| { if !busy.get_untracked() { screen.set(Screen::Palace(id.clone())); } }>
            {move || selected_preview.get().map(|preview| {
                let choices = previews_for_button.clone();
                let palace_id = palace_for_cover.clone();
                view! {
                <div class="palace-cover">
                    {if preview.media.kind == locora_app_api::MediaKind::Video {
                        // The first frame is the thumbnail: paused, muted, no controls.
                        view! { <VideoThumb media=preview.media class="palace-cover-media" /> }.into_any()
                    } else {
                        view! { <Shown media=preview.media class="palace-cover-media" /> }.into_any()
                    }}
                    <button type="button" class="palace-cover-open" title=t("Palast öffnen")
                        disabled=move || busy.get() on:click={
                            let open_id = cover_open_id.clone();
                            move |ev| { ev.stop_propagation(); screen.set(Screen::Palace(open_id.clone())); }
                        }></button>
                    <button type="button" class="palace-cover-change" title=t("Raumvorschaubild wechseln")
                        disabled=!can_change_cover on:click=move |ev| {
                            ev.stop_propagation();
                            let Some(current) = selected_preview.get_untracked() else { return; };
                            let index = choices.iter().position(|item| item.id == current.id).unwrap_or(0);
                            if let Some(next) = choices.get((index + 1) % choices.len()) {
                                startup::set_cover(&palace_id, &next.id);
                                selected.set(Some(next.id.clone()));
                            }
                        }>
                        {format!("{} ↻", preview.name)}
                    </button>
                </div>
                }
            })}
            <button class="name palace-title" disabled=move || busy.get()>{palace.name.clone()}</button>
            {palace.strength.filter(|s| s.loci > 0).map(|s| {
                let text = crate::achievements::strength_text(s);
                view! {
                    <div class="strength">
                        <span class="meta">{text.clone()}<Info text="Ein Ort gilt als gefestigt, wenn er gezeigt hat, dass er mindestens 30 Tage hält: Es zählen nur Wiederholungen an verschiedenen Tagen, und ein Ort braucht dafür eine richtige Antwort an einem späteren Tag als dem ersten. Mehrmals am selben Tag zu üben bringt ihn nicht dorthin." /></span>
                        <meter min="0" max=s.loci.to_string() value=s.solid.to_string() aria-label=text></meter>
                    </div>
                }
            })}
            <div class="row">
                <span class="meta">{format!("{} · {}", loci_text(palace.loci), status)}</span>
                {built_in.then(|| {
                    let delete_id = delete_id.clone();
                    let delete_name = delete_name.clone();
                    view! {
                        <button class="ghost" disabled=move || busy.get() on:click=move |ev| {
                            ev.stop_propagation();
                            if busy.get_untracked() { return; }
                            let question = crate::i18n::tf("„{0}“ löschen? Du kannst ihn später wieder anlegen.", &[&delete_name]);
                            let confirmed = web_sys::window()
                                .and_then(|window| window.confirm_with_message(&question).ok())
                                .unwrap_or(false);
                            if !confirmed { return; }
                            busy.set(true);
                            let palace_id = delete_id.clone();
                            let reload = expect_context::<HomeReload>().0;
                            spawn_local(async move {
                                match call::<()>("palace_delete", DeletePalaceArgs { palace_id }).await {
                                    Ok(()) => {
                                        busy.set(false);
                                        reload.run(());
                                    }
                                    Err(error) => {
                                        busy.set(false);
                                        if let Some(window) = web_sys::window() {
                                            let _ = window.alert_with_message(&error);
                                        }
                                    }
                                }
                            });
                        }>{t("Löschen")}</button>
                    }
                })}
                <button
                    class="primary play"
                    title=t("Route ablaufen")
                    disabled=move || busy.get()
                    on:click=move |ev| {
                        ev.stop_propagation();
                        if busy.get_untracked() { return; }
                        screen
                            .set(
                                Screen::Journey(JourneyStart {
                                    palace_id: Some(play_id.clone()),
                                    ..Default::default()
                                }),
                            )
                    }
                >
                    "▶"
                </button>
            </div>
        </article>
    }
}

/// Unsaved editor changes keep the command palette from navigating away.
fn guard_palette(blocked: RwSignal<bool>) {
    if let Some(palette::Editing(editing)) = use_context::<palette::Editing>() {
        Effect::new(move |_| editing.set(blocked.get()));
        on_cleanup(move || editing.set(false));
    }
}

#[derive(Serialize)]
struct OffsetArgs {
    offset: locora_app_api::UtcOffset,
}

#[derive(Serialize)]
struct NotificationArgs {
    notification: NotificationView,
}

#[derive(Serialize)]
struct IdArgs {
    id: String,
}

#[component]
fn PalaceScreen(
    id: String,
    screen: RwSignal<Screen>,
    /// Open with the editor on (a palace just made from a picture or video).
    #[prop(optional)]
    editing: bool,
) -> impl IntoView {
    // The editor may delete the palace it shows and must then leave this screen.
    provide_context(screen);
    let palace = load(call::<PalaceView>(CMD_PALACE, IdArgs { id: id.clone() }));
    let blocked = RwSignal::new(false);
    guard_palette(blocked);
    let saved = RwSignal::new(Ok::<String, String>(String::new()));
    // The dialog closes before the file is written: no second export meanwhile.
    let exporting = RwSignal::new(false);
    let reverse = RwSignal::new(false);
    let numbers = RwSignal::new(false);
    let shuffle = RwSignal::new(false);
    let quiz_answers = RwSignal::new(4u8);
    let sudden_death = RwSignal::new(false);
    let time_limit = RwSignal::new(0u8);
    let id = StoredValue::new(id);
    let start = move |due_only: bool| {
        screen.set(Screen::Journey(JourneyStart {
            palace_id: Some(id.get_value()),
            due_only,
            reverse: reverse.get_untracked(),
            numbers: numbers.get_untracked(),
            shuffle: shuffle.get_untracked(),
            inputs: Vec::new(),
            quiz_answers: Some(quiz_answers.get_untracked()),
            sudden_death: sudden_death.get_untracked(),
            time_limit: Some(time_limit.get_untracked()).filter(|seconds| *seconds > 0),
        }))
    };
    view! {
        <header class="bar">
            <button class="back" title=t("Zurück") disabled=move || blocked.get() on:click=move |_| screen.set(start_screen())>
                "←"
            </button>
            <h1>
                {move || palace.get().and_then(|p| p.ok()).map(|p| p.name).unwrap_or_default()}
            </h1>
            <button disabled=move || blocked.get() on:click=move |_| start(true)>{t("Fällige")}</button>
            <button
                disabled=move || blocked.get() || exporting.get()
                title=t("Als .locora-Datei speichern (mit Wiederholungsstand)")
                on:click=move |_| {
                    if exporting.get_untracked() {
                        return;
                    }
                    let id = id.get_value();
                    saved.set(Ok(String::new()));
                    exporting.set(true);
                    spawn_local(async move {
                        match call::<Option<String>>(locora_app_api::CMD_EXPORT_PICK, IdArgs { id })
                            .await
                        {
                            Ok(Some(text)) => saved.set(Ok(crate::i18n::translate(&text))),
                            Err(text) => saved.set(Err(text)),
                            Ok(None) => {}
                        }
                        exporting.set(false);
                    });
                }
            >
                {t("Exportieren…")}
            </button>
            <button disabled=move || blocked.get() || exporting.get()
                title=t("Bearbeitbare TOML-Quelle und Medien in einen Ordner exportieren (ohne Wiederholungsstand)")
                on:click=move |_| {
                    if exporting.get_untracked() { return; }
                    let id = id.get_value();
                    saved.set(Ok(String::new()));
                    exporting.set(true);
                    spawn_local(async move {
                        match call::<Option<String>>(locora_app_api::CMD_DUMP_PICK, IdArgs { id }).await {
                            Ok(Some(text)) => saved.set(Ok(crate::i18n::translate(&text))),
                            Err(text) => saved.set(Err(text)),
                            Ok(None) => {}
                        }
                        exporting.set(false);
                    });
                }>
                {t("TOML exportieren…")}
            </button>
            <button id="tour-palace-route" class="primary" disabled=move || blocked.get() on:click=move |_| start(false)>
                {t("▶ Route ablaufen")}
            </button>
        </header>
        {move || match saved.get() {
            Ok(text) => view! { <p class="status" role="status">{text}</p> }.into_any(),
            Err(text) => view! { <p class="notes" role="alert">{text}</p> }.into_any(),
        }}
        <first_palace::GuidePanel palace_id=id.get_value() screen=screen />
        <details class="modes" open=!editing && wide_screen()>
            <summary>{t("Modi und Optionen")}</summary>
            <label>
                <input type="checkbox" bind:checked=reverse />
                {t("Rückwärts (Inhalt → Ort)")}
            </label>
            <label>
                <input type="checkbox" bind:checked=numbers />
                {t("Nur Ortsnummern")}
            </label>
            <label>
                <input type="checkbox" bind:checked=shuffle />
                {t("Gemischt")}
            </label>
            <label>
                {t("Antworten pro Quizfrage ")}
                <input type="number" min="2" max="30" prop:value=move || quiz_answers.get().to_string()
                    on:change=move |event| {
                        if let Ok(count) = event_target_value(&event).parse::<u8>() {
                            quiz_answers.set(count.clamp(2, 30));
                        }
                    } />
            </label>
            <label>
                <input type="checkbox" bind:checked=sudden_death />
                {t("Sudden Death: die erste falsche Antwort beendet die Runde")}
            </label>
            <label>
                {t("Sekunden pro Frage (0 = ohne Zeitlimit) ")}
                <input type="number" min="0" max="120" prop:value=move || time_limit.get().to_string()
                    on:change=move |event| {
                        if let Ok(seconds) = event_target_value(&event).parse::<u8>() {
                            time_limit.set(seconds.min(120));
                        }
                    } />
            </label>
        </details>
        {move || match palace.get() {
            None => view! { <Loading /> }.into_any(),
            Some(Err(e)) => view! { <p class="notes">{e}</p> }.into_any(),
            Some(Ok(p)) => view! { <editor::Rooms palace=p blocked=blocked initial_editing=editing /> }.into_any(),
        }}
    }
}

/// A revealed picture, sized relative to the stage rather than the marker.
#[component]
pub fn LocusPicture(media: Media, x: f64, y: f64, size: f64) -> impl IntoView {
    view! {
        <div class="locus-picture"
            style:left=format!("{}%", x * 100.0)
            style:top=format!("{}%", y * 100.0)
            style:width=format!("{}%", size * 100.0)
        >
            <Shown media=media />
        </div>
    }
}

/// The answer side: text, the locus picture and sound clip (they belong to the
/// answer) and the memory aid. `autoplay` plays the sound clip at once.
#[component]
pub fn Answer(
    solution: String,
    note: String,
    picture: Option<Media>,
    #[prop(default = None)] audio: Option<Media>,
    #[prop(optional)] autoplay: bool,
) -> impl IntoView {
    // The picture sits beside the solution, so the answer takes less height on a phone.
    view! {
        <div class="answer">
            <div class="solution">{solution}</div>
            {picture
                .map(|m| {
                    view! {
                        <div class="picture">
                            <Shown media=m />
                        </div>
                    }
                })}
        </div>
        {audio.map(|media| view! { <audio::LocusAudio media=media autoplay=autoplay /> })}
        {(!note.is_empty()).then(|| view! { <div class="note">{format!("✎ {note}")}</div> })}
    }
}
