//! Locora app UI: start screen, palace with rooms and markers, journeys.

mod journey;
mod sply;
mod tauri;

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_HOME, CMD_PALACE, CMD_TUTORIAL, Home, JourneyStart, LocusView, Media, PalaceCard,
    PalaceView,
};
use serde::Serialize;

use crate::sply::{Shown, Sply};
use crate::tauri::call;

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Home,
    Palace(String),
    Journey(JourneyStart),
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let screen = RwSignal::new(Screen::Home);
    view! {
        <main>
            {move || match screen.get() {
                Screen::Home => view! { <HomeView screen=screen/> }.into_any(),
                Screen::Palace(id) => view! { <PalaceScreen id=id screen=screen/> }.into_any(),
                Screen::Journey(start) => {
                    view! { <journey::JourneyView start=start screen=screen/> }.into_any()
                }
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
    let start_tutorial = move |_| {
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
        </header>
        {move || match home.get() {
            None => view! { <p class="loading">"Lädt …"</p> }.into_any(),
            Some(Err(e)) => view! { <p class="notes">{e}</p> }.into_any(),
            Some(Ok(home)) => {
                let waiting = home.due + home.fresh;
                let tutorial = home.tutorial;
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
                            disabled=waiting == 0
                            on:click=move |_| {
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
                    {if home.palaces.is_empty() {
                        view! {
                            <p class="empty">
                                "Noch keine Paläste. Leg einen mit der CLI an: "
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
                                    .map(|p| view! { <PalaceTile palace=p screen=screen/> })
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

#[component]
fn PalaceTile(palace: PalaceCard, screen: RwSignal<Screen>) -> impl IntoView {
    let id = palace.id.clone();
    let play_id = palace.id.clone();
    let status = match (palace.due, palace.fresh) {
        (0, 0) => "alles gewusst".to_owned(),
        (due, 0) => format!("{due} fällig"),
        (0, fresh) => format!("{fresh} neu"),
        (due, fresh) => format!("{due} fällig · {fresh} neu"),
    };
    view! {
        <article class="card palace" on:click=move |_| screen.set(Screen::Palace(id.clone()))>
            <span class="name">{palace.name.clone()}</span>
            <div class="row">
                <span class="meta">{format!("{} · {}", loci_text(palace.loci), status)}</span>
                <button
                    class="primary play"
                    title="Route ablaufen"
                    on:click=move |ev| {
                        ev.stop_propagation();
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

#[derive(Serialize)]
struct IdArgs {
    id: String,
}

#[component]
fn PalaceScreen(id: String, screen: RwSignal<Screen>) -> impl IntoView {
    let palace = load(call::<PalaceView>(CMD_PALACE, IdArgs { id: id.clone() }));
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
        }))
    };
    view! {
        <header class="bar">
            <button class="back" title="Zurück" on:click=move |_| screen.set(Screen::Home)>
                "←"
            </button>
            <h1>
                {move || palace.get().and_then(|p| p.ok()).map(|p| p.name).unwrap_or_default()}
            </h1>
            <button on:click=move |_| start(true)>"Fällige"</button>
            <button class="primary" on:click=move |_| start(false)>
                "▶ Route ablaufen"
            </button>
        </header>
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
            Some(Ok(p)) => view! { <Rooms palace=p /> }.into_any(),
        }}
    }
}

/// Rooms as tabs; the room shows its backdrop with a marker at every locus.
/// Clicking a marker shows the place; revealing shows item, picture and note.
#[component]
fn Rooms(palace: PalaceView) -> impl IntoView {
    let room = RwSignal::new(0usize);
    let selected = RwSignal::new(None::<LocusView>);
    let revealed = RwSignal::new(false);
    let rooms = StoredValue::new(palace.rooms.clone());
    let marker = StoredValue::new(palace.marker.clone());
    view! {
        <div class="tabs" style:margin-top="1rem">
            {palace
                .rooms
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    view! {
                        <button
                            class:on=move || room.get() == i
                            on:click=move |_| {
                                room.set(i);
                                selected.set(None);
                            }
                        >
                            {r.name.clone()}
                        </button>
                    }
                })
                .collect_view()}
        </div>
        {move || {
            let Some(current) = rooms.with_value(|r| r.get(room.get()).cloned()) else {
                return view! { <p class="empty">"Dieser Palast hat noch keine Räume."</p> }
                    .into_any();
            };
            view! {
                <div class="stage">
                    {current.backdrop.clone().map(|m| view! { <Shown media=m class="backdrop" /> })}
                    {current
                        .loci
                        .into_iter()
                        .map(|locus| {
                            let state = locus.status.event();
                            let pick = locus.clone();
                            view! {
                                <div
                                    class="marker"
                                    style:left=format!("{}%", locus.x * 100.0)
                                    style:top=format!("{}%", locus.y * 100.0)
                                >
                                    <Sply media=marker.get_value() state=Signal::stored(state) />
                                    <button
                                        title=format!("Ort {}: {}", locus.number, locus.place)
                                        on:click=move |_| {
                                            selected.set(Some(pick.clone()));
                                            revealed.set(false);
                                        }
                                    ></button>
                                    <span class="num">{locus.number}</span>
                                </div>
                            }
                        })
                        .collect_view()}
                </div>
            }
                .into_any()
        }}
        {move || {
            selected
                .get()
                .map(|locus| {
                    let answer = StoredValue::new(locus.clone());
                    view! {
                        <section class="card panel">
                            <div class="progress">{format!("Ort {}", locus.number)}</div>
                            <div class="prompt">{locus.place.clone()}</div>
                            <Show
                                when=move || revealed.get()
                                fallback=move || {
                                    view! {
                                        <button class="primary" on:click=move |_| revealed.set(true)>
                                            "Aufdecken"
                                        </button>
                                    }
                                }
                            >
                                {move || {
                                    let l = answer.get_value();
                                    view! { <Answer solution=l.item note=l.note picture=l.picture /> }
                                }}
                            </Show>
                        </section>
                    }
                })
        }}
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
