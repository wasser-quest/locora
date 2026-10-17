//! Locora plugin host.
//!
//! Plugins are WebAssembly components built from Rust with `locora-sdk`. The
//! host loads them at runtime, gives each one an isolated sandbox with only the
//! permissions it declared, and routes events between them. Plugins can be
//! replaced while the app runs ([`PluginHost::load`] on an already loaded id
//! hot-reloads it); their storage survives the reload.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result, bail};
use sha2::{Digest, Sha256};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../locora-sdk/wit",
        world: "plugin",
    });
}

mod palaces;

pub use bindings::locora::plugin::types::Level;
use bindings::locora::plugin::{host, types};

/// Fuel granted to each call into a plugin. A plugin that runs out (e.g. an
/// endless loop) is stopped and disabled instead of freezing the app.
const FUEL_PER_CALL: u64 = 2_000_000_000;
/// Events a single publish may cascade into before the host stops delivering.
const MAX_EVENTS_PER_DISPATCH: usize = 10_000;

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
}

impl ApprovalReceipt {
    pub fn new(manifest: Manifest, bytes: &[u8]) -> Self {
        Self {
            manifest,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }

    pub fn matches(&self, manifest: &Manifest, bytes: &[u8]) -> bool {
        self.manifest == *manifest && self.matches_bytes(bytes)
    }

    pub fn matches_bytes(&self, bytes: &[u8]) -> bool {
        self.sha256 == format!("{:x}", Sha256::digest(bytes))
    }
}

impl From<types::Manifest> for Manifest {
    fn from(m: types::Manifest) -> Self {
        Self {
            id: m.id,
            name: m.name,
            version: m.version,
            api: m.api,
            permissions: m.permissions,
        }
    }
}

/// Something the host observed that the UI or CLI may want to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// A validated, capability-checked tour step requested via `ui.highlight@1`.
    Highlighted {
        plugin: String,
        step: locora_api::ui::HighlightRequest,
    },
    Log {
        plugin: String,
        level: Level,
        message: String,
    },
    Loaded(Manifest),
    Reloaded(Manifest),
    Unloaded(String),
    /// A plugin trapped, panicked or returned an error and was disabled.
    Disabled {
        plugin: String,
        reason: String,
    },
    /// A plugin returned an error from an event handler but stays active.
    HandlerError {
        plugin: String,
        topic: String,
        message: String,
    },
    /// A plugin published an event (also delivered to subscribers).
    Published {
        plugin: String,
        topic: String,
        payload: Vec<u8>,
    },
}

/// Where plugin key-value storage lives. Keys are private per plugin id, so
/// storage survives hot reloads, unloads and app restarts (with a persistent
/// backend such as `locora-store`).
pub trait KvStore: Send + Sync {
    fn get(&self, plugin: &str, key: &str) -> Result<Option<Vec<u8>>, String>;
    fn set(&self, plugin: &str, key: &str, value: &[u8]) -> Result<(), String>;
}

/// In-memory storage, used when the host is created without a backend.
#[derive(Default)]
pub struct MemoryKv(Mutex<HashMap<(String, String), Vec<u8>>>);

impl KvStore for MemoryKv {
    fn get(&self, plugin: &str, key: &str) -> Result<Option<Vec<u8>>, String> {
        let map = self.0.lock().map_err(|e| e.to_string())?;
        Ok(map.get(&(plugin.to_owned(), key.to_owned())).cloned())
    }

    fn set(&self, plugin: &str, key: &str, value: &[u8]) -> Result<(), String> {
        let mut map = self.0.lock().map_err(|e| e.to_string())?;
        map.insert((plugin.to_owned(), key.to_owned()), value.to_vec());
        Ok(())
    }
}

/// Per-plugin sandbox state.
struct PluginState {
    id: String,
    permissions: Vec<String>,
    kv: Arc<dyn KvStore>,
    palaces: Option<Arc<locora_store::Store>>,
    filters: BTreeMap<String, i32>,
    subscriptions: Vec<String>,
    provided: Vec<String>,
    outbox: Vec<(String, String, Vec<u8>)>,
    highlights: Vec<locora_api::ui::HighlightRequest>,
    logs: Vec<(Level, String)>,
    wasi: WasiCtx,
    table: ResourceTable,
}

