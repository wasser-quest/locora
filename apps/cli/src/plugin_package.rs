//! Small, closed ZIP format for distributing one exact WASM component.

use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use locora_host::{Manifest, PluginHost};
use serde::Deserialize;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::plugin::validate_install;
use crate::plugin_sign;

const MAX_PACKAGE: u64 = 72 * 1024 * 1024;
const MAX_WASM: u64 = 64 * 1024 * 1024;
const MAX_ICON: u64 = 1024 * 1024;

pub struct Input {
    pub bytes: Vec<u8>,
    pub signer: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageManifest {
    id: String,
    name: String,
    version: String,
    api: String,
    permissions: Vec<String>,
}

impl From<PackageManifest> for Manifest {
    fn from(value: PackageManifest) -> Self {
        Self {
            id: value.id,
            name: value.name,
            version: value.version,
            api: value.api,
            permissions: value.permissions,
        }
    }
}

pub fn read_input(file: &Path) -> Result<Input> {
    if file.extension().is_some_and(|ext| ext == "locora-plugin") {
        return read_package(file);
    }
    let bytes = locora_runtime::read_regular(file)
        .with_context(|| format!("Plugin {} lesen", file.display()))?;
    let signer = plugin_sign::signer_of(file, &bytes)?;
    Ok(Input { bytes, signer })
}

pub fn pack(file: &Path, output: Option<&Path>, icon: Option<&Path>) -> Result<()> {
    if file.extension().is_none_or(|ext| ext != "wasm") {
        bail!("`plugin pack` erwartet eine .wasm-Datei");
    }
    let bytes = locora_runtime::read_regular(file)?;
    if bytes.len() as u64 > MAX_WASM {
        bail!("WASM-Komponente überschreitet 64 MiB");
    }
    let host = PluginHost::new(None)?;
    let manifest = host.inspect_bytes(&bytes)?;
    validate_install(&manifest)?;
    // A sidecar signature authenticates the component bytes, never mutable ZIP metadata.
    // The manifest is checked against the component again whenever unpacked.
    plugin_sign::signer_of(file, &bytes)?;
    let signature_path = plugin_sign::signature_path(file);
    let signature = if fs::symlink_metadata(&signature_path).is_ok() {
        let contents = locora_runtime::read_regular(&signature_path)?;
        if contents.len() > 4 * 1024 {
            bail!("Signaturdatei überschreitet 4 KiB");
        }
        Some(contents)
    } else {
        None
    };
    let icon = icon.map(read_icon).transpose()?;
    let target = output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(format!("{}.locora-plugin", manifest.id)));
    if target.extension().is_none_or(|ext| ext != "locora-plugin") {
        bail!("Paketdatei muss auf .locora-plugin enden");
    }
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    writer.start_file("plugin.wasm", options)?;
    writer.write_all(&bytes)?;
    writer.start_file("manifest.toml", options)?;
    writer.write_all(toml::to_string(&manifest)?.as_bytes())?;
    if let Some(signature) = signature {
        writer.start_file("plugin.wasm.sig", options)?;
        writer.write_all(&signature)?;
    }
    if let Some((name, bytes)) = icon {
        writer.start_file(name, options)?;
        writer.write_all(&bytes)?;
    }
    let zip = writer.finish()?.into_inner();
    if zip.len() as u64 > MAX_PACKAGE {
        bail!("Paket überschreitet 72 MiB");
    }
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&zip)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(&target)
        .map_err(|e| e.error)
        .with_context(|| format!("Paket {} existiert bereits", target.display()))?;
    println!("✓ Paket {} erstellt", target.display());
    Ok(())
}

fn read_icon(file: &Path) -> Result<(&'static str, Vec<u8>)> {
    let name = match file.extension().and_then(|ext| ext.to_str()) {
        Some("png") => "icon.png",
        _ => bail!("Icon muss .png sein"),
    };
    let bytes = locora_runtime::read_regular(file)?;
    if bytes.len() as u64 > MAX_ICON {
        bail!("Icon überschreitet 1 MiB");
    }
    validate_icon(&bytes)?;
    Ok((name, bytes))
}

fn validate_icon(bytes: &[u8]) -> Result<()> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        bail!("Icon ist keine PNG-Datei");
    }
    Ok(())
}

