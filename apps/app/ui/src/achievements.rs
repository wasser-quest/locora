//! The built-in achievements on the start page: one medal each. Unlocked, the medal is coloured
//! and clicking it spins it; locked, it is grey, still, and says what unlocks it.

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{AchievementView, CMD_ACHIEVEMENTS, Media, UiAssets};

use crate::Art;
use crate::i18n::t;
use crate::sply::Sply;
use crate::tauri::{NoArgs, call};

/// The medal, title and unlock hint of one achievement; `None` for an unknown id.
fn medal(
    art: &UiAssets,
    achievement: &AchievementView,
) -> Option<(Media, &'static str, &'static str)> {
    let (medal, locked, title, hint) = match achievement.id.as_str() {
        "tutorial-done" => (
            &art.medal_tutorial,
            &art.medal_tutorial_locked,
            "Tutorial abgeschlossen",
            "Schließe das Tutorial „Locora lernen“ ab.",
        ),
        "formulas-done" => (
            &art.medal_formulas,
            &art.medal_formulas_locked,
            "Formeln verstanden",
            "Schließe den Formel-Guide ab.",
        ),
        _ => return None,
    };
    let media = if achievement.unlocked { medal } else { locked };
    Some((media.clone(), title, hint))
}

#[component]
pub fn Achievements() -> impl IntoView {
    let achievements = RwSignal::new(None::<Result<Vec<AchievementView>, String>>);
    spawn_local(async move {
        achievements.set(Some(
            call::<Vec<AchievementView>>(CMD_ACHIEVEMENTS, NoArgs {}).await,
        ));
    });
    let art = expect_context::<Art>().0;
    move || {
        let list = match achievements.get()? {
            Ok(list) if !list.is_empty() => list,
            // Nothing to show yet (still loading) or nothing to unlock.
            _ => return None,
        };
        let art = art.get()?;
        let items = list
            .into_iter()
            .filter_map(|achievement| {
                let (media, title, hint) = medal(&art, &achievement)?;
                let unlocked = achievement.unlocked;
                Some(view! {
                    <li class="medal" class:locked=!unlocked aria-disabled=(!unlocked).then_some("true")>
                        <Sply media=media still=!unlocked class="medal-art"/>
                        <span class="medal-title">{t(title)}</span>
                        {(!unlocked).then(|| view! { <span class="medal-hint">{t(hint)}</span> })}
                    </li>
                })
            })
            .collect_view();
        Some(view! {
            <section class="card panel achievements" aria-label=t("Erfolge")>
                <h2>{t("Erfolge")}</h2>
                <ul class="medals">{items}</ul>
            </section>
        })
    }
}
