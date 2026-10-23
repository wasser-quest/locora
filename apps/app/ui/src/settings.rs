//! Settings (new loci per day, target retention) and learning statistics.

use crate::i18n::t;
use crate::info::Info;
use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_CLOUD_STATUS, CMD_GAME_RESET, CMD_GAME_WEEK_GOAL, CMD_GAMIFICATION_STATUS, CMD_SETTINGS,
    CMD_SETTINGS_OPTIMIZE, CMD_SETTINGS_SET, CloudStatus, GameResetReport, GameStatusArgs,
    GamificationStatus, OptimizeDone, RankingsDeletion, RemindersChange, RemindersView,
    SettingsChange, SettingsView, StatsView, WeekGoalArgs,
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
            <button class="back" title=t("Zurück") on:click=move |_| screen.set(start_screen())>
                "←"
            </button>
            <h1>{t("Einstellungen & Statistik")}</h1>
        </header>
        <LocusDisplay />
        <StartView />
        {move || match data.get() {
            None => view! { <crate::Loading /> }.into_any(),
            Some(Err(e)) => view! { <p class="notes">{e}</p> }.into_any(),
            Some(Ok(v)) => {
                let reminders = v.reminders.clone();
                view! {
                    <Form view=v data=data message=message />
                    <Reminders view=reminders data=data message=message />
                }
                .into_any()
            }
        }}
        <crate::cloud_settings::CloudSettings />
    }
}

#[component]
fn StartView() -> impl IntoView {
    let in_core = RwSignal::new(startup::in_core());
    view! {
        <section class="card panel" aria-label=t("Startansicht")>
            <h2>{t("Startansicht")}</h2>
            <label><input type="checkbox" prop:checked=move || in_core.get()
                on:change=move |event| {
                    let value = event_target_checked(&event);
                    startup::set_in_core(value);
                    in_core.set(value);
                } />{t("Direkt in Core starten")}<Info text="Die App öffnet dann eine schlichte Startseite nur mit deinen Palästen, ohne die anderen Kacheln." /></label>
        </section>
    }
}

