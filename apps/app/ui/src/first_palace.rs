//! „Dein erster eigener Palast“: a guided exercise with the existing flows. A photo of your own
//! room becomes a palace (media import), you set 5 places in the editor, write a 5-item
//! shopping list into them and walk the route once (journey). The journey schedules the first
//! review; walking the route unlocks the achievement „Erster eigener Palast“.

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    AchievementView, CMD_ACHIEVEMENTS, CMD_HOME_PICK_MEDIA, CMD_PALACE, CMD_SETTINGS, JourneyStart,
    PalaceView, SettingsView, Status,
};
use serde::Serialize;

use crate::achievements::Celebration;
use crate::i18n::{t, tf};
use crate::sply::Sply;
use crate::tauri::{NoArgs, call};
use crate::{Screen, art};

const KEY: &str = "locora-first-palace-v1";
/// Whether „Erster eigener Palast“ was unlocked when the guide started.
const HAD: &str = "locora-first-palace-had-v1";

async fn first_palace_unlocked() -> bool {
    call::<Vec<AchievementView>>(CMD_ACHIEVEMENTS, NoArgs {})
        .await
        .is_ok_and(|list| list.iter().any(|a| a.id == "first-palace" && a.unlocked))
}
/// Places and list items the exercise asks for.
const COUNT: usize = 5;

/// The palace the guide is about, kept on this device so it survives a reload.
#[derive(Clone, Copy)]
pub struct Guide(pub RwSignal<Option<String>>);

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn load() -> Option<String> {
    storage()?
        .get_item(KEY)
        .ok()
        .flatten()
        .filter(|id| !id.is_empty())
}

fn save(id: Option<&str>) {
    if let Some(storage) = storage() {
        let _ = match id {
            Some(id) => storage.set_item(KEY, id),
            None => storage.remove_item(KEY),
        };
    }
}

/// Step ④ and the lesson „Einen Palast wählen“: pick a photo; the new palace opens in the editor
/// with the guide above it.
pub fn start(screen: RwSignal<Screen>) {
    let Guide(guide) = expect_context::<Guide>();
    spawn_local(async move {
        if let Ok(Some(palace)) = call::<Option<PalaceView>>(CMD_HOME_PICK_MEDIA, NoArgs {}).await {
            // Whether the achievement was there before: then finishing is not new.
            let had = first_palace_unlocked().await;
            if let Some(storage) = storage() {
                let _ = storage.set_item(HAD, if had { "1" } else { "0" });
            }
            save(Some(&palace.id));
            guide.set(Some(palace.id.clone()));
            screen.set(Screen::Imported(palace));
        }
    });
}

#[derive(Serialize)]
struct IdArgs {
    id: String,
}

/// What the palace has so far: places, and places with something to remember.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Progress {
    places: usize,
    filled: usize,
}

