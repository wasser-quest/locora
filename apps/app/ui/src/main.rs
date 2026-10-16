//! Locora app UI: start screen, palace with rooms and markers, journeys.

mod editor;
mod journey;
mod palette;
mod settings;
mod sply;
mod tauri;

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_HOME, CMD_PALACE, CMD_TUTORIAL, CMD_UI_ASSETS, Home, JourneyStart, Media, PalaceCard,
    PalaceView, UiAssets,
};
use serde::Serialize;

use crate::sply::{Shown, Sply};
use crate::tauri::call;

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Home,
    Palace(String),
    Imported(PalaceView),
    Journey(JourneyStart),
    Settings,
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let screen = RwSignal::new(Screen::Home);
    let editing = palette::Editing(RwSignal::new(false));
    provide_context(editing);
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
        <main>
            {move || match screen.get() {
                Screen::Home => view! {
                    <HomeView screen=screen offset_revision=offset_revision/>
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

#[component]
fn HomeView(screen: RwSignal<Screen>, offset_revision: RwSignal<u64>) -> impl IntoView {
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
    view! {
        <header class="bar">
            <h1>"Locora"</h1>
            <button disabled=move || busy.get() on:click=pick_media>"Bild/Video wählen…"</button>
            <button disabled=move || busy.get() on:click=import title="Paläste aus einer .locora-Datei oder locora.toml">
                "Importieren…"
            </button>
            <button title="Einstellungen & Statistik" on:click=move |_| screen.set(Screen::Settings)>
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
                if let Some(simo) = home.welcome.clone() {
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
                    <Tiles tiles=home.tiles.clone() />
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
    (!tiles.is_empty()).then(|| {
        view! {
            <section class="tiles" aria-label="Plugins">
                {tiles
                    .into_iter()
                    .map(|t| {
                        view! {
                            <article class="card tile" title=t.plugin>
                                {t.icon.map(|m| view! { <div class="icon"><Sply media=m /></div> })}
                                <div class="text">
                                    <span class="meta">{t.title}</span>
                                    <span class="count">{t.text}</span>
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
    let play_id = palace.id.clone();
    let status = match (palace.due, palace.fresh) {
        (0, 0) => "alles gewusst".to_owned(),
        (due, 0) => format!("{due} fällig"),
        (0, fresh) => format!("{fresh} neu"),
        (due, fresh) => format!("{due} fällig · {fresh} neu"),
    };
    view! {
        <article class="card palace" aria-disabled=move || busy.get()
            on:click=move |_| { if !busy.get_untracked() { screen.set(Screen::Palace(id.clone())); } }>
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
                on:click=move |_| screen.set(Screen::Home)>"←"</button>
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
            <button class="back" title="Zurück" disabled=move || blocked.get() on:click=move |_| screen.set(Screen::Home)>
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
            <button class="primary" disabled=move || blocked.get() on:click=move |_| start(false)>
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