impl PluginState {
    fn new(kv: Arc<dyn KvStore>, palaces: Option<Arc<locora_store::Store>>) -> Self {
        Self {
            id: String::new(),
            permissions: Vec::new(),
            kv,
            palaces,
            filters: BTreeMap::new(),
            subscriptions: Vec::new(),
            provided: Vec::new(),
            outbox: Vec::new(),
            highlights: Vec::new(),
            logs: Vec::new(),
            // No files, network, env or clock access beyond the defaults.
            wasi: WasiCtx::builder().build(),
            table: ResourceTable::new(),
        }
    }

    fn require(&self, permission: &str) -> Result<(), String> {
        if self.permissions.iter().any(|p| p == permission) {
            Ok(())
        } else {
            Err(format!(
                "Plugin `{}` braucht die Berechtigung `{permission}`; füge sie in `#[locora_plugin(permissions = [...])]` hinzu",
                self.id
            ))
        }
    }
}

impl WasiView for PluginState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl types::Host for PluginState {}

impl host::Host for PluginState {
    fn log(&mut self, level: Level, message: String) {
        self.logs.push((level, message));
    }

    fn subscribe(&mut self, topic: String) {
        if !self.subscriptions.contains(&topic) {
            self.subscriptions.push(topic);
        }
    }

    fn publish(&mut self, topic: String, payload: Vec<u8>) {
        self.outbox.push((self.id.clone(), topic, payload));
    }

    fn provide(&mut self, service: String) {
        if service == <locora_api::palaces::Palaces as locora_api::Service>::NAME
            || service == <locora_api::ui::Highlight as locora_api::Service>::NAME
        {
            self.logs.push((
                Level::Warn,
                format!("Dienst `{service}` ist für den Host reserviert"),
            ));
            return;
        }
        if !self.provided.contains(&service) {
            self.provided.push(service);
        }
    }

    fn register_filter(&mut self, hook: String, priority: i32) -> Result<(), String> {
        if self.filters.contains_key(&hook) {
            return Err(format!("Filter `{hook}` ist bereits registriert"));
        }
        self.filters.insert(hook, priority);
        Ok(())
    }

    fn call_host(&mut self, service: String, request: Vec<u8>) -> Result<Vec<u8>, String> {
        if service == <locora_api::ui::Highlight as locora_api::Service>::NAME {
            self.require(locora_api::permission::UI_HIGHLIGHT)?;
            if request.len() > 1_024 {
                return Err("Tour-Schritt ist zu groß".into());
            }
            let step: locora_api::ui::HighlightRequest = locora_api::decode(&request)
                .map_err(|e| format!("Ungültiger Tour-Schritt: {e}"))?;
            if !step.target.starts_with("tour-")
                || step.target.len() > 64
                || !step.target.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                || step.title.trim().is_empty()
                || step.title.chars().count() > 80
                || step.text.trim().is_empty()
                || step.text.chars().count() > 320
                || step.total == 0
                || step.total > 12
                || step.step == 0
                || step.step > step.total
            {
                return Err("Ungültiges Ziel oder Inhalt des Tour-Schritts".into());
            }
            self.highlights.push(step);
            return locora_api::encode(&true).map_err(|e| e.to_string());
        }
        if service != <locora_api::palaces::Palaces as locora_api::Service>::NAME {
            return Err(format!("Unbekannter Host-Dienst `{service}`"));
        }
        let request: locora_api::palaces::Request =
            locora_api::decode(&request).map_err(|e| format!("Ungültige Palastanfrage: {e}"))?;
        self.require(request.permission())?;
        let store = self
            .palaces
            .as_ref()
            .ok_or("Kein Palastspeicher angebunden")?;
        let outcome = palaces::call(store, request)?;
        self.outbox.extend(
            outcome
                .events
                .into_iter()
                .map(|(topic, payload)| (String::from("host"), topic, payload)),
        );
        locora_api::encode(&outcome.response).map_err(|e| e.to_string())
    }

    fn kv_get(&mut self, key: String) -> Result<Option<Vec<u8>>, String> {
        self.require(locora_api::permission::STORAGE)?;
        self.kv.get(&self.id, &key)
    }

    fn kv_set(&mut self, key: String, value: Vec<u8>) -> Result<(), String> {
        self.require(locora_api::permission::STORAGE)?;
        self.kv.set(&self.id, &key, &value)
    }
}

