//! The plugin host for the browser build. It has the same public API as `locora-host`, so
//! `locora-runtime` compiles unchanged for `wasm32`. Plugins are not run here: the browser host
//! (apps/web/plugin-host) runs them with the browser's WebAssembly engine. Until that bridge is
//! connected, plugin services are reported as unavailable, and the native palace service works.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

#[path = "../../locora-host/src/palaces.rs"]
mod palaces;

pub use locora_api::ui::HighlightRequest;

const NOT_IN_BROWSER: &str = "plugins run in the browser host, which is not connected yet";

/// Metadata a plugin declares about itself.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: String,
    pub permissions: Vec<String>,
}

/// Consent for one exact component, not just its self-declared metadata.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalReceipt {
    pub manifest: Manifest,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
}

impl ApprovalReceipt {
    pub fn new(manifest: Manifest, bytes: &[u8]) -> Self {
        Self { manifest, sha256: format!("{:x}", Sha256::digest(bytes)), signer: None }
    }

    pub fn signed_by(mut self, publisher: String) -> Self {
        self.signer = Some(publisher);
        self
    }

    pub fn matches(&self, manifest: &Manifest, bytes: &[u8]) -> bool {
        self.manifest == *manifest && self.matches_bytes(bytes)
    }

    pub fn matches_bytes(&self, bytes: &[u8]) -> bool {
        self.sha256 == format!("{:x}", Sha256::digest(bytes))
    }
}

/// Log levels as the plugins see them (same names as the WIT `level`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Something the host observed that the UI may want to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    Highlighted { plugin: String, step: HighlightRequest },
    Log { plugin: String, level: Level, message: String },
    Loaded(Manifest),
    Reloaded(Manifest),
    Unloaded(String),
    Disabled { plugin: String, reason: String },
    HandlerError { plugin: String, topic: String, message: String },
    Published { plugin: String, topic: String, payload: Vec<u8> },
}

/// Where plugin key-value storage lives (same contract as the native host).
pub trait KvStore: Send + Sync {
    fn get(&self, plugin: &str, key: &str) -> Result<Option<Vec<u8>>, String>;
    fn set(&self, plugin: &str, key: &str, value: &[u8]) -> Result<(), String>;
}

/// In-memory storage.
#[derive(Default)]
pub struct MemoryKv(std::sync::Mutex<BTreeMap<(String, String), Vec<u8>>>);

impl KvStore for MemoryKv {
    fn get(&self, plugin: &str, key: &str) -> Result<Option<Vec<u8>>, String> {
        let map = self.0.lock().map_err(|_| "storage is poisoned".to_owned())?;
        Ok(map.get(&(plugin.to_owned(), key.to_owned())).cloned())
    }

    fn set(&self, plugin: &str, key: &str, value: &[u8]) -> Result<(), String> {
        let mut map = self.0.lock().map_err(|_| "storage is poisoned".to_owned())?;
        map.insert((plugin.to_owned(), key.to_owned()), value.to_vec());
        Ok(())
    }
}

/// Answers plugin services from outside the core: the browser's plugin host runs the plugins.
/// Requests and replies are the postcard bytes the native host passes to plugins.
pub trait ServiceBridge {
    /// Whether `service` exists, provided by `plugin` when one is named.
    fn provides(&self, plugin: Option<&str>, service: &str) -> bool;
    fn call(&self, plugin: Option<&str>, service: &str, request: &[u8]) -> Result<Vec<u8>, String>;
}

/// The plugin host of the browser build.
pub struct PluginHost {
    palaces: Option<Arc<locora_store::Store>>,
    bridge: Option<Box<dyn ServiceBridge>>,
    events: Vec<HostEvent>,
}

impl PluginHost {
    pub fn new(_cache_dir: Option<PathBuf>) -> Result<Self> {
        Ok(Self { palaces: None, bridge: None, events: Vec::new() })
    }

    pub fn with_kv(cache_dir: Option<PathBuf>, _kv: Arc<dyn KvStore>) -> Result<Self> {
        Self::new(cache_dir)
    }

    pub fn with_palaces(_cache_dir: Option<PathBuf>, _kv: Arc<dyn KvStore>, palaces: Arc<locora_store::Store>) -> Result<Self> {
        Ok(Self { palaces: Some(palaces), bridge: None, events: Vec::new() })
    }

