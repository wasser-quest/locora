//! End-to-end encryption for Locora sync.
//!
//! Everything a device sends to Locora Cloud is sealed here first, so the
//! server only stores ciphertext and opaque ids.
//!
//! - Each account has a random 256-bit data key (DEK). It never leaves a
//!   device unencrypted.
//! - The DEK is stored on the server twice, wrapped by a key derived from the
//!   user's encryption passphrase (Argon2id) and by a key derived from a
//!   one-time recovery key. The server holds neither secret.
//! - Operations and media are sealed with XChaCha20-Poly1305 under subkeys of
//!   the DEK (HKDF). The account and the operation id are bound in as
//!   associated data, so a ciphertext cannot be moved to another account or id.
//! - Ids sent to the server are keyed HMACs, so the server cannot link a
//!   timestamp or a picture to its content.
//!
//! A lost passphrase and a lost recovery key mean lost data. That is the price
//! of end-to-end encryption.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

pub const MIN_PASSPHRASE_CHARS: usize = 12;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;
const KDF_MEMORY_KIB: u32 = 64 * 1024;
const KDF_ITERATIONS: u32 = 3;
const KDF_PARALLELISM: u32 = 1;
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("Das Passwort für die Verschlüsselung ist falsch")]
    WrongPassphrase,
    #[error("Der Wiederherstellungsschlüssel ist falsch")]
    WrongRecoveryKey,
    #[error("Das Passwort braucht mindestens {MIN_PASSPHRASE_CHARS} Zeichen")]
    PassphraseTooShort,
    #[error("Verschlüsselte Daten sind beschädigt oder gehören zu einem anderen Konto")]
    Corrupt,
    #[error("Schlüsseldaten sind ungültig")]
    Malformed,
}

/// The wrapped copies of the account key, as stored on the server.
/// Contains no secret that is usable without a passphrase or recovery key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBundle {
    pub v: u32,
    /// Argon2 salt, base64.
    pub salt: String,
    /// DEK wrapped by the passphrase key, base64 (nonce || ciphertext).
    pub wrapped_dek: String,
    /// DEK wrapped by the recovery key, base64 (nonce || ciphertext).
    pub recovery_wrapped: String,
}

/// The 256-bit account key. Wiped from memory when dropped.
pub struct Dek(Zeroizing<[u8; KEY_LEN]>);

/// A new account: its key bundle, the key itself, and the recovery key to show once.
pub struct NewAccountKeys {
    pub bundle: KeyBundle,
    pub dek: Dek,
    pub recovery_key: String,
}

fn check_passphrase(passphrase: &str) -> Result<(), CryptoError> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(CryptoError::PassphraseTooShort);
    }
    Ok(())
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

/// Argon2id with fixed parameters; the salt makes each account's key unique.
fn passphrase_key(passphrase: &str, salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, CryptoError> {
    let params = Params::new(
        KDF_MEMORY_KIB,
        KDF_ITERATIONS,
        KDF_PARALLELISM,
        Some(KEY_LEN),
    )
    .map_err(|_| CryptoError::Malformed)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(passphrase.as_bytes(), salt, key.as_mut())
        .map_err(|_| CryptoError::Malformed)?;
    Ok(key)
}

fn recovery_secret(recovery_key: &str) -> Result<Zeroizing<[u8; KEY_LEN]>, CryptoError> {
    let hex: String = recovery_key
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    if hex.len() != KEY_LEN * 2 || !hex.is_ascii() {
        return Err(CryptoError::WrongRecoveryKey);
    }
    let mut bytes = Zeroizing::new([0u8; KEY_LEN]);
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16)
            .map_err(|_| CryptoError::WrongRecoveryKey)?;
    }
    Ok(bytes)
}

/// Key for wrapping the DEK with a recovery key, derived from its 32 random bytes.
fn recovery_wrap_key(recovery: &[u8; KEY_LEN]) -> Zeroizing<[u8; KEY_LEN]> {
    let hk = Hkdf::<Sha256>::new(None, recovery);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    hk.expand(b"locora/recovery-wrap/v1", key.as_mut())
        .expect("32 bytes is a valid HKDF length");
    key
}

fn seal_raw(key: &[u8; KEY_LEN], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let nonce: [u8; NONCE_LEN] = random();
    let cipher = XChaCha20Poly1305::new(key.into());
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("encryption of in-memory data does not fail");
    [nonce.as_slice(), ciphertext.as_slice()].concat()
}

fn open_raw(key: &[u8; KEY_LEN], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if sealed.len() < NONCE_LEN {
        return Err(CryptoError::Corrupt);
    }
    let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| CryptoError::Corrupt)
}

