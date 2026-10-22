//! Before a sync and without a password: the summary of pending changes, auto-sync with the key
//! kept in the system's secure store, and the confirmation e-mail.

use std::sync::atomic::Ordering;

use locora_app_api::{CloudAutoOutcome, CloudAutoState, CloudPending, CloudStatus};
use locora_app_core::cloud_view;
use locora_runtime::cloud;
use locora_runtime::cloud_key;
use locora_runtime::cloud_sync::{self, SyncOptions, Unlock};
use tauri::Manager;
use zeroize::Zeroizing;

use crate::cloud_setup::begin;
use crate::keystore::SystemKeys;
use crate::{Shared, err, lock};

fn profile(
    app: &tauri::AppHandle,
) -> Result<(std::path::PathBuf, std::sync::Arc<locora_store::Store>), String> {
    let state = app.state::<Shared>();
    let backend = lock(&state)?;
    Ok((backend.data.clone(), backend.profile.store.clone()))
}

/// What the next sync sends, by palace, and what waits on the server.
#[tauri::command]
pub async fn cloud_pending(app: tauri::AppHandle) -> Result<CloudPending, String> {
    let (data, store) = profile(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let local = store
            .pending_changes(store.sync_cursor("push").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let remote = match cloud::load(&data).map_err(err)? {
            Some(state) => cloud::pending(&state, &store).map_err(err)?,
            None => None,
        };
        Ok(CloudPending {
            local: cloud_view::changes(&local),
            remote,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cloud_auto_state(app: tauri::AppHandle) -> Result<CloudAutoState, String> {
    let (data, store) = profile(&app)?;
    let state = cloud::load(&data).map_err(err)?;
    cloud_view::auto_state(state.as_ref(), &store).map_err(err)
}

/// Unlocks the key with the encryption password once and keeps it in the secure store.
#[tauri::command]
pub async fn cloud_auto_enable(
    app: tauri::AppHandle,
    passphrase: String,
) -> Result<CloudStatus, String> {
    if !(12..=1024).contains(&passphrase.chars().count()) {
        return Err("Verschlüsselungs-Passwort braucht 12 bis 1024 Zeichen".into());
    }
    let (data, busy) = begin(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy;
        let passphrase = Zeroizing::new(passphrase);
        let state = cloud::load(&data)
            .map_err(err)?
            .ok_or_else(|| "Kein Cloud-Konto angemeldet".to_owned())?;
        let bundle = cloud::get_keys(&state)
            .map_err(err)?
            .ok_or_else(|| "Verschlüsselung fehlt. In den Einstellungen einrichten".to_owned())?;
        let dek = locora_crypto::unlock(&bundle, &passphrase).map_err(|e| e.to_string())?;
        let state = cloud_key::remember(&data, &SystemKeys, &dek).map_err(err)?;
        Ok(cloud_view::status(Some(&state)))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Forgets the kept key on this device.
#[tauri::command]
pub async fn cloud_auto_disable(app: tauri::AppHandle) -> Result<CloudStatus, String> {
    let (data, _) = profile(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        cloud_key::forget(&data, &SystemKeys).map_err(err)?;
        Ok(cloud_view::status(
            cloud::load(&data).map_err(err)?.as_ref(),
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A sync with the kept key. It waits for confirmed palace deletions and stops before starting
/// the cloud over; while another cloud action runs it does nothing.
#[tauri::command]
pub async fn cloud_auto_sync(
    app: tauri::AppHandle,
    confirmed: Option<Vec<String>>,
) -> Result<CloudAutoOutcome, String> {
    let (data, store) = profile(&app)?;
    let busy = {
        let state = app.state::<Shared>();
        lock(&state)?.cloud_sync_busy.clone()
    };
    if busy.swap(true, Ordering::AcqRel) {
        return Ok(CloudAutoOutcome {
            enabled: true,
            ..CloudAutoOutcome::default()
        });
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = crate::cloud_setup::CloudOperation(busy);
        let dek = match cloud_key::recall(&data, &SystemKeys) {
            Ok(Some(dek)) => dek,
            Ok(None) => return Ok(CloudAutoOutcome::default()),
            Err(error) => {
                return Ok(CloudAutoOutcome {
                    enabled: true,
                    error: Some(format!("{error:#}")),
                    ..CloudAutoOutcome::default()
                });
            }
        };
        let options = SyncOptions {
            confirmed: Some(confirmed.unwrap_or_default()),
            automatic: true,
        };
        let result = cloud_sync::sync_with(&data, &store, Unlock::Key(Box::new(dek)), options);
        Ok(cloud_view::auto_outcome(result))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cloud_email_resend(app: tauri::AppHandle) -> Result<(), String> {
    let (data, _) = profile(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = cloud::load(&data)
            .map_err(err)?
            .ok_or_else(|| "Kein Cloud-Konto angemeldet".to_owned())?;
        cloud::resend_email(&state).map_err(err)
    })
    .await
    .map_err(|e| e.to_string())?
}