    /// Connects the plugins the browser host runs. Until this is set, only the palace service exists.
    pub fn set_bridge(&mut self, bridge: Box<dyn ServiceBridge>) {
        self.bridge = Some(bridge);
    }

    pub fn load_file(&mut self, _path: &Path) -> Result<Manifest> {
        bail!(NOT_IN_BROWSER)
    }

    pub fn load_additional_file(&mut self, path: &Path) -> Result<Manifest> {
        self.load_file(path)
    }

    pub fn load_approved_file(&mut self, path: &Path, _approved: &ApprovalReceipt) -> Result<Manifest> {
        self.load_file(path)
    }

    pub fn load_bytes(&mut self, _bytes: &[u8]) -> Result<Manifest> {
        bail!(NOT_IN_BROWSER)
    }

    pub fn inspect_bytes(&self, _bytes: &[u8]) -> Result<Manifest> {
        bail!(NOT_IN_BROWSER)
    }

    pub fn unload(&mut self, _id: &str) -> bool {
        false
    }

    pub fn publish<E: locora_api::Event>(&mut self, event: &E) -> Result<()> {
        self.publish_raw(E::TOPIC, locora_api::encode(event)?);
        Ok(())
    }

    pub fn publish_raw(&mut self, topic: &str, payload: Vec<u8>) {
        self.events.push(HostEvent::Published { plugin: "host".to_owned(), topic: topic.to_owned(), payload });
    }

    pub fn call<S: locora_api::Service>(&mut self, request: &S::Request) -> Result<S::Response> {
        let bytes = self.call_raw(S::NAME, &locora_api::encode(request)?)?;
        Ok(locora_api::decode(&bytes)?)
    }

    pub fn call_plugin<S: locora_api::Service>(&mut self, plugin: &str, request: &S::Request) -> Result<S::Response> {
        let bytes = self.bridge_call(Some(plugin), S::NAME, &locora_api::encode(request)?)?;
        Ok(locora_api::decode(&bytes)?)
    }

    pub fn plugin_provides(&self, plugin: &str, service: &str) -> bool {
        self.bridge.as_ref().is_some_and(|bridge| bridge.provides(Some(plugin), service))
    }

    pub fn provides(&self, service: &str) -> bool {
        (service == <locora_api::palaces::Palaces as locora_api::Service>::NAME && self.palaces.is_some())
            || self.bridge.as_ref().is_some_and(|bridge| bridge.provides(None, service))
    }

    pub fn call_raw(&mut self, service: &str, request: &[u8]) -> Result<Vec<u8>> {
        if service != <locora_api::palaces::Palaces as locora_api::Service>::NAME {
            return self.bridge_call(None, service, request);
        }
        let Some(store) = self.palaces.clone() else {
            bail!("the palace service is not bound");
        };
        let request: locora_api::palaces::Request = locora_api::decode(request)?;
        let outcome = palaces::call(&store, request).map_err(anyhow::Error::msg)?;
        for (topic, payload) in outcome.events {
            self.events.push(HostEvent::Published { plugin: "core".to_owned(), topic, payload });
        }
        Ok(locora_api::encode(&outcome.response)?)
    }

    fn bridge_call(&self, plugin: Option<&str>, service: &str, request: &[u8]) -> Result<Vec<u8>> {
        match self.bridge.as_ref().filter(|bridge| bridge.provides(plugin, service)) {
            Some(bridge) => bridge.call(plugin, service, request).map_err(anyhow::Error::msg),
            None => match plugin {
                Some(plugin) => bail!("plugin {plugin} does not provide {service} here: {NOT_IN_BROWSER}"),
                None => bail!("service {service} is not available: {NOT_IN_BROWSER}"),
            },
        }
    }

    /// Filters run in the browser host; here values pass through unchanged.
    pub fn filter<H: locora_api::Hook>(&mut self, value: H::Value) -> Result<H::Value> {
        Ok(value)
    }

    pub fn plugins(&self) -> impl Iterator<Item = &Manifest> {
        std::iter::empty()
    }

    pub fn source(&self, _id: &str) -> Option<&Path> {
        None
    }

    pub fn drain_events(&mut self) -> Vec<HostEvent> {
        std::mem::take(&mut self.events)
    }
}