#[component]
fn LocusDisplay() -> impl IntoView {
    let settings = locus_display::signal();
    view! {
        <section class="card panel" aria-label=t("Darstellung")>
            <h2>{t("Darstellung")}</h2>
            <label class="info-head">{t("Farben ")}
                <Info text="„System“ folgt der Einstellung deines Geräts. „Hoher Kontrast“ hilft, wenn du schlecht siehst." />
                <select prop:value=move || crate::theme::current().key() on:change=move |event| {
                    let theme = crate::theme::Theme::from_key(&event_target_value(&event));
                    crate::theme::save(theme);
                    crate::theme::apply(theme);
                }>
                    {crate::theme::Theme::ALL.into_iter().map(|theme| view! {
                        <option value=theme.key()>{theme.label()}</option>
                    }).collect_view()}
                </select>
            </label>
            <label>{t("Sprache")}
                <select prop:value=move || crate::i18n::current().code() on:change=move |event| {
                    crate::i18n::switch(crate::i18n::Lang::from_code(&event_target_value(&event)));
                }>
                    {crate::i18n::Lang::ALL.into_iter().map(|lang| view! {
                        <option value=lang.code()>{lang.name()}</option>
                    }).collect_view()}
                </select>
            </label>
        </section>
        <section class="card panel" aria-label=t("Locus-Anzeige")>
            <h2>{t("Loci anzeigen")}</h2>
            <p class="notes">{t("Die Auswahl gilt für dieses Gerät. Der aktuelle Ort ist immer sichtbar, bis du ihn aufdeckst.")}</p>
            <label class="info-head">{t("Vorherige Orte ")}
                <Info text="So viele Orte vor dem aktuellen Ort im selben Raum zeigen beim Lernen ihren Markerpunkt. 0 heißt: keiner." />
                <input type="number" min="0" max="10" prop:value=move || settings.get().previous.to_string()
                    on:change=move |event| {
                        if let Ok(value) = event_target_value(&event).parse::<u8>() {
                            settings.update(|p| p.previous = value.min(10));
                            locus_display::save(settings.get_untracked());
                        }
                    } />
            </label>
            <label class="info-head">{t("Nächste Orte ")}
                <Info text="So viele Orte nach dem aktuellen Ort im selben Raum zeigen beim Lernen schon ihren Markerpunkt. Das verrät, wo es weitergeht." />
                <input type="number" min="0" max="10" prop:value=move || settings.get().next.to_string()
                    on:change=move |event| {
                        if let Ok(value) = event_target_value(&event).parse::<u8>() {
                            settings.update(|p| p.next = value.min(10));
                            locus_display::save(settings.get_untracked());
                        }
                    } />
            </label>
            <label><input type="checkbox" prop:checked=move || settings.get().numbers
                on:change=move |event| { settings.update(|p| p.numbers = event_target_checked(&event)); locus_display::save(settings.get_untracked()); } />{t("Ortsnummern im Markerpunkt anzeigen")}<Info text="Im Markerpunkt steht dann die Nummer des Orts auf der Route." /></label>
            <label><input type="checkbox" prop:checked=move || settings.get().hide_revealed
                on:change=move |event| { settings.update(|p| p.hide_revealed = event_target_checked(&event)); locus_display::save(settings.get_untracked()); } />{t("Aktuellen Ort nach dem Aufdecken ausblenden")}<Info text="Nach dem Aufdecken verschwindet der Markerpunkt des aktuellen Orts, damit er das Bild nicht verdeckt." /></label>
            <label><input type="checkbox" prop:checked=move || settings.get().keep_previous
                on:change=move |event| { settings.update(|p| p.keep_previous = event_target_checked(&event)); locus_display::save(settings.get_untracked()); } />{t("Aufgedeckte Bilder bereits durchlaufener Orte weiter anzeigen")}<Info text="Orte im selben Raum, die du schon hattest, zeigen ihr Bild statt ihres Markerpunkts. So siehst du deinen bisherigen Weg." /></label>
            <label><input type="checkbox" prop:checked=move || settings.get().quick_next
                on:change=move |event| { settings.update(|p| p.quick_next = event_target_checked(&event)); locus_display::save(settings.get_untracked()); } />{t("Schnell weiter: nach der Bewertung oder der Eingabe direkt zum nächsten Ort")}<Info text="Nach deiner Bewertung oder Antwort geht es sofort zum nächsten Ort, ohne Klick auf „Weiter“. Bewerten musst du weiter selbst." /></label>
            <label class="info-head">{t("Rückwärts: Trefferradius (% der Bühnenbreite) ")}
                <Info text="Rückwärts tippst du auf die Stelle des Orts im Bild. So weit darf dein Tipp daneben liegen, in Prozent der Bildbreite." />
                <input type="number" min="2" max="30" step="1" prop:value=move || (settings.get().reverse_radius * 100.0).round().to_string()
                    on:change=move |event| {
                        if let Ok(percent) = event_target_value(&event).parse::<f64>() {
                            settings.update(|p| p.reverse_radius = (percent / 100.0).clamp(0.02, 0.3));
                            locus_display::save(settings.get_untracked());
                        }
                    } />
            </label>
            <label class="info-head">{t("Video: Marker frühestens vor dem Stopp (Sekunden) ")}
                <Info text="In Videos erscheint der Markerpunkt eines Orts so viele Sekunden vor seinem Zeitpunkt. Bei 0 erst genau dann." />
                <input type="number" min="0" max="30" step="0.1" prop:value=move || settings.get().video_lead.to_string()
                    on:change=move |event| {
                        if let Ok(value) = event_target_value(&event).parse::<f64>()
                            && value.is_finite() { settings.update(|p| p.video_lead = value.clamp(0.0, 30.0)); locus_display::save(settings.get_untracked()); }
                    } />
            </label>
        </section>
    }
}

