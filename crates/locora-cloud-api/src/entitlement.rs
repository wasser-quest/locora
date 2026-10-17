//! Signed entitlement tokens.
//!
//! Format: `locora-ent.v1.<base64url(claims JSON)>.<base64url(Ed25519 signature)>`.
//! The signature covers the exact claims bytes, so the app verifies offline with
//! the public key built into it. After `expires` the app keeps working for
//! [`OFFLINE_GRACE_SECS`] without contacting the server.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

pub const TOKEN_PREFIX: &str = "locora-ent.v1";
/// Days the app keeps granting access after `expires` without a network check.
pub const OFFLINE_GRACE_SECS: u64 = 14 * 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plan {
    Trial,
    Monthly,
    Annual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// Account id on Locora Cloud.
    pub sub: String,
    pub plan: Plan,
    /// Unix seconds when the token was issued.
    pub issued: u64,
    /// Unix seconds after which the token needs a refresh.
    pub expires: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Token is current.
    Active,
    /// Token expired, but the offline grace period still applies.
    Grace,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EntitlementError {
    #[error("kein gültiges Locora-Cloud-Token")]
    Malformed,
    #[error("Signatur des Locora-Cloud-Tokens ist ungültig")]
    BadSignature,
    #[error("Locora-Cloud-Token ist abgelaufen; bitte online erneuern")]
    Expired,
}

/// Parses the base64 public key that the app ships with.
pub fn parse_public_key(encoded: &str) -> Result<VerifyingKey, EntitlementError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| EntitlementError::Malformed)?;
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| EntitlementError::Malformed)?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| EntitlementError::Malformed)
}

/// Signs `claims` and returns the token string.
pub fn sign(claims: &Claims, key: &SigningKey) -> String {
    let payload = serde_json::to_vec(claims).expect("claims serialize to JSON");
    let signature = key.sign(&payload);
    format!(
        "{TOKEN_PREFIX}.{}.{}",
        URL_SAFE_NO_PAD.encode(&payload),
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    )
}

/// Checks signature and time window. Returns the claims and whether the token
/// is current or only inside the offline grace period.
pub fn verify(
    token: &str,
    key: &VerifyingKey,
    now: u64,
) -> Result<(Claims, Access), EntitlementError> {
    let mut parts = token.split('.');
    let (Some("locora-ent"), Some("v1"), Some(payload), Some(signature), None) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return Err(EntitlementError::Malformed);
    };
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| EntitlementError::Malformed)?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| EntitlementError::Malformed)?;
    let signature = Signature::from_slice(&signature).map_err(|_| EntitlementError::Malformed)?;
    key.verify(&payload, &signature)
        .map_err(|_| EntitlementError::BadSignature)?;
    let claims: Claims =
        serde_json::from_slice(&payload).map_err(|_| EntitlementError::Malformed)?;
    if now <= claims.expires {
        Ok((claims, Access::Active))
    } else if now <= claims.expires.saturating_add(OFFLINE_GRACE_SECS) {
        Ok((claims, Access::Grace))
    } else {
        Err(EntitlementError::Expired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    fn claims(expires: u64) -> Claims {
        Claims {
            sub: "acc_1".into(),
            plan: Plan::Annual,
            issued: 1_000,
            expires,
        }
    }

    #[test]
    fn round_trip_and_grace_window() {
        let key = SigningKey::generate(&mut OsRng);
        let token = sign(&claims(10_000), &key);
        let public = key.verifying_key();
        assert_eq!(verify(&token, &public, 9_999).unwrap().1, Access::Active);
        assert_eq!(verify(&token, &public, 10_000).unwrap().1, Access::Active);
        let after = 10_001;
        assert_eq!(verify(&token, &public, after).unwrap().1, Access::Grace);
        let last_grace = 10_000 + OFFLINE_GRACE_SECS;
        assert_eq!(
            verify(&token, &public, last_grace).unwrap().1,
            Access::Grace
        );
        assert_eq!(
            verify(&token, &public, last_grace + 1),
            Err(EntitlementError::Expired)
        );
    }

    #[test]
    fn public_keys_round_trip_through_base64() {
        let key = SigningKey::generate(&mut OsRng);
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(key.verifying_key().as_bytes());
        assert_eq!(parse_public_key(&encoded).unwrap(), key.verifying_key());
        assert_eq!(
            parse_public_key("not base64!"),
            Err(EntitlementError::Malformed)
        );
        assert_eq!(parse_public_key("AAAA"), Err(EntitlementError::Malformed));
    }

    #[test]
    fn tampering_and_foreign_keys_are_rejected() {
        let key = SigningKey::generate(&mut OsRng);
        let other = SigningKey::generate(&mut OsRng);
        let token = sign(&claims(10_000), &key);
        assert_eq!(
            verify(&token, &other.verifying_key(), 1),
            Err(EntitlementError::BadSignature)
        );
        // Swap in a payload with a later expiry but keep the old signature.
        let forged_payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims(99_999)).unwrap());
        let parts: Vec<&str> = token.split('.').collect();
        let forged = format!("{}.{}.{}.{}", parts[0], parts[1], forged_payload, parts[3]);
        assert_eq!(
            verify(&forged, &key.verifying_key(), 1),
            Err(EntitlementError::BadSignature)
        );
        assert_eq!(
            verify("garbage", &key.verifying_key(), 1),
            Err(EntitlementError::Malformed)
        );
    }
}