struct LoadedPlugin {
    manifest: Manifest,
    source: Option<PathBuf>,
    store: Store<PluginState>,
    instance: bindings::Plugin,
}

/// Loads plugins and routes events between them.
pub struct PluginHost {
    engine: Engine,
    linker: Linker<PluginState>,
    cache_dir: Option<PathBuf>,
    plugins: BTreeMap<String, LoadedPlugin>,
    kv: Arc<dyn KvStore>,
    palaces: Option<Arc<locora_store::Store>>,
    events: Vec<HostEvent>,
}

impl PluginHost {
    /// Create a host with in-memory plugin storage. `cache_dir` stores
    /// compiled plugins so later starts skip compilation.
    pub fn new(cache_dir: Option<PathBuf>) -> Result<Self> {
        Self::with_kv(cache_dir, Arc::new(MemoryKv::default()))
    }

    /// Create a host whose plugins keep their storage in `kv`.
    pub fn with_kv(cache_dir: Option<PathBuf>, kv: Arc<dyn KvStore>) -> Result<Self> {
        Self::with_backends(cache_dir, kv, None)
    }

    /// Bind the profile's palace store before any plugin is instantiated.
    pub fn with_palaces(
        cache_dir: Option<PathBuf>,
        kv: Arc<dyn KvStore>,
        palaces: Arc<locora_store::Store>,
    ) -> Result<Self> {
        Self::with_backends(cache_dir, kv, Some(palaces))
    }

    fn with_backends(
        cache_dir: Option<PathBuf>,
        kv: Arc<dyn KvStore>,
        palaces: Option<Arc<locora_store::Store>>,
    ) -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.consume_fuel(true);
        let engine = Engine::new(&config)?;

        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        bindings::Plugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;