fn wrap_dek(key: &[u8; KEY_LEN], dek: &Dek) -> String {
    STANDARD.encode(seal_raw(key, b"locora/dek-wrap/v1", dek.0.as_ref()))
}

fn unwrap_dek(key: &[u8; KEY_LEN], wrapped: &str) -> Result<Dek, CryptoError> {
    let sealed = STANDARD
        .decode(wrapped)
        .map_err(|_| CryptoError::Malformed)?;
    let mut raw = open_raw(key, b"locora/dek-wrap/v1", &sealed)?;
    let bytes: [u8; KEY_LEN] = raw
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::Malformed)?;
    raw.zeroize();
    Ok(Dek(Zeroizing::new(bytes)))
}

/// Creates a new account key. Returns the recovery key, which must be shown to the user once.
pub fn create(passphrase: &str) -> Result<NewAccountKeys, CryptoError> {
    check_passphrase(passphrase)?;
    let dek = Dek(Zeroizing::new(random()));
    let recovery_bytes = Zeroizing::new(random::<KEY_LEN>());
    let salt: [u8; 16] = random();
    let bundle = build_bundle(&dek, passphrase, &salt, &recovery_bytes)?;
    Ok(NewAccountKeys {
        bundle,
        dek,
        recovery_key: format_recovery_key(&recovery_bytes),
    })
}

fn build_bundle(
    dek: &Dek,
    passphrase: &str,
    salt: &[u8],
    recovery: &[u8; KEY_LEN],
) -> Result<KeyBundle, CryptoError> {
    let kek = passphrase_key(passphrase, salt)?;
    Ok(KeyBundle {
        v: FORMAT_VERSION,
        salt: STANDARD.encode(salt),
        wrapped_dek: wrap_dek(&kek, dek),
        recovery_wrapped: wrap_dek(&recovery_wrap_key(recovery), dek),
    })
}

/// The account key, using the passphrase. Fails if the passphrase is wrong.
pub fn unlock(bundle: &KeyBundle, passphrase: &str) -> Result<Dek, CryptoError> {
    check_bundle(bundle)?;
    let salt = STANDARD
        .decode(&bundle.salt)
        .map_err(|_| CryptoError::Malformed)?;
    let kek = passphrase_key(passphrase, &salt)?;
    unwrap_dek(&kek, &bundle.wrapped_dek).map_err(|_| CryptoError::WrongPassphrase)
}

/// The account key, using the recovery key instead of the passphrase.
pub fn unlock_with_recovery(bundle: &KeyBundle, recovery_key: &str) -> Result<Dek, CryptoError> {
    check_bundle(bundle)?;
    let recovery = recovery_secret(recovery_key)?;
    unwrap_dek(&recovery_wrap_key(&recovery), &bundle.recovery_wrapped)
        .map_err(|_| CryptoError::WrongRecoveryKey)
}

/// A new bundle for the same account key after the passphrase was forgotten.
/// The recovery key stays valid.
pub fn rewrap(
    dek: &Dek,
    recovery_key: &str,
    new_passphrase: &str,
) -> Result<KeyBundle, CryptoError> {
    check_passphrase(new_passphrase)?;
    let recovery = recovery_secret(recovery_key)?;
    let salt: [u8; 16] = random();
    build_bundle(dek, new_passphrase, &salt, &recovery)
}

fn check_bundle(bundle: &KeyBundle) -> Result<(), CryptoError> {
    if bundle.v != FORMAT_VERSION {
        return Err(CryptoError::Malformed);
    }
    Ok(())
}

fn format_recovery_key(bytes: &[u8; KEY_LEN]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    hex.as_bytes()
        .chunks(4)
        .map(|chunk| std::str::from_utf8(chunk).expect("hex is ASCII"))
        .collect::<Vec<_>>()
        .join("-")
}

impl Dek {
    /// The raw key, for a secure store of this device (the OS keychain) that keeps it between
    /// syncs. Never send it anywhere.
    pub fn to_bytes(&self) -> Zeroizing<[u8; KEY_LEN]> {
        Zeroizing::new(*self.0)
    }

    /// The key from the bytes `to_bytes` gave.
    pub fn from_bytes(bytes: &[u8]) -> Result<Dek, CryptoError> {
        let bytes: [u8; KEY_LEN] = bytes.try_into().map_err(|_| CryptoError::Malformed)?;
        Ok(Dek(Zeroizing::new(bytes)))
    }

    fn subkey(&self, info: &[u8]) -> Zeroizing<[u8; KEY_LEN]> {
        let hk = Hkdf::<Sha256>::new(None, self.0.as_ref());
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        hk.expand(info, key.as_mut())
            .expect("32 bytes is a valid HKDF length");
        key
    }

