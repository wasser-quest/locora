//! Cloud sign-in, account-key setup and manually triggered encrypted sync.

use crate::i18n::{t, tf};
use crate::info::Info;
use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_CLOUD_CONNECT, CMD_CLOUD_DELETE_ACCOUNT, CMD_CLOUD_EMAIL_RESEND, CMD_CLOUD_ERASE,
    CMD_CLOUD_KEYS_SETUP, CMD_CLOUD_KEYS_STATUS, CMD_CLOUD_LOGOUT, CMD_CLOUD_REFRESH,
    CMD_CLOUD_STATUS, CMD_SAVE_TEXT_PICK, CloudConnect, CloudKeyCreated, CloudKeySetup,
    CloudStatus,
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
struct DeleteAccountArgs {
    password: String,
}

/// Whether the cloud forms show the passwords as typed; one switch for all of them.
#[derive(Clone, Copy)]
struct PasswordsShown(RwSignal<bool>);

/// The `type` of a password field: plain text while the passwords are shown.
pub(crate) fn password_type() -> impl Fn() -> &'static str + Copy {
    let shown = use_context::<PasswordsShown>().map(|shown| shown.0);
    move || {
        if shown.is_some_and(|shown| shown.get()) {
            "text"
        } else {
            "password"
        }
    }
}

