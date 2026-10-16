//! Shared CLI context: the local store plus the plugin host with the
//! official plugins and any plugins the user installed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

use anyhow::{Context, Result};
use locora_host::{HostEvent, KvStore, Level, PluginHost};
use locora_store::Store;

pub(crate) mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_plugins.rs"));
}

/// Plugin storage backed by the local database.
struct StoreKv(Arc<Store>);

impl KvStore for StoreKv {
    fn get(&self, plugin: &str, key: &str) -> Result<Option<Vec<u8>>, String> {
        self.0.kv_get(plugin, key).map_err(|e| e.to_string())
    }

    fn set(&self, plugin: &str, key: &str, value: &[u8]) -> Result<(), String> {
        self.0.kv_set(plugin, key, value).map_err(|e| e.to_string())
    }
}

pub struct App {
    pub store: Arc<Store>,
    pub host: PluginHost,
}

/// Profile directory: `--data`, `$LOCORA_DATA` or the platform data dir.
pub fn data_dir(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = explicit {
        return Ok(dir.to_owned());
    }
    if let Some(dir) = env::var_os("LOCORA_DATA") {
        return Ok(PathBuf::from(dir));
    }
    Ok(dirs::data_dir()
        .context("Kein Datenordner gefunden; gib einen mit --data an")?
        .join("locora"))
}

/// Installed components must live in a real directory beneath the chosen profile.
/// The profile itself may be a user-selected symlink; its plugin directory may not.
pub(crate) fn plugin_dir(data: &Path) -> Result<PathBuf> {
    let dir = data.join("plugins");
    match fs::symlink_metadata(&dir) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            anyhow::bail!(
                "Plugin-Ordner {} muss ein Verzeichnis ohne symbolischen Link sein",
                dir.display()
            );
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).context("Plugin-Ordner prüfen"),
    }
    Ok(dir)
}

/// Current time in seconds; `LOCORA_NOW` overrides it (tests, demos).
pub fn now() -> u64 {
    env::var("LOCORA_NOW")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        })
}

impl App {
    pub fn open(data: Option<&Path>) -> Result<Self> {
        let data_dir = data_dir(data)?;
        let installed = plugin_dir(&data_dir)?;
        let store = Arc::new(
            Store::open(&data_dir)
                .with_context(|| format!("Datenbank in {} öffnen", data_dir.display()))?,
        );
        let cache = dirs::cache_dir().map(|d| d.join("locora").join("plugins"));
        let mut host =
            PluginHost::with_palaces(cache, Arc::new(StoreKv(store.clone())), store.clone())?;
        for (name, bytes) in embedded::EMBEDDED {
            if let Err(err) = host.load_bytes(bytes) {
                eprintln!("⚠ Eingebautes Plugin `{name}` konnte nicht laden: {err:#}");
            }
        }
        if let Ok(entries) = fs::read_dir(&installed) {
            let mut files: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "wasm"))
                .collect();
            files.sort();
            for file in files {
                let load = (|| -> Result<locora_host::Manifest> {
                    let approval = crate::plugin::read_regular(&file.with_extension("wasm.json"))
                        .context(
                        "Keine Installationszustimmung; bitte `locora plugin install` verwenden",
                    )?;
                    let approved = serde_json::from_slice(&approval)?;
                    host.load_approved_file(&file, &approved)
                })();
                if let Err(err) = load {
                    eprintln!("⚠ Plugin {} konnte nicht laden: {err:#}", file.display());
                }
            }
        }
        let mut app = Self { store, host };
        app.report();
        Ok(app)
    }

    /// Publish an event and show what plugins reported (only problems).
    pub fn publish<E: locora_api::Event>(&mut self, event: &E) -> Result<()> {
        self.host.publish(event)?;
        self.report();
        Ok(())
    }

    /// Print plugin warnings and errors; normal plugin chatter stays quiet.
    pub fn report(&mut self) {
        for event in self.host.drain_events() {
            match event {
                HostEvent::Log {
                    plugin,
                    level: Level::Warn | Level::Error,
                    message,
                } => eprintln!("⚠ [{plugin}] {message}"),
                HostEvent::HandlerError {
                    plugin,
                    topic,
                    message,
                } => eprintln!("⚠ [{plugin}] Fehler bei {topic}: {message}"),
                HostEvent::Disabled { plugin, reason } => {
                    eprintln!("⚠ Plugin `{plugin}` wurde deaktiviert: {reason}")
                }
                _ => {}
            }
        }
    }
}
