//! The built-in achievements: one medal each. Unlocked, the medal is coloured, says since when
//! and clicking it spins it; locked, it is grey, still, and says what unlocks it. The lessons of
//! „Mnemotechnik lernen“ are grouped under „Mnemotechnik“, the gamification plugin's trophies
//! under „Journey-Trophäen“ and the palace medals (from the review schedule) under
//! „Palastmedaillen“. New achievements are celebrated with a short card each, one after another.

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

/// The title of a palace medal of `level` days (`7`, `30`, `180`), without its palace.
pub fn palace_level_title(level: &str) -> Option<&'static str> {
    Some(match level {
        "7" => t("Verankert"),
        "30" => t("Gefestigt"),
        "180" => t("Dauerhaft"),
        _ => return None,
    })
}

/// Title and hint of a palace medal of `level` days for the palace `name`.
fn describe_palace(level: &str, name: &str) -> Option<(String, String)> {
    let title = palace_level_title(level)?;
    Some((
        format!("{title} · {name}"),
        crate::i18n::tf(
            "Mindestens 9 von 10 Orten von „{0}“ halten {1} Tage oder länger.",
            &[&name, &level],
        ),
    ))
}

/// Title and unlock hint of an achievement; `None` for an unknown id. A palace medal comes as
/// `palace-<level>@<palace name>`.
pub fn describe(id: &str) -> Option<(String, String)> {
    let fixed = |title: &str, hint: &str| Some((title.to_owned(), hint.to_owned()));
    if let Some(rest) = id.strip_prefix("palace-") {
        let (level, name) = rest.split_once('@').unwrap_or((rest, ""));
        return describe_palace(level, name);
    }
    match id {
        "game-first-journey" => fixed(t("Erste Journey"), t("Schließe eine Journey ab.")),
        "game-hundred-xp" => fixed(
            t("100 XP"),
            t("Sammle 100 XP mit fälligen Wiederholungen und neuen Orten."),
        ),
        "game-seven-day-streak" => fixed(
            t("7 Lerntage"),
            t("Lerne an 7 Tagen – sie müssen nicht aufeinanderfolgen."),
        ),
        "game-due-cleared" => fixed(
            t("Alles Fällige erledigt"),
            t("Mach an einem Tag jede fällige Wiederholung."),
        ),
        "game-rhythm-4" => fixed(
            t("Wochenziel 4-mal"),
            t("Erreiche dein Wochenziel in 4 Wochen – sie müssen nicht aufeinanderfolgen."),
        ),
        "game-perfect-journey" => fixed(
            t("Fehlerfreie Journey"),
            t("Eine Journey ohne Fehler (aus früheren Versionen)."),
        ),
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

/// The medal shown for an achievement, coloured or grey (a palace medal by its level).
pub fn medal_for(art: &UiAssets, id: &str, unlocked: bool) -> Option<Media> {
    let id = id.split_once('@').map_or(id, |(medal, _)| medal);
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
        if let (Ok(true), Some(celebration)) = (new, celebration) {
            celebration.celebrate(id);
        }
    });
}

/// New achievements waiting to be celebrated, in the order they were earned: the first is
/// shown, the next follows when it is closed or after a while. Nothing earned at the same
/// moment is lost.
#[derive(Clone, Copy)]
pub struct Celebration(RwSignal<Vec<String>>);

impl Celebration {
    pub fn new() -> Self {
        Self(RwSignal::new(Vec::new()))
    }

    /// Celebrates `id` after those waiting (once while it waits).
    pub fn celebrate(&self, id: &str) {
        self.0.update(|queue| {
            if !queue.iter().any(|queued| queued == id) {
                queue.push(id.to_owned());
            }
        });
    }

    /// The one shown now.
    pub fn shown(&self) -> Option<String> {
        self.0.with(|queue| queue.first().cloned())
    }

    /// Closes the one shown (when it is still `id`).
    fn close(&self, id: &str) {
        self.0.update(|queue| {
            if queue.first().is_some_and(|first| first == id) {
                queue.remove(0);
            }
        });
    }
}