        Ok(Self {
            engine,
            linker,
            cache_dir,
            plugins: BTreeMap::new(),
            kv,
            palaces,
            events: Vec::new(),
        })
    }

    /// Load a plugin from a `.wasm` component file. Loading an id that is
    /// already active hot-reloads it and keeps its storage.
    pub fn load_file(&mut self, path: &Path) -> Result<Manifest> {
        self.load_file_with_policy(path, true)
    }

    /// Load an additional plugin without replacing an existing identity.
    /// Profile loading uses this; explicit development reloads use `load_file`.
    pub fn load_additional_file(&mut self, path: &Path) -> Result<Manifest> {
        self.load_file_with_policy(path, false)
    }

    /// Load a profile component only if its bytes and manifest match approval.
    /// The comparison and activation use the same component bytes.
    pub fn load_approved_file(
        &mut self,
        path: &Path,
        approved: &ApprovalReceipt,
    ) -> Result<Manifest> {
        self.load_file_with_approval(path, false, Some(approved))
    }

    fn load_file_with_policy(&mut self, path: &Path, allow_reload: bool) -> Result<Manifest> {
        self.load_file_with_approval(path, allow_reload, None)
    }

    fn load_file_with_approval(
        &mut self,
        path: &Path,
        allow_reload: bool,
        approved: Option<&ApprovalReceipt>,
    ) -> Result<Manifest> {
        if !fs::symlink_metadata(path)?.file_type().is_file() {
            bail!("Keine reguläre Plugin-Datei: {}", path.display());
        }
        let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
        let manifest = self.load_bytes_with_approval(&bytes, allow_reload, approved)?;
        if let Some(plugin) = self.plugins.get_mut(&manifest.id) {
            plugin.source = Some(path.to_owned());
        }
        Ok(manifest)
    }

    /// Load a plugin from component bytes. See [`PluginHost::load_file`].
    pub fn load_bytes(&mut self, bytes: &[u8]) -> Result<Manifest> {
        self.load_bytes_with_policy(bytes, true)
    }

    fn load_bytes_with_policy(&mut self, bytes: &[u8], allow_reload: bool) -> Result<Manifest> {
        self.load_bytes_with_approval(bytes, allow_reload, None)
    }

    fn load_bytes_with_approval(
        &mut self,
        bytes: &[u8],
        allow_reload: bool,
        approved: Option<&ApprovalReceipt>,
    ) -> Result<Manifest> {
        if approved.is_some_and(|approved| !approved.matches_bytes(bytes)) {
            bail!("Plugin-Datei stimmt nicht mit der bestätigten Installation überein");
        }
        let component = self.compile(bytes)?;
        let manifest = self.inspect_component(&component)?;
        if !allow_reload && matches!(manifest.id.as_str(), "core" | "review" | "notify" | "tutorial" | "gamification") {
            bail!("Eingebaute Plugin-ID `{}` ist geschützt", manifest.id);
        }
        if approved.is_some_and(|approved| !approved.matches(&manifest, bytes)) {
            bail!(
                "Plugin-Datei oder Manifest stimmt nicht mit der bestätigten Installation überein"
            );
        }

        let reloading = self.plugins.contains_key(&manifest.id);
        if reloading && !allow_reload {
            bail!(
                "Plugin-ID `{}` ist bereits geladen; kein Überschreiben durch Profil-Plugins",
                manifest.id
            );
        }
        if let Some(old) = self.plugins.remove(&manifest.id) {
            self.shutdown(old);
        }

        let mut store = self.new_store(self.kv.clone());
        // Start functions run without capabilities, just like the manifest probe.
        // Grant access only once instantiation succeeds, before activation.
        let instance = bindings::Plugin::instantiate(&mut store, &component, &self.linker)?;
        store.data_mut().id = manifest.id.clone();
        store.data_mut().permissions = manifest.permissions.clone();
        let mut plugin = LoadedPlugin {
            manifest: manifest.clone(),
            source: None,
            store,
            instance,
        };

        plugin.store.set_fuel(FUEL_PER_CALL)?;
        let activated = plugin.instance.call_activate(&mut plugin.store);
        let mut outbox = self.collect(&mut plugin);
        match activated {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                outbox.retain(|(sender, _, _)| sender == "host");
                self.deliver(&mut outbox);
                bail!("Plugin `{}` konnte nicht starten: {message}", manifest.id);
            }
            Err(trap) => {
                outbox.retain(|(sender, _, _)| sender == "host");
                self.deliver(&mut outbox);
                bail!(
                    "Plugin `{}` ist beim Start abgestürzt: {trap:#}",
                    manifest.id
                );
            }
        }

        self.plugins.insert(manifest.id.clone(), plugin);
        self.events.push(if reloading {
            HostEvent::Reloaded(manifest.clone())
        } else {
            HostEvent::Loaded(manifest.clone())
        });
        self.deliver(&mut outbox);
        Ok(manifest)
    }

    /// Inspect metadata and API compatibility without activation or cache writes.
    /// Component start/manifest code runs with fuel, no permissions, no profile
    /// backends and no inherited WASI resources. Its events/logs are discarded.
    pub fn inspect_bytes(&self, bytes: &[u8]) -> Result<Manifest> {
        let component = Component::new(&self.engine, bytes)?;
        self.inspect_component(&component)
    }

    fn inspect_component(&self, component: &Component) -> Result<Manifest> {
        let mut store = Store::new(
            &self.engine,
            PluginState::new(Arc::new(MemoryKv::default()), None),
        );
        store.set_fuel(FUEL_PER_CALL)?;
        let instance = bindings::Plugin::instantiate(&mut store, component, &self.linker)?;
        let manifest = Manifest::from(instance.call_manifest(&mut store)?);
        if manifest.id.is_empty() || manifest.id == "host" {
            bail!("Die Plugin-ID darf weder leer noch `host` sein");
        }
        if !api_compatible(&manifest.api) {
            bail!(
                "Plugin `{}` wurde für API {} gebaut, diese App spricht API {}",
                manifest.id,
                manifest.api,
                locora_api::API_VERSION
            );
        }

        Ok(manifest)
    }

    /// Unload a plugin. Its storage is kept in case it is loaded again.
    pub fn unload(&mut self, id: &str) -> bool {
        let Some(plugin) = self.plugins.remove(id) else {
            return false;
        };
        self.shutdown(plugin);
        self.events.push(HostEvent::Unloaded(id.to_owned()));
        true
    }

    /// Publish an event from the host (or a native component) to all plugins.
    pub fn publish<E: locora_api::Event>(&mut self, event: &E) -> Result<()> {
        self.publish_raw(E::TOPIC, locora_api::encode(event)?);
        Ok(())
    }

    /// Publish an already encoded event.
    pub fn publish_raw(&mut self, topic: &str, payload: Vec<u8>) {
        let mut queue = VecDeque::from([(String::from("host"), topic.to_owned(), payload)]);
        self.dispatch(&mut queue);
    }

    /// Call service `S` on the plugin that provides it.
    pub fn call<S: locora_api::Service>(&mut self, request: &S::Request) -> Result<S::Response> {
        let bytes = self.call_raw(S::NAME, &locora_api::encode(request)?)?;
        Ok(locora_api::decode(&bytes)?)
    }

    /// Call a particular provider, never falling back to a competing plugin.
    pub fn call_plugin<S: locora_api::Service>(
        &mut self,
        plugin: &str,
        request: &S::Request,
    ) -> Result<S::Response> {
        let bytes = self.call_plugin_raw(plugin, S::NAME, &locora_api::encode(request)?)?;
        Ok(locora_api::decode(&bytes)?)
    }

    /// Whether this specific active plugin provides `service`.
    pub fn plugin_provides(&self, plugin: &str, service: &str) -> bool {
        self.plugins
            .get(plugin)
            .is_some_and(|p| p.store.data().provided.iter().any(|s| s == service))
    }

    /// Whether some active plugin provides `service`.
    pub fn provides(&self, service: &str) -> bool {
        (service == <locora_api::palaces::Palaces as locora_api::Service>::NAME
            && self.palaces.is_some())
            || self
                .plugins
                .values()
                .any(|p| p.store.data().provided.iter().any(|s| s == service))
    }

    /// Call an already encoded service request.
    pub fn call_raw(&mut self, service: &str, request: &[u8]) -> Result<Vec<u8>> {
        // Native callers are trusted. Guest calls always go through PluginState::call_host.
        if service == <locora_api::palaces::Palaces as locora_api::Service>::NAME {
            let store = self
                .palaces
                .as_ref()
                .context("Kein Palastspeicher angebunden")?;
            let outcome =
                palaces::call(store, locora_api::decode(request)?).map_err(anyhow::Error::msg)?;
            let bytes = locora_api::encode(&outcome.response)?;
            let mut queue = outcome
                .events
                .into_iter()
                .map(|(topic, payload)| (String::from("host"), topic, payload))
                .collect();
            self.deliver(&mut queue);
            return Ok(bytes);
        }
        let Some(id) = self
            .plugins
            .iter()
            .find(|(_, p)| p.store.data().provided.iter().any(|s| s == service))
            .map(|(id, _)| id.clone())
        else {
            bail!("Kein aktives Plugin bietet den Dienst `{service}` an");
        };
        self.call_plugin_raw(&id, service, request)
    }

    fn call_plugin_raw(&mut self, id: &str, service: &str, request: &[u8]) -> Result<Vec<u8>> {
        if !self.plugin_provides(id, service) {
            bail!("Plugin `{id}` bietet den Dienst `{service}` nicht an");
        }
        let id = id.to_owned();
        let mut plugin = self.plugins.remove(&id).expect("checked above");
        let _ = plugin.store.set_fuel(FUEL_PER_CALL);
        let result = plugin
            .instance
            .call_on_call(&mut plugin.store, service, request);
        let mut outbox = self.collect(&mut plugin);
        let response = match result {
            Ok(Ok(bytes)) => {
                self.plugins.insert(id, plugin);
                Ok(bytes)
            }
            Ok(Err(message)) => {
                self.plugins.insert(id.clone(), plugin);
                Err(anyhow::anyhow!("Dienst `{service}` von `{id}`: {message}"))
            }
            Err(trap) => {
                self.events.push(HostEvent::Disabled {
                    plugin: id.clone(),
                    reason: format!("{trap:#}"),
                });
                Err(anyhow::anyhow!(
                    "Plugin `{id}` ist beim Dienst `{service}` abgestürzt"
                ))
            }
        };
        self.deliver(&mut outbox);
        response
    }

    /// Apply a typed chain in ascending (priority, plugin id) order.
    /// Errors or invalid responses preserve the preceding value; traps disable only
    /// the failing plugin. Events are delivered after the entire chain completes.
    pub fn filter<H: locora_api::Hook>(&mut self, mut value: H::Value) -> Result<H::Value> {
        let mut filters: Vec<_> = self
            .plugins
            .iter()
            .filter_map(|(id, plugin)| {
                plugin
                    .store
                    .data()
                    .filters
                    .get(H::NAME)
                    .map(|priority| (*priority, id.clone()))
            })
            .collect();
        filters.sort();
        let mut queue = VecDeque::new();
        let mut input = locora_api::encode(&value)?;
        for (_, id) in filters {
            let Some(mut plugin) = self.plugins.remove(&id) else {
                continue;
            };
            let _ = plugin.store.set_fuel(FUEL_PER_CALL);
            let result = plugin
                .instance
                .call_on_filter(&mut plugin.store, H::NAME, &input);
            queue.extend(self.collect(&mut plugin));
            match result {
                Ok(response) => {
                    let response = response.and_then(|bytes| {
                        locora_api::decode::<H::Value>(&bytes)
                            .map(|next| (next, bytes))
                            .map_err(|e| format!("Ungültiger Filterwert: {e}"))
                    });
                    match response {
                        Ok((next, bytes)) => {
                            value = next;
                            input = bytes;
                        }
                        Err(message) => self.events.push(HostEvent::HandlerError {
                            plugin: id.clone(),
                            topic: H::NAME.into(),
                            message,
                        }),
                    }
                    self.plugins.insert(id, plugin);
                }
                Err(trap) => self.events.push(HostEvent::Disabled {
                    plugin: id,
                    reason: format!("{trap:#}"),
                }),
            }
        }
        self.deliver(&mut queue);
        Ok(value)
    }

    /// Manifests of all active plugins, ordered by id.
    pub fn plugins(&self) -> impl Iterator<Item = &Manifest> {
        self.plugins.values().map(|p| &p.manifest)
    }

    /// File a plugin was loaded from, if any.
    pub fn source(&self, id: &str) -> Option<&Path> {
        self.plugins.get(id)?.source.as_deref()
    }

    /// Take everything that happened since the last call.
    pub fn drain_events(&mut self) -> Vec<HostEvent> {
        std::mem::take(&mut self.events)
    }

    fn new_store(&self, kv: Arc<dyn KvStore>) -> Store<PluginState> {
        let mut store = Store::new(&self.engine, PluginState::new(kv, self.palaces.clone()));
        // Instantiation runs the component's start code, which also costs fuel.
        store
            .set_fuel(FUEL_PER_CALL)
            .expect("fuel is enabled in the engine config");
        store
    }

    /// Compile a component, using the on-disk cache when possible.
    fn compile(&self, bytes: &[u8]) -> Result<Component> {
        let Some(dir) = &self.cache_dir else {
            return Ok(Component::new(&self.engine, bytes)?);
        };
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        // Compiled code is only valid for this exact engine build and config.
        let mut compat = std::hash::DefaultHasher::new();
        std::hash::Hash::hash(&self.engine.precompile_compatibility_hash(), &mut compat);
        hasher.update(std::hash::Hasher::finish(&compat).to_le_bytes());
        let key: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let path = dir.join(format!("{key}.cwasm"));
        if path.exists() {
            // SAFETY: the file was written by `Component::serialize` of this
            // engine configuration (the key includes the compatibility hash)
            // into the host's private cache directory.
            if let Ok(component) = unsafe { Component::deserialize_file(&self.engine, &path) } {
                return Ok(component);
            }
        }
        let component = Component::new(&self.engine, bytes)?;
        if fs::create_dir_all(dir).is_ok() {
            let tmp = path.with_extension("tmp");
            if fs::write(&tmp, component.serialize()?).is_ok() {
                let _ = fs::rename(&tmp, &path);
            }
        }
        Ok(component)
    }

    /// Deactivate a plugin. Its storage stays in the kv backend.
    fn shutdown(&mut self, mut plugin: LoadedPlugin) {
        let _ = plugin.store.set_fuel(FUEL_PER_CALL);
        let _ = plugin.instance.call_deactivate(&mut plugin.store);
        let mut outbox = self.collect(&mut plugin);
        outbox.retain(|(sender, _, _)| sender == "host");
        self.deliver(&mut outbox);
    }

    /// Move logs into host events and return the plugin's published events.
    fn collect(&mut self, plugin: &mut LoadedPlugin) -> VecDeque<(String, String, Vec<u8>)> {
        let state = plugin.store.data_mut();
        for (level, message) in state.logs.drain(..) {
            self.events.push(HostEvent::Log {
                plugin: state.id.clone(),
                level,
                message,
            });
        }
        for step in state.highlights.drain(..) {
            self.events.push(HostEvent::Highlighted {
                plugin: state.id.clone(),
                step,
            });
        }
        state.outbox.drain(..).collect()
    }

    fn deliver(&mut self, queue: &mut VecDeque<(String, String, Vec<u8>)>) {
        self.dispatch(queue);
    }

    /// Deliver queued events breadth-first. Events published while handling
    /// an event are appended to the queue, so plugins never re-enter each other.
    fn dispatch(&mut self, queue: &mut VecDeque<(String, String, Vec<u8>)>) {
        let mut delivered = 0;
        while let Some((sender, topic, payload)) = queue.pop_front() {
            delivered += 1;
            if delivered > MAX_EVENTS_PER_DISPATCH {
                self.events.push(HostEvent::Log {
                    plugin: "host".into(),
                    level: Level::Error,
                    message: format!(
                        "Event-Kaskade nach {MAX_EVENTS_PER_DISPATCH} Events abgebrochen"
                    ),
                });
                return;
            }
            if sender != "host" {
                self.events.push(HostEvent::Published {
                    plugin: sender.clone(),
                    topic: topic.clone(),
                    payload: payload.clone(),
                });
            }
            let receivers: Vec<String> = self
                .plugins
                .iter()
                .filter(|(id, p)| **id != sender && p.store.data().subscriptions.contains(&topic))
                .map(|(id, _)| id.clone())
                .collect();
            for id in receivers {
                let Some(mut plugin) = self.plugins.remove(&id) else {
                    continue;
                };
                let _ = plugin.store.set_fuel(FUEL_PER_CALL);
                let result = plugin
                    .instance
                    .call_on_event(&mut plugin.store, &topic, &payload);
                queue.extend(self.collect(&mut plugin));
                match result {
                    Ok(Ok(())) => {
                        self.plugins.insert(id, plugin);
                    }
                    Ok(Err(message)) => {
                        self.events.push(HostEvent::HandlerError {
                            plugin: id.clone(),
                            topic: topic.clone(),
                            message,
                        });
                        self.plugins.insert(id, plugin);
                    }
                    Err(trap) => {
                        // A trapped instance is unusable; disable it (its data stays in kv).
                        self.events.push(HostEvent::Disabled {
                            plugin: id,
                            reason: format!("{trap:#}"),
                        });
                    }
                }
            }
        }
    }
}

