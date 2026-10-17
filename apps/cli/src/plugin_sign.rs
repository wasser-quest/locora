//! Signing and trusting plugins, and updating an installed plugin from its
//! publisher. The install itself lives in `plugin.rs`.

use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use locora_host::{ApprovalReceipt, PluginHost};
use locora_runtime::publishers;
use locora_signing::SigningKey;

use crate::plugin::{BUILT_INS, validate_install};

/// `<plugin>.wasm.sig`
pub fn signature_path(file: &Path) -> PathBuf {
    PathBuf::from(format!("{}.sig", file.display()))
}

/// Writes a new key file (mode 0600) and prints its public key.
pub fn keygen(path: &Path) -> Result<()> {
    let key = locora_signing::generate();
    create_private(path, &format!("{}\n", locora_signing::encode_seed(&key)))?;
    println!("✓ Schlüssel {} angelegt.", path.display());
    println!(
        "  Öffentlicher Schlüssel: {}",
        locora_signing::encode_public(&key.verifying_key())
    );
    println!("  Der private Schlüssel bleibt geheim; weitergeben nur den öffentlichen.");
    Ok(())
}

/// Signs the plugin file and writes `<file>.sig` next to it.
pub fn sign(file: &Path, key_path: &Path) -> Result<()> {
    let bytes =
        locora_runtime::read_regular(file).with_context(|| format!("{} lesen", file.display()))?;
    let key = read_key(key_path)?;
    let signature = locora_signing::sign(&bytes, &key);
    let json = serde_json::to_string_pretty(&signature)?;
    create_private_public(&signature_path(file), &format!("{json}\n"))?;
    println!("✓ Signiert: {}", signature_path(file).display());
    println!("  Herausgeber: {}", signature.publisher);
    Ok(())
}

pub fn trust(data: &Path, key: &str, name: &str) -> Result<()> {
    publishers::trust(data, key, name)?;
    println!("✓ {name} ist jetzt vertrauenswürdig.");
    Ok(())
}

pub fn list_publishers(data: &Path) -> Result<()> {
    let known = publishers::load(data)?;
    if known.is_empty() {
        println!(
            "Keine vertrauenswürdigen Herausgeber. Hinzufügen: locora plugin trust <SCHLÜSSEL> --name <NAME>"
        );
    }
    for publisher in known {
        println!("{}  {}", publisher.name, publisher.key);
    }
    Ok(())
}

/// The verified publisher of the signature next to `file`, or `None` if there is none.
/// A signature that is present but invalid is an error, never ignored.
pub fn signer_of(file: &Path, bytes: &[u8]) -> Result<Option<String>> {
    let path = signature_path(file);
    if fs::symlink_metadata(&path).is_err() {
        return Ok(None);
    }
    let text =
        locora_runtime::read_regular(&path).with_context(|| format!("{} lesen", path.display()))?;
    let text = String::from_utf8(text).context("Signaturdatei ist kein Text")?;
    let file_signature = locora_signing::parse(&text).map_err(|e| anyhow::anyhow!("{e}"))?;
    let publisher =
        locora_signing::verify(bytes, &file_signature).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(Some(publisher))
}

