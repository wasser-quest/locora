//! The system's secure store for the auto-sync key: the Keychain on macOS and iOS, the
//! Credential Manager on Windows, the Secret Service (GNOME Keyring, KWallet) on Linux, and the
//! Android Keystore (which encrypts the entry kept in the app's private preferences). Where none
//! is available, auto-sync is refused: the key is never written to a plain file.

use std::sync::{Arc, OnceLock};

use anyhow::{Result, anyhow};
use keyring_core::{Entry, Error};
use locora_runtime::cloud_key::KeyStore;
use zeroize::Zeroizing;

/// The service name under which the entries appear in the system's keychain tools.
const SERVICE: &str = "Locora Cloud";

/// Why there is no secure store, when there is none.
fn ready() -> Result<()> {
    static STORE: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    STORE
        .get_or_init(|| {
            let store: Arc<keyring_core::CredentialStore> =
                platform_store().map_err(|error| error.to_string())?;
            keyring_core::set_default_store(store);
            Ok(())
        })
        .clone()
        .map_err(|error| anyhow!(error).context(UNAVAILABLE))
}

const UNAVAILABLE: &str = "Auf diesem Gerät gibt es keinen sicheren Schlüsselspeicher. Auto-Sync ist deshalb nicht möglich";

#[cfg(target_os = "macos")]
fn platform_store() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    Ok(apple_native_keyring_store::keychain::Store::new()?)
}

#[cfg(target_os = "ios")]
fn platform_store() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    Ok(apple_native_keyring_store::protected::Store::new()?)
}

#[cfg(target_os = "windows")]
fn platform_store() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    Ok(windows_native_keyring_store::Store::new()?)
}

#[cfg(target_os = "android")]
fn platform_store() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    Ok(android_native_keyring_store::Store::new()?)
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn platform_store() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    Ok(zbus_secret_service_keyring_store::Store::new()?)
}

/// The secure store of this system.
pub struct SystemKeys;

fn entry(name: &str) -> Result<Entry> {
    ready()?;
    Entry::new(SERVICE, name).map_err(|error| anyhow!(error).context(UNAVAILABLE))
}

impl KeyStore for SystemKeys {
    fn save(&self, name: &str, secret: &[u8]) -> Result<()> {
        entry(name)?
            .set_secret(secret)
            .map_err(|error| anyhow!(error).context(UNAVAILABLE))
    }

    fn load(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        match entry(name)?.get_secret() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(anyhow!("Schlüsselspeicher nicht lesbar: {error}")),
        }
    }

    fn forget(&self, name: &str) -> Result<()> {
        // Without a store nothing can have been kept.
        let Ok(entry) = entry(name) else {
            return Ok(());
        };
        match entry.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(anyhow!("Schlüssel konnte nicht vergessen werden: {error}")),
        }
    }
}
