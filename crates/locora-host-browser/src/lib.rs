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
        Self {
            manifest,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            signer: None,
        }
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
    Highlighted {
        plugin: String,
        step: HighlightRequest,
    },
    Log {
        plugin: String,
        level: Level,
        message: String,
    },
    Loaded(Manifest),
    Reloaded(Manifest),
    Unloaded(String),
    Disabled {
        plugin: String,
        reason: String,
    },
    HandlerError {
        plugin: String,
        topic: String,
        message: String,
    },
    Published {
        plugin: String,
        topic: String,
        payload: Vec<u8>,
    },
}

/// Where plugin key-value storage lives (same contract as the native host).
pub trait KvStore: Send + Sync {
    fn get(&self, plugin: &str, key: &str) -> Result<Option<Vec<u8>>, String>;
    fn set(&self, plugin: &str, key: &str, value: &[u8]) -> Result<(), String>;
    /// The keys of `plugin` starting with `prefix` that changed after `after_rev`, in the order
    /// they changed (`storage.list@1`). Storage without revisions cannot list.
    fn list(&self, plugin: &str, prefix: &str, after_rev: u64) -> Result<ListResponse, String> {
        let _ = (plugin, prefix, after_rev);
        Err("Dieser Speicher kann keine Schlüssel auflisten".into())
    }
}

pub use locora_api::storage::{ListEntry, ListResponse};

/// Values by (plugin, key) with the revision of their last change, and the latest revision.
type Revised = (BTreeMap<(String, String), (Vec<u8>, u64)>, u64);

/// In-memory storage.
#[derive(Default)]
pub struct MemoryKv(std::sync::Mutex<Revised>);

impl KvStore for MemoryKv {
    fn get(&self, plugin: &str, key: &str) -> Result<Option<Vec<u8>>, String> {
        let map = self
            .0
            .lock()
            .map_err(|_| "storage is poisoned".to_owned())?;
        Ok(map
            .0
            .get(&(plugin.to_owned(), key.to_owned()))
            .map(|(value, _)| value.clone()))
    }

    fn set(&self, plugin: &str, key: &str, value: &[u8]) -> Result<(), String> {
        let mut map = self
            .0
            .lock()
            .map_err(|_| "storage is poisoned".to_owned())?;
        let id = (plugin.to_owned(), key.to_owned());
        if map.0.get(&id).is_some_and(|(old, _)| old == value) {
            return Ok(());
        }
        map.1 += 1;
        let rev = map.1;
        map.0.insert(id, (value.to_vec(), rev));
        Ok(())
    }

    fn list(&self, plugin: &str, prefix: &str, after_rev: u64) -> Result<ListResponse, String> {
        let map = self
            .0
            .lock()
            .map_err(|_| "storage is poisoned".to_owned())?;
        let mut changed: Vec<(u64, String, Vec<u8>)> = map
            .0
            .iter()
            .filter(|((owner, key), (_, rev))| {
                owner == plugin && key.starts_with(prefix) && *rev > after_rev
            })
            .map(|((_, key), (value, rev))| (*rev, key.clone(), value.clone()))
            .collect();
        changed.sort_by_key(|(rev, _, _)| *rev);
        let more = changed.len() > locora_api::storage::PAGE;
        changed.truncate(locora_api::storage::PAGE);
        let rev = if more {
            changed.last().map_or(after_rev, |(rev, _, _)| *rev)
        } else {
            map.1.max(after_rev)
        };
        Ok(ListResponse {
            entries: changed
                .into_iter()
                .map(|(_, key, value)| ListEntry { key, value })
                .collect(),
            rev,
            more,
        })
    }
}

/// Answers plugin services from outside the core: the browser's plugin host runs the plugins.
/// Requests and replies are the postcard bytes the native host passes to plugins.
pub trait ServiceBridge {
    /// Whether `service` exists, provided by `plugin` when one is named.
    fn provides(&self, plugin: Option<&str>, service: &str) -> bool;
    fn call(&self, plugin: Option<&str>, service: &str, request: &[u8]) -> Result<Vec<u8>, String>;
    /// Runs the filters of `hook` on an encoded value; the result is the encoded value after them.
    fn filter(&self, hook: &str, value: &[u8]) -> Result<Vec<u8>, String>;
}

