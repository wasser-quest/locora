//! Starting the app tour from any button. While the lessons of „Mnemotechnik lernen“ are not all
//! done, a friendly question comes first: learn the techniques first, or start the tour anyway.
//! The learn page's closing card starts the tour directly.

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    AchievementView, CMD_ACHIEVEMENTS, CMD_FORMULA_GUIDE, CMD_TOUR, CMD_TUTORIAL, HighlightRequest,
    LanguageArgs, TourAction,
};

use crate::i18n::t;
use crate::sply::Sply;
use crate::tauri::{NoArgs, call};
use crate::{HomeReload, Screen, TourArgs, art, current_language, route_tour_step};

/// The app tour's state, shared by the start buttons, the question and the tour panel.
#[derive(Clone, Copy)]
pub struct TourControl {
    pub tour: RwSignal<Option<HighlightRequest>>,
    pub error: RwSignal<String>,
    pub route_ready: RwSignal<bool>,
    /// A start is on its way: the buttons wait.
    pub busy: RwSignal<bool>,
    /// The question „lessons first?“ is open.
    pub asking: RwSignal<bool>,
}

pub fn control() -> TourControl {
    expect_context::<TourControl>()
}

/// Whether every lesson's exercise is done (the achievement `learn-all`). When the
/// achievements cannot be read, the tour is not held up.
pub async fn lessons_done() -> bool {
    match call::<Vec<AchievementView>>(CMD_ACHIEVEMENTS, NoArgs {}).await {
        Ok(list) => list
            .iter()
            .any(|achievement| achievement.id == "learn-all" && achievement.unlocked),
        Err(_) => true,
    }
}

/// A tour button: asks first while lessons are open, otherwise starts the tour.
pub fn request(screen: RwSignal<Screen>) {
    let control = control();
    if control.busy.get_untracked() {
        return;
    }
    let reload = home_reload(screen);
    control.busy.set(true);
    spawn_local(async move {
        let done = lessons_done().await;
        control.busy.set(false);
        if done {
            begin_with(control, screen, reload);
        } else {
            control.asking.set(true);
        }
    });
}

/// Starts the tour at once.
pub fn begin(screen: RwSignal<Screen>) {
    begin_with(control(), screen, home_reload(screen));
}

/// The start page's reload, only while the start page is shown: a start page left earlier may
/// still be found in the context, but its reload is gone.
fn home_reload(screen: RwSignal<Screen>) -> Option<HomeReload> {
    matches!(screen.get_untracked(), Screen::Home | Screen::Core)
        .then(use_context::<HomeReload>)
        .flatten()
}

fn begin_with(control: TourControl, screen: RwSignal<Screen>, reload: Option<HomeReload>) {
    if control.busy.get_untracked() {
        return;
    }
    control.asking.set(false);
    control.busy.set(true);
    control.route_ready.set(false);
    spawn_local(async move {
        // The tour always sets up both built-in palaces again, also after they were deleted.
        let lang = || LanguageArgs {
            lang: current_language(),
        };
        let mut created = call::<String>(CMD_TUTORIAL, lang()).await.is_ok();
        created |= call::<String>(CMD_FORMULA_GUIDE, lang()).await.is_ok();
        if created {
            match reload {
                Some(HomeReload(reload)) => reload.run(()),
                // Not on the start page: it loads afresh when the tour opens it.
                None => {
                    let now = screen.get_untracked();
                    if matches!(now, Screen::Home | Screen::Core) {
                        screen.set(now);
                    }
                }
            }
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
                control.tour.set(step.clone());
                match route_tour_step(step, screen).await {
                    Ok(()) => {
                        control.error.set(String::new());
                        control.route_ready.set(true);
                    }
                    Err(error) => control.error.set(error),
                }
            }
            Err(error) => control.error.set(error),
        }
        control.busy.set(false);
    });
}

/// The question before the tour, with Mina.
#[component]
pub fn TourQuestion(screen: RwSignal<Screen>) -> impl IntoView {
    let control = control();
    let art = art();
    move || {
        control.asking.get().then(|| {
            view! {
                <div class="tour-ask-backdrop">
                    <section class="card tour-ask" role="dialog" aria-modal="true" aria-labelledby="tour-ask-title">
                        <div class="tour-ask-figure" aria-hidden="true">
                            {move || art.get().and_then(|a| a.characters.get("mina").cloned()).map(|m| view! { <Sply media=m /> })}
                        </div>
                        <div class="tour-ask-text">
                            <h2 id="tour-ask-title">{t("Hast du die Mnemotechniken schon geübt?")}</h2>
                            <p>{t("Mach am besten zuerst das – dann verstehst du die Tour viel besser.")}</p>
                            <div class="row">
                                <button class="primary" on:click=move |_| {
                                    control.asking.set(false);
                                    screen.set(Screen::Learn);
                                }>{t("Zuerst Mnemotechnik lernen")}</button>
                                <button disabled=move || control.busy.get() on:click=move |_| begin(screen)>
                                    {t("Ich kenne das schon – Tour starten")}
                                </button>
                            </div>
                        </div>
                    </section>
                </div>
            }
        })
    }
}
