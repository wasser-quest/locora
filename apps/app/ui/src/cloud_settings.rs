//! Cloud sign-in, account-key setup and manually triggered encrypted sync.

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_CLOUD_CONNECT, CMD_CLOUD_KEYS_SETUP, CMD_CLOUD_KEYS_STATUS, CMD_CLOUD_LOGOUT,
    CMD_CLOUD_REFRESH, CMD_CLOUD_STATUS, CMD_CLOUD_SYNC, CloudConnect, CloudKeyCreated,
    CloudKeySetup, CloudStatus, CloudSyncResult,
};
use serde::Serialize;

use crate::load;
use crate::tauri::{NoArgs, call};

#[derive(Serialize)]
struct ConnectArgs {
    request: CloudConnect,
}

#[derive(Serialize)]
struct KeyArgs {
    request: CloudKeySetup,
}

#[derive(Serialize)]
struct SyncArgs {
    passphrase: String,
}

fn refresh_keys_for(
    status: RwSignal<Option<Result<CloudStatus, String>>>,
    keys: RwSignal<Option<Result<bool, String>>>,
    generation: RwSignal<u64>,
) {
    generation.update(|value| *value = value.wrapping_add(1));
    let request = generation.get_untracked();
    keys.set(None);
    let account = status
        .get_untracked()
        .and_then(|result| result.ok()?.account);
    let Some(account) = account else { return };
    spawn_local(async move {
        let result = call::<bool>(CMD_CLOUD_KEYS_STATUS, NoArgs {}).await;
        let current = status
            .get_untracked()
            .and_then(|result| result.ok()?.account);
        if generation.get_untracked() == request && current.as_deref() == Some(&account) {
            keys.set(Some(result));
        }
    });
}

#[component]
pub fn CloudSettings() -> impl IntoView {
    let status = load(call::<CloudStatus>(CMD_CLOUD_STATUS, NoArgs {}));
    let keys = RwSignal::new(None::<Result<bool, String>>);
    let recovery = RwSignal::new(None::<CloudKeyCreated>);
    let message = RwSignal::new(None::<Result<String, String>>);
    let busy = RwSignal::new(false);
    let generation = RwSignal::new(0u64);

    Effect::new(move |_| {
        let _ = status.get();
        refresh_keys_for(status, keys, generation);
    });

    let refresh_keys = move || refresh_keys_for(status, keys, generation);
    let refresh_access = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match call::<CloudStatus>(CMD_CLOUD_REFRESH, NoArgs {}).await {
                Ok(updated) => {
                    status.set(Some(Ok(updated)));
                    message.set(Some(Ok("Cloud-Freischaltung aktualisiert.".into())));
                }
                Err(error) => message.set(Some(Err(error))),
            }
            busy.set(false);
        });
    };
    let logout = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match call::<()>(CMD_CLOUD_LOGOUT, NoArgs {}).await {
                Ok(()) => {
                    recovery.set(None);
                    status.set(Some(Ok(CloudStatus {
                        account: None,
                        server: None,
                        access: false,
                    })));
                    message.set(Some(Ok("Auf diesem Gerät abgemeldet.".into())));
                }
                Err(error) => message.set(Some(Err(error))),
            }
            busy.set(false);
        });
    };

    view! {
        <section class="card panel" aria-label="Locora Cloud">
            <h2>"Locora Cloud"</h2>
            {move || match status.get() {
                None => view! { <p class="progress">"Cloud-Konto wird geprüft …"</p> }.into_any(),
                Some(Err(error)) => view! { <p class="notes" role="alert">{error}</p> }.into_any(),
                Some(Ok(CloudStatus { account: None, .. })) => view! {
                    <ConnectForm status=status message=message busy=busy />
                }.into_any(),
                Some(Ok(CloudStatus { account: Some(account), server, access })) => view! {
                    <p class="progress">{format!("Angemeldet als {account} bei {}.", server.unwrap_or_default())}</p>
                    <button type="button" disabled=move || busy.get() on:click=move |_| refresh_access()>
                        "Freischaltung aktualisieren"
                    </button>
                    <button type="button" disabled=move || busy.get() on:click=move |_| logout()>
                        "Auf diesem Gerät abmelden"
                    </button>
                    {move || match keys.get() {
                        None => view! { <p class="progress">"Verschlüsselung wird geprüft …"</p> }.into_any(),
                        Some(Err(error)) => view! {
                            <p class="notes" role="alert">{error}</p>
                            <button type="button" disabled=move || busy.get() on:click=move |_| refresh_keys()>"Erneut prüfen"</button>
                        }.into_any(),
                        Some(Ok(false)) => view! {
                            <p class="notes">"Für dieses Konto ist noch kein Ende-zu-Ende-Schlüssel hinterlegt."</p>
                            {move || recovery.get().is_none().then(|| view! {
                                <KeySetupForm keys=keys recovery=recovery message=message busy=busy generation=generation />
                            })}
                            <button type="button" disabled=move || busy.get() on:click=move |_| refresh_keys()>"Schlüsselstatus prüfen"</button>
                        }.into_any(),
                        Some(Ok(true)) => view! {
                            {if recovery.get().is_some() {
                                view! { <p class="notes">"Wiederherstellungsschlüssel zuerst sichern."</p> }.into_any()
                            } else if access {
                                view! { <SyncForm /> }.into_any()
                            } else {
                                view! { <p class="notes">"Verschlüsselung eingerichtet. Für Cloud-Sync fehlt derzeit eine gültige Freischaltung."</p> }.into_any()
                            }}
                        }.into_any(),
                    }}
                }.into_any(),
            }}
            {move || recovery.get().map(|result| view! {
                <div class="verdict" role="status">
                    <p>"Wiederherstellungsschlüssel – nur jetzt sichtbar. Sicher außerhalb der App aufbewahren:"</p>
                    <code class="recovery-key">{result.recovery_key}</code>
                    {result.warning.map(|warning| view! { <p class="notes" role="alert">{warning}</p> })}
                    <p>"Ohne Verschlüsselungs-Passwort und diesen Schlüssel sind Cloud-Daten nicht mehr lesbar."</p>
                    {move || match keys.get() {
                        Some(Ok(true)) => view! {
                            <button type="button" on:click=move |_| recovery.set(None)>
                                "Ich habe den Schlüssel gesichert"
                            </button>
                        }.into_any(),
                        Some(Ok(false)) => view! {
                            <button type="button" on:click=move |_| recovery.set(None)>
                                "Server hat keinen Schlüssel – neuen Versuch starten"
                            </button>
                        }.into_any(),
                        _ => view! {
                            <p class="notes">"Schlüsselstatus zuerst prüfen. Bis dahin diesen Wiederherstellungsschlüssel sichtbar lassen."</p>
                        }.into_any(),
                    }}
                </div>
            })}
            {move || message.get().map(|result| match result {
                Ok(value) => view! { <p class="verdict ok" role="status">{value}</p> }.into_any(),
                Err(value) => view! { <p class="notes" role="alert">{value}</p> }.into_any(),
            })}
        </section>
    }
}

