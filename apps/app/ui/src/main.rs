//! Locora app UI: start screen, palace with rooms and markers, journeys.

mod cloud_settings;
mod editor;
mod journey;
mod locus_display;
mod palette;
mod settings;
mod sply;
mod startup;
mod tauri;
mod video_frames;

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_FORMULA_GUIDE, CMD_GAMIFICATION_STATUS, CMD_HOME, CMD_PALACE, CMD_TOUR, CMD_TUTORIAL,
    CMD_UI_ASSETS, GamificationStatus, HighlightRequest, Home, JourneyStart, Media,
    NotificationView, PalaceCard, PalaceView, TourAction, UiAssets,
};
use serde::Serialize;

use crate::sply::{Shown, Sply};
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
    mount_to_body(App);
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
    target_ready: RwSignal<bool>,
    action: TourAction,
) {
    if busy.get_untracked() {
        return;
    }
    if action == TourAction::Next {
        let exists = active.get_untracked().is_some_and(|step| {
            web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.get_element_by_id(&step.target))
                .is_some()
        });
        if !(route_ready.get_untracked() && target_ready.get_untracked() && exists) {
            target_ready.set(false);
            return;
        }
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
                    Err(message) => error.set(message),
                }
            }
            Err(message) => error.set(message),
        }
        busy.set(false);
    });
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
        "tour-palace-route" | "tour-editor-toggle" | "tour-journey-prompt" => {
            let home = call::<Home>(CMD_HOME, tauri::NoArgs {}).await?;
            let id = if let Some(palace) = home.palaces.iter().find(|p| p.name == "Locora lernen") {
                palace.id.clone()
            } else {
                call::<String>(CMD_TUTORIAL, tauri::NoArgs {}).await?
            };
            if step.target == "tour-journey-prompt" {
                if !matches!(screen.get_untracked(), Screen::Journey(ref start) if start.palace_id.as_ref() == Some(&id))
                {
                    screen.set(Screen::Journey(JourneyStart {
                        palace_id: Some(id),
                        ..Default::default()
                    }));
                }
            } else {
                if !matches!(screen.get_untracked(), Screen::Palace(ref current) if current == &id)
                {
                    screen.set(Screen::Palace(id));
                }
            }
        }
        _ => return Err("Unbekanntes Ziel der App-Tour".into()),
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
    let focused = StoredValue::new(None::<(u8, String)>);
    let before = StoredValue::new_local(None::<web_sys::HtmlElement>);
    let highlight = move || {
        let current = active.get_untracked();
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        for id in [
            "tour-home-media",
            "tour-home-import",
            "tour-home-settings",
            "tour-palace-route",
            "tour-editor-toggle",
            "tour-journey-prompt",
        ] {
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
                target_ready.set(false);
            }
        } else {
            focused.set_value(None);
            target_ready.set(false);
            if let Some(element) = before.get_value() {
                let _ = element.focus();
                before.set_value(None);
            }
        }
    };
    highlight();
    let timer = set_interval_with_handle(highlight, std::time::Duration::from_millis(150)).ok();
    on_cleanup(move || {
        if let Some(timer) = timer {
            timer.clear();
        }
    });
    view! {
        <Show when=move || active.get().is_some()>
                <aside class="tour-panel card" role="region" aria-label="App-Tour" aria-live="polite">
                    <div class="tour-simo">{move || art().get().map(|assets| view! { <Sply media=assets.simo /> })}</div>
                    <div class="tour-copy">
                        <small>{move || active.get().map(|step| format!("Schritt {} von {}", step.step, step.total)).unwrap_or_default()}</small>
                        <h2>{move || active.get().map(|step| step.title).unwrap_or_default()}</h2>
                        <p>{move || active.get().map(|step| step.text).unwrap_or_default()}</p>
                        <p class="notes" role="status">{move || {
                            let message = error.get();
                            if !message.is_empty() { message }
                            else if route_ready.get() && !target_ready.get() {
                                "Tour-Ziel nicht gefunden. Du kannst den Schritt erneut laden oder die Tour schließen.".into()
                            } else { String::new() }
                        }}</p>
                        <div class="row">
                            <button disabled=move || busy.get() on:click=move |_| tour_action(active, screen, busy, error, route_ready, target_ready, TourAction::Skip)>"Schließen"</button>
                            <button disabled=move || busy.get() || active.get().is_none_or(|step| step.step <= 1)
                                on:click=move |_| tour_action(active, screen, busy, error, route_ready, target_ready, TourAction::Previous)>"Zurück"</button>
                            <button class="primary" disabled=move || busy.get()
                                on:click=move |_| tour_action(active, screen, busy, error, route_ready, target_ready,
                                    if route_ready.get_untracked() && target_ready.get_untracked() { TourAction::Next } else { TourAction::Resume })>
                                {move || if !route_ready.get() || !target_ready.get() { "Erneut versuchen" } else if active.get().is_some_and(|step| step.step == step.total) { "Fertig" } else { "Weiter" }}
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
    let tour_error = RwSignal::new(String::new());
    let tour_route_ready = RwSignal::new(false);
    spawn_local(async move {
        match call::<Option<HighlightRequest>>(
            CMD_TOUR,
            TourArgs {
                action: TourAction::Resume,
            },
        )
        .await
        {
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
    view! {
        <palette::Palette screen=screen editing=editing />
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

                Screen::Imported(palace) => view! { <ImportedPalace palace=palace screen=screen/> }.into_any(),
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
            <p>"Lädt …"</p>
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
                format!("„{}“ importiert", p.name)
            } else {
                format!("„{}“ importiert als „{}“", p.original_name, p.name)
            }
        })
        .collect();
    if let Some(n) = result.restored {
        parts.push(format!("Wiederholungsstand für {n} Loci übernommen"));
    }
    parts.extend(result.notes.iter().cloned());
    parts.join(" · ")
}

fn loci_text(n: u32) -> String {
    if n == 1 {
        "1 Locus".into()
    } else {
        format!("{n} Loci")
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
        <div class="notifications" role="region" aria-label="Erinnerungen">
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
                                aria-label="Erinnerung schließen"
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
            match call::<String>(CMD_TUTORIAL, tauri::NoArgs {}).await {
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
            match call::<String>(CMD_FORMULA_GUIDE, tauri::NoArgs {}).await {
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
        spawn_local(async move {
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
    view! {
        <header class="bar">
            <h1>{if core_only { "Core" } else { "Locora" }}</h1>
            <button on:click=move |_| screen.set(if core_only { Screen::Home } else { Screen::Core })>
                {if core_only { "Startseite" } else { "Core" }}
            </button>
            <button id="tour-home-media" disabled=move || busy.get() on:click=pick_media>"Bild/Video wählen…"</button>
            <button id="tour-home-import" disabled=move || busy.get() on:click=import title="Paläste aus einer .locora-Datei oder locora.toml">
                "Importieren…"
            </button>
            <button disabled=move || busy.get() on:click=start_tour>"App-Tour"</button>
            <button id="tour-home-settings" title="Einstellungen & Statistik" on:click=move |_| screen.set(Screen::Settings)>
                "⚙"
            </button>
        </header>
        <p class="notes" role="status">{move || message.get()}</p>
        <p class="status" role="status">{move || info.get()}</p>
        {move || match home.get() {
            None => view! { <Loading /> }.into_any(),
            Some(Err(e)) => view! { <p class="notes">{e}</p> }.into_any(),
            Some(Ok(home)) => {
                let waiting = home.due + home.fresh;
                let tutorial = home.tutorial;
                let formula_guide = home.formula_guide;
                if let Some(simo) = home.welcome.clone().filter(|_| !core_only) {
                    return view! {
                        <section class="card welcome">
                            <div class="simo">
                                <Sply media=simo />
                            </div>
                            <div class="text">
                                <h2>"Willkommen bei Locora"</h2>
                                <p>
                                    "Mit der Loci-Methode legst du, was du behalten willst, an Orte, die du "
                                    "gut kennst – und gehst die Route später im Kopf ab."
                                </p>
                                <p class="meta">
                                    "Am schnellsten lernst du Locora mit Locora selbst: Das Tutorial ist ein "
                                    "kleiner Gedächtnispalast mit 3 Räumen und 9 Orten."
                                </p>
                                <div class="row">
                                    <button
                                        class="primary"
                                        disabled=move || busy.get()
                                        on:click=start_tutorial
                                    >
                                        "Tutorial starten"
                                    </button>
                                    <button disabled=move || busy.get() on:click=pick_media>
                                        "Eigener Palast aus Bild/Video…"
                                    </button>
                                    <button disabled=move || busy.get() on:click=start_formula_guide>
                                        "Formel-Guide öffnen"
                                    </button>
                                </div>
                            </div>
                        </section>
                        <Tiles tiles=home.tiles.clone() />
                        {home
                            .notes
                            .into_iter()
                            .map(|n| view! { <p class="notes">{n}</p> })
                            .collect_view()}
                    }
                        .into_any();
                }
                view! {
                    {(!core_only).then(|| view! { <>
                    <section class="card weiter">
                        <div class="text">
                            <div class="count">
                                {if waiting == 0 {
                                    "Heute ist nichts fällig".to_owned()
                                } else {
                                    format!("{} fällig · {} neu", home.due, home.fresh)
                                }}
                            </div>
                            <div class="meta">
                                {if waiting == 0 {
                                    "Simo hält die Laterne – morgen geht es weiter."
                                } else {
                                    "Wiederholen, was dein Gedächtnis gerade braucht."
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
                            "Weiter ▶"
                        </button>
                    </section>
                    {(!tutorial)
                        .then(|| {
                            view! {
                                <section class="card weiter">
                                    <div class="text">
                                        <div class="count">"Locora lernen"</div>
                                        <div class="meta">
                                            "Das Tutorial ist selbst ein Gedächtnispalast: 3 Räume, 9 Orte."
                                        </div>
                                    </div>
                                    <button disabled=move || busy.get() on:click=start_tutorial>
                                        "Tutorial anlegen"
                                    </button>
                                </section>
                            }
                        })}
                    {(!formula_guide)
                        .then(|| {
                            view! {
                                <section class="card weiter">
                                    <div class="text">
                                        <div class="count">"Formeln verstehen"</div>
                                        <div class="meta">"Variablen, Tabellen und Bindungen in einem eigenen Palast üben."</div>
                                    </div>
                                    <button disabled=move || busy.get() on:click=start_formula_guide>
                                        "Formel-Guide anlegen"
                                    </button>
                                </section>
                            }
                        })}
                    <Tiles tiles=home.tiles.clone() />
                    </> })}
                    {if home.palaces.is_empty() {
                        view! {
                            <p class="empty">
                                "Noch keine Paläste. Wähle oben ein Bild oder Video, oder nutze die CLI: "
                                <code>"locora new Planeten -- Merkur Venus Erde"</code>
                            </p>
                        }
                            .into_any()
                    } else {
                        view! {
                            <section class="palaces">
                                {home
                                    .palaces
                                    .into_iter()
                                    .map(|p| view! { <PalaceTile palace=p screen=screen busy=busy/> })
                                    .collect_view()}
                            </section>
                        }
                            .into_any()
                    }}
                    {home
                        .notes
                        .into_iter()
                        .map(|n| view! { <p class="notes">{n}</p> })
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
            <section class="tiles" aria-label="Plugins">
                {tiles
                    .into_iter()
                    .map(|t| {
                        let gamification = t.plugin == "gamification";
                        view! {
                            <article class="card tile" title=t.plugin>
                                {t.icon.map(|m| view! { <div class="icon"><Sply media=m /></div> })}
                                <div class="text">
                                    <span class="meta">{t.title}</span>
                                    <span class="count">{t.text}</span>
                                    {gamification.then(|| view! {
                                        <span class="tile-streak">
                                            {move || art.get().map(|assets| view! { <span class="game-icon" aria-hidden="true"><Sply media=assets.streak /></span> })}
                                            {move || streak.get().map(|days| format!("{days} Tage in Folge"))}
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

#[component]
fn PalaceTile(palace: PalaceCard, screen: RwSignal<Screen>, busy: RwSignal<bool>) -> impl IntoView {
    let id = palace.id.clone();
    let cover_open_id = palace.id.clone();
    let play_id = palace.id.clone();
    let status = match (palace.due, palace.fresh) {
        (0, 0) => "alles gewusst".to_owned(),
        (due, 0) => format!("{due} fällig"),
        (0, fresh) => format!("{fresh} neu"),
        (due, fresh) => format!("{due} fällig · {fresh} neu"),
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
                        view! { <div class="palace-video-preview"><RoomIcon backdrop=Some(preview.media) /></div> }.into_any()
                    } else {
                        view! { <Shown media=preview.media class="palace-cover-media" /> }.into_any()
                    }}
                    <button type="button" class="palace-cover-open" title="Palast öffnen"
                        disabled=move || busy.get() on:click={
                            let open_id = cover_open_id.clone();
                            move |ev| { ev.stop_propagation(); screen.set(Screen::Palace(open_id.clone())); }
                        }></button>
                    <button type="button" class="palace-cover-change" title="Raumvorschaubild wechseln"
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
                <button
                    class="primary play"
                    title="Route ablaufen"
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

/// The import response is already a complete snapshot: open directly in edit mode.
#[component]
fn ImportedPalace(palace: PalaceView, screen: RwSignal<Screen>) -> impl IntoView {
    let blocked = RwSignal::new(false);
    guard_palette(blocked);
    view! {
        <header class="bar">
            <button class="back" disabled=move || blocked.get()
                on:click=move |_| screen.set(start_screen())>"←"</button>
            <h1>{palace.name.clone()}</h1>
        </header>
        <editor::Rooms palace=palace blocked=blocked initial_editing=true />
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
fn PalaceScreen(id: String, screen: RwSignal<Screen>) -> impl IntoView {
    let palace = load(call::<PalaceView>(CMD_PALACE, IdArgs { id: id.clone() }));
    let blocked = RwSignal::new(false);
    guard_palette(blocked);
    let saved = RwSignal::new(Ok::<String, String>(String::new()));
    // The dialog closes before the file is written: no second export meanwhile.
    let exporting = RwSignal::new(false);
    let reverse = RwSignal::new(false);
    let numbers = RwSignal::new(false);
    let shuffle = RwSignal::new(false);
    let id = StoredValue::new(id);
    let start = move |due_only: bool| {
        screen.set(Screen::Journey(JourneyStart {
            palace_id: Some(id.get_value()),
            due_only,
            reverse: reverse.get_untracked(),
            numbers: numbers.get_untracked(),
            shuffle: shuffle.get_untracked(),
            inputs: Vec::new(),
        }))
    };
    view! {
        <header class="bar">
            <button class="back" title="Zurück" disabled=move || blocked.get() on:click=move |_| screen.set(start_screen())>
                "←"
            </button>
            <h1>
                {move || palace.get().and_then(|p| p.ok()).map(|p| p.name).unwrap_or_default()}
            </h1>
            <button disabled=move || blocked.get() on:click=move |_| start(true)>"Fällige"</button>
            <button
                disabled=move || blocked.get() || exporting.get()
                title="Als .locora-Datei speichern (mit Wiederholungsstand)"
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
                            Ok(Some(text)) => saved.set(Ok(text)),
                            Err(text) => saved.set(Err(text)),
                            Ok(None) => {}
                        }
                        exporting.set(false);
                    });
                }
            >
                "Exportieren…"
            </button>
            <button disabled=move || blocked.get() || exporting.get()
                title="Bearbeitbare TOML-Quelle und Medien in einen Ordner exportieren (ohne Wiederholungsstand)"
                on:click=move |_| {
                    if exporting.get_untracked() { return; }
                    let id = id.get_value();
                    saved.set(Ok(String::new()));
                    exporting.set(true);
                    spawn_local(async move {
                        match call::<Option<String>>(locora_app_api::CMD_DUMP_PICK, IdArgs { id }).await {
                            Ok(Some(text)) => saved.set(Ok(text)),
                            Err(text) => saved.set(Err(text)),
                            Ok(None) => {}
                        }
                        exporting.set(false);
                    });
                }>
                "TOML exportieren…"
            </button>
            <button id="tour-palace-route" class="primary" disabled=move || blocked.get() on:click=move |_| start(false)>
                "▶ Route ablaufen"
            </button>
        </header>
        {move || match saved.get() {
            Ok(text) => view! { <p class="status" role="status">{text}</p> }.into_any(),
            Err(text) => view! { <p class="notes" role="alert">{text}</p> }.into_any(),
        }}
        <div class="modes">
            <label>
                <input type="checkbox" bind:checked=reverse />
                "Rückwärts (Inhalt → Ort)"
            </label>
            <label>
                <input type="checkbox" bind:checked=numbers />
                "Nur Ortsnummern"
            </label>
            <label>
                <input type="checkbox" bind:checked=shuffle />
                "Gemischt"
            </label>
        </div>
        {move || match palace.get() {
            None => view! { <Loading /> }.into_any(),
            Some(Err(e)) => view! { <p class="notes">{e}</p> }.into_any(),
            Some(Ok(p)) => view! { <editor::Rooms palace=p blocked=blocked /> }.into_any(),
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

/// The answer side: text, the locus picture (it belongs to the answer) and
/// the memory aid.
#[component]
pub fn Answer(solution: String, note: String, picture: Option<Media>) -> impl IntoView {
    view! {
        <div class="solution">{solution}</div>
        {picture
            .map(|m| {
                view! {
                    <div class="picture">
                        <Shown media=m />
                    </div>
                }
            })}
        {(!note.is_empty()).then(|| view! { <div class="note">{format!("✎ {note}")}</div> })}
    }
}
