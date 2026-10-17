//! Signatures for Locora plugins.
//!
//! A publisher signs the exact bytes of a `.wasm` file with an Ed25519 key. The
//! signature is stored next to the file as `<plugin>.wasm.sig` (JSON, see
//! [`SignatureFile`]). The app trusts a signature only if the publisher's public
//! key is in its local list of trusted publishers, or the user accepts it.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

pub use ed25519_dalek::SigningKey;

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SignatureError {
    #[error("Signaturdatei ist ungültig")]
    Malformed,
    #[error("Signaturformat {0} wird nicht unterstützt")]
    UnsupportedVersion(u32),
    #[error("Signatur passt nicht zur Datei")]
    Mismatch,
}

/// Contents of a `.sig` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureFile {
    pub v: u32,
    /// Publisher's public key, base64 (32 bytes).
    pub publisher: String,
    /// Ed25519 signature over the file's bytes, base64 (64 bytes).
    pub signature: String,
}

/// A new random signing key.
pub fn generate() -> SigningKey {
    SigningKey::generate(&mut rand_core::OsRng)
}

/// Signs `bytes` with the publisher's key.
pub fn sign(bytes: &[u8], key: &SigningKey) -> SignatureFile {
    SignatureFile {
        v: FORMAT_VERSION,
        publisher: encode_public(&key.verifying_key()),
        signature: STANDARD.encode(key.sign(bytes).to_bytes()),
    }
}

/// Checks the signature over `bytes` and returns the publisher's public key (base64).
pub fn verify(bytes: &[u8], file: &SignatureFile) -> Result<String, SignatureError> {
    if file.v != FORMAT_VERSION {
        return Err(SignatureError::UnsupportedVersion(file.v));
    }
    let public = decode_public(&file.publisher)?;
    let signature = STANDARD
        .decode(&file.signature)
        .ok()
        .and_then(|raw| Signature::from_slice(&raw).ok())
        .ok_or(SignatureError::Malformed)?;
    public
        .verify(bytes, &signature)
        .map_err(|_| SignatureError::Mismatch)?;
    Ok(file.publisher.clone())
}

pub fn parse(json: &str) -> Result<SignatureFile, SignatureError> {
    serde_json::from_str(json).map_err(|_| SignatureError::Malformed)
}

pub fn encode_public(key: &VerifyingKey) -> String {
    STANDARD.encode(key.as_bytes())
}

pub fn decode_public(encoded: &str) -> Result<VerifyingKey, SignatureError> {
    let raw = STANDARD
        .decode(encoded.trim())
        .map_err(|_| SignatureError::Malformed)?;
    let raw: [u8; 32] = raw.try_into().map_err(|_| SignatureError::Malformed)?;
    VerifyingKey::from_bytes(&raw).map_err(|_| SignatureError::Malformed)
}

/// Seed of a signing key as base64 (32 bytes), the format of key files.
pub fn encode_seed(key: &SigningKey) -> String {
    STANDARD.encode(key.to_bytes())
}

pub fn decode_seed(encoded: &str) -> Result<SigningKey, SignatureError> {
    let raw = STANDARD
        .decode(encoded.trim())
        .map_err(|_| SignatureError::Malformed)?;
    let raw: [u8; 32] = raw.try_into().map_err(|_| SignatureError::Malformed)?;
    Ok(SigningKey::from_bytes(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_verifies_only_for_the_signed_bytes() {
        let key = generate();
        let file = sign(b"component", &key);
        assert_eq!(
            verify(b"component", &file).unwrap(),
            encode_public(&key.verifying_key())
        );
        assert_eq!(verify(b"component!", &file), Err(SignatureError::Mismatch));
    }

    #[test]
    fn a_swapped_publisher_key_is_rejected() {
        let key = generate();
        let other = generate();
        let mut file = sign(b"component", &key);
        file.publisher = encode_public(&other.verifying_key());
        assert_eq!(verify(b"component", &file), Err(SignatureError::Mismatch));
    }

    #[test]
    fn files_round_trip_and_unknown_versions_are_refused() {
        let key = generate();
        let file = sign(b"x", &key);
        let json = serde_json::to_string(&file).unwrap();
        assert_eq!(parse(&json).unwrap(), file);
        assert_eq!(parse("{nope"), Err(SignatureError::Malformed));
        let future = SignatureFile { v: 2, ..file };
        assert_eq!(
            verify(b"x", &future),
            Err(SignatureError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn key_files_round_trip() {
        let key = generate();
        let restored = decode_seed(&encode_seed(&key)).unwrap();
        assert_eq!(restored.to_bytes(), key.to_bytes());
        assert!(decode_seed("AAAA").is_err());
    }
}