/// Replaces an installed plugin with a newer version from the same publisher.
pub fn update(file: &Path, data: &Path, yes: bool) -> Result<()> {
    let bytes =
        locora_runtime::read_regular(file).with_context(|| format!("{} lesen", file.display()))?;
    let host = PluginHost::new(None)?;
    let manifest = host.inspect_bytes(&bytes)?;
    validate_install(&manifest)?;
    if BUILT_INS.contains(&manifest.id.as_str()) {
        bail!("Eingebautes Plugin {:?} ist geschützt", manifest.id);
    }
    let dir = crate::app::plugin_dir(data)?;
    let destination = dir.join(format!("{}.wasm", manifest.id));
    let approval_path = destination.with_extension("wasm.json");
    let installed: ApprovalReceipt = serde_json::from_slice(
        &fs::read(&approval_path)
            .with_context(|| format!("Plugin {:?} ist nicht installiert", manifest.id))?,
    )?;
    if !fs::symlink_metadata(&destination)?.file_type().is_file() {
        bail!("Keine reguläre Plugin-Datei: {}", destination.display());
    }
    if !installed.matches_bytes(&fs::read(&destination)?) {
        bail!("Installierte Plugin-Datei passt nicht zur Freigabe");
    }
    let Some(old_signer) = installed.signer.clone() else {
        bail!(
            "Plugin {:?} wurde nicht signiert installiert; ein Update ist nicht möglich",
            manifest.id
        );
    };
    let Some(new_signer) = signer_of(file, &bytes)? else {
        bail!(
            "Das Update braucht eine Signatur ({})",
            signature_path(file).display()
        );
    };
    if new_signer != old_signer {
        bail!("Das Update ist von einem anderen Herausgeber signiert");
    }
    if !newer(&manifest.version, &installed.manifest.version)? {
        bail!(
            "Version {:?} ist nicht neuer als die installierte {:?}",
            manifest.version,
            installed.manifest.version
        );
    }
    eprintln!(
        "Update {:?}: {} → {}",
        manifest.id, installed.manifest.version, manifest.version
    );
    if installed.manifest.permissions != manifest.permissions {
        eprintln!(
            "  Rechte geändert: {} → {}",
            list(&installed.manifest.permissions),
            list(&manifest.permissions)
        );
    }
    consent(yes, "Update installieren?")?;
    let receipt = ApprovalReceipt::new(manifest.clone(), &bytes).signed_by(new_signer);
    // Approval first, then the component: a crash never leaves new code without consent.
    replace(&approval_path, &serde_json::to_vec(&receipt)?)?;
    replace(&destination, &bytes)?;
    println!(
        "✓ {:?} auf Version {:?} aktualisiert.",
        manifest.id, manifest.version
    );
    Ok(())
}

/// Asks before installing unless `yes` was given.
pub fn consent(yes: bool, question: &str) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        bail!("Keine interaktive Zustimmung möglich; nach Prüfung ausdrücklich --yes angeben");
    }
    eprint!("{question} [j/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim().to_lowercase().as_str(), "j" | "ja") {
        bail!("Installation abgebrochen");
    }
    Ok(())
}

fn list(permissions: &[String]) -> String {
    if permissions.is_empty() {
        "keine".into()
    } else {
        permissions.join(", ")
    }
}

/// Compares dotted numeric versions (`1.2.3`).
fn newer(candidate: &str, installed: &str) -> Result<bool> {
    let parse = |v: &str| -> Result<Vec<u64>> {
        v.split('.')
            .map(|part| {
                part.parse::<u64>()
                    .with_context(|| format!("Version {v:?} ist keine Zahlenversion"))
            })
            .collect()
    };
    Ok(parse(candidate)? > parse(installed)?)
}

fn read_key(path: &Path) -> Result<SigningKey> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)
            .with_context(|| format!("{} nicht lesbar", path.display()))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            bail!("{} ist für andere lesbar; bitte chmod 600", path.display());
        }
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("{} nicht lesbar", path.display()))?;
    locora_signing::decode_seed(&text).map_err(|e| anyhow::anyhow!("{e}"))
}

fn create_private(path: &Path, text: &str) -> Result<()> {
    write_new(path, text, 0o600)
}

fn create_private_public(path: &Path, text: &str) -> Result<()> {
    write_new(path, text, 0o644)
}

fn write_new(path: &Path, text: &str, mode: u32) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options
        .open(path)
        .with_context(|| format!("{} kann nicht neu angelegt werden", path.display()))?;
    file.write_all(text.as_bytes())?;
    Ok(())
}

/// Atomically replaces `path` with `bytes` (same directory, then rename).
fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().context("Kein Plugin-Ordner")?)?;
    io::Write::write_all(&mut temp, bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