thread_local! {
    /// What plugins did while the core was busy with a call (highlights, published events): the core
    /// reads it with `drain_events`, in `Profile::report`.
    static NESTED: std::cell::RefCell<Vec<HostEvent>> = const { std::cell::RefCell::new(Vec::new()) };
    /// What the core's palace reads wrote while a plugin asked for them: the page delivers these
    /// to the plugins with the rest of the command's events (see `take_events`).
    static PAGE: std::cell::RefCell<Vec<HostEvent>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Limits of a tour step, as the native host enforces them.
const MAX_TOUR_REQUEST: usize = 1_024;

/// A plugin's palace read, answered while the core is busy. Writes the plugin makes are queued as
/// events of the command in progress.
pub fn nested_palace(store: &locora_store::Store, request: &[u8]) -> Result<Vec<u8>, String> {
    let request: locora_api::palaces::Request =
        locora_api::decode(request).map_err(|e| e.to_string())?;
    let outcome = palaces::call(store, request)?;
    PAGE.with(|queue| {
        queue
            .borrow_mut()
            .extend(
                outcome
                    .events
                    .into_iter()
                    .map(|(topic, payload)| HostEvent::Published {
                        plugin: "core".to_owned(),
                        topic,
                        payload,
                    }),
            )
    });
    locora_api::encode(&outcome.response).map_err(|e| e.to_string())
}

/// A tour step a plugin shows (`ui.highlight@1`), checked the way the native host checks it.
/// Returns the encoded reply the plugin expects (`bool`: the step was shown).
pub fn nested_highlight(plugin: &str, request: &[u8]) -> Result<Vec<u8>, String> {
    if request.len() > MAX_TOUR_REQUEST {
        return Err("Tour-Schritt ist zu groß".into());
    }
    let step: locora_api::ui::HighlightRequest =
        locora_api::decode(request).map_err(|e| format!("Ungültiger Tour-Schritt: {e}"))?;
    if !step.target.starts_with("tour-")
        || step.target.len() > 64
        || !step
            .target
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || step.title.trim().is_empty()
        || step.title.chars().count() > 80
        || step.text.trim().is_empty()
        || step.text.chars().count() > 320
        || step.total == 0
        || step.total > 20
        || step.step == 0
        || step.step > step.total
    {
        return Err("Ungültiges Ziel oder Inhalt des Tour-Schritts".into());
    }
    NESTED.with(|queue| {
        queue.borrow_mut().push(HostEvent::Highlighted {
            plugin: plugin.to_owned(),
            step,
        })
    });
    locora_api::encode(&true).map_err(|e| e.to_string())
}

/// An event a plugin published, for the core (the UI declarations of plugins are read from these).
pub fn nested_publish(plugin: &str, topic: &str, payload: Vec<u8>) {
    NESTED.with(|queue| {
        queue.borrow_mut().push(HostEvent::Published {
            plugin: plugin.to_owned(),
            topic: topic.to_owned(),
            payload,
        })
    });
}

/// The plugin host of the browser build.
pub struct PluginHost {
    palaces: Option<Arc<locora_store::Store>>,
    bridge: Option<Box<dyn ServiceBridge>>,
    events: Vec<HostEvent>,
    /// The plugins the page runs (set by the page after it starts them).
    plugins: Vec<Manifest>,
}

impl PluginHost {
    pub fn new(_cache_dir: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            palaces: None,
            bridge: None,
            events: Vec::new(),
            plugins: Vec::new(),
        })
    }

    pub fn with_kv(cache_dir: Option<PathBuf>, _kv: Arc<dyn KvStore>) -> Result<Self> {
        Self::new(cache_dir)
    }

    pub fn with_palaces(
        _cache_dir: Option<PathBuf>,
        _kv: Arc<dyn KvStore>,
        palaces: Arc<locora_store::Store>,
    ) -> Result<Self> {
        Ok(Self {
            palaces: Some(palaces),
            bridge: None,
            events: Vec::new(),
            plugins: Vec::new(),
        })
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

    pub fn load_approved_file(
        &mut self,
        path: &Path,
        _approved: &ApprovalReceipt,
    ) -> Result<Manifest> {
        self.load_file(path)
    }

    /// The built-in plugins are loaded by the page's plugin host, not here: accepting them is enough.
    pub fn load_bytes(&mut self, _bytes: &[u8]) -> Result<Manifest> {
        Ok(Manifest {
            id: String::new(),
            name: String::new(),
            version: String::new(),
            api: String::new(),
            permissions: Vec::new(),
        })
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

    /// Events the core published since the last call, for the page to deliver to the plugins.
    pub fn take_events(&mut self) -> Vec<HostEvent> {
        let mut events = std::mem::take(&mut self.events);
        PAGE.with(|queue| events.append(&mut queue.borrow_mut()));
        events
    }

    pub fn publish_raw(&mut self, topic: &str, payload: Vec<u8>) {
        self.events.push(HostEvent::Published {
            plugin: "host".to_owned(),
            topic: topic.to_owned(),
            payload,
        });
    }

    pub fn call<S: locora_api::Service>(&mut self, request: &S::Request) -> Result<S::Response> {
        let bytes = self.call_raw(S::NAME, &locora_api::encode(request)?)?;
        Ok(locora_api::decode(&bytes)?)
    }

    pub fn call_plugin<S: locora_api::Service>(
        &mut self,
        plugin: &str,
        request: &S::Request,
    ) -> Result<S::Response> {
        let bytes = self.bridge_call(Some(plugin), S::NAME, &locora_api::encode(request)?)?;
        Ok(locora_api::decode(&bytes)?)
    }

    pub fn plugin_provides(&self, plugin: &str, service: &str) -> bool {
        self.bridge
            .as_ref()
            .is_some_and(|bridge| bridge.provides(Some(plugin), service))
    }

    pub fn provides(&self, service: &str) -> bool {
        (service == <locora_api::palaces::Palaces as locora_api::Service>::NAME
            && self.palaces.is_some())
            || self
                .bridge
                .as_ref()
                .is_some_and(|bridge| bridge.provides(None, service))
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
            self.events.push(HostEvent::Published {
                plugin: "core".to_owned(),
                topic,
                payload,
            });
        }
        Ok(locora_api::encode(&outcome.response)?)
    }

    fn bridge_call(&self, plugin: Option<&str>, service: &str, request: &[u8]) -> Result<Vec<u8>> {
        match self
            .bridge
            .as_ref()
            .filter(|bridge| bridge.provides(plugin, service))
        {
            Some(bridge) => bridge
                .call(plugin, service, request)
                .map_err(anyhow::Error::msg),
            None => match plugin {
                Some(plugin) => {
                    bail!("plugin {plugin} does not provide {service} here: {NOT_IN_BROWSER}")
                }
                None => bail!("service {service} is not available: {NOT_IN_BROWSER}"),
            },
        }
    }

    /// Filters run in the plugin host (in the page). A failing filter keeps the value it was given.
    pub fn filter<H: locora_api::Hook>(&mut self, value: H::Value) -> Result<H::Value> {
        let Some(bridge) = self.bridge.as_ref() else {
            return Ok(value);
        };
        let input = locora_api::encode(&value)?;
        Ok(match bridge.filter(H::NAME, &input) {
            Ok(bytes) => locora_api::decode::<H::Value>(&bytes).unwrap_or(value),
            Err(_) => value,
        })
    }

    /// The plugins the page runs. Their tiles and declared blocks are shown only for these.
    pub fn set_plugins(&mut self, plugins: Vec<Manifest>) {
        self.plugins = plugins;
    }

    pub fn plugins(&self) -> impl Iterator<Item = &Manifest> {
        self.plugins.iter()
    }

    pub fn source(&self, _id: &str) -> Option<&Path> {
        None
    }

    /// What plugins caused for the core (highlights, published events), since the last call.
    pub fn drain_events(&mut self) -> Vec<HostEvent> {
        NESTED.with(|queue| std::mem::take(&mut *queue.borrow_mut()))
    }
}
