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
    CMD_HOME, CMD_PALACE, CMD_TUTORIAL, Home, JourneyStart, Media, PalaceCard, PalaceView,
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
    // Learning days follow local time: tell the backend the system's offset
    // (JavaScript reports minutes *west* of UTC).
    spawn_local(async {
        let minutes = -(js_sys::Date::new_0().get_timezone_offset() as i32);
        let _ = call::<()>(locora_app_api::CMD_SET_UTC_OFFSET, UtcOffset { minutes }).await;
    });
    view! {
        <palette::Palette screen=screen editing=editing />
        <main>
            {move || match screen.get() {
                Screen::Home => view! { <HomeView screen=screen/> }.into_any(),
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
fn HomeView(screen: RwSignal<Screen>) -> impl IntoView {
    let home = load(call::<Home>(CMD_HOME, tauri::NoArgs {}));
    let busy = RwSignal::new(false);
    let message = RwSignal::new(String::new());
    let info = RwSignal::new(String::new());
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
            <button disabled=move || busy.get() on:click=import title="Paläste aus einer .locora-Datei">
                "Importieren…"
            </button>
            <button title="Einstellungen & Statistik" on:click=move |_| screen.set(Screen::Settings)>
                "⚙"
            </button>
        </header>
        <p class="notes" role="status">{move || message.get()}</p>
        <p class="status" role="status">{move || info.get()}</p>
        {move || match home.get() {
            None => view! { <p class="loading">"Lädt …"</p> }.into_any(),
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
                                "Wiederholen, was dein Gedächtnis gerade braucht."
                            </div>
                        </div>
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
            <span class="name">{palace.name.clone()}</span>
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
struct UtcOffset {
    minutes: i32,
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
            None => view! { <p class="loading">"Lädt …"</p> }.into_any(),
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