/// Daily reminders: on or off, the first and last time of the day, and the gap between
/// them. The times saved last are listed; the reminders appear while the app runs.
#[component]
fn Reminders(
    view: RemindersView,
    data: RwSignal<Option<Result<SettingsView, String>>>,
    message: RwSignal<Option<Result<String, String>>>,
) -> impl IntoView {
    let enabled = RwSignal::new(view.enabled);
    let start = RwSignal::new(view.start.clone());
    let end = RwSignal::new(view.end.clone());
    let every = RwSignal::new(view.every.to_string());
    let busy = RwSignal::new(false);
    let saved = if view.times.is_empty() {
        t("Keine Erinnerung eingeschaltet.").to_owned()
    } else {
        crate::i18n::tf("Erinnerungen um {0}", &[&view.times.join(" · ")])
    };
    let save = move || {
        let Ok(hours) = every.get_untracked().parse::<u8>() else {
            message.set(Some(Err(t("Abstand: ganze Stunden von 1 bis 12").into())));
            return;
        };
        let reminders = RemindersChange {
            enabled: enabled.get_untracked(),
            start: start.get_untracked(),
            end: end.get_untracked(),
            every: hours,
        };
        let change = SettingsChange {
            reminders: Some(reminders),
            ..Default::default()
        };
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match call::<SettingsView>(CMD_SETTINGS_SET, ChangeArgs { change }).await {
                Ok(v) => {
                    message.set(Some(Ok(t("Erinnerungen gespeichert.").into())));
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
        <section class="card panel" aria-label=t("Tägliche Erinnerungen")>
            <h2>{t("Tägliche Erinnerungen")}</h2>
            <p class="notes">{t("Die Erinnerung erscheint in der App, solange sie läuft.")}</p>
            <label><input type="checkbox" bind:checked=enabled disabled=move || busy.get() />{t("Erinnerungen einschalten")}</label>
            <div class="editor-group-body">
                <label class="info-head">{t("Erste Erinnerung ")}
                    <Info text="Um diese Uhrzeit kommt die erste Erinnerung des Tages." />
                    <input type="time" bind:value=start disabled=move || busy.get() || !enabled.get() />
                </label>
                <label class="info-head">{t("Letzte Erinnerung ")}
                    <Info text="Nach dieser Uhrzeit kommt an diesem Tag keine Erinnerung mehr." />
                    <input type="time" bind:value=end disabled=move || busy.get() || !enabled.get() />
                </label>
                <label class="info-head">{t("Abstand ")}
                    <Info text="So viele Stunden liegen zwischen zwei Erinnerungen." />
                    <select bind:value=every disabled=move || busy.get() || !enabled.get()>
                        {(1..=12u8).map(|hours| view! {
                            <option value=hours.to_string()>{crate::i18n::tf("{0} Std.", &[&hours])}</option>
                        }).collect_view()}
                    </select>
                </label>
            </div>
            <p class="status">{saved}</p>
            <button class="primary" disabled=move || busy.get() on:click=move |_| save()>{t("Erinnerungen speichern")}</button>
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
            message.set(Some(Err(crate::i18n::tf(
                "Neue Loci pro Tag: ganze Zahl von 0 bis {0}",
                &[&max],
            ))));
            return;
        };
        let Ok(hour) = day_start.get_untracked().trim().parse::<u8>() else {
            message.set(Some(Err(
                t("Lerntag beginnt: volle Stunde von 0 bis 23").into()
            )));
            return;
        };
        let change = SettingsChange {
            day_start_hour: Some(hour),
            new_per_day: Some(count),
            retention: Some(retention.get_untracked())
                .filter(|text| has_review && *text != shown_retention),
            reminders: None,
        };
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match call::<SettingsView>(CMD_SETTINGS_SET, ChangeArgs { change }).await {
                Ok(v) => {
                    message.set(Some(Ok(t("Gespeichert.").into())));
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
                <span class="info-head">{t("Neue Loci pro Tag")}<Info text="So viele Orte lernst du an einem Tag höchstens neu. Wiederholungen zählen nicht dazu." /></span>
                <input type="text" inputmode="numeric" bind:value=new_per_day />
            </label>
            <label class="input">
                <span class="info-head">
                    <span>
                        {t("Lerntag beginnt um (volle Stunde, 0–23)")}
                        <small>{crate::i18n::tf(" – jetzt {0}", &[&view.day_start])}</small>
                    </span>
                    <Info text="Ab dieser Stunde beginnt ein neuer Lerntag. Bei 4 zählt Lernen kurz nach Mitternacht noch zum Vortag." />
                </span>
                <input type="text" inputmode="numeric" bind:value=day_start />
            </label>
            {has_review
                .then(|| {
                    view! {
                        <label class="input">
                            <span class="info-head">{t("Ziel-Behaltensrate (70–99 %)")}<Info text="Wie viel du beim Abfragen noch wissen sollst. Je höher, desto öfter kommen die Orte zur Wiederholung." /></span>
                            <input type="text" inputmode="decimal" bind:value=retention />
                        </label>
                        <div class="progress">
                            {if view.personal_weights {
                                {t("FSRS-Gewichte: persönlich (aus deinem Verlauf)")}
                            } else {
                                {t("FSRS-Gewichte: Standard")}
                            }}
                        </div>
                        <div class="optimize">
                            <span class="progress">
                                {if view.optimize_hint {
                                    crate::i18n::tf(
                                        "Aus {0} Wiederholungen kann Locora deine Gewichte neu berechnen.",
                                        &[&view.usable_reviews],
                                    )
                                } else {
                                    crate::i18n::tf(
                                        "{0} von mindestens {1} Wiederholungen mit einem Tag Abstand",
                                        &[&view.usable_reviews, &view.needed_reviews],
                                    )
                                }}
                            </span>
                            <button
                                type="button"
                                class:primary=view.optimize_hint
                                disabled=move || busy.get() || !enough
                                on:click=move |_| optimize()
                            >
                                {t("Persönliche Gewichte berechnen")}
                            </button>
                        </div>
                    }
                })}
            <button class="primary" type="submit" disabled=move || busy.get()>
                {t("Speichern")}
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
        <GameProgress />
    }
}

/// What the confirmation says about the rankings on the server.
pub fn rankings_note(signed_in: bool) -> &'static str {
    if signed_in {
        t("Deine Ranglisteneinträge werden auch vom Locora-Cloud-Server gelöscht.")
    } else {
        t(
            "Dieses Gerät ist nicht bei Locora Cloud angemeldet; Ranglisteneinträge auf dem Server bleiben unberührt.",
        )
    }
}

/// What happened, after the game progress was deleted.
pub fn reset_message(report: GameResetReport) -> String {
    let rankings = match report.rankings {
        RankingsDeletion::NotSignedIn => None,
        RankingsDeletion::Deleted => Some(t("Ranglisteneinträge vom Server gelöscht.")),
        RankingsDeletion::ServerTooOld => Some(t(
            "Der Server kann Ranglisteneinträge noch nicht löschen; sie bleiben, bis er aktualisiert ist. Dein Spielfortschritt ist trotzdem gelöscht.",
        )),
        RankingsDeletion::Pending => Some(t(
            "Der Server war nicht erreichbar. Die Ranglisteneinträge werden beim nächsten Sync gelöscht.",
        )),
        RankingsDeletion::SignInAgain => Some(t(
            "Melde dich wieder bei Locora Cloud an, um deine Ranglisteneinträge zu löschen.",
        )),
    };
    match rankings {
        Some(rankings) => format!("{} {rankings}", t("Spielfortschritt gelöscht.")),
        None => t("Spielfortschritt gelöscht.").to_owned(),
    }
}

/// The game: its numbers, the weekly goal, the journey bar, and deleting the progress on every
/// device (with a confirmation that says what goes and what stays). Only with the plugin.
#[component]
fn GameProgress() -> impl IntoView {
    let status = RwSignal::new(None::<GamificationStatus>);
    let signed_in = RwSignal::new(false);
    let confirming = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let message = RwSignal::new(None::<Result<String, String>>);
    let warning = NodeRef::<leptos::html::P>::new();
    let reload = move || {
        spawn_local(async move {
            let args = GameStatusArgs { full: true };
            if let Ok(value) =
                call::<Option<GamificationStatus>>(CMD_GAMIFICATION_STATUS, args).await
            {
                status.set(value);
            }
        });
    };
    reload();
    spawn_local(async move {
        if let Ok(cloud) = call::<CloudStatus>(CMD_CLOUD_STATUS, NoArgs {}).await {
            signed_in.set(cloud.account.is_some());
        }
    });
    // The warning takes the focus, so a screen reader reads it first.
    Effect::new(move |_| {
        if confirming.get()
            && let Some(element) = warning.get()
        {
            let _ = element.focus();
        }
    });
    let set_goal = move |days: u8| {
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match call::<u8>(CMD_GAME_WEEK_GOAL, WeekGoalArgs { days }).await {
                Ok(_) => {
                    message.set(Some(Ok(t("Wochenziel gespeichert.").into())));
                    reload();
                }
                Err(error) => message.set(Some(Err(error))),
            }
            busy.set(false);
        });
    };
    let reset = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match call::<GameResetReport>(CMD_GAME_RESET, NoArgs {}).await {
                Ok(report) => {
                    message.set(Some(Ok(reset_message(report))));
                    // The other devices hear of it at their next sync: soon, when it runs by itself.
                    if let Some(auto) = crate::auto_sync::use_auto_sync() {
                        auto.nudge();
                    }
                    reload();
                }
                Err(error) => message.set(Some(Err(crate::i18n::tf(
                    "Spielfortschritt konnte nicht gelöscht werden: {0}",
                    &[&error],
                )))),
            }
            confirming.set(false);
            busy.set(false);
        });
    };
    let medals = |game: &GamificationStatus| {
        game.trophies.len()
            + game
                .palace_medals
                .iter()
                .filter(|medal| medal.unlocked.is_some())
                .count()
    };
    view! {
        <Show when=move || status.with(Option::is_some)>
            <section class="card panel game-progress" aria-label=t("Spielfortschritt & Erfolge")>
                <h2>{t("Spielfortschritt & Erfolge")}</h2>
                <p class="progress">{move || status.with(|game| game.as_ref().map(|game| crate::i18n::tf(
                    "{0} XP · {1} Medaillen · {2} Lerntage",
                    &[&game.xp, &medals(game), &game.learning_days],
                )).unwrap_or_default())}</p>
                <p class="notes">{move || if signed_in.get() {
                    t("Wird über Locora Cloud mit deinen Geräten abgeglichen.")
                } else {
                    t("Nur auf diesem Gerät gespeichert.")
                }}</p>
                <label class="info-head">{t("Wochenziel")}
                    <Info text="Ein Lerntag zählt, wenn du auf einem deiner Geräte etwas wiederholst oder neu lernst, oder wenn nichts fällig ist. Ein verpasster Tag kostet nichts." />
                    <select disabled=move || busy.get()
                        prop:value=move || status.with(|game| game.as_ref().map_or(4, |game| game.week.goal)).to_string()
                        on:change=move |event| {
                            if let Ok(days) = event_target_value(&event).parse::<u8>() {
                                set_goal(days);
                            }
                        }>
                        {(1..=7u8).map(|days| view! {
                            <option value=days.to_string()>{crate::i18n::tf("An {0} Tagen pro Woche lernen", &[&days])}</option>
                        }).collect_view()}
                    </select>
                </label>
                <label><input type="checkbox" prop:checked=move || crate::locus_display::signal().get().gamification
                    on:change=move |event| { crate::locus_display::signal().update(|p| p.gamification = event_target_checked(&event)); crate::locus_display::save(crate::locus_display::signal().get_untracked()); } />{t("Fortschrittsleiste in der Journey anzeigen")}<Info text="Zeigt beim Lernen XP, das heutige Fälligkeitsziel und deine Woche. Ausschalten blendet sie nur aus." /></label>
                <Show
                    when=move || confirming.get()
                    fallback=move || view! {
                        <button type="button" class="danger" disabled=move || busy.get() on:click=move |_| confirming.set(true)>
                            {t("Spielfortschritt löschen …")}
                        </button>
                    }
                >
                    <div class="game-reset" on:keydown=move |event| {
                        if event.key() == "Escape" {
                            event.prevent_default();
                            confirming.set(false);
                        }
                    }>
                        <h3>{t("Spielfortschritt löschen?")}</h3>
                        <p role="alert" tabindex="-1" node_ref=warning>{t("Gelöscht werden: XP, Trophäen und Medaillen (die Schritte von „Dein Weg in Locora“ sind wieder offen), Lerntage und Wochenserien, das Tagesziel und der Fortschritt in Challenges.")}</p>
                        <p>{t("Erhalten bleiben: deine Paläste, Orte und Medien, der Wiederholungsplan und -verlauf (wann ein Ort fällig ist, ändert sich nicht), die Statistik, deine Einstellungen samt Wochenziel und dein Cloud-Konto.")}</p>
                        <p>{t("Palastmedaillen kommen von selbst zurück, wo deine Paläste sie noch erfüllen, denn der Wiederholungsplan bleibt.")}</p>
                        <p>{t("Auf deinen anderen Geräten wird der Spielfortschritt beim nächsten Sync gelöscht. Was ein Gerät bis dahin offline sammelt, verfällt. Geräte mit einer älteren Locora-Version zeigen ihre alten Zahlen, bis sie aktualisiert sind.")}</p>
                        {move || signed_in.get().then(|| view! {
                            <p>{t("Frühere verschlüsselte Kopien bleiben im Änderungsprotokoll des Servers, bis du „Cloud-Daten löschen“ verwendest; dort kann sie niemand lesen.")}</p>
                        })}
                        <p class="game-rankings">{move || rankings_note(signed_in.get())}</p>
                        <button type="button" class="danger" disabled=move || busy.get() on:click=move |_| reset()>
                            {t("Ja, Spielfortschritt auf allen Geräten löschen")}
                        </button>
                        <button type="button" disabled=move || busy.get() on:click=move |_| confirming.set(false)>
                            {t("Abbrechen")}
                        </button>
                    </div>
                </Show>
                {move || message.get().map(|m| match m {
                    Ok(text) => view! { <p class="verdict ok" role="status">{text}</p> }.into_any(),
                    Err(text) => view! { <p class="notes" role="alert">{text}</p> }.into_any(),
                })}
            </section>
        </Show>
    }
}