/// Plugins built against API `major.minor` run on any host with the same
/// major version (or the same `0.minor` before 1.0).
fn api_compatible(plugin_api: &str) -> bool {
    let parse = |version: &str| {
        let parts: Vec<_> = version.split('.').collect();
        if !(2..=3).contains(&parts.len())
            || parts.iter().any(|part| {
                part.is_empty()
                    || !part.bytes().all(|c| c.is_ascii_digit())
                    || (part.len() > 1 && part.starts_with('0'))
                    || part.parse::<u64>().is_err()
            })
        {
            return None;
        }
        Some((parts[0].parse::<u64>().ok()?, parts[1].parse::<u64>().ok()?))
    };
    match (parse(plugin_api), parse(locora_api::API_VERSION)) {
        (Some((major, minor)), Some((host_major, host_minor))) => {
            major == host_major && (major != 0 || minor == host_minor)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_versions() {
        assert!(api_compatible("0.6"));
        assert!(api_compatible("0.6.123"));
        for malformed in [
            "",
            "0",
            "0.6.",
            "0.6.bad",
            "0.6.1.extra",
            "0.06",
            "0.6.01",
            "0.6.-1",
        ] {
            assert!(!api_compatible(malformed), "{malformed}");
        }
        assert!(!api_compatible("0.5"));
        assert!(!api_compatible("0.4"));
        assert!(!api_compatible("0.3"));
        assert!(!api_compatible("0.2"));
        assert!(!api_compatible("0.1"));
        assert!(!api_compatible("1.0"));
    }
}