    /// The key proof K = HKDF-SHA256(key, info "locora/key-proof/v1"), 32 bytes. Only a holder of
    /// the account key can make it, so it goes to the server with every key bundle the device
    /// stores or replaces; the server keeps only its SHA-256 and refuses a replacement without
    /// it. It reveals nothing about the key or the other subkeys.
    pub fn key_proof(&self) -> Zeroizing<[u8; KEY_LEN]> {
        self.subkey(b"locora/key-proof/v1")
    }

    /// Server-side id of an operation: keyed, so it reveals neither the timestamp nor the device.
    pub fn op_id(&self, hlc: &str) -> String {
        let key = self.subkey(b"locora/op-id/v1");
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(key.as_ref()).expect("any key length works");
        mac.update(hlc.as_bytes());
        mac.finalize().into_bytes()[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// Seals one plaintext operation for `account_id` under the operation `id`.
    pub fn seal_op(&self, account_id: &str, id: &str, plaintext: &[u8]) -> Vec<u8> {
        let key = self.subkey(b"locora/op/v1");
        seal_raw(&key, &op_aad(account_id, id), plaintext)
    }

    pub fn open_op(
        &self,
        account_id: &str,
        id: &str,
        sealed: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let key = self.subkey(b"locora/op/v1");
        open_raw(&key, &op_aad(account_id, id), sealed)
    }

    /// Server-side id of a picture or video, from the SHA-256 of its plaintext.
    pub fn media_id(&self, content_sha256_hex: &str) -> String {
        let key = self.subkey(b"locora/media-id/v1");
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(key.as_ref()).expect("any key length works");
        mac.update(content_sha256_hex.as_bytes());
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// Seals a media file. The MIME type travels inside the ciphertext.
    pub fn seal_media(&self, media_id: &str, mime: &str, bytes: &[u8]) -> Vec<u8> {
        let key = self.subkey(b"locora/media/v1");
        let mut plaintext = Vec::with_capacity(2 + mime.len() + bytes.len());
        plaintext.extend_from_slice(&(mime.len() as u16).to_be_bytes());
        plaintext.extend_from_slice(mime.as_bytes());
        plaintext.extend_from_slice(bytes);
        seal_raw(&key, media_id.as_bytes(), &plaintext)
    }

    /// Opens a media file; returns its MIME type and bytes.
    pub fn open_media(
        &self,
        media_id: &str,
        sealed: &[u8],
    ) -> Result<(String, Vec<u8>), CryptoError> {
        let key = self.subkey(b"locora/media/v1");
        let plaintext = open_raw(&key, media_id.as_bytes(), sealed)?;
        let length = plaintext
            .get(..2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
            .ok_or(CryptoError::Corrupt)?;
        let mime = plaintext.get(2..2 + length).ok_or(CryptoError::Corrupt)?;
        let mime = String::from_utf8(mime.to_vec()).map_err(|_| CryptoError::Corrupt)?;
        Ok((mime, plaintext[2 + length..].to_vec()))
    }
}

fn op_aad(account_id: &str, id: &str) -> Vec<u8> {
    format!("locora/op/v1|{account_id}|{id}").into_bytes()
}

/// SHA-256 of bytes as lower-case hex (the content hash used across Locora).
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASS: &str = "ein langes geheimes passwort";

    #[test]
    fn the_passphrase_and_the_recovery_key_both_unlock_the_same_key() {
        let account = create(PASS).unwrap();
        let by_passphrase = unlock(&account.bundle, PASS).unwrap();
        let by_recovery = unlock_with_recovery(&account.bundle, &account.recovery_key).unwrap();
        assert_eq!(by_passphrase.0.as_ref(), account.dek.0.as_ref());
        assert_eq!(by_recovery.0.as_ref(), account.dek.0.as_ref());
    }

    #[test]
    fn the_key_bytes_give_back_the_same_key() {
        let account = create(PASS).unwrap();
        let again = Dek::from_bytes(account.dek.to_bytes().as_ref()).unwrap();
        let id = again.op_id("0000000000001-00000-dev");
        assert_eq!(id, account.dek.op_id("0000000000001-00000-dev"));
        let sealed = account.dek.seal_op("acc", &id, b"x");
        assert_eq!(again.open_op("acc", &id, &sealed).unwrap(), b"x");
        assert_eq!(
            Dek::from_bytes(&[1, 2, 3]).err(),
            Some(CryptoError::Malformed)
        );
    }

    #[test]
    fn wrong_secrets_are_refused() {
        let account = create(PASS).unwrap();
        assert_eq!(
            unlock(&account.bundle, "ein anderes langes passwort").err(),
            Some(CryptoError::WrongPassphrase)
        );
        assert_eq!(
            unlock_with_recovery(&account.bundle, &"00".repeat(32)).err(),
            Some(CryptoError::WrongRecoveryKey)
        );
        assert_eq!(
            create("zu kurz").err(),
            Some(CryptoError::PassphraseTooShort)
        );
    }

    #[test]
    fn a_new_passphrase_can_be_set_with_the_recovery_key() {
        let account = create(PASS).unwrap();
        let bundle = rewrap(
            &account.dek,
            &account.recovery_key,
            "ein neues langes passwort",
        )
        .unwrap();
        let dek = unlock(&bundle, "ein neues langes passwort").unwrap();
        assert_eq!(
            dek.0.as_ref(),
            account.dek.0.as_ref(),
            "the data key does not change"
        );
        assert!(
            unlock(&bundle, PASS).is_err(),
            "the old passphrase no longer works"
        );
    }

    #[test]
    fn sealed_operations_open_only_for_their_account_and_id() {
        let account = create(PASS).unwrap();
        let dek = &account.dek;
        let id = dek.op_id("0000000000001-00000-dev");
        let sealed = dek.seal_op("acc_1", &id, b"{\"entity\":\"palace\"}");
        assert_eq!(
            dek.open_op("acc_1", &id, &sealed).unwrap(),
            b"{\"entity\":\"palace\"}"
        );
        assert_eq!(
            dek.open_op("acc_2", &id, &sealed),
            Err(CryptoError::Corrupt),
            "other account"
        );
        assert_eq!(
            dek.open_op("acc_1", "other", &sealed),
            Err(CryptoError::Corrupt),
            "other id"
        );
        let mut tampered = sealed.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert_eq!(
            dek.open_op("acc_1", &id, &tampered),
            Err(CryptoError::Corrupt)
        );
    }

    #[test]
    fn ids_are_keyed_and_stable() {
        let a = create(PASS).unwrap();
        let b = create(PASS).unwrap();
        assert_eq!(a.dek.op_id("x"), a.dek.op_id("x"));
        assert_ne!(
            a.dek.op_id("x"),
            b.dek.op_id("x"),
            "another key gives other ids"
        );
        assert_ne!(a.dek.op_id("x"), a.dek.op_id("y"));
        let sha = sha256_hex(b"picture");
        assert_eq!(a.dek.media_id(&sha), a.dek.media_id(&sha));
        assert!(!a.dek.op_id("x").contains("x"), "ids do not show the input");
    }

    #[test]
    fn media_round_trips_with_its_type_and_rejects_other_ids() {
        let account = create(PASS).unwrap();
        let id = account.dek.media_id(&sha256_hex(b"png-bytes"));
        let sealed = account.dek.seal_media(&id, "image/png", b"png-bytes");
        let (mime, bytes) = account.dek.open_media(&id, &sealed).unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, b"png-bytes");
        assert!(account.dek.open_media("another", &sealed).is_err());
    }

    #[test]
    fn the_key_proof_is_stable_for_a_key_and_differs_between_keys() {
        let a = create(PASS).unwrap();
        let b = create(PASS).unwrap();
        assert_eq!(*a.dek.key_proof(), *a.dek.key_proof());
        // A new passphrase or the recovery key unlock the same key, so the proof stays.
        let rewrapped = rewrap(&a.dek, &a.recovery_key, "ein neues langes passwort").unwrap();
        let again = unlock(&rewrapped, "ein neues langes passwort").unwrap();
        assert_eq!(*again.key_proof(), *a.dek.key_proof());
        let recovered = unlock_with_recovery(&a.bundle, &a.recovery_key).unwrap();
        assert_eq!(*recovered.key_proof(), *a.dek.key_proof());
        assert_ne!(*a.dek.key_proof(), *b.dek.key_proof(), "another key");
        // A subkey of its own: neither the key nor the key of the operations.
        assert_ne!(*a.dek.key_proof(), *a.dek.to_bytes());
        assert_ne!(*a.dek.key_proof(), *a.dek.subkey(b"locora/op/v1"));
        // HKDF-SHA256 of the key, without salt, with the info "locora/key-proof/v1".
        let fixed = Dek::from_bytes(&[7u8; KEY_LEN]).unwrap();
        let mut expected = [0u8; KEY_LEN];
        Hkdf::<Sha256>::new(None, &[7u8; KEY_LEN])
            .expand(b"locora/key-proof/v1", &mut expected)
            .unwrap();
        assert_eq!(*fixed.key_proof(), expected);
    }

    #[test]
    fn the_recovery_key_is_grouped_and_forgiving_about_spaces() {
        let account = create(PASS).unwrap();
        assert_eq!(account.recovery_key.matches('-').count(), 15);
        let spaced = account.recovery_key.replace('-', " ");
        assert!(unlock_with_recovery(&account.bundle, &spaced).is_ok());
    }
}
