//! Settings (new loci per day, target retention) and learning statistics.

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{CMD_SETTINGS, CMD_SETTINGS_SET, SettingsChange, SettingsView, StatsView};
use serde::Serialize;

use crate::tauri::{NoArgs, call};
use crate::{Screen, load};

#[derive(Serialize)]
struct ChangeArgs {
    change: SettingsChange,
}

fn percent(v: f64) -> String {
    format!("{:.0} %", v * 100.0)
}

#[component]
pub fn SettingsView(screen: RwSignal<Screen>) -> impl IntoView {
    let data = load(call::<SettingsView>(CMD_SETTINGS, NoArgs {}));
    // Lives here, not in `Form`: a save rebuilds the form with the new values.
    let message = RwSignal::new(None::<Result<String, String>>);
    view! {
        <header class="bar">
            <button class="back" title="Zurück" on:click=move |_| screen.set(Screen::Home)>
                "←"
            </button>
            <h1>"Einstellungen & Statistik"</h1>
        </header>
        {move || match data.get() {
            None => view! { <p class="loading">"Lädt …"</p> }.into_any(),
            Some(Err(e)) => view! { <p class="notes">{e}</p> }.into_any(),
            Some(Ok(v)) => view! { <Form view=v data=data message=message /> }.into_any(),
        }}
    }
}

#[component]
fn Form(
    view: SettingsView,
    data: RwSignal<Option<Result<SettingsView, String>>>,
    message: RwSignal<Option<Result<String, String>>>,
) -> impl IntoView {
    let new_per_day = RwSignal::new(view.new_per_day.to_string());
    // Shown rounded (0,875 → „88 %“); sent only when it was edited, so a
    // save elsewhere never rounds the stored value.
    let shown_retention = view.retention.map(percent).unwrap_or_default();
    let retention = RwSignal::new(shown_retention.clone());
    let busy = RwSignal::new(false);
    let has_review = view.retention.is_some();
    let max = view.max_new_per_day;
    let save = move || {
        let count = new_per_day.get_untracked();
        let Ok(count) = count.trim().parse::<u32>() else {
            message.set(Some(Err(format!(
                "Neue Loci pro Tag: ganze Zahl von 0 bis {max}"
            ))));
            return;
        };
        let change = SettingsChange {
            new_per_day: Some(count),
            retention: Some(retention.get_untracked())
                .filter(|text| has_review && *text != shown_retention),
        };
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match call::<SettingsView>(CMD_SETTINGS_SET, ChangeArgs { change }).await {
                Ok(v) => {
                    message.set(Some(Ok("Gespeichert.".into())));
                    // Rebuilds the form; `busy` belongs to the old one.
                    data.set(Some(Ok(v)));
                }
                Err(e) => {
                    message.set(Some(Err(e)));
                    busy.set(false);
                }
            }
        });
    };
    view! {
        <form
            class="card panel"
            on:submit=move |e| {
                e.prevent_default();
                save();
            }
        >
            <label class="input">
                <span>"Neue Loci pro Tag"</span>
                <input type="text" inputmode="numeric" bind:value=new_per_day />
            </label>
            {has_review
                .then(|| {
                    view! {
                        <label class="input">
                            <span>"Ziel-Behaltensrate (70–99 %)"</span>
                            <input type="text" inputmode="decimal" bind:value=retention />
                        </label>
                        <div class="progress">
                            {if view.personal_weights {
                                "FSRS-Gewichte: persönlich (aus deinem Verlauf)"
                            } else {
                                "FSRS-Gewichte: Standard – persönliche mit „locora optimize“"
                            }}
                        </div>
                    }
                })}
            <button class="primary" type="submit" disabled=move || busy.get()>
                "Speichern"
            </button>
            {move || {
                message
                    .get()
                    .map(|m| match m {
                        Ok(text) => view! { <p class="verdict ok" role="status">{text}</p> }.into_any(),
                        Err(text) => view! { <p class="notes" role="alert">{text}</p> }.into_any(),
                    })
            }}
        </form>
        {view.stats.map(|s| view! { <Stats stats=s /> })}
    }
}

#[component]
fn Stats(stats: StatsView) -> impl IntoView {
    let rate = if stats.recent == 0 {
        "–".to_owned()
    } else {
        format!(
            "{} ({} von {})",
            percent(f64::from(stats.recent_known) / f64::from(stats.recent)),
            stats.recent_known,
            stats.recent
        )
    };
    let rows = [
        ("Antworten insgesamt", stats.answers.to_string()),
        (
            "Heute",
            format!(
                "{} Antworten, {} neue Loci",
                stats.answered_today, stats.new_today
            ),
        ),
        ("Trefferquote 30 Tage", rate),
        (
            "Serie",
            format!(
                "{} {}",
                stats.streak,
                if stats.streak == 1 { "Tag" } else { "Tage" }
            ),
        ),
        (
            "Geplante Loci",
            format!("{} (davon {} fällig)", stats.scheduled, stats.due),
        ),
    ];
    view! {
        <section class="card panel">
            <div class="prompt">"Statistik"</div>
            <dl class="stats">
                {rows
                    .into_iter()
                    .map(|(k, v)| view! { <dt>{k}</dt><dd>{v}</dd> })
                    .collect_view()}
            </dl>
        </section>
    }
}
