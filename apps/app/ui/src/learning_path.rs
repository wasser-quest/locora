//! The way into Locora on the start page, in four numbered steps: ① learn the techniques,
//! ② the app tour, ③ one walk through the tutorial palace, ④ a first own palace. A step is done
//! when its achievement is unlocked (kept and synced with the others); the next open step's
//! button is the primary one.

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{AchievementView, CMD_ACHIEVEMENTS, CMD_TUTORIAL, LanguageArgs};

use crate::i18n::t;
use crate::tauri::{NoArgs, call};
use crate::{Screen, current_language};

/// The steps' achievements, in order.
const STEPS: [&str; 4] = ["learn-all", "tour-done", "tutorial-done", "first-palace"];

/// Whether each step is done; `None` while loading.
fn load() -> RwSignal<Option<[bool; 4]>> {
    let done = RwSignal::new(None);
    spawn_local(async move {
        if let Ok(list) = call::<Vec<AchievementView>>(CMD_ACHIEVEMENTS, NoArgs {}).await {
            let unlocked = |id: &str| list.iter().any(|a| a.id == id && a.unlocked);
            done.try_set(Some(STEPS.map(unlocked)));
        }
    });
    done
}

/// The four steps. `always` shows them even when all are done (the welcome card).
#[component]
pub fn LearningPath(screen: RwSignal<Screen>, #[prop(optional)] always: bool) -> impl IntoView {
    let done = load();
    let tour = crate::tour_gate::control();
    let busy = RwSignal::new(false);
    let open_tutorial = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            if let Ok(id) = call::<String>(
                CMD_TUTORIAL,
                LanguageArgs {
                    lang: current_language(),
                },
            )
            .await
            {
                screen.set(Screen::Palace(id));
            }
            busy.try_set(false);
        });
    };
    move || {
        let state = done.get()?;
        if !always && state.iter().all(|d| *d) {
            return None;
        }
        let next = state.iter().position(|d| !d);
        let labels = [
            t("Mnemotechnik lernen"),
            t("App-Tour"),
            t("Tutorial-Palast durchgehen"),
            t("Dein erster eigener Palast"),
        ];
        let steps = (0..4)
            .map(|i| {
                let finished = state[i];
                let primary = next == Some(i);
                let button = (!finished).then(|| match i {
                    0 => view! { <button class:primary=primary on:click=move |_| screen.set(Screen::Learn)>{t("Lernen")}</button> }.into_any(),
                    1 => view! { <button class:primary=primary disabled=move || tour.busy.get() on:click=move |_| crate::tour_gate::request(screen)>{t("App-Tour starten")}</button> }.into_any(),
                    2 => view! { <button class:primary=primary disabled=move || busy.get() on:click=move |_| open_tutorial()>{t("Tutorial öffnen")}</button> }.into_any(),
                    _ => view! { <button class:primary=primary on:click=move |_| crate::first_palace::start(screen)>{t("Foto wählen…")}</button> }.into_any(),
                });
                view! {
                    <li class="path-step" class:done=finished class:next=primary>
                        <span class="path-num" aria-hidden="true">{if finished { "✓".to_owned() } else { (i + 1).to_string() }}</span>
                        <span class="path-label">
                            {labels[i]}
                            {finished.then(|| view! { <span class="visually-hidden">{t(" (erledigt)")}</span> })}
                        </span>
                        {button}
                    </li>
                }
            })
            .collect_view();
        Some(view! {
            <ol class="learning-path" aria-label=t("Dein Weg in Locora")>{steps}</ol>
        })
    }
}
