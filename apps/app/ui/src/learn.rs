//! „Mnemotechnik lernen“: tiles for every technique the method of loci needs, each explained by
//! Simo or one of his friends, with an example, a small exercise and a check that unlocks the
//! lesson's achievement. After the last lesson a closing card offers the app tour.

use std::collections::HashSet;

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    AchievementArgs, AchievementView, CMD_ACHIEVEMENT_UNLOCK, CMD_ACHIEVEMENTS, CMD_TUTORIAL,
    LanguageArgs, Media, UiAssets,
};

use crate::achievements::{Achievements, Celebration};
use crate::i18n::{t, tf};
use crate::learn_content::{Action, Group, Lesson, Who, lessons};
use crate::sply::Sply;
use crate::tauri::{NoArgs, call};
use crate::{Screen, art, current_language, start_screen};

const WHO: [Who; 5] = [Who::Simo, Who::Mina, Who::Cami, Who::Bruno, Who::Hafiza];
const GROUPS: [Group; 4] = [Group::Start, Group::Pictures, Group::Route, Group::Extra];

fn name(who: Who) -> &'static str {
    match who {
        Who::Simo => "Simo",
        Who::Mina => "Mina",
        Who::Cami => "Cami",
        Who::Bruno => "Bruno",
        Who::Hafiza => "Hafiza",
    }
}

fn role(who: Who) -> &'static str {
    match who {
        Who::Simo => t("Simo erklärt Grundlagen, Routen und Geschichte"),
        Who::Mina => t("Mina erklärt Fehler, Tipps und Forschung"),
        Who::Cami => t("Cami erklärt, wie du Bilder baust"),
        Who::Bruno => t("Bruno erklärt Paläste und Orte"),
        Who::Hafiza => t("Hafiza erklärt Zahlen, Wiederholen und Gedächtnissport"),
    }
}

fn group_title(group: Group) -> &'static str {
    match group {
        Group::Start => t("Los geht's"),
        Group::Pictures => t("Bilder bauen"),
        Group::Route => t("Palast & Route"),
        Group::Extra => t("Nützliches"),
    }
}

/// The character's graphic: Simo, or a friend from `UiAssets::characters`.
fn figure(art: &UiAssets, who: Who) -> Option<Media> {
    match who {
        Who::Simo => Some(art.simo.clone()),
        other => art.characters.get(&name(other).to_lowercase()).cloned(),
    }
}

#[component]
fn Figure(
    who: Who,
    #[prop(optional)] still: bool,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let art = art();
    view! {
        <div class=class aria-hidden="true">
            {move || art.get().and_then(|art| figure(&art, who)).map(|m| view! { <Sply media=m still=still /> })}
        </div>
    }
}

/// A prefilled GitHub issue for a wrong translation: where, which language, the version and the
/// text as shown, cut so the address stays short. Nothing personal goes into it.
pub fn report_url(id: &str, text: &str) -> String {
    let lang = crate::i18n::current().code();
    let title = format!("Translation: {id} ({lang})");
    let build = |text: &str| {
        let body = format!(
            "**Where:** Mnemotechnik lernen – {id}\n**Language:** {lang}\n**Version:** {}\n\n**Current text:**\n{text}\n\n**Better wording:**\n\n",
            env!("CARGO_PKG_VERSION")
        );
        format!(
            "{}?labels=translation&title={}&body={}",
            crate::crash::REPO,
            crate::crash::encode(&title),
            crate::crash::encode(&body)
        )
    };
    let mut keep = text.chars().count();
    loop {
        let shown: String = text.chars().take(keep).collect();
        let cut = if keep < text.chars().count() {
            format!("{shown} …")
        } else {
            shown
        };
        let url = build(&cut);
        if url.len() < 6000 || keep == 0 {
            return url;
        }
        keep = keep * 3 / 4;
    }
}

#[component]
fn ReportLink(id: String, text: String) -> impl IntoView {
    view! {
        <p class="learn-report">
            <a href=report_url(&id, &text) target="_blank" rel="noopener">{t("Übersetzung falsch? Melden")}</a>
        </p>
    }
}

