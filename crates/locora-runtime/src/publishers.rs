//! Plugin publishers the user trusts, by the public key of their signing key.
//! Stored as `plugin-publishers.json` in the profile.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const FILE: &str = "plugin-publishers.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Publisher {
    /// Public key, base64.
    pub key: String,
    pub name: String,
}

pub fn path(data: &Path) -> PathBuf {
    data.join(FILE)
}

pub fn load(data: &Path) -> Result<Vec<Publisher>> {
    match fs::read_to_string(path(data)) {
        Ok(text) => serde_json::from_str(&text).context("plugin-publishers.json ist beschädigt"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err).context("plugin-publishers.json lesen"),
    }
}

/// Name of the publisher with this key, if the user trusts it.
pub fn trusted_name(data: &Path, key: &str) -> Result<Option<String>> {
    Ok(load(data)?
        .into_iter()
        .find(|publisher| publisher.key == key)
        .map(|publisher| publisher.name))
}

/// Adds a publisher. The key must be a valid Ed25519 public key.
pub fn trust(data: &Path, key: &str, name: &str) -> Result<()> {
    if locora_signing::decode_public(key).is_err() {
        bail!("Kein gültiger öffentlicher Schlüssel");
    }
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        bail!("Der Name des Herausgebers braucht 1 bis 80 Zeichen");
    }
    let mut publishers = load(data)?;
    publishers.retain(|publisher| publisher.key != key);
    publishers.push(Publisher {
        key: key.to_owned(),
        name: name.to_owned(),
    });
    fs::create_dir_all(data)?;
    fs::write(path(data), serde_json::to_string_pretty(&publishers)?)?;
    Ok(())
}