/// The "Passwörter anzeigen" checkbox under a form's password fields.
#[component]
pub(crate) fn ShowPasswords() -> impl IntoView {
    let shown =
        use_context::<PasswordsShown>().map_or_else(|| RwSignal::new(false), |shown| shown.0);
    view! {
        <label class="input check"><input type="checkbox" bind:checked=shown />{t("Passwörter anzeigen")}</label>
    }
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
    provide_context(PasswordsShown(RwSignal::new(false)));
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
                    message.set(Some(Ok(t("Cloud-Freischaltung aktualisiert.").into())));
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
                    status.set(Some(Ok(CloudStatus::signed_out())));
                    if let Some(auto) = crate::auto_sync::use_auto_sync() {
                        auto.set_enabled(false);
                    }
                    message.set(Some(Ok(t("Auf diesem Gerät abgemeldet.").into())));
                }
                Err(error) => message.set(Some(Err(error))),
            }
            busy.set(false);
        });
    };

    view! {
        <section class="card panel" aria-label=t("Locora Cloud")>
            <h2>{t("Locora Cloud")}</h2>
            {move || match status.get() {
                None => view! { <p class="progress">{t("Cloud-Konto wird geprüft …")}</p> }.into_any(),
                Some(Err(error)) => view! { <p class="notes" role="alert">{error}</p> }.into_any(),
                Some(Ok(CloudStatus { account: None, .. })) => view! {
                    <ConnectForm status=status message=message busy=busy />
                }.into_any(),
                Some(Ok(CloudStatus { account: Some(account), server, access, email_verified, .. })) => view! {
                    <p class="progress">{tf("Angemeldet als {0} bei {1}.", &[&account, &server.unwrap_or_default()])}</p>
                    {(email_verified == Some(false)).then(|| view! { <EmailPrompt status=status email=account.clone() /> })}
                    <button type="button" disabled=move || busy.get() on:click=move |_| refresh_access()>
                        {t("Freischaltung aktualisieren")}
                    </button>
                    <button type="button" disabled=move || busy.get() on:click=move |_| logout()>
                        {t("Auf diesem Gerät abmelden")}
                    </button>
                    <DeleteCloud status=status message=message busy=busy recovery=recovery />
                    {move || match keys.get() {
                        None => view! { <p class="progress">{t("Verschlüsselung wird geprüft …")}</p> }.into_any(),
                        Some(Err(error)) => view! {
                            <p class="notes" role="alert">{error}</p>
                            <button type="button" disabled=move || busy.get() on:click=move |_| refresh_keys()>{t("Erneut prüfen")}</button>
                        }.into_any(),
                        Some(Ok(false)) => view! {
                            <p class="notes">{t("Für dieses Konto ist noch kein Ende-zu-Ende-Schlüssel hinterlegt.")}</p>
                            {move || recovery.get().is_none().then(|| view! {
                                <KeySetupForm keys=keys recovery=recovery message=message busy=busy generation=generation />
                            })}
                            <button type="button" disabled=move || busy.get() on:click=move |_| refresh_keys()>{t("Schlüsselstatus prüfen")}</button>
                        }.into_any(),
                        Some(Ok(true)) => view! {
                            {if recovery.get().is_some() {
                                view! { <p class="notes">{t("Wiederherstellungsschlüssel zuerst sichern.")}</p> }.into_any()
                            } else if access {
                                view! { <crate::cloud_sync_form::SyncPanel status=status /> }.into_any()
                            } else {
                                view! { <p class="notes">{t("Verschlüsselung eingerichtet. Für Cloud-Sync fehlt derzeit eine gültige Freischaltung.")}</p> }.into_any()
                            }}
                        }.into_any(),
                    }}
                }.into_any(),
            }}
            {move || recovery.get().map(|result| view! {
                <div class="recovery-panel" role="status">
                    <p><strong>{t("Wiederherstellungsschlüssel – nur jetzt sichtbar. Sicher außerhalb der App aufbewahren:")}</strong></p>
                    <pre class="recovery-key">{result.recovery_key.clone()}</pre>
                    <SaveRecoveryKey key=result.recovery_key />
                    {result.warning.map(|warning| view! { <p class="notes" role="alert">{warning}</p> })}
                    <p class="meta">{t("Ohne Verschlüsselungs-Passwort und diesen Schlüssel sind Cloud-Daten nicht mehr lesbar.")}</p>
                    {move || match keys.get() {
                        Some(Ok(true)) => view! {
                            <button type="button" class="primary wide" on:click=move |_| recovery.set(None)>
                                {t("Ich habe den Schlüssel gesichert")}
                            </button>
                        }.into_any(),
                        Some(Ok(false)) => view! {
                            <button type="button" class="wide" on:click=move |_| recovery.set(None)>
                                {t("Server hat keinen Schlüssel – neuen Versuch starten")}
                            </button>
                        }.into_any(),
                        _ => view! {
                            <p class="notes">{t("Schlüsselstatus zuerst prüfen. Bis dahin diesen Wiederherstellungsschlüssel sichtbar lassen.")}</p>
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

/// Deleting by hand: the synced data on the server, or the whole account. The devices keep their data.
#[component]
fn DeleteCloud(
    status: RwSignal<Option<Result<CloudStatus, String>>>,
    message: RwSignal<Option<Result<String, String>>>,
    busy: RwSignal<bool>,
    recovery: RwSignal<Option<CloudKeyCreated>>,
) -> impl IntoView {
    let confirming = RwSignal::new(false);
    let password = RwSignal::new(String::new());
    let erase = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match call::<()>(CMD_CLOUD_ERASE, NoArgs {}).await {
                Ok(()) => message.set(Some(Ok(t("Cloud-Daten gelöscht. Die Daten auf diesem Gerät bleiben; der nächste Sync lädt sie wieder hoch.").into()))),
                Err(error) => message.set(Some(Err(error))),
            }
            confirming.set(false);
            busy.set(false);
        });
    };
    let delete_account = move || {
        if busy.get_untracked() {
            return;
        }
        let secret = password.get_untracked();
        if secret.is_empty() {
            message.set(Some(Err(t("Cloud-Passwort eingeben.").into())));
            return;
        }
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match call::<()>(
                CMD_CLOUD_DELETE_ACCOUNT,
                DeleteAccountArgs { password: secret },
            )
            .await
            {
                Ok(()) => {
                    password.set(String::new());
                    recovery.set(None);
                    status.set(Some(Ok(CloudStatus::signed_out())));
                    if let Some(auto) = crate::auto_sync::use_auto_sync() {
                        auto.set_enabled(false);
                    }
                    message.set(Some(Ok(t(
                        "Cloud-Konto gelöscht. Die Daten auf diesem Gerät bleiben.",
                    )
                    .into())));
                }
                Err(error) => message.set(Some(Err(error))),
            }
            busy.set(false);
        });
    };
    view! {
        <details class="cloud-delete">
            <summary>{t("Daten in der Cloud löschen")}</summary>
            <p class="notes">{t("Nur die Kopie auf dem Server wird gelöscht. Die Daten auf deinen Geräten bleiben erhalten.")}</p>
            <Show
                when=move || confirming.get()
                fallback=move || view! {
                    <button type="button" disabled=move || busy.get() on:click=move |_| confirming.set(true)>
                        {t("Cloud-Daten löschen …")}
                    </button>
                }
            >
                <p role="alert">{t("Alle synchronisierten Änderungen und Medien werden vom Server gelöscht. Andere angemeldete Geräte laden ihre Daten beim nächsten Sync wieder hoch.")}</p>
                <button type="button" class="danger" disabled=move || busy.get() on:click=move |_| erase()>
                    {t("Ja, Cloud-Daten löschen")}
                </button>
                <button type="button" disabled=move || busy.get() on:click=move |_| confirming.set(false)>
                    {t("Abbrechen")}
                </button>
            </Show>
            <form on:submit=move |event| { event.prevent_default(); delete_account(); }>
                <p class="notes">{t("Konto löschen entfernt das Konto mit allen Daten vom Server und meldet dieses Gerät ab. Ein Abo beim Zahlungsanbieter bitte dort kündigen.")}</p>
                <label class="input"><span class="info-head">{t("Cloud-Passwort")}<Info text="Damit meldest du dich bei Locora Cloud an. Es ist ein anderes Passwort als das Verschlüsselungs-Passwort, das deine Daten schützt." /></span>
                    <input type=password_type() autocomplete="current-password" bind:value=password />
                </label>
                <ShowPasswords />
                <button type="submit" class="danger" disabled=move || busy.get()>{t("Konto endgültig löschen")}</button>
            </form>
        </details>
    }
}

#[derive(Serialize)]
struct SaveTextArgs {
    name: String,
    text: String,
}

/// Save the recovery key as a text file or copy it, so it can be kept outside the app.
#[component]
fn SaveRecoveryKey(key: String) -> impl IntoView {
    let key = StoredValue::new(key);
    let saved = RwSignal::new(None::<Result<String, String>>);
    let busy = RwSignal::new(false);
    let save = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        saved.set(None);
        let text = format!(
            "{}\n\n{}\n\n{}\n",
            t("Locora Cloud – Wiederherstellungsschlüssel"),
            key.get_value(),
            t(
                "Ohne Verschlüsselungs-Passwort und diesen Schlüssel sind Cloud-Daten nicht mehr lesbar."
            ),
        );
        spawn_local(async move {
            let args = SaveTextArgs {
                name: "locora-recovery-key.txt".into(),
                text,
            };
            match call::<Option<String>>(CMD_SAVE_TEXT_PICK, args).await {
                Ok(Some(message)) => saved.set(Some(Ok(crate::i18n::translate(&message)))),
                Ok(None) => {}
                Err(message) => saved.set(Some(Err(message))),
            }
            busy.set(false);
        });
    };
    let copy = move |_| {
        // The clipboard API is reached through JavaScript; a refusal shows as an error.
        let clipboard = web_sys::window()
            .and_then(|window| js_sys::Reflect::get(&window.navigator(), &"clipboard".into()).ok())
            .filter(|clipboard| !clipboard.is_undefined());
        let written = clipboard.and_then(|clipboard| {
            let write = js_sys::Reflect::get(&clipboard, &"writeText".into()).ok()?;
            let write: js_sys::Function = wasm_bindgen::JsCast::dyn_into(write).ok()?;
            let promise = write.call1(&clipboard, &key.get_value().into()).ok()?;
            wasm_bindgen::JsCast::dyn_into::<js_sys::Promise>(promise).ok()
        });
        match written {
            Some(promise) => spawn_local(async move {
                saved.set(Some(
                    match wasm_bindgen_futures::JsFuture::from(promise).await {
                        Ok(_) => Ok(t("Kopiert.").into()),
                        Err(_) => Err(t(
                            "Kopieren nicht möglich. Bitte den Schlüssel abschreiben.",
                        )
                        .into()),
                    },
                ));
            }),
            None => saved.set(Some(Err(t(
                "Kopieren nicht möglich. Bitte den Schlüssel abschreiben.",
            )
            .into()))),
        }
    };
    view! {
        <div class="row">
            <button type="button" class="primary" disabled=move || busy.get() on:click=save>
                {t("Als Datei speichern…")}
            </button>
            <button type="button" on:click=copy>{t("Kopieren")}</button>
        </div>
        {move || saved.get().map(|result| match result {
            Ok(message) => view! { <p class="verdict ok" role="status">{message}</p> }.into_any(),
            Err(message) => view! { <p class="notes" role="alert">{message}</p> }.into_any(),
        })}
    }
}

#[component]
fn ConnectForm(
    status: RwSignal<Option<Result<CloudStatus, String>>>,
    message: RwSignal<Option<Result<String, String>>>,
    busy: RwSignal<bool>,
) -> impl IntoView {
    let register = RwSignal::new(false);
    // Same address as locora_runtime::cloud::DEFAULT_SERVER; change it only for another server.
    let server = RwSignal::new(String::from("https://api.locora.cloud"));
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
                t("Die Cloud-Passwörter stimmen nicht überein.").into()
            )));
            return;
        }
        let request = CloudConnect {
            server: server.get_untracked(),
            email: email.get_untracked(),
            password: secret,
            display_name: creating.then(|| display_name.get_untracked()),
            register: creating,
            // Every account is a guest until it pays: there is no other kind of account to choose.
            guest: false,
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
        <p class="notes">{t("Konto und Verschlüsselung lassen sich hier direkt auf diesem Gerät einrichten.")}</p>
        <form on:submit=move |event| { event.prevent_default(); submit(); }>
            <label class="input"><span class="info-head">{t("Cloud-Server (HTTPS)")}<Info text="Die Adresse des Servers, mit dem du synchronisierst. Lass sie so, außer du hast einen eigenen Server." /></span>
                <input type="url" placeholder="https://api.locora.cloud" autocomplete="url" bind:value=server required />
            </label>
            <label class="input"><span>{t("E-Mail")}</span>
                <input type="email" autocomplete="username" bind:value=email required />
            </label>
            <label><input type="checkbox" prop:checked=move || register.get()
                on:change=move |event| register.set(event_target_checked(&event)) />{t("Neues Konto erstellen")}<Info text="Einschalten, wenn du noch kein Cloud-Konto hast. Sonst meldest du dich mit deinem Konto an." /></label>
            <Show when=move || register.get()>
                <p class="meta">{t("Die Daten bleiben, solange du synchronisierst; ohne Synchronisation sechs Monate. Mit einem Abo bleiben sie dauerhaft.")}</p>
                <label class="input"><span class="info-head">{t("Anzeigename")}<Info text="So heißt du für deine Freunde in Locora Cloud, z. B. in der Rangliste." /></span>
                    <input type="text" maxlength="32" bind:value=display_name />
                </label>
            </Show>
            <label class="input"><span class="info-head">{t("Cloud-Passwort")}<Info text="Damit meldest du dich bei Locora Cloud an. Es ist ein anderes Passwort als das Verschlüsselungs-Passwort, das deine Daten schützt." /></span>
                <input type=password_type() autocomplete="current-password" bind:value=password required />
            </label>
            <Show when=move || register.get()>
                <label class="input"><span>{t("Cloud-Passwort wiederholen")}</span>
                    <input type=password_type() autocomplete="new-password" bind:value=confirmation />
                </label>
            </Show>
            <ShowPasswords />
            <button class="primary" type="submit" disabled=move || busy.get()>
                {move || if register.get() { t("Konto erstellen") } else { t("Anmelden") }}
            </button>
            <Show when=move || !register.get()>
                <p class="meta"><a href=locora_app_api::CLOUD_FORGOT_URL target="_blank" rel="noopener">{t("Passwort vergessen?")}</a></p>
            </Show>
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
            message.set(Some(Err(t(
                "Die Verschlüsselungs-Passwörter stimmen nicht überein.",
            )
            .into())));
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
                            t("Schlüsselstatus unklar. Bitte erneut prüfen.").into()
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
            <label class="input"><span class="info-head">{t("Neues Verschlüsselungs-Passwort (mindestens 12 Zeichen)")}<Info text="Damit werden deine Daten verschlüsselt, bevor sie in die Cloud gehen. Es ist ein eigenes Passwort, nicht dein Cloud-Passwort, und der Server kennt es nicht. Verlierst du es und den Wiederherstellungsschlüssel, kann niemand die Cloud-Daten wieder lesbar machen." /></span>
                <input type=password_type() autocomplete="new-password" bind:value=passphrase required />
            </label>
            <label class="input"><span>{t("Verschlüsselungs-Passwort wiederholen")}</span>
                <input type=password_type() autocomplete="new-password" bind:value=confirmation required />
            </label>
            <ShowPasswords />
            <button class="primary" type="submit" disabled=move || busy.get()>
                {t("Verschlüsselung einrichten")}
            </button>
        </form>
    }
}