fn lesson_text(lesson: &Lesson) -> String {
    let mut parts = vec![lesson.title, lesson.teaser];
    parts.extend(lesson.paragraphs);
    parts.extend([lesson.example, lesson.exercise, lesson.question]);
    parts.extend(lesson.options);
    parts.push(lesson.hint);
    parts.extend(lesson.in_locora);
    parts.join("\n\n")
}

#[component]
pub fn LearnView(screen: RwSignal<Screen>) -> impl IntoView {
    let done = RwSignal::new(HashSet::<String>::new());
    spawn_local(async move {
        if let Ok(list) = call::<Vec<AchievementView>>(CMD_ACHIEVEMENTS, NoArgs {}).await {
            done.set(
                list.into_iter()
                    .filter(|a| a.unlocked)
                    .map(|a| a.id)
                    .collect(),
            );
        }
    });
    let open = RwSignal::new(None::<usize>);
    let finish = RwSignal::new(false);
    // „Später“ hides the closing card on the overview for now.
    let later = RwSignal::new(false);
    Effect::new(move |_| {
        open.track();
        finish.track();
        if let Some(window) = web_sys::window() {
            window.scroll_to_with_x_and_y(0.0, 0.0);
        }
    });
    move || {
        if finish.get() {
            view! {
                <LearnBar label=t("← Alle Lektionen") title=t("Mnemotechnik lernen")
                    back=Callback::new(move |()| { finish.set(false); open.set(None); }) />
                <FinishCard screen done on_later=Callback::new(move |()| { finish.set(false); open.set(None); later.set(true); }) />
            }
            .into_any()
        } else if let Some(index) = open.get() {
            view! { <LessonView index done open finish screen /> }.into_any()
        } else {
            view! { <Overview done open later screen /> }.into_any()
        }
    }
}

#[component]
fn LearnBar(label: &'static str, title: &'static str, back: Callback<()>) -> impl IntoView {
    view! {
        <header class="bar learn-bar">
            <button class="back" on:click=move |_| back.run(())>{label}</button>
            <h1>{title}</h1>
        </header>
    }
}

fn count_done(done: &HashSet<String>) -> usize {
    locora_app_api::LEARN_LESSONS
        .iter()
        .filter(|lesson| done.contains(&format!("learn-{lesson}")))
        .count()
}

#[component]
fn Overview(
    done: RwSignal<HashSet<String>>,
    open: RwSignal<Option<usize>>,
    later: RwSignal<bool>,
    screen: RwSignal<Screen>,
) -> impl IntoView {
    let all = lessons();
    let total = all.len();
    let footer: String = all
        .iter()
        .map(|l| format!("{} – {}", l.title, l.teaser))
        .collect::<Vec<_>>()
        .join("\n");
    let groups = GROUPS
        .into_iter()
        .map(|group| {
            let tiles = all
                .iter()
                .enumerate()
                .filter(|(_, lesson)| lesson.group == group)
                .map(|(index, lesson)| {
                    let key = format!("learn-{}", lesson.id);
                    let finished = {
                        let key = key.clone();
                        move || done.with(|d| d.contains(&key))
                    };
                    let shown = move || done.with(|d| d.contains(&key));
                    view! {
                        <button class="card learn-tile" class:done=finished on:click=move |_| open.set(Some(index))>
                            <Figure who=lesson.who still=true class="learn-tile-figure" />
                            <span class="learn-tile-text">
                                <span class="learn-tile-title">{lesson.title}</span>
                                <span class="learn-tile-teaser">{lesson.teaser}</span>
                            </span>
                            {move || shown().then(|| view! { <span class="learn-check" title=t("geschafft")>"✓"</span> })}
                        </button>
                    }
                })
                .collect_view();
            view! {
                <h2 class="learn-group">{group_title(group)}</h2>
                <div class="learn-tiles">{tiles}</div>
            }
        })
        .collect_view();
    let cast = WHO
        .into_iter()
        .map(|who| {
            view! {
                <li>
                    <Figure who class="cast-figure" />
                    <strong>{name(who)}</strong>
                    <span>{role(who)}</span>
                </li>
            }
        })
        .collect_view();
    view! {
        <LearnBar label=t("← Startseite") title=t("Mnemotechnik lernen")
            back=Callback::new(move |()| screen.set(start_screen())) />
        <section class="card learn-intro">
            <p>{t("Simo und seine Freunde erklären dir Schritt für Schritt, wie die Loci-Methode funktioniert. Jede Lektion dauert nur ein paar Minuten und endet mit einer kleinen Übung.")}</p>
            <ul class="cast">{cast}</ul>
            <div class="learn-progress">
                <span>{move || tf("{0} von {1} geschafft", &[&done.with(count_done), &total])}</span>
                <progress max=total value=move || done.with(count_done)></progress>
            </div>
        </section>
        {move || (done.with(count_done) == total && !later.get()).then(|| view! {
            <FinishCard screen done on_later=Callback::new(move |()| later.set(true)) />
        })}
        {groups}
        <Achievements />
        <ReportLink id="overview".to_owned() text=footer />
    }
}