impl Default for Celebration {
    fn default() -> Self {
        Self::new()
    }
}

/// The heading of a celebration: one word per kind (trophies of the game, palace medals, the
/// app's achievements).
fn celebration_heading(id: &str) -> &'static str {
    if id.starts_with("game-") {
        t("Neue Trophäe!")
    } else if id.starts_with("palace-") {
        t("Neue Medaille!")
    } else {
        t("Neuer Erfolg!")
    }
}

/// A short card for a new achievement: its medal, its title, a button to share it and one to
/// close it. It goes by itself after 10 seconds, but not while a finger or the focus is on its
/// buttons (4 seconds after they leave), and it waits while the share dialog is open.
#[component]
pub fn CelebrationCard() -> impl IntoView {
    let celebration = expect_context::<Celebration>();
    let art = expect_context::<Art>().0;
    let sharing = use_context::<crate::share::Sharing>().map(|sharing| sharing.0);
    let open = move || sharing.is_some_and(|request| request.with(Option::is_some));
    let paused = RwSignal::new(false);
    // Each new timer makes the ones before it void.
    let generation = StoredValue::new(0_u32);
    // The element that had the focus before the card's buttons got it: back there after sharing.
    let before = StoredValue::new_local(None::<web_sys::HtmlElement>);
    let schedule = move |id: String, secs: u64| {
        let now = generation.get_value().wrapping_add(1);
        generation.set_value(now);
        set_timeout(
            move || {
                let waiting = sharing.is_some_and(|request| {
                    request.try_with_untracked(Option::is_some).unwrap_or(false)
                });
                if generation.try_get_value() == Some(now)
                    && paused.try_get_untracked() == Some(false)
                    && !waiting
                {
                    celebration.close(&id);
                }
            },
            std::time::Duration::from_secs(secs),
        );
    };
    Effect::new(move |_| {
        if let Some(id) = celebration.shown()
            && !open()
        {
            paused.set(false);
            schedule(id, 10);
        }
    });
    let pause = move || {
        paused.set(true);
        generation.update_value(|now| *now = now.wrapping_add(1));
    };
    let resume = move || {
        paused.set(false);
        if let Some(id) = celebration.shown() {
            schedule(id, 4);
        }
    };
    move || {
        if open() {
            return None;
        }
        let id = celebration.shown()?;
        let (title, _) = describe(&id)?;
        let medal = art.get().and_then(|art| medal_for(&art, &id, true));
        let close = id.clone();
        let shared = id.clone();
        let share = move |_| {
            let opener = before
                .get_value()
                .filter(|element| element.is_connected())
                .or_else(|| {
                    crate::modal::focused()
                        .filter(|element| element.closest(".celebration").ok().flatten().is_none())
                });
            celebration.close(&shared);
            crate::share::open(
                vec![crate::share::CardKind::achievement(
                    &shared,
                    Some(crate::share::today()),
                )],
                opener,
            );
        };
        Some(view! {
            <aside class="card celebration" role="status" aria-live="polite"
                on:focusin=move |event: web_sys::FocusEvent| {
                    use wasm_bindgen::JsCast;
                    let from = event
                        .related_target()
                        .and_then(|from| from.dyn_into::<web_sys::HtmlElement>().ok())
                        .filter(|from| from.closest(".celebration").ok().flatten().is_none());
                    if from.is_some() {
                        before.set_value(from);
                    }
                }>
                {medal.map(|m| view! { <div class="celebration-medal"><Sply media=m /></div> })}
                <div class="text">
                    <strong>{celebration_heading(&id)}</strong>
                    <span class="celebration-title">{title.clone()}</span>
                </div>
                <div class="celebration-actions"
                    on:pointerenter=move |_| pause() on:pointerleave=move |_| resume()
                    on:focusin=move |_| pause() on:focusout=move |_| resume()>
                    <button type="button" aria-label=tf("„{0}“ teilen…", &[&title]) on:click=share>{t("Teilen…")}</button>
                    <button type="button" on:click=move |_| celebration.close(&close)>{t("Schließen")}</button>
                </div>
            </aside>
        })
    }
}