#[component]
fn Stats(stats: StatsView) -> impl IntoView {
    let rate = if stats.recent == 0 {
        "–".to_owned()
    } else {
        crate::i18n::tf(
            "{0} ({1} von {2})",
            &[
                &percent(f64::from(stats.recent_known) / f64::from(stats.recent)),
                &stats.recent_known,
                &stats.recent,
            ],
        )
    };
    let mut rows = vec![
        (t("Antworten insgesamt"), stats.answers.to_string()),
        (
            t("Heute"),
            crate::i18n::tf(
                "{0} Antworten, {1} neue Loci",
                &[&stats.answered_today, &stats.new_today],
            ),
        ),
        (t("Trefferquote 30 Tage"), rate),
        // With the game: learning days and weekly goals on every device; else the review log's run.
        match stats.learning_days {
            Some(days) => (
                t("Lerntage"),
                if days == 1 {
                    t("1 Lerntag").to_owned()
                } else {
                    crate::i18n::tf("{0} Lerntage", &[&days])
                },
            ),
            None => (
                t("Serie"),
                if stats.streak == 1 {
                    t("1 Tag").to_owned()
                } else {
                    crate::i18n::tf("{0} Tage", &[&stats.streak])
                },
            ),
        },
        (
            t("Geplante Loci"),
            crate::i18n::tf("{0} (davon {1} fällig)", &[&stats.scheduled, &stats.due]),
        ),
    ];
    if let Some(weeks) = stats.week_streak {
        rows.insert(
            4,
            (
                t("Wochenziel in Folge"),
                if weeks == 1 {
                    t("1 Woche").to_owned()
                } else {
                    crate::i18n::tf("{0} Wochen", &[&weeks])
                },
            ),
        );
    }
    view! {
        <section class="card panel">
            <div class="prompt">{t("Statistik")}</div>
            <dl class="stats">
                {rows
                    .into_iter()
                    .map(|(k, v)| view! { <dt>{k}</dt><dd>{v}</dd> })
                    .collect_view()}
            </dl>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_confirmation_says_what_happens_to_the_rankings() {
        crate::i18n::set_current(crate::i18n::Lang::En);
        assert_eq!(
            rankings_note(true),
            "Your ranking entries are also deleted from the Locora Cloud server."
        );
        assert_eq!(
            rankings_note(false),
            "This device is not signed in to Locora Cloud, so ranking entries on the server are not touched."
        );
        let older = reset_message(GameResetReport {
            rankings: RankingsDeletion::ServerTooOld,
        });
        assert!(older.starts_with("Game progress deleted."), "{older}");
        assert!(
            older.contains("cannot delete ranking entries yet"),
            "{older}"
        );
        assert_eq!(
            reset_message(GameResetReport {
                rankings: RankingsDeletion::NotSignedIn
            }),
            "Game progress deleted."
        );
        crate::i18n::set_current(crate::i18n::Lang::De);
    }
}
