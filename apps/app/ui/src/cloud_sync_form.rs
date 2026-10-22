//! Syncing in the cloud settings: what a sync sends (asked before a palace deletion goes out),
//! what other devices changed, and auto-sync with its status line.

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_CLOUD_AUTO_DISABLE, CMD_CLOUD_AUTO_ENABLE, CMD_CLOUD_PENDING, CMD_CLOUD_SYNC,
    CloudChangeKind, CloudChanges, CloudPalaceChange, CloudPending, CloudStatus, CloudSyncResult,
};
use serde::Serialize;
use wasm_bindgen::JsValue;

use crate::auto_sync::{ConfirmText, use_auto_sync};
use crate::i18n::{t, tf, translate_message};
use crate::info::Info;
use crate::tauri::{NoArgs, call};

#[derive(Serialize)]
struct SyncArgs {
    passphrase: String,
    confirmed: Vec<String>,
}

#[derive(Serialize)]
struct PassphraseArgs {
    passphrase: String,
}

/// A time as the person's language writes it.
fn when(secs: i64) -> String {
    let date = js_sys::Date::new(&JsValue::from_f64(secs as f64 * 1000.0));
    date.to_locale_string(crate::i18n::current().code(), &JsValue::UNDEFINED)
        .into()
}

fn name(palace: &CloudPalaceChange) -> String {
    palace
        .name
        .clone()
        .unwrap_or_else(|| t("(ohne Namen)").into())
}

/// What this device sends, palace by palace.
fn outgoing(pending: &CloudPending) -> impl IntoView + use<> {
    let local = &pending.local;
    let lines: Vec<String> = local
        .palaces
        .iter()
        .map(|palace| match palace.kind {
            CloudChangeKind::New => tf("„{0}“ – neu", &[&name(palace)]),
            CloudChangeKind::Changed => {
                tf("„{0}“ – {1} Änderungen", &[&name(palace), &palace.changes])
            }
            CloudChangeKind::Deleted => tf("„{0}“ – gelöscht", &[&name(palace)]),
        })
        .chain((local.other > 0).then(|| {
            tf(
                "{0} weitere Änderungen, zum Beispiel Wiederholungen",
                &[&local.other],
            )
        }))
        .collect();
    let remote = match pending.remote {
        Some(0) => {
            Some(t("In der Cloud wartet nichts Neues von deinen anderen Geräten.").to_owned())
        }
        Some(count) => Some(tf(
            "{0} Änderungen von deinen anderen Geräten warten in der Cloud.",
            &[&count],
        )),
        None => None,
    };
    view! {
        <div class="cloud-summary" role="status">
            {if lines.is_empty() {
                view! { <p>{t("Dieses Gerät hat keine neuen Änderungen.")}</p> }.into_any()
            } else {
                view! {
                    <p>{t("Dieses Gerät sendet:")}</p>
                    <ul>{lines.into_iter().map(|line| view! { <li>{line}</li> }).collect_view()}</ul>
                }.into_any()
            }}
            {remote.map(|line| view! { <p>{line}</p> })}
        </div>
    }
}

/// What the other devices changed here, in words.
fn incoming(received: &CloudChanges) -> impl IntoView + use<> {
    let lines: Vec<String> = received
        .palaces
        .iter()
        .map(|palace| match palace.kind {
            CloudChangeKind::New => tf("„{0}“ ist neu dazugekommen", &[&name(palace)]),
            CloudChangeKind::Changed => tf(
                "„{0}“ wurde geändert ({1} Änderungen)",
                &[&name(palace), &palace.changes],
            ),
            CloudChangeKind::Deleted => tf("„{0}“ wurde gelöscht", &[&name(palace)]),
        })
        .chain((received.other > 0).then(|| {
            tf(
                "{0} weitere Änderungen, zum Beispiel Wiederholungen",
                &[&received.other],
            )
        }))
        .collect();
    if lines.is_empty() {
        return view! { <p>{t("Von deinen anderen Geräten kam nichts Neues.")}</p> }.into_any();
    }
    view! {
        <p>{t("Von deinen anderen Geräten:")}</p>
        <ul>{lines.into_iter().map(|line| view! { <li>{line}</li> }).collect_view()}</ul>
    }
    .into_any()
}

