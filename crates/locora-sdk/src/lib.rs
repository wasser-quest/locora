//! Write Locora plugins in plain Rust.
//!
//! ```ignore
//! use locora_sdk::prelude::*;
//!
//! #[locora_plugin(name = "Lernzeit", permissions = [storage])]
//! struct Lernzeit;
//!
//! impl Plugin for Lernzeit {
//!     fn activate(ctx: &mut Ctx) -> Result {
//!         ctx.on::<JourneyCompleted>(|ctx, ev| {
//!             let minutes = ctx.store().add("minutes", ev.duration_secs / 60)?;
//!             ctx.tile("Lernzeit", format!("{minutes} min"))
//!         })
//!     }
//! }
//! ```
//!
//! Build with `locora plugin dev`; it compiles the crate to a WebAssembly
//! component and hot-reloads it into the running app.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

pub use locora_api as api;
pub use locora_api::{Event, Hook, Service};
pub use locora_sdk_macros::locora_plugin;
use serde::{Serialize, de::DeserializeOwned};

#[doc(hidden)]
pub mod __bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "plugin",
        pub_export_macro: true,
        export_macro_name: "__export_plugin",
        default_bindings_module: "locora_sdk::__bindings",
    });
}

use __bindings::locora::plugin::{host, types};

/// Everything a plugin usually needs.
pub mod prelude {
    pub use crate::api::core::{
        Grade, JourneyCompleted, LocusAnswered, LocusDeleted, PalaceCreated, PalaceDeleted,
    };
    pub use crate::api::core::{JourneyScore, Score};
    pub use crate::api::host::Started;
    pub use crate::api::journey::{
        Answer, AnswerKind, Evaluate, EvaluateRequest, Evaluation, QuizOption, Scored, Submission,
        normalize,
    };
    pub use crate::api::palaces;
    pub use crate::{Ctx, Error, Event, Hook, Plugin, Result, Service, Store, locora_plugin};
}

/// Error returned by plugin code. Any standard error converts into it with `?`.
pub struct Error(String);

impl Error {
    /// Create an error from a message.
    pub fn msg(message: impl fmt::Display) -> Self {
        Self(message.to_string())
    }
}

impl<E: std::error::Error> From<E> for Error {
    fn from(err: E) -> Self {
        Self(err.to_string())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Result type for plugin code.
pub type Result<T = (), E = Error> = std::result::Result<T, E>;

/// A Locora plugin. Mark the type with [`locora_plugin`] to export it.
pub trait Plugin {
    /// Called once after loading. Register event handlers here.
    fn activate(ctx: &mut Ctx) -> Result;

    /// Called before unloading or hot-reloading.
    fn deactivate(_ctx: &mut Ctx) {}
}

type Handler = Rc<RefCell<dyn FnMut(&mut Ctx, &[u8]) -> Result>>;
type ServiceHandler = Rc<RefCell<dyn FnMut(&mut Ctx, &[u8]) -> Result<Vec<u8>>>>;

thread_local! {
    static HANDLERS: RefCell<HashMap<String, Vec<Handler>>> = RefCell::new(HashMap::new());
    static SERVICES: RefCell<HashMap<String, ServiceHandler>> = RefCell::new(HashMap::new());
    static FILTERS: RefCell<HashMap<String, ServiceHandler>> = RefCell::new(HashMap::new());
}

/// Access to the host: events, storage, logging and UI.
pub struct Ctx {
    _private: (),
}

impl Ctx {
    /// Run `handler` whenever an event of type `E` is published.
    pub fn on<E: Event + 'static>(
        &mut self,
        mut handler: impl FnMut(&mut Ctx, E) -> Result + 'static,
    ) -> Result {
        let erased: Handler = Rc::new(RefCell::new(move |ctx: &mut Ctx, bytes: &[u8]| {
            let event = api::decode::<E>(bytes)
                .map_err(|e| Error::msg(format!("cannot decode `{}`: {e}", E::TOPIC)))?;
            handler(ctx, event)
        }));
        let first = HANDLERS.with_borrow_mut(|handlers| {
            let list = handlers.entry(E::TOPIC.to_owned()).or_default();
            list.push(erased);
            list.len() == 1
        });
        if first {
            host::subscribe(E::TOPIC);
        }
        Ok(())
    }

    /// Answer calls to service `S` (one provider per service).
    pub fn provide<S: Service + 'static>(
        &mut self,
        mut handler: impl FnMut(&mut Ctx, S::Request) -> Result<S::Response> + 'static,
    ) -> Result {
        let erased: ServiceHandler = Rc::new(RefCell::new(move |ctx: &mut Ctx, bytes: &[u8]| {
            let request = api::decode::<S::Request>(bytes)
                .map_err(|e| Error::msg(format!("cannot decode request for `{}`: {e}", S::NAME)))?;
            Ok(api::encode(&handler(ctx, request)?)?)
        }));
        SERVICES.with_borrow_mut(|services| services.insert(S::NAME.to_owned(), erased));
        host::provide(S::NAME);
        Ok(())
    }

