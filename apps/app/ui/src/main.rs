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
mod formulas;
mod i18n;
mod i18n_catalog;
mod info;
mod journey;
mod language_choice;
mod locus_display;
mod look;
mod media_format;
mod palette;
mod settings;
mod sply;
mod startup;
mod tauri;
mod theme;
mod video_frames;

use crate::i18n::{t, tf};
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

/// The visible part of the window while an on-screen keyboard covers some of it: its top, its
/// height and how much of the window's bottom is covered, all in CSS pixels. None without a
/// keyboard, or where the browser has no `visualViewport`.
fn visible_area() -> Option<(f64, f64, f64)> {
    let window = web_sys::window()?;
    let full = window.inner_height().ok()?.as_f64()?;
    let viewport = js_sys::Reflect::get(&window, &"visualViewport".into()).ok()?;
    let number = |name: &str| {
        js_sys::Reflect::get(&viewport, &name.into())
            .ok()
            .and_then(|value| value.as_f64())
    };
    let (top, height) = (number("offsetTop")?, number("height")?);
    let covered = full - top - height;
    // Pinch zoom also shrinks the visual viewport; a keyboard takes at least this much.
    (covered > 120.0).then_some((top, height, covered))
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
    // Where the highlighted control is on screen (with its label): the spotlight frames it, and
    // the panel moves to the other half of the screen so it never covers it.
    let spot = RwSignal::new(None::<(f64, f64, f64, f64)>);
    // The part of the window an on-screen keyboard leaves visible (top, height, covered at the
    // bottom). Fixed elements stay put while the keyboard covers them, so the panel follows this.
    let visible = RwSignal::new(None::<(f64, f64, f64)>);
    // A text field has the focus: on a phone its keyboard covers the screen's lower half, also
    // where the browser does not report it (an edge-to-edge app), so the panel goes to the top.
    let typing = RwSignal::new(false);
    let focused = StoredValue::new(None::<(u8, String)>);
    let before = StoredValue::new_local(None::<web_sys::HtmlElement>);
    let highlight = move || {
        let keyboard = visible_area();
        if visible.get_untracked() != keyboard {
            visible.set(keyboard);
        }
        let entering = text_entry_focused();
        if typing.get_untracked() != entering {
            typing.set(entering);
        }
        let current = active.get_untracked();
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        for id in TOUR_TARGETS {
            if let Some(node) = document.get_element_by_id(id) {
                let _ = node.remove_attribute("data-tour-active");
            }
        }
        if let Some(step) = current {
            if before.get_value().is_none() {
                before.set_value(document.active_element().and_then(|element| {
                    wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(element).ok()
                }));
            }
            if !route_ready.get_untracked() {
                focused.set_value(None);
                target_ready.set(false);
                return;
            }
            if let Some(node) = document.get_element_by_id(&step.target) {
                let _ = node.set_attribute("data-tour-active", "true");
                // A checkbox or field is shown with its label, so its text is in the light too.
                let framed = node
                    .closest("label")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| node.clone());
                let rect = framed.get_bounding_client_rect();
                let next = Some((rect.left(), rect.top(), rect.width(), rect.height()));
                if spot.get_untracked() != next {
                    spot.set(next);
                }
                let key = (step.step, step.target);
                if focused.get_value().as_ref() != Some(&key) {
                    node.scroll_into_view_with_bool(false);
                    if let Some(element) =
                        wasm_bindgen::JsCast::dyn_ref::<web_sys::HtmlElement>(&node)
                    {
                        let _ = element.focus();
                    }
                    focused.set_value(Some(key));
                }
                target_ready.set(true);
            } else {
                focused.set_value(None);
                spot.set(None);
                target_ready.set(true);
            }
        } else {
            focused.set_value(None);
            spot.set(None);
            target_ready.set(false);
            if let Some(element) = before.get_value() {
                let _ = element.focus();
                before.set_value(None);
            }
        }
    };
    // The panel takes the half of the visible area the highlighted control is not in.
    let panel_top = move || {
        if typing.get() && visible.get().is_none() {
            return true;
        }
        let (top, height) = match visible.get() {
            Some((top, height, _)) => (top, height),
            None => (
                0.0,
                web_sys::window()
                    .and_then(|w| w.inner_height().ok())
                    .and_then(|h| h.as_f64())
                    .unwrap_or(800.0),
            ),
        };
        spot.get()
            .is_some_and(|(_, y, _, h)| y + h / 2.0 > top + height / 2.0)
    };
    highlight();
    let timer = set_interval_with_handle(highlight, std::time::Duration::from_millis(150)).ok();
    on_cleanup(move || {
        if let Some(timer) = timer {
            timer.clear();
        }
    });
    view! {
        {move || active.get().is_some().then(|| match spot.get() {
            // A frame over everything with the target as its bright hole; the rest is only dimmed.
            Some((x, y, w, h)) => view! {
                <div class="tour-spot" aria-hidden="true"
                    style:left=format!("{}px", x - 8.0) style:top=format!("{}px", y - 8.0)
                    style:width=format!("{}px", w + 16.0) style:height=format!("{}px", h + 16.0)></div>
            }.into_any(),
            None => view! { <div class="tour-scrim" aria-hidden="true"></div> }.into_any(),
        })}
        <Show when=move || active.get().is_some()>
                <aside class="tour-panel card" role="region" aria-label=t("App-Tour") aria-live="polite"
                    class:top=panel_top
                    // With a keyboard open the panel sits in the visible part, above the keyboard
                    // or at the top, and scrolls inside when that part is small.
                    style:bottom=move || visible.get().filter(|_| !panel_top()).map(|(_, _, covered)| format!("{}px", covered + 8.0))
                    style:top=move || visible.get().filter(|_| panel_top()).map(|(top, _, _)| format!("{}px", top + 8.0))
                    style:max-height=move || visible.get().map(|(_, height, _)| format!("{}px", (height / 2.0 - 16.0).max(120.0)))
                    style:overflow-y=move || visible.get().map(|_| "auto")>
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
        <main>
            {move || match screen.get() {
                Screen::Home => view! {
                    <HomeView screen=screen offset_revision=offset_revision tour=tour
                        tour_error=tour_error tour_route_ready=tour_route_ready core_only=false/>
                }.into_any(),
                Screen::Core => view! {
                    <HomeView screen=screen offset_revision=offset_revision tour=tour
                        tour_error=tour_error tour_route_ready=tour_route_ready core_only=true/>
                }.into_any(),

                // A new palace opens like any other, with the editor already on.
                Screen::Imported(palace) => view! { <PalaceScreen id=palace.id screen=screen editing=true/> }.into_any(),
                Screen::Palace(id) => view! { <PalaceScreen id=id screen=screen/> }.into_any(),
                Screen::Journey(start) => {
                    view! { <journey::JourneyView start=start screen=screen/> }.into_any()
                }
                Screen::Settings => view! { <settings::SettingsView screen=screen/> }.into_any(),
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
    tour: RwSignal<Option<HighlightRequest>>,
    tour_error: RwSignal<String>,
    tour_route_ready: RwSignal<bool>,
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
    let start_tour = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        tour_route_ready.set(false);
        let reload = expect_context::<HomeReload>().0;
        spawn_local(async move {
            // The tour always sets up both built-in palaces again, also after they were deleted.
            let mut created = false;
            created |= call::<String>(
                CMD_TUTORIAL,
                LanguageArgs {
                    lang: current_language(),
                },
            )
            .await
            .is_ok();
            created |= call::<String>(
                CMD_FORMULA_GUIDE,
                LanguageArgs {
                    lang: current_language(),
                },
            )
            .await
            .is_ok();
            if created {
                reload.run(());
            }
            match call::<Option<HighlightRequest>>(
                CMD_TOUR,
                TourArgs {
                    action: TourAction::Start,
                },
            )
            .await
            {
                Ok(step) => {
                    tour.set(step.clone());
                    match route_tour_step(step, screen).await {
                        Ok(()) => {
                            tour_error.set(String::new());
                            tour_route_ready.set(true);
                        }
                        Err(error) => tour_error.set(error),
                    }
                }
                Err(error) => {
                    tour_error.set(error.clone());
                    message.set(error);
                }
            }
            busy.set(false);
        });
    };
    let dashboard_open = expect_context::<dashboard::Open>().0;
    view! {
        <header class="bar home-bar">
            <h1>{if core_only { "Core" } else { "Locora" }}</h1>
            <div class="home-actions">
                {core_only.then(|| view! {
                    <button class="wide" on:click=move |_| screen.set(Screen::Home)>{t("Startseite")}</button>
                })}
                <button class="primary wide" id="tour-home-media" disabled=move || busy.get() on:click=pick_media>{t("Bild/Video wählen…")}</button>
                <button id="tour-home-import" disabled=move || busy.get() on:click=import title=t("Paläste aus einer .locora-Datei oder locora.toml")>
                    {t("Importieren…")}
                </button>
                <button disabled=move || busy.get() on:click=start_tour>{t("App-Tour")}</button>
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
                                <p>
                                    {t("Mit der Loci-Methode legst du, was du behalten willst, an Orte, die du ")}
                                    {t("gut kennst – und gehst die Route später im Kopf ab.")}
                                </p>
                                <p class="meta">
                                    {t("Am schnellsten lernst du Locora mit Locora selbst: Das Tutorial ist ein ")}
                                    {t("kleiner Gedächtnispalast mit 3 Räumen und 9 Orten.")}
                                </p>
                                <div class="row">
                                    <button
                                        class="primary"
                                        disabled=move || busy.get()
                                        on:click=start_tour
                                    >
                                        {t("App-Tour starten")}
                                    </button>
                                    <button disabled=move || busy.get() on:click=pick_media>
                                        {t("Eigener Palast aus Bild/Video…")}
                                    </button>
                                    <button disabled=move || busy.get() on:click=start_formula_guide>
                                        {t("Formel-Guide öffnen")}
                                    </button>
                                </div>
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
    let streak = RwSignal::new(None::<u32>);
    if tiles.iter().any(|tile| tile.plugin == "gamification") {
        spawn_local(async move {
            if let Ok(Some(status)) =
                call::<Option<GamificationStatus>>(CMD_GAMIFICATION_STATUS, tauri::NoArgs {}).await
            {
                streak.set(Some(status.streak));
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
                                        <span class="tile-streak">
                                            {move || art.get().map(|assets| view! { <span class="game-icon" aria-hidden="true"><Sply media=assets.streak /></span> })}
                                            {move || streak.get().map(|days| crate::i18n::tf("{0} Tage in Folge", &[&days]))}
                                        </span>
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
