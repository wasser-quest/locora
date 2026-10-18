//! Settings (new loci per day, target retention) and learning statistics.

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_SETTINGS, CMD_SETTINGS_OPTIMIZE, CMD_SETTINGS_SET, OptimizeDone, SettingsChange,
    SettingsView, StatsView,
};
use serde::Serialize;

use crate::locus_display;
use crate::tauri::{NoArgs, call};
use crate::{Screen, load, start_screen, startup};

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
            <button class="back" title="Zurück" on:click=move |_| screen.set(start_screen())>
                "←"
            </button>
            <h1>"Einstellungen & Statistik"</h1>
        </header>
        <LocusDisplay />
        <StartView />
        {move || match data.get() {
            None => view! { <crate::Loading /> }.into_any(),
            Some(Err(e)) => view! { <p class="notes">{e}</p> }.into_any(),
            Some(Ok(v)) => view! { <Form view=v data=data message=message /> }.into_any(),
        }}
        <crate::cloud_settings::CloudSettings />
    }
}

#[component]
fn StartView() -> impl IntoView {
    let in_core = RwSignal::new(startup::in_core());
    view! {
        <section class="card panel" aria-label="Startansicht">
            <h2>"Startansicht"</h2>
            <label><input type="checkbox" prop:checked=move || in_core.get()
                on:change=move |event| {
                    let value = event_target_checked(&event);
                    startup::set_in_core(value);
                    in_core.set(value);
                } />"Direkt in Core starten"</label>
        </section>
    }
}

#[component]
fn LocusDisplay() -> impl IntoView {
    let settings = locus_display::signal();
    view! {
        <section class="card panel" aria-label="Locus-Anzeige">
            <h2>"Loci anzeigen"</h2>
            <p class="notes">"Die Auswahl gilt für dieses Gerät. Der aktuelle Ort ist immer sichtbar, bis du ihn aufdeckst."</p>
            <label>"Vorherige Orte "
                <input type="number" min="0" max="10" prop:value=move || settings.get().previous.to_string()
                    on:change=move |event| {
                        if let Ok(value) = event_target_value(&event).parse::<u8>() {
                            settings.update(|p| p.previous = value.min(10));
                            locus_display::save(settings.get_untracked());
                        }
                    } />
            </label>
            <label>"Nächste Orte "
                <input type="number" min="0" max="10" prop:value=move || settings.get().next.to_string()
                    on:change=move |event| {
                        if let Ok(value) = event_target_value(&event).parse::<u8>() {
                            settings.update(|p| p.next = value.min(10));
                            locus_display::save(settings.get_untracked());
                        }
                    } />
            </label>
            <label><input type="checkbox" prop:checked=move || settings.get().numbers
                on:change=move |event| { settings.update(|p| p.numbers = event_target_checked(&event)); locus_display::save(settings.get_untracked()); } />"Ortsnummern im Markerpunkt anzeigen"</label>
            <label><input type="checkbox" prop:checked=move || settings.get().hide_revealed
                on:change=move |event| { settings.update(|p| p.hide_revealed = event_target_checked(&event)); locus_display::save(settings.get_untracked()); } />"Aktuellen Ort nach dem Aufdecken ausblenden"</label>
            <label><input type="checkbox" prop:checked=move || settings.get().keep_previous
                on:change=move |event| { settings.update(|p| p.keep_previous = event_target_checked(&event)); locus_display::save(settings.get_untracked()); } />"Aufgedeckte Bilder bereits durchlaufener Orte weiter anzeigen"</label>
            <label><input type="checkbox" prop:checked=move || settings.get().quick_next
                on:change=move |event| { settings.update(|p| p.quick_next = event_target_checked(&event)); locus_display::save(settings.get_untracked()); } />"Schnell weiter: nach dem Aufdecken oder der Eingabe direkt zum nächsten Ort"</label>
            <label>"Video: Marker frühestens vor dem Stopp (Sekunden) "
                <input type="number" min="0" max="30" step="0.1" prop:value=move || settings.get().video_lead.to_string()
                    on:change=move |event| {
                        if let Ok(value) = event_target_value(&event).parse::<f64>() {
                            if value.is_finite() { settings.update(|p| p.video_lead = value.clamp(0.0, 30.0)); locus_display::save(settings.get_untracked()); }
                        }
                    } />
            </label>
        </section>
    }
}

#[component]
fn Form(
    view: SettingsView,
    data: RwSignal<Option<Result<SettingsView, String>>>,
    message: RwSignal<Option<Result<String, String>>>,
) -> impl IntoView {
    let new_per_day = RwSignal::new(view.new_per_day.to_string());
    let day_start = RwSignal::new(view.day_start_hour.to_string());
    // Shown rounded (0,875 → „88 %“); sent only when it was edited, so a
    // save elsewhere never rounds the stored value.
    let shown_retention = view.retention.map(percent).unwrap_or_default();
    let retention = RwSignal::new(shown_retention.clone());
    let busy = RwSignal::new(false);
    let has_review = view.retention.is_some();
    let max = view.max_new_per_day;
    let enough = view.usable_reviews >= view.needed_reviews;
    // Fitting can take a moment; the answer replaces the whole view.
    let optimize = move || {
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match call::<OptimizeDone>(CMD_SETTINGS_OPTIMIZE, NoArgs {}).await {
                Ok(done) => {
                    message.set(Some(Ok(done.message)));
                    data.set(Some(Ok(done.view)));
                }
                Err(e) => {
                    message.set(Some(Err(e)));
                    busy.set(false);
                }
            }
        });
    };
    let save = move || {
        let count = new_per_day.get_untracked();
        let Ok(count) = count.trim().parse::<u32>() else {
            message.set(Some(Err(format!(
                "Neue Loci pro Tag: ganze Zahl von 0 bis {max}"
            ))));
            return;
        };
        let Ok(hour) = day_start.get_untracked().trim().parse::<u8>() else {
            message.set(Some(Err(
                "Lerntag beginnt: volle Stunde von 0 bis 23".into()
            )));
            return;
        };
        let change = SettingsChange {
            day_start_hour: Some(hour),
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
            <label class="input">
                <span>
                    "Lerntag beginnt um (volle Stunde, 0–23)"
                    <small>{format!(" – jetzt {}", view.day_start)}</small>
                </span>
                <input type="text" inputmode="numeric" bind:value=day_start />
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
                                "FSRS-Gewichte: Standard"
                            }}
                        </div>
                        <div class="optimize">
                            <span class="progress">
                                {if view.optimize_hint {
                                    format!(
                                        "Aus {} Wiederholungen kann Locora deine Gewichte neu berechnen.",
                                        view.usable_reviews,
                                    )
                                } else {
                                    format!(
                                        "{} von mindestens {} Wiederholungen mit einem Tag Abstand",
                                        view.usable_reviews,
                                        view.needed_reviews,
                                    )
                                }}
                            </span>
                            <button
                                type="button"
                                class:primary=view.optimize_hint
                                disabled=move || busy.get() || !enough
                                on:click=move |_| optimize()
                            >
                                "Persönliche Gewichte berechnen"
                            </button>
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