fn read_package(file: &Path) -> Result<Input> {
    let meta = fs::symlink_metadata(file)?;
    if !meta.file_type().is_file() || meta.len() > MAX_PACKAGE {
        bail!("Keine reguläre Paketdatei unter 72 MiB: {}", file.display());
    }
    let bytes = locora_runtime::read_regular(file)?;
    if bytes.len() as u64 > MAX_PACKAGE {
        bail!("Paket überschreitet 72 MiB");
    }
    let declared_entries = declared_entry_count(&bytes)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).context("Ungültiges Plugin-Paket")?;
    // zip's indexed reader coalesces duplicate names. Compare with the ZIP
    // directory count before trusting its view of the archive.
    if archive.len() != declared_entries {
        bail!("Doppelte Paketeinträge sind nicht erlaubt");
    }
    if archive.len() > 4 {
        bail!("Plugin-Paket enthält zu viele Einträge");
    }
    let mut seen = HashSet::new();
    let mut wasm = None;
    let mut manifest = None;
    let mut signature = None;
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let name = entry.name()?.into_owned();
        let limit = match name.as_str() {
            "plugin.wasm" => MAX_WASM,
            "manifest.toml" => 16 * 1024,
            "plugin.wasm.sig" => 4 * 1024,
            "icon.png" => MAX_ICON,
            _ => bail!("Unbekannter oder unsicherer Paketeintrag {name:?}"),
        };
        if !seen.insert(name.clone()) {
            bail!("Doppelter Paketeintrag {name:?}");
        }
        if entry.is_dir() || entry.size() > limit {
            bail!("Paketeintrag {name:?} ist kein erlaubter Dateiinhalt oder zu groß");
        }
        if let Some(mode) = entry.unix_mode()
            && mode & 0o170000 != 0
            && mode & 0o170000 != 0o100000
        {
            bail!("Paketeintrag {name:?} ist keine reguläre Datei");
        }
        let mut contents = Vec::new();
        entry.take(limit + 1).read_to_end(&mut contents)?;
        if contents.len() as u64 > limit {
            bail!("Paketeintrag {name:?} überschreitet Größenlimit");
        }
        match name.as_str() {
            "plugin.wasm" => wasm = Some(contents),
            "manifest.toml" => manifest = Some(contents),
            "plugin.wasm.sig" => signature = Some(contents),
            "icon.png" => validate_icon(&contents)?,
            _ => {}
        }
    }
    let wasm = wasm.context("Paket enthält kein plugin.wasm")?;
    let manifest = manifest.context("Paket enthält kein manifest.toml")?;
    let declared: Manifest = toml::from_str::<PackageManifest>(std::str::from_utf8(&manifest)?)
        .context("Ungültiges manifest.toml")?
        .into();
    let host = PluginHost::new(None)?;
    let actual = host.inspect_bytes(&wasm)?;
    validate_install(&actual)?;
    if declared != actual {
        bail!("Paketmanifest stimmt nicht mit der WASM-Komponente überein");
    }
    let signer = signature
        .map(|sig| -> Result<String> {
            let parsed = locora_signing::parse(std::str::from_utf8(&sig)?)?;
            Ok(locora_signing::verify(&wasm, &parsed)?)
        })
        .transpose()?;
    Ok(Input {
        bytes: wasm,
        signer,
    })
}

fn declared_entry_count(bytes: &[u8]) -> Result<usize> {
    const EOCD: &[u8; 4] = b"PK\x05\x06";
    let start = bytes.len().saturating_sub(65_557);
    for i in (start..bytes.len().saturating_sub(21)).rev() {
        if &bytes[i..i + 4] != EOCD {
            continue;
        }
        let count = u16::from_le_bytes([bytes[i + 10], bytes[i + 11]]);
        let total = u16::from_le_bytes([bytes[i + 8], bytes[i + 9]]);
        let comment = u16::from_le_bytes([bytes[i + 20], bytes[i + 21]]) as usize;
        if i + 22 + comment != bytes.len() {
            continue;
        }
        if bytes[i + 4..i + 8] != [0, 0, 0, 0] || count != total || count == u16::MAX {
            bail!("Mehrteilige oder ZIP64-Pakete werden nicht unterstützt");
        }
        return Ok(count as usize);
    }
    bail!("Ungültiges Plugin-Paket: ZIP-Endverzeichnis fehlt")
}
