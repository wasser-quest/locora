//! The account key kept on this device for auto-sync.
//!
//! After the person enters the encryption password once, the unlocked key goes into a secure
//! store of the system (`KeyStore`: the OS keychain on the desktop, the Android Keystore, the iOS
//! keychain). `cloud.json` only notes that auto-sync is on; the key itself is never written there.
//! Signing out, deleting the cloud data or the account, and turning auto-sync off forget it.

use std::path::Path;

use anyhow::{Result, anyhow};
use locora_crypto::Dek;
use zeroize::Zeroizing;

use crate::cloud::{self, CloudState};

/// A place for one secret per account that only this device (and user) can read.
pub trait KeyStore {
    fn save(&self, entry: &str, secret: &[u8]) -> Result<()>;
    /// `None` when nothing is stored under `entry`.
    fn load(&self, entry: &str) -> Result<Option<Zeroizing<Vec<u8>>>>;
    /// Removes the entry; removing a missing one is not an error.
    fn forget(&self, entry: &str) -> Result<()>;
}

/// The name of an account's key in the store: one per server and account.
pub fn entry(state: &CloudState) -> String {
    format!("{} {}", state.account.id, state.server)
}

/// Keeps the key and turns auto-sync on. The key is stored first, so the flag never claims a
/// key that is not there; a store that refuses leaves auto-sync off.
pub fn remember(data: &Path, keys: &dyn KeyStore, dek: &Dek) -> Result<CloudState> {
    let mut state = cloud::load(data)?.ok_or_else(|| anyhow!("Kein Cloud-Konto angemeldet"))?;
    keys.save(&entry(&state), dek.to_bytes().as_ref())?;
    state.auto_sync = true;
    cloud::save(data, &state)?;
    Ok(state)
}

/// The kept key, when auto-sync is on and the store still has it. A key that went missing (or
/// is damaged) turns auto-sync off, so the app asks for the password again.
pub fn recall(data: &Path, keys: &dyn KeyStore) -> Result<Option<Dek>> {
    let Some(mut state) = cloud::load(data)? else {
        return Ok(None);
    };
    if !state.auto_sync {
        return Ok(None);
    }
    let found = keys
        .load(&entry(&state))?
        .and_then(|bytes| Dek::from_bytes(&bytes).ok());
    if found.is_none() {
        state.auto_sync = false;
        cloud::save(data, &state)?;
    }
    Ok(found)
}

/// Forgets the key of the signed-in account and turns auto-sync off. Call it before the session
/// itself is removed (the entry's name comes from it).
pub fn forget(data: &Path, keys: &dyn KeyStore) -> Result<()> {
    let Some(mut state) = cloud::load(data)? else {
        return Ok(());
    };
    keys.forget(&entry(&state))?;
    if state.auto_sync {
        state.auto_sync = false;
        cloud::save(data, &state)?;
    }
    Ok(())
}

/// A store in memory, for tests.
#[derive(Default)]
pub struct MemoryKeys {
    entries: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    /// Refuses every save, like a system without a keychain.
    pub unavailable: bool,
}

impl KeyStore for MemoryKeys {
    fn save(&self, entry: &str, secret: &[u8]) -> Result<()> {
        if self.unavailable {
            anyhow::bail!("Kein Schlüsselbund verfügbar");
        }
        self.entries
            .lock()
            .map_err(|_| anyhow!("blocked"))?
            .insert(entry.to_owned(), secret.to_vec());
        Ok(())
    }

    fn load(&self, entry: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(self
            .entries
            .lock()
            .map_err(|_| anyhow!("blocked"))?
            .get(entry)
            .cloned()
            .map(Zeroizing::new))
    }

    fn forget(&self, entry: &str) -> Result<()> {
        self.entries
            .lock()
            .map_err(|_| anyhow!("blocked"))?
            .remove(entry);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_cloud_api::api::{Account, SubscriptionState};

    fn signed_in(data: &Path) {
        let state = CloudState {
            server: "https://cloud.example".into(),
            token: "token".into(),
            account: Account {
                id: "acc".into(),
                email: "a@example.invalid".into(),
                display_name: "A".into(),
                subscription: SubscriptionState::None,
                plan: None,
                period_end: None,
                email_verified: None,
            },
            sync_generation: 0,
            entitlement: None,
            last_sync_at: None,
            auto_sync: false,
            game_delete_pending: false,
        };
        cloud::save(data, &state).unwrap();
    }

    fn key() -> Dek {
        Dek::from_bytes(&[7; 32]).unwrap()
    }

    #[test]
    fn the_key_is_kept_in_the_store_and_never_in_the_session_file() {
        let dir = tempfile::tempdir().unwrap();
        signed_in(dir.path());
        let keys = MemoryKeys::default();
        assert!(recall(dir.path(), &keys).unwrap().is_none(), "off at first");
        let state = remember(dir.path(), &keys, &key()).unwrap();
        assert!(state.auto_sync);
        let file = std::fs::read(cloud::state_path(dir.path())).unwrap();
        assert!(
            !file.windows(32).any(|window| window == [7; 32]),
            "the key is not in cloud.json"
        );
        let again = recall(dir.path(), &keys).unwrap().expect("kept");
        assert_eq!(again.to_bytes().as_ref(), &[7; 32]);
        forget(dir.path(), &keys).unwrap();
        assert!(recall(dir.path(), &keys).unwrap().is_none());
        assert!(!cloud::load(dir.path()).unwrap().unwrap().auto_sync);
    }

    #[test]
    fn without_a_secure_store_auto_sync_stays_off() {
        let dir = tempfile::tempdir().unwrap();
        signed_in(dir.path());
        let keys = MemoryKeys {
            unavailable: true,
            ..MemoryKeys::default()
        };
        assert!(remember(dir.path(), &keys, &key()).is_err());
        assert!(!cloud::load(dir.path()).unwrap().unwrap().auto_sync);
    }

    #[test]
    fn a_key_that_went_missing_turns_auto_sync_off() {
        let dir = tempfile::tempdir().unwrap();
        signed_in(dir.path());
        let keys = MemoryKeys::default();
        remember(dir.path(), &keys, &key()).unwrap();
        let state = cloud::load(dir.path()).unwrap().unwrap();
        keys.forget(&entry(&state)).unwrap();
        assert!(recall(dir.path(), &keys).unwrap().is_none());
        assert!(!cloud::load(dir.path()).unwrap().unwrap().auto_sync);
    }
}
