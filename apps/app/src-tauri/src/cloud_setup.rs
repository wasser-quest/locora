//! Cloud account onboarding runs off the UI thread and shares the sync lock.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use locora_app_api::{CloudConnect, CloudKeyCreated, CloudKeySetup, CloudStatus};
use locora_cloud_api::api::SubscriptionState;
use locora_runtime::cloud::{self, CloudState};
use tauri::Manager;
use zeroize::Zeroizing;

use crate::{Shared, err, lock};

struct CloudOperation(Arc<AtomicBool>);

impl Drop for CloudOperation {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn begin(app: &tauri::AppHandle) -> Result<(PathBuf, CloudOperation), String> {
    let backend = app.state::<Shared>();
    let backend = lock(&backend)?;
    let busy = backend.cloud_sync_busy.clone();
    if busy.swap(true, Ordering::AcqRel) {
        return Err("Eine Cloud-Aktion läuft bereits".into());
    }
    Ok((backend.data.clone(), CloudOperation(busy)))
}

fn status(state: &CloudState) -> CloudStatus {
    CloudStatus {
        account: Some(state.account.email.clone()),
        server: Some(state.server.clone()),
        access: cloud::access(state, cloud::now_secs()).is_some(),
    }
}

fn validate_connect(request: &CloudConnect) -> Result<(), String> {
    if request.server.len() > 2048 {
        return Err("Serveradresse ist zu lang".into());
    }
    cloud::check_server(&request.server).map_err(err)?;
    let email = request.email.trim();
    if email.is_empty() || email.len() > 254 || !email.contains('@') {
        return Err("Eine gültige E-Mail-Adresse eingeben".into());
    }
    if request.password.chars().count() < 10 || request.password.chars().count() > 256 {
        return Err("Cloud-Passwort braucht 10 bis 256 Zeichen".into());
    }
    if request.register {
        let name = request.display_name.as_deref().unwrap_or("").trim();
        if name.is_empty() || name.chars().count() > 32 {
            return Err("Anzeigename braucht 1 bis 32 Zeichen".into());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn cloud_connect(
    app: tauri::AppHandle,
    request: CloudConnect,
) -> Result<CloudStatus, String> {
    validate_connect(&request)?;
    let (data, busy) = begin(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy;
        if cloud::load(&data).map_err(err)?.is_some() {
            return Err("Auf diesem Gerät ist bereits ein Cloud-Konto angemeldet".into());
        }
        let password = Zeroizing::new(request.password);
        let state = if request.register {
            cloud::register(
                &request.server,
                request.email.trim(),
                &password,
                request.display_name.as_deref().unwrap_or("").trim(),
                request.guest,
            )
        } else {
            cloud::login(&request.server, request.email.trim(), &password)
        }
        .map_err(err)?;
        cloud::save(&data, &state).map_err(err)?;
        Ok(status(&state))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cloud_keys_status(app: tauri::AppHandle) -> Result<bool, String> {
    let data = app.state::<Shared>();
    let data = lock(&data)?.data.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = cloud::load(&data)
            .map_err(err)?
            .ok_or_else(|| "Kein Cloud-Konto angemeldet".to_owned())?;
        cloud::get_keys(&state)
            .map(|keys| keys.is_some())
            .map_err(err)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cloud_keys_setup(
    app: tauri::AppHandle,
    request: CloudKeySetup,
) -> Result<CloudKeyCreated, String> {
    if !(12..=1024).contains(&request.passphrase.chars().count()) {
        return Err("Verschlüsselungs-Passwort braucht 12 bis 1024 Zeichen".into());
    }
    let (data, busy) = begin(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy;
        let state = cloud::load(&data)
            .map_err(err)?
            .ok_or_else(|| "Kein Cloud-Konto angemeldet".to_owned())?;
        if cloud::get_keys(&state).map_err(err)?.is_some() {
            return Err("Die Verschlüsselung ist für dieses Konto bereits eingerichtet".into());
        }
        let passphrase = Zeroizing::new(request.passphrase);
        let account = locora_crypto::create(&passphrase).map_err(|e| e.to_string())?;
        let result = cloud::create_keys(&state, &account.bundle);
        // A lost HTTP response may hide a successful create. Always return the
        // recovery key once it exists, so this ambiguity cannot lose the key.
        let (stored, warning) = match result {
            Ok(()) => (true, None),
            Err(error) => {
                let confirmed = match cloud::get_keys(&state) {
                    Ok(Some(remote)) if remote == account.bundle => true,
                    Ok(Some(_)) => {
                        return Err("Ein anderes Gerät hat die Verschlüsselung inzwischen eingerichtet. Diesen neu erzeugten Schlüssel nicht verwenden.".into());
                    }
                    _ => false,
                };
                let warning = (!confirmed).then(|| format!(
                    "Der Server hat die Schlüsseleinrichtung nicht bestätigt: {error:#}. Wiederherstellungsschlüssel sichern und den Status erneut prüfen."
                ));
                (confirmed, warning)
            }
        };
        Ok(CloudKeyCreated {
            recovery_key: account.recovery_key,
            stored,
            warning,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cloud_logout(app: tauri::AppHandle) -> Result<(), String> {
    let (data, busy) = begin(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy;
        if let Some(state) = cloud::load(&data).map_err(err)? {
            cloud::logout(&data, &state).map_err(err)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cloud_refresh(app: tauri::AppHandle) -> Result<CloudStatus, String> {
    let (data, busy) = begin(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy;
        let mut state = cloud::load(&data)
            .map_err(err)?
            .ok_or_else(|| "Kein Cloud-Konto angemeldet".to_owned())?;
        cloud::me(&mut state).map_err(err)?;
        if state.account.subscription == SubscriptionState::None {
            state.entitlement = None;
        } else {
            cloud::refresh(&mut state).map_err(err)?;
        }
        cloud::save(&data, &state).map_err(err)?;
        Ok(status(&state))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_rejects_oversized_and_insecure_input_before_network() {
        let mut request = CloudConnect {
            server: "http://example.com".into(),
            email: "a@example.com".into(),
            password: "long enough password".into(),
            display_name: None,
            register: false,
        };
        assert!(validate_connect(&request).is_err());
        request.server = "https://example.com".into();
        assert!(validate_connect(&request).is_ok());
        request.password = "short".into();
        assert!(validate_connect(&request).is_err());
        request.password = "long enough password".into();
        request.register = true;
        assert!(validate_connect(&request).is_err());
        request.display_name = Some("Lernende".into());
        assert!(validate_connect(&request).is_ok());
    }
}