/// Answers in a mixed order (the right one is first in the lesson): (text, right).
fn choices(lesson: &Lesson, index: usize) -> Vec<(&'static str, bool)> {
    let mut list: Vec<(&'static str, bool)> = lesson
        .options
        .iter()
        .enumerate()
        .map(|(i, text)| (*text, i == 0))
        .collect();
    let len = list.len();
    list.rotate_right((index * 2 + 1) % len);
    list
}

#[component]
fn LessonView(
    index: usize,
    done: RwSignal<HashSet<String>>,
    open: RwSignal<Option<usize>>,
    finish: RwSignal<bool>,
    screen: RwSignal<Screen>,
) -> impl IntoView {
    let all = lessons();
    let total = all.len();
    let Some(lesson) = all.into_iter().nth(index) else {
        return view! { <p class="notes">{t("Diese Lektion gibt es nicht.")}</p> }.into_any();
    };
    let key = format!("learn-{}", lesson.id);
    let already = done.with_untracked(|d| d.contains(&key));
    // The chosen answer: its position and whether it was right.
    let picked = RwSignal::new(None::<(usize, bool)>);
    let celebration = use_context::<Celebration>();
    let choose = move |position: usize, right: bool| {
        picked.set(Some((position, right)));
        if !right {
            return;
        }
        let key = key.clone();
        spawn_local(async move {
            let new =
                call::<bool>(CMD_ACHIEVEMENT_UNLOCK, AchievementArgs { id: key.clone() }).await;
            if new.is_ok() {
                done.update(|d| {
                    d.insert(key.clone());
                });
            }
            if let (Ok(true), Some(celebration)) = (new, celebration) {
                celebration.celebrate(&key);
            }
        });
    };
    let buttons = choices(&lesson, index)
        .into_iter()
        .enumerate()
        .map(|(position, (text, right))| {
            let state = move || picked.get().filter(|(at, _)| *at == position).map(|(_, ok)| ok);
            view! {
                <button class="choice" class:right=move || state() == Some(true) class:wrong=move || state() == Some(false)
                    disabled=move || picked.get().is_some_and(|(_, ok)| ok)
                    on:click={let choose = choose.clone(); move |_| choose(position, right)}>{text}</button>
            }
        })
        .collect_view();
    let hint = lesson.hint;
    let action = match lesson.action {
        Action::None => None,
        Action::Tutorial => Some(view! {
            <button on:click=move |_| {
                spawn_local(async move {
                    if let Ok(id) = call::<String>(CMD_TUTORIAL, LanguageArgs { lang: current_language() }).await {
                        screen.set(Screen::Palace(id));
                    }
                });
            }>{t("Tutorial-Palast öffnen")}</button>
        }.into_any()),
        Action::FirstPalace => Some(view! {
            <button on:click=move |_| crate::first_palace::start(screen)>{t("Eigenen Palast anlegen…")}</button>
        }.into_any()),
        Action::Settings => Some(view! {
            <button on:click=move |_| screen.set(Screen::Settings)>{t("Einstellungen öffnen")}</button>
        }.into_any()),
    };
    let last = index + 1 == total;
    let report = lesson_text(&lesson);
    view! {
        <LearnBar label=t("← Alle Lektionen") title=lesson.title back=Callback::new(move |()| open.set(None)) />
        <p class="meta lesson-count">{tf("Lektion {0} von {1}", &[&(index + 1), &total])}
            {already.then(|| view! { " · " <span class="learn-done">{t("✓ schon geschafft")}</span> })}</p>
        <section class="lesson-talk">
            <Figure who=lesson.who class="lesson-figure" />
            <div class="bubble">
                <strong class="bubble-name">{name(lesson.who)}</strong>
                {lesson.paragraphs.map(|p| view! { <p>{p}</p> })}
            </div>
        </section>
        <section class="card lesson-example">
            <h2>{t("Zum Beispiel")}</h2>
            <p>{lesson.example}</p>
        </section>
        <section class="card lesson-try">
            <h2>{t("Probier es aus")}</h2>
            <p>{lesson.exercise}</p>
            <h3>{t("Kurzer Check")}</h3>
            <p class="question">{lesson.question}</p>
            <div class="choices" role="group" aria-label=lesson.question>{buttons}</div>
            <p class="check-feedback" role="status">{move || match picked.get() {
                Some((_, true)) => t("Richtig! Die Übung ist geschafft.").to_owned(),
                Some((_, false)) => format!("{} {}", t("Nicht ganz."), hint),
                None => String::new(),
            }}</p>
        </section>
        {lesson.in_locora.map(|text| view! {
            <section class="card lesson-locora">
                <h2>{t("So machst du es in Locora")}</h2>
                <p>{text}</p>
                {action}
            </section>
        })}
        <nav class="lesson-nav" aria-label=t("Lektionen")>
            <button disabled=index == 0 on:click=move |_| open.set(Some(index.saturating_sub(1)))>{t("◀ Vorherige Lektion")}</button>
            <button class="primary" on:click=move |_| if last { finish.set(true) } else { open.set(Some(index + 1)) }>
                {if last { t("Zum Abschluss ▶") } else { t("Nächste Lektion ▶") }}
            </button>
        </nav>
        <ReportLink id=lesson.id.to_owned() text=report />
    }
    .into_any()
}

/// After the last lesson: the characters congratulate and offer the app tour.
#[component]
fn FinishCard(
    screen: RwSignal<Screen>,
    done: RwSignal<HashSet<String>>,
    on_later: Callback<()>,
) -> impl IntoView {
    let total = locora_app_api::LEARN_LESSONS.len();
    let cast = WHO
        .into_iter()
        .map(|who| view! { <Figure who class="finish-figure" /> })
        .collect_view();
    let tour = crate::tour_gate::control();
    view! {
        <section class="card learn-finish" aria-labelledby="learn-finish-title">
            <div class="finish-cast">{cast}</div>
            {move || {
                let count = done.with(count_done);
                if count == total {
                    view! {
                        <h2 id="learn-finish-title">{t("Geschafft! Du kennst jetzt alle Techniken.")}</h2>
                        <p>{t("Simo und seine Freunde gratulieren dir! Die App-Tour zeigt dir jetzt, wie du das alles in Locora machst.")}</p>
                    }.into_any()
                } else {
                    view! {
                        <h2 id="learn-finish-title">{t("Du bist am Ende angekommen.")}</h2>
                        <p>{tf("{0} von {1} Übungen geschafft – die anderen kannst du jederzeit nachholen. Die App-Tour zeigt dir, wie du das alles in Locora machst.", &[&count, &total])}</p>
                    }.into_any()
                }
            }}
            <div class="row">
                <button class="primary big" disabled=move || tour.busy.get() on:click=move |_| crate::tour_gate::begin(screen)>{t("App-Tour starten")}</button>
                <button on:click=move |_| on_later.run(())>{t("Später")}</button>
            </div>
        </section>
    }
}
