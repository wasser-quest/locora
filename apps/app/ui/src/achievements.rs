//! The built-in achievements: one medal each. Unlocked, the medal is coloured and clicking it
//! spins it; locked, it is grey, still, and says what unlocks it. The lessons of „Mnemotechnik
//! lernen“ are grouped under „Mnemotechnik“. A new achievement is celebrated with a short card.

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    AchievementArgs, AchievementView, CMD_ACHIEVEMENT_UNLOCK, CMD_ACHIEVEMENTS, Media, UiAssets,
};

use crate::Art;
use crate::i18n::{t, tf};
use crate::learn_content::lessons;
use crate::sply::Sply;
use crate::tauri::{NoArgs, call};

/// Title and unlock hint of an achievement; `None` for an unknown id.
pub fn describe(id: &str) -> Option<(String, String)> {
    let fixed = |title: &str, hint: &str| Some((title.to_owned(), hint.to_owned()));
    match id {
        "tutorial-done" => fixed(
            t("Tutorial abgeschlossen"),
            t("Schließe das Tutorial „Locora lernen“ ab."),
        ),
        "formulas-done" => fixed(t("Formeln verstanden"), t("Schließe den Formel-Guide ab.")),
        "tour-done" => fixed(
            t("Tour gemeistert"),
            t("Geh die App-Tour bis zum letzten Schritt und tippe auf „Fertig“."),
        ),
        "learn-all" => fixed(
            t("Alle Techniken gelernt"),
            t("Schaffe die Übungen aller Lektionen in „Mnemotechnik lernen“."),
        ),
        "first-palace" => fixed(
            t("Erster eigener Palast"),
            t("Bau einen eigenen Palast und geh seine Route einmal ab."),
        ),
        _ => {
            let lesson = id.strip_prefix("learn-")?;
            let lesson = lessons().into_iter().find(|l| l.id == lesson)?;
            Some((
                lesson.title.to_owned(),
                tf("Schaffe die Übung der Lektion „{0}“.", &[&lesson.title]),
            ))
        }
    }
}

/// The medal shown for an achievement, coloured or grey.
pub fn medal_for(art: &UiAssets, id: &str, unlocked: bool) -> Option<Media> {
    let (on, off) = match id {
        "tutorial-done" => (&art.medal_tutorial, &art.medal_tutorial_locked),
        "formulas-done" => (&art.medal_formulas, &art.medal_formulas_locked),
        _ => {
            let pair = art.medals.get(id)?;
            (&pair.unlocked, &pair.locked)
        }
    };
    Some(if unlocked { on.clone() } else { off.clone() })
}

/// Unlocks an achievement the UI earned and celebrates it when it is new.
pub fn unlock(id: &'static str) {
    let celebration = use_context::<Celebration>();
    spawn_local(async move {
        let new = call::<bool>(
            CMD_ACHIEVEMENT_UNLOCK,
            AchievementArgs { id: id.to_owned() },
        )
        .await;
        if let (Ok(true), Some(Celebration(signal))) = (new, celebration) {
            signal.set(Some(id.to_owned()));
        }
    });
}

/// The achievement being celebrated right now.
#[derive(Clone, Copy)]
pub struct Celebration(pub RwSignal<Option<String>>);

/// A short card for a new achievement: its medal, its title, and a button to close it.
#[component]
pub fn CelebrationCard() -> impl IntoView {
    let Celebration(shown) = expect_context::<Celebration>();
    let art = expect_context::<Art>().0;
    // It goes by itself after a while.
    Effect::new(move |_| {
        if let Some(id) = shown.get() {
            set_timeout(
                move || {
                    if shown.get_untracked().as_deref() == Some(id.as_str()) {
                        shown.set(None);
                    }
                },
                std::time::Duration::from_secs(10),
            );
        }
    });
    move || {
        let id = shown.get()?;
        let (title, _) = describe(&id)?;
        let medal = art.get().and_then(|art| medal_for(&art, &id, true));
        Some(view! {
            <aside class="card celebration" role="status" aria-live="polite">
                {medal.map(|m| view! { <div class="celebration-medal"><Sply media=m /></div> })}
                <div class="text">
                    <strong>{t("Neuer Erfolg!")}</strong>
                    <span class="celebration-title">{title}</span>
                </div>
                <button on:click=move |_| shown.set(None)>{t("Schließen")}</button>
            </aside>
        })
    }
}

fn item(art: &UiAssets, achievement: &AchievementView) -> Option<impl IntoView + use<>> {
    let (title, hint) = describe(&achievement.id)?;
    let unlocked = achievement.unlocked;
    let media = medal_for(art, &achievement.id, unlocked)?;
    Some(view! {
        <li class="medal" class:locked=!unlocked aria-disabled=(!unlocked).then_some("true")
            data-achievement=achievement.id.clone()>
            <Sply media=media still=!unlocked class="medal-art"/>
            <span class="medal-title">{title}</span>
            {(!unlocked).then(|| view! { <span class="medal-hint">{hint}</span> })}
        </li>
    })
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
        let (lessons, others): (Vec<_>, Vec<_>) = list
            .into_iter()
            .partition(|a| a.id.starts_with("learn-") && a.id != "learn-all");
        let done = lessons.iter().filter(|a| a.unlocked).count();
        let total = lessons.len();
        let main = others.iter().filter_map(|a| item(&art, a)).collect_view();
        let grouped = lessons.iter().filter_map(|a| item(&art, a)).collect_view();
        Some(view! {
            <section class="card panel achievements" aria-label=t("Erfolge")>
                <h2>{t("Erfolge")}</h2>
                <ul class="medals">{main}</ul>
                <details class="medal-group">
                    <summary>{t("Mnemotechnik")}" · "{tf("{0} von {1}", &[&done, &total])}</summary>
                    <ul class="medals">{grouped}</ul>
                </details>
            </section>
        })
    }
}