/// How much of a palace is consolidated, for its card (the meter shows the same).
pub fn strength_text(strength: locora_app_api::Strength) -> String {
    if strength.loci == 1 {
        crate::i18n::tf("Gefestigt: {0} von 1 Ort", &[&strength.solid])
    } else {
        crate::i18n::tf(
            "Gefestigt: {0} von {1} Orten",
            &[&strength.solid, &strength.loci],
        )
    }
}

/// The progress of a palace medal: how many loci hold long enough.
pub fn palace_progress(id: &str, (hold, loci): (u32, u32)) -> String {
    let level = id.strip_prefix("palace-").unwrap_or_default();
    crate::i18n::tf(
        "{0} von {1} Orten halten {2}+ Tage",
        &[&hold, &loci, &level],
    )
}

fn item(art: &UiAssets, achievement: &AchievementView) -> Option<impl IntoView + use<>> {
    let (title, hint) = match &achievement.palace {
        Some(name) => describe(&format!("{}@{name}", achievement.id))?,
        None => describe(&achievement.id)?,
    };
    let unlocked = achievement.unlocked;
    let media = medal_for(art, &achievement.id, unlocked)?;
    let since = achievement
        .day
        .as_ref()
        .filter(|_| unlocked)
        .map(|day| crate::i18n::tf("Seit {0}", &[day]));
    let progress = achievement
        .progress
        .map(|progress| palace_progress(&achievement.id, progress));
    Some(view! {
        <li class="medal" class:locked=!unlocked aria-disabled=(!unlocked).then_some("true")
            data-achievement=achievement.id.clone()>
            <Sply media=media still=!unlocked class="medal-art"/>
            <span class="medal-title">{title}</span>
            {since.map(|since| view! { <span class="medal-since">{since}</span> })}
            {(!unlocked).then(|| view! { <span class="medal-hint">{hint}</span> })}
            {progress.map(|progress| view! { <span class="medal-hint">{progress}</span> })}
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
        let (game, rest): (Vec<_>, Vec<_>) =
            list.into_iter().partition(|a| a.id.starts_with("game-"));
        let (palaces, rest): (Vec<_>, Vec<_>) =
            rest.into_iter().partition(|a| a.id.starts_with("palace-"));
        let (lessons, others): (Vec<_>, Vec<_>) = rest
            .into_iter()
            .partition(|a| a.id.starts_with("learn-") && a.id != "learn-all");
        let count = |list: &[AchievementView]| {
            tf(
                "{0} von {1}",
                &[&list.iter().filter(|a| a.unlocked).count(), &list.len()],
            )
        };
        let (done_lessons, done_palaces) = (count(&lessons), count(&palaces));
        let main = others.iter().filter_map(|a| item(&art, a)).collect_view();
        let grouped = lessons.iter().filter_map(|a| item(&art, a)).collect_view();
        let trophies = (!game.is_empty()).then(|| {
            let items = game.iter().filter_map(|a| item(&art, a)).collect_view();
            view! {
                <h3>{t("Journey-Trophäen")}</h3>
                <ul class="medals">{items}</ul>
            }
        });
        let medals = (!palaces.is_empty()).then(|| {
            let items = palaces.iter().filter_map(|a| item(&art, a)).collect_view();
            view! {
                <details class="medal-group">
                    <summary>{t("Palastmedaillen")}" · "{done_palaces}</summary>
                    <p class="notes">{t("Paläste mit mindestens 5 Orten bekommen Medaillen.")}</p>
                    <p class="notes">{t("Medaillen zählen nur, was Wiederholungen an verschiedenen Tagen gezeigt haben; mehrmals am selben Tag zu üben bringt keine.")}</p>
                    <ul class="medals">{items}</ul>
                </details>
            }
        });
        // Every unlocked one can be shared; the newest comes first in the choice.
        let mut unlocked: Vec<&AchievementView> = others
            .iter()
            .chain(&lessons)
            .chain(&game)
            .chain(&palaces)
            .filter(|a| a.unlocked)
            .collect();
        unlocked.sort_by(|a, b| b.day.cmp(&a.day));
        let cards: Vec<crate::share::CardKind> = unlocked
            .into_iter()
            .map(|a| crate::share::CardKind::Achievement {
                id: a.id.clone(),
                palace: a.palace.clone(),
                day: a.day.clone(),
            })
            .collect();
        let share = (!cards.is_empty()).then(|| {
            let cards = StoredValue::new(cards);
            view! {
                <button type="button" class="achievement-share" on:click=move |event| {
                    crate::share::open(cards.get_value(), crate::modal::opener_of(&event));
                }>{t("Erfolg teilen…")}</button>
            }
        });
        Some(view! {
            <section class="card panel achievements" aria-label=t("Erfolge")>
                <h2>{t("Erfolge")}</h2>
                <ul class="medals">{main}</ul>
                <details class="medal-group">
                    <summary>{t("Mnemotechnik")}" · "{done_lessons}</summary>
                    <ul class="medals">{grouped}</ul>
                </details>
                {trophies}
                {medals}
                {share}
            </section>
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_app_api::{MedalArt, MediaKind};

    fn media(hash: &str) -> Media {
        Media {
            hash: hash.into(),
            kind: MediaKind::Sply,
        }
    }

    #[test]
    fn every_game_and_palace_medal_is_described_and_drawn() {
        crate::i18n::set_current(crate::i18n::Lang::De);
        let ids = [
            "game-first-journey",
            "game-hundred-xp",
            "game-seven-day-streak",
            "game-due-cleared",
            "game-rhythm-4",
            "game-perfect-journey",
            "palace-7",
            "palace-30",
            "palace-180",
        ];
        let art = UiAssets {
            right: media("r"),
            again: media("a"),
            done: media("d"),
            combo: media("c"),
            trophy: media("t"),
            streak: media("s"),
            room_blank: media("b"),
            room_image: media("i"),
            room_video: media("v"),
            logo: media("l"),
            simo: media("m"),
            medal_tutorial: media("x"),
            medal_tutorial_locked: media("x"),
            medal_formulas: media("x"),
            medal_formulas_locked: media("x"),
            medals: ids
                .iter()
                .map(|id| {
                    (
                        (*id).to_owned(),
                        MedalArt {
                            unlocked: media(id),
                            locked: media("grau"),
                        },
                    )
                })
                .collect(),
            characters: Default::default(),
        };
        for id in ids {
            assert!(describe(id).is_some(), "{id}");
            assert_eq!(medal_for(&art, id, true).unwrap().hash, id);
            assert_eq!(medal_for(&art, id, false).unwrap().hash, "grau");
        }
        let (title, hint) = describe("palace-30@Küche").unwrap();
        assert_eq!(title, "Gefestigt · Küche");
        assert!(hint.contains("„Küche“") && hint.contains("30"), "{hint}");
        assert_eq!(
            medal_for(&art, "palace-30@Küche", true).unwrap().hash,
            "palace-30"
        );
        assert_eq!(
            palace_progress("palace-30", (4, 9)),
            "4 von 9 Orten halten 30+ Tage"
        );
        assert!(describe("palace-9@Küche").is_none());
        assert_eq!(palace_level_title("180"), Some("Dauerhaft"));
        assert_eq!(palace_level_title("9"), None);
        crate::i18n::set_current(crate::i18n::Lang::En);
        let strength = locora_app_api::Strength { loci: 9, solid: 2 };
        assert_eq!(strength_text(strength), "Consolidated: 2 of 9 loci");
        let one = locora_app_api::Strength { loci: 1, solid: 0 };
        assert_eq!(strength_text(one), "Consolidated: 0 of 1 locus");
        assert_eq!(celebration_heading("game-hundred-xp"), "New trophy!");
        assert_eq!(celebration_heading("palace-7@Küche"), "New medal!");
        assert_eq!(celebration_heading("tour-done"), "New achievement!");
        crate::i18n::set_current(crate::i18n::Lang::De);
    }
}