/// The guide above the palace it is about; nothing on other palaces. With `only_finished` (on
/// the journey) it shows only once the route is walked.
#[component]
pub fn GuidePanel(
    palace_id: String,
    screen: RwSignal<Screen>,
    #[prop(optional)] only_finished: bool,
) -> impl IntoView {
    let Guide(guide) = expect_context::<Guide>();
    let mine = {
        let palace_id = palace_id.clone();
        move || guide.get().as_deref() == Some(palace_id.as_str())
    };
    let progress = RwSignal::new(None::<Progress>);
    let finished = RwSignal::new(false);
    let reminders = RwSignal::new(false);
    let celebration = use_context::<Celebration>();
    let list = RwSignal::new(
        [t("Milch"), t("Brot"), t("Äpfel"), t("Käse"), t("Kaffee")]
            .map(String::from)
            .to_vec(),
    );
    let id = StoredValue::new(palace_id);
    let check = {
        let mine = mine.clone();
        move || {
            if !mine() || finished.get_untracked() {
                return;
            }
            spawn_local(async move {
                let Ok(palace) =
                    call::<PalaceView>(CMD_PALACE, IdArgs { id: id.get_value() }).await
                else {
                    return;
                };
                let loci = palace.rooms.iter().flat_map(|room| &room.loci);
                let places = loci.clone().count();
                let filled: Vec<_> = loci.filter(|locus| !locus.item.trim().is_empty()).collect();
                // The journey gave every filled place its first review.
                let walked = places >= COUNT
                    && filled.len() >= COUNT
                    && filled.iter().all(|locus| locus.status != Status::Neu);
                let now = Some(Progress {
                    places,
                    filled: filled.len(),
                });
                if progress.get_untracked() != now {
                    progress.set(now);
                }
                if walked && !finished.get_untracked() {
                    let unlocked = first_palace_unlocked().await;
                    // Unlocked before this guide (another own palace): nothing new to celebrate.
                    let before = storage()
                        .and_then(|s| s.get_item(HAD).ok().flatten())
                        .is_some_and(|had| had == "1");
                    if let Ok(settings) = call::<SettingsView>(CMD_SETTINGS, NoArgs {}).await {
                        reminders.set(settings.reminders.enabled);
                    }
                    finished.set(true);
                    if let (true, false, Some(Celebration(signal))) =
                        (unlocked, before, celebration)
                    {
                        signal.set(Some("first-palace".into()));
                    }
                }
            });
        }
    };
    check();
    let timer = set_interval_with_handle(check, std::time::Duration::from_millis(1500)).ok();
    on_cleanup(move || {
        if let Some(timer) = timer {
            timer.clear();
        }
    });
    let close = move || {
        save(None);
        guide.set(None);
    };
    let art = art();
    move || {
        (mine() && (!only_finished || finished.get())).then(|| {
            let body = move || {
                if finished.get() {
                    return view! {
                        <h2>{t("Geschafft – dein erster eigener Palast!")}</h2>
                        <p>{t("Du hast deine Einkaufsliste im eigenen Palast abgelegt und die Route einmal abgelaufen.")}</p>
                        <p>{if reminders.get() {
                            t("Morgen erinnert dich Locora an die erste Wiederholung.")
                        } else {
                            t("Morgen ist die erste Wiederholung fällig. Du siehst sie auf der Startseite – oder schalte in den Einstellungen Erinnerungen ein.")
                        }}</p>
                        <button class="primary" on:click=move |_| close()>{t("Fertig")}</button>
                    }.into_any();
                }
                let Progress { places, filled } = progress.get().unwrap_or(Progress { places: 0, filled: 0 });
                if places < COUNT {
                    view! {
                        <h2>{t("Schritt 1 von 3: Orte setzen")}</h2>
                        <p>{t("Tippe nacheinander auf 5 Stellen im Foto, die dir auffallen – immer im Uhrzeigersinn. Schreib bei jedem Ort ins Feld „Ort“, was dort ist, zum Beispiel „Tür“.")}</p>
                        <p class="meta">{tf("{0} von {1} Orten gesetzt", &[&places.min(COUNT), &COUNT])}</p>
                    }.into_any()
                } else if filled < COUNT {
                    let inputs = (0..COUNT).map(|i| view! {
                        <li><input type="text" aria-label=tf("Ding {0}", &[&(i + 1)])
                            prop:value=move || list.with(|l| l[i].clone())
                            on:input=move |ev| list.update(|l| l[i] = event_target_value(&ev)) /></li>
                    }).collect_view();
                    view! {
                        <h2>{t("Schritt 2 von 3: Einkaufsliste ablegen")}</h2>
                        <p>{t("Schreib bei jedem Ort ein Ding aus der Liste ins Feld „Inhalt“ – in dieser Reihenfolge. Ins Feld „Merkhilfe“ kommt dein verrücktes Bild, das Ding und Ort verbindet.")}</p>
                        <ol class="first-palace-list">{inputs}</ol>
                        <p class="meta">{tf("{0} von {1} Inhalten eingetragen", &[&filled.min(COUNT), &COUNT])}</p>
                    }.into_any()
                } else {
                    view! {
                        <h2>{t("Schritt 3 von 3: Route ablaufen")}</h2>
                        <p>{t("Geh deine Route jetzt einmal durch. Locora fragt dich Ort für Ort; deck auf und sag, wie gut du es wusstest.")}</p>
                        <button class="primary" on:click=move |_| screen.set(Screen::Journey(JourneyStart {
                            palace_id: Some(id.get_value()),
                            ..Default::default()
                        }))>{t("Jetzt die Route ablaufen ▶")}</button>
                    }.into_any()
                }
            };
            view! {
                <section class="card first-palace" aria-label=t("Dein erster eigener Palast")>
                    <div class="first-palace-figure" aria-hidden="true">
                        {move || art.get().map(|a| view! { <Sply media=a.simo /> })}
                    </div>
                    <div class="first-palace-text">
                        {body}
                        {move || (!finished.get()).then(|| view! {
                            <button class="link" on:click=move |_| close()>{t("Anleitung schließen")}</button>
                        })}
                    </div>
                </section>
            }
        })
    }
}