#[component]
fn ConnectForm(
    status: RwSignal<Option<Result<CloudStatus, String>>>,
    message: RwSignal<Option<Result<String, String>>>,
    busy: RwSignal<bool>,
) -> impl IntoView {
    let register = RwSignal::new(false);
    let server = RwSignal::new(String::new());
    let email = RwSignal::new(String::new());
    let display_name = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let confirmation = RwSignal::new(String::new());
    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        let creating = register.get_untracked();
        let secret = password.get_untracked();
        if creating && secret != confirmation.get_untracked() {
            password.set(String::new());
            confirmation.set(String::new());
            message.set(Some(Err(
                "Die Cloud-Passwörter stimmen nicht überein.".into()
            )));
            return;
        }
        let request = CloudConnect {
            server: server.get_untracked(),
            email: email.get_untracked(),
            password: secret,
            display_name: creating.then(|| display_name.get_untracked()),
            register: creating,
        };
        password.set(String::new());
        confirmation.set(String::new());
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match call::<CloudStatus>(CMD_CLOUD_CONNECT, ConnectArgs { request }).await {
                Ok(connected) => status.set(Some(Ok(connected))),
                Err(error) => {
                    message.set(Some(Err(error)));
                    // A response can fail after the session was persisted.
                    status.set(Some(call::<CloudStatus>(CMD_CLOUD_STATUS, NoArgs {}).await));
                }
            }
            busy.set(false);
        });
    };
    view! {
        <p class="notes">"Konto und Verschlüsselung lassen sich hier direkt auf diesem Gerät einrichten."</p>
        <form on:submit=move |event| { event.prevent_default(); submit(); }>
            <label class="input"><span>"Cloud-Server (HTTPS)"</span>
                <input type="url" placeholder="https://cloud.example.org" autocomplete="url" bind:value=server required />
            </label>
            <label class="input"><span>"E-Mail"</span>
                <input type="email" autocomplete="username" bind:value=email required />
            </label>
            <label><input type="checkbox" prop:checked=move || register.get()
                on:change=move |event| register.set(event_target_checked(&event)) />"Neues Konto erstellen"</label>
            <Show when=move || register.get()>
                <label class="input"><span>"Anzeigename"</span>
                    <input type="text" maxlength="32" bind:value=display_name />
                </label>
            </Show>
            <label class="input"><span>"Cloud-Passwort"</span>
                <input type="password" autocomplete="current-password" bind:value=password required />
            </label>
            <Show when=move || register.get()>
                <label class="input"><span>"Cloud-Passwort wiederholen"</span>
                    <input type="password" autocomplete="new-password" bind:value=confirmation />
                </label>
            </Show>
            <button class="primary" type="submit" disabled=move || busy.get()>
                {move || if register.get() { "Konto erstellen" } else { "Anmelden" }}
            </button>
        </form>
    }
}