/// The result of a sync, as the person reads it.
pub fn report(result: &CloudSyncResult) -> impl IntoView + use<> {
    view! {
        <div class="verdict ok" role="status">
            <p><strong>{t("Sync abgeschlossen.")}</strong>" "{tf(
                "{0} Änderungen gesendet, {1} Bilder oder Videos hochgeladen.",
                &[&result.pushed, &result.media_uploaded],
            )}</p>
            {incoming(&result.received)}
        </div>
        {(result.too_large != 0).then(|| view! {
            <p class="notes" role="alert">{tf(
                "{0} Änderungen sind zu groß für die Cloud und bleiben nur auf diesem Gerät.",
                &[&result.too_large],
            )}</p>
        })}
        {(result.failed != 0 || result.quarantined != 0).then(|| view! {
            <p class="notes" role="alert">{tf(
                "{0} Änderungen konnten nicht angewendet werden, {1} wurden in Quarantäne abgelegt. Bitte lokalen Datenbestand prüfen.",
                &[&result.failed, &result.quarantined],
            )}</p>
        })}
    }
}

/// The sync form: the password for a sync by hand or for turning auto-sync on, the summary before
/// a sync, the question for deletions, and the result.
#[component]
pub fn SyncPanel(status: RwSignal<Option<Result<CloudStatus, String>>>) -> impl IntoView {
    let auto = use_auto_sync();
    let passphrase = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let outcome = RwSignal::new(None::<Result<CloudSyncResult, String>>);
    let pending = RwSignal::new(None::<CloudPending>);
    // The password while the person decides about a deletion; cleared right after.
    let held = RwSignal::new(None::<String>);
    let current = move || {
        status
            .get()
            .and_then(|result| result.ok())
            .unwrap_or_else(CloudStatus::signed_out)
    };
    let auto_on = move || current().auto_sync;

    let run = move |secret: String, confirmed: Vec<String>| {
        busy.set(true);
        spawn_local(async move {
            let result = call::<CloudSyncResult>(
                CMD_CLOUD_SYNC,
                SyncArgs {
                    passphrase: secret,
                    confirmed,
                },
            )
            .await;
            if result.is_ok() {
                let at = (js_sys::Date::now() / 1000.0) as i64;
                if let Some(auto) = auto {
                    auto.synced(at);
                }
                status.update(|status| {
                    if let Some(Ok(status)) = status {
                        status.last_sync_at = Some(at);
                    }
                });
            }
            outcome.set(Some(result));
            busy.set(false);
        });
    };
    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        let secret = passphrase.get_untracked();
        if secret.is_empty() {
            outcome.set(Some(Err(t("Verschlüsselungs-Passwort eingeben.").into())));
            return;
        }
        passphrase.set(String::new());
        busy.set(true);
        outcome.set(None);
        pending.set(None);
        spawn_local(async move {
            match call::<CloudPending>(CMD_CLOUD_PENDING, NoArgs {}).await {
                Ok(summary) => {
                    let deleted = summary
                        .local
                        .palaces
                        .iter()
                        .any(|palace| palace.kind == CloudChangeKind::Deleted);
                    pending.set(Some(summary));
                    if deleted {
                        // Nothing is sent before the person agrees.
                        held.set(Some(secret));
                        busy.set(false);
                    } else {
                        run(secret, Vec::new());
                    }
                }
                Err(error) => {
                    outcome.set(Some(Err(error)));
                    busy.set(false);
                }
            }
        });
    };
    let deletions = move || {
        pending
            .get()
            .map(|summary| {
                summary
                    .local
                    .palaces
                    .into_iter()
                    .filter(|palace| palace.kind == CloudChangeKind::Deleted)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let confirm = move |_| {
        let Some(secret) = held.get_untracked() else {
            return;
        };
        held.set(None);
        let ids = deletions().into_iter().map(|palace| palace.id).collect();
        run(secret, ids);
    };
    let cancel = move |_| {
        held.set(None);
        pending.set(None);
    };
    let enable = move |_| {
        if busy.get_untracked() {
            return;
        }
        let secret = passphrase.get_untracked();
        if secret.is_empty() {
            outcome.set(Some(Err(t("Verschlüsselungs-Passwort eingeben.").into())));
            return;
        }
        passphrase.set(String::new());
        busy.set(true);
        outcome.set(None);
        spawn_local(async move {
            match call::<CloudStatus>(CMD_CLOUD_AUTO_ENABLE, PassphraseArgs { passphrase: secret })
                .await
            {
                Ok(updated) => {
                    status.set(Some(Ok(updated)));
                    if let Some(auto) = auto {
                        auto.set_enabled(true);
                    }
                }
                Err(error) => outcome.set(Some(Err(error))),
            }
            busy.set(false);
        });
    };
    let disable = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match call::<CloudStatus>(CMD_CLOUD_AUTO_DISABLE, NoArgs {}).await {
                Ok(updated) => {
                    status.set(Some(Ok(updated)));
                    if let Some(auto) = auto {
                        auto.set_enabled(false);
                    }
                }
                Err(error) => outcome.set(Some(Err(error))),
            }
            busy.set(false);
        });
    };
    let last_sync = move || {
        let from_auto = auto.and_then(|auto| auto.view.get().last_sync_at);
        current().last_sync_at.max(from_auto)
    };
    view! {
        <p class="progress cloud-last-sync">{move || match last_sync() {
            _ if auto.is_some_and(|auto| auto.view.get().running) => t("Synchronisiert …").to_owned(),
            Some(at) => tf("Zuletzt synchronisiert: {0}", &[&when(at)]),
            None => t("Noch nicht synchronisiert.").to_owned(),
        }}</p>
        {move || auto.map(|auto| {
            let view = auto.view.get();
            view! {
                {(!view.confirm.is_empty()).then(|| {
                    let palaces = StoredValue::new(view.confirm.clone());
                    view! {
                        <div class="notes" role="alert">
                            <ConfirmText palaces=view.confirm.clone() />
                            <button class="danger" on:click=move |_| auto.confirm(&palaces.get_value())>
                                {t("Löschen bestätigen")}
                            </button>
                        </div>
                    }
                })}
                {view.paused.clone().map(|paused| view! {
                    <p class="notes">{translate_message(&paused)}" "
                        <button type="button" on:click=move |_| auto.nudge()>{t("Erneut versuchen")}</button>
                    </p>
                })}
                {view.error.clone().map(|error| view! {
                    <p class="meta">{tf(
                        "Auto-Sync klappt gerade nicht: {0}. Die App versucht es später noch einmal.",
                        &[&translate_message(&error)],
                    )}</p>
                })}
            }
        })}
        <form on:submit=move |event| { event.prevent_default(); submit(); }>
            <label class="input"><span class="info-head">{t("Verschlüsselungs-Passwort")}<Info text="Damit werden deine Daten verschlüsselt, bevor sie in die Cloud gehen. Es ist ein eigenes Passwort, nicht dein Cloud-Passwort, und der Server kennt es nicht. Verlierst du es und den Wiederherstellungsschlüssel, kann niemand die Cloud-Daten wieder lesbar machen." /></span>
                <input type=crate::cloud_settings::password_type() autocomplete="off" bind:value=passphrase />
            </label>
            <crate::cloud_settings::ShowPasswords />
            <button class="primary" type="submit" disabled=move || busy.get() || held.with(Option::is_some)>
                {move || if busy.get() { t("Synchronisiert …") } else { t("Jetzt synchronisieren") }}
            </button>
            <Show when=move || !auto_on()>
                <button type="button" disabled=move || busy.get() on:click=enable>{t("Auto-Sync einschalten")}</button>
                <p class="meta">{t("Mit Auto-Sync synchronisiert die App von selbst. Dafür wird der Schlüssel sicher auf diesem Gerät gespeichert, im Schlüsselspeicher des Systems.")}</p>
            </Show>
        </form>
        <Show when=auto_on>
            <p class="progress">{t("Auto-Sync ist an: Die App synchronisiert beim Start, kurz nach deinen Änderungen und alle zehn Minuten.")}</p>
            <button type="button" disabled=move || busy.get() on:click=disable>{t("Schlüssel auf diesem Gerät vergessen")}</button>
        </Show>
        {move || pending.get().map(|summary| outgoing(&summary))}
        {move || {
            let waiting = deletions();
            (held.with(Option::is_some) && !waiting.is_empty()).then(|| view! {
                <div class="notes" role="alert">
                    <ConfirmText palaces=waiting />
                    <button type="button" class="danger" on:click=confirm>{t("Löschen bestätigen und synchronisieren")}</button>
                    " "
                    <button type="button" on:click=cancel>{t("Abbrechen")}</button>
                </div>
            })
        }}
        {move || outcome.get().map(|result| match result {
            Ok(result) => report(&result).into_any(),
            Err(error) => view! { <p class="notes" role="alert">{error}</p> }.into_any(),
        })}
    }
}