/// After registering: the address is confirmed with the link from an e-mail. Shown while the
/// server says it is not confirmed (servers that do not confirm say nothing).
#[component]
fn EmailPrompt(
    status: RwSignal<Option<Result<CloudStatus, String>>>,
    email: String,
) -> impl IntoView {
    let note = RwSignal::new(None::<Result<String, String>>);
    let busy = RwSignal::new(false);
    let resend = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            note.set(Some(
                match call::<()>(CMD_CLOUD_EMAIL_RESEND, NoArgs {}).await {
                    Ok(()) => Ok(t("Wir haben den Link noch einmal geschickt.").into()),
                    Err(error) => Err(error),
                },
            ));
            busy.set(false);
        });
    };
    let check = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match call::<CloudStatus>(CMD_CLOUD_REFRESH, NoArgs {}).await {
                Ok(updated) => {
                    if updated.email_verified == Some(false) {
                        note.set(Some(Err(t("Die Adresse ist noch nicht bestätigt. Bitte öffne den Link in der E-Mail.").into())));
                    }
                    status.set(Some(Ok(updated)));
                }
                Err(error) => note.set(Some(Err(error))),
            }
            busy.set(false);
        });
    };
    view! {
        <div class="notes cloud-email" role="status">
            <p><strong>{tf("Bitte bestätige deine E-Mail-Adresse: Wir haben dir einen Link an {0} geschickt.", &[&email])}</strong></p>
            <button type="button" disabled=move || busy.get() on:click=resend>{t("Link erneut senden")}</button>
            " "
            <button type="button" disabled=move || busy.get() on:click=check>{t("Ich habe bestätigt – prüfen")}</button>
            {move || note.get().map(|result| match result {
                Ok(text) => view! { <p class="verdict ok">{text}</p> }.into_any(),
                Err(text) => view! { <p class="meta" role="alert">{text}</p> }.into_any(),
            })}
        </div>
    }
}