#[component]
fn KeySetupForm(
    keys: RwSignal<Option<Result<bool, String>>>,
    recovery: RwSignal<Option<CloudKeyCreated>>,
    message: RwSignal<Option<Result<String, String>>>,
    busy: RwSignal<bool>,
    generation: RwSignal<u64>,
) -> impl IntoView {
    let passphrase = RwSignal::new(String::new());
    let confirmation = RwSignal::new(String::new());
    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        let secret = passphrase.get_untracked();
        if secret != confirmation.get_untracked() {
            passphrase.set(String::new());
            confirmation.set(String::new());
            message.set(Some(Err(
                "Die Verschlüsselungs-Passwörter stimmen nicht überein.".into(),
            )));
            return;
        }
        passphrase.set(String::new());
        confirmation.set(String::new());
        message.set(None);
        busy.set(true);
        spawn_local(async move {
            match call::<CloudKeyCreated>(
                CMD_CLOUD_KEYS_SETUP,
                KeyArgs {
                    request: CloudKeySetup { passphrase: secret },
                },
            )
            .await
            {
                Ok(created) => {
                    generation.update(|value| *value = value.wrapping_add(1));
                    if created.stored {
                        keys.set(Some(Ok(true)));
                    } else {
                        keys.set(Some(Err(
                            "Schlüsselstatus unklar. Bitte erneut prüfen.".into()
                        )));
                    }
                    recovery.set(Some(created));
                }
                Err(error) => message.set(Some(Err(error))),
            }
            busy.set(false);
        });
    };
    view! {
        <form on:submit=move |event| { event.prevent_default(); submit(); }>
            <label class="input"><span>"Neues Verschlüsselungs-Passwort (mindestens 12 Zeichen)"</span>
                <input type="password" autocomplete="new-password" bind:value=passphrase required />
            </label>
            <label class="input"><span>"Verschlüsselungs-Passwort wiederholen"</span>
                <input type="password" autocomplete="new-password" bind:value=confirmation required />
            </label>
            <button class="primary" type="submit" disabled=move || busy.get()>
                "Verschlüsselung einrichten"
            </button>
        </form>
    }
}

#[component]
fn SyncForm() -> impl IntoView {
    let passphrase = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let outcome = RwSignal::new(None::<Result<CloudSyncResult, String>>);
    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        let secret = passphrase.get_untracked();
        if secret.is_empty() {
            outcome.set(Some(Err("Verschlüsselungs-Passwort eingeben.".into())));
            return;
        }
        passphrase.set(String::new());
        busy.set(true);
        outcome.set(None);
        spawn_local(async move {
            outcome.set(Some(
                call::<CloudSyncResult>(CMD_CLOUD_SYNC, SyncArgs { passphrase: secret }).await,
            ));
            busy.set(false);
        });
    };
    view! {
        <p class="progress">"Änderungen werden nur auf Knopfdruck Ende-zu-Ende-verschlüsselt synchronisiert."</p>
        <form on:submit=move |event| { event.prevent_default(); submit(); }>
            <label class="input"><span>"Verschlüsselungs-Passwort"</span>
                <input type="password" autocomplete="off" bind:value=passphrase />
            </label>
            <button class="primary" type="submit" disabled=move || busy.get()>
                {move || if busy.get() { "Synchronisiert …" } else { "Jetzt synchronisieren" }}
            </button>
        </form>
        {move || outcome.get().map(|result| match result {
            Ok(report) => view! {
                <p class="verdict ok" role="status">{format!(
                    "Sync abgeschlossen: {} Änderungen gesendet, {} Medien hochgeladen, {} Änderungen empfangen, {} Medien geladen; {} angewendet, {} schon bekannt, {} mit fehlendem Ort.",
                    report.pushed, report.media_uploaded, report.pulled,
                    report.media_downloaded, report.applied, report.stale, report.missing,
                )}</p>
                {(report.failed != 0 || report.quarantined != 0).then(|| view! {
                    <p class="notes" role="alert">{format!(
                        "{} Änderungen konnten nicht angewendet werden, {} wurden in Quarantäne abgelegt. Bitte lokalen Datenbestand prüfen.",
                        report.failed, report.quarantined,
                    )}</p>
                })}
            }.into_any(),
            Err(error) => view! { <p class="notes" role="alert">{error}</p> }.into_any(),
        })}
    }
}