    /// Register one filter for `H`. Lower priorities run first, ties use plugin id.
    /// The native host drives chains; plugins are never synchronously re-entered.
    pub fn filter<H: Hook + 'static>(
        &mut self,
        priority: i32,
        mut handler: impl FnMut(&mut Ctx, H::Value) -> Result<H::Value> + 'static,
    ) -> Result {
        host::register_filter(H::NAME, priority).map_err(Error::msg)?;
        let erased: ServiceHandler = Rc::new(RefCell::new(move |ctx: &mut Ctx, bytes: &[u8]| {
            let value = api::decode::<H::Value>(bytes)?;
            Ok(api::encode(&handler(ctx, value)?)?)
        }));
        FILTERS.with_borrow_mut(|filters| filters.insert(H::NAME.to_owned(), erased));
        Ok(())
    }

    /// Call a native host service. This does not route calls to other plugins.
    pub fn call_host<S: Service>(&mut self, request: &S::Request) -> Result<S::Response> {
        let bytes = host::call_host(S::NAME, &api::encode(request)?).map_err(Error::msg)?;
        Ok(api::decode(&bytes)?)
    }

    /// Access the palace service; the host checks read/write rights per operation.
    pub fn palaces(&mut self, request: &api::palaces::Request) -> Result<api::palaces::Response> {
        self.call_host::<api::palaces::Palaces>(request)
    }

    /// Publish an event to all subscribed plugins.
    pub fn emit<E: Event>(&mut self, event: &E) -> Result {
        host::publish(E::TOPIC, &api::encode(event)?);
        Ok(())
    }

    /// Plugin-private storage. Requires the `storage` permission.
    pub fn store(&mut self) -> Store {
        Store { _private: () }
    }

    /// Show or update this plugin's tile on the home screen.
    pub fn tile(&mut self, title: impl Into<String>, text: impl Into<String>) -> Result {
        self.emit(&api::ui::Tile {
            title: title.into(),
            text: text.into(),
        })
    }

    /// Declare a card the user can place on any page. Declaring the same id again replaces it.
    /// Ids: 1–48 lowercase letters, digits, `-` or `_`; at most 24 blocks per plugin.
    pub fn block(
        &mut self,
        id: impl Into<String>,
        title: impl Into<String>,
        text: impl Into<String>,
    ) -> Result {
        self.emit(&api::ui::Block {
            id: id.into(),
            title: title.into(),
            text: text.into(),
        })
    }

    /// Suggest a page: the user adds it in one step, with these blocks of this plugin.
    /// At most 8 pages per plugin.
    pub fn page(&mut self, id: impl Into<String>, name: impl Into<String>, blocks: &[&str]) -> Result {
        self.emit(&api::ui::Page {
            id: id.into(),
            name: name.into(),
            blocks: blocks.iter().map(|block| (*block).to_owned()).collect(),
        })
    }

    /// Remove one of this plugin's blocks or pages.
    pub fn remove_ui(&mut self, id: impl Into<String>) -> Result {
        self.emit(&api::ui::Remove { id: id.into() })
    }

    pub fn debug(&mut self, message: impl AsRef<str>) {
        host::log(types::Level::Debug, message.as_ref());
    }

    pub fn info(&mut self, message: impl AsRef<str>) {
        host::log(types::Level::Info, message.as_ref());
    }

    pub fn warn(&mut self, message: impl AsRef<str>) {
        host::log(types::Level::Warn, message.as_ref());
    }

    pub fn error(&mut self, message: impl AsRef<str>) {
        host::log(types::Level::Error, message.as_ref());
    }
}

/// Plugin-private key-value storage with typed values.
pub struct Store {
    _private: (),
}

impl Store {
    /// Read a value, or `None` if the key was never written.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match host::kv_get(key).map_err(Error::msg)? {
            Some(bytes) => Ok(Some(api::decode(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Write a value.
    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> Result {
        host::kv_set(key, &api::encode(value)?).map_err(Error::msg)
    }

    /// Add `amount` to a counter and return the new total.
    pub fn add(&self, key: &str, amount: u64) -> Result<u64> {
        let total = self.get::<u64>(key)?.unwrap_or(0).saturating_add(amount);
        self.set(key, &total)?;
        Ok(total)
    }
}

#[doc(hidden)]
pub mod __rt {
    use super::*;

    pub fn manifest(id: &str, name: &str, version: &str, permissions: &[&str]) -> types::Manifest {
        types::Manifest {
            id: id.to_owned(),
            name: name.to_owned(),
            version: version.to_owned(),
            api: api::API_VERSION.to_owned(),
            permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    pub fn activate<P: Plugin>() -> std::result::Result<(), String> {
        P::activate(&mut Ctx { _private: () }).map_err(|e| e.0)
    }

    pub fn deactivate<P: Plugin>() {
        P::deactivate(&mut Ctx { _private: () });
        HANDLERS.with_borrow_mut(HashMap::clear);
        SERVICES.with_borrow_mut(HashMap::clear);
        FILTERS.with_borrow_mut(HashMap::clear);
    }

    pub fn call(service: &str, request: &[u8]) -> std::result::Result<Vec<u8>, String> {
        let handler = SERVICES
            .with_borrow(|s| s.get(service).cloned())
            .ok_or_else(|| format!("service `{service}` is not provided by this plugin"))?;
        let mut ctx = Ctx { _private: () };
        (handler.borrow_mut())(&mut ctx, request).map_err(|e| e.0)
    }

    pub fn filter(hook: &str, value: &[u8]) -> std::result::Result<Vec<u8>, String> {
        let handler = FILTERS
            .with_borrow(|filters| filters.get(hook).cloned())
            .ok_or_else(|| format!("Filter `{hook}` ist nicht registriert"))?;
        let mut ctx = Ctx { _private: () };
        (handler.borrow_mut())(&mut ctx, value).map_err(|e| e.0)
    }

    pub fn dispatch(topic: &str, payload: &[u8]) -> std::result::Result<(), String> {
        let handlers = HANDLERS.with_borrow(|h| h.get(topic).cloned().unwrap_or_default());
        let mut ctx = Ctx { _private: () };
        for handler in handlers {
            (handler.borrow_mut())(&mut ctx, payload).map_err(|e| e.0)?;
        }
        Ok(())
    }
}
