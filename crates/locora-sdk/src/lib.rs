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
pub use locora_api::Event;
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
    pub use crate::api::core::{Grade, JourneyCompleted, LocusAnswered, PalaceCreated};
    pub use crate::api::host::Started;
    pub use crate::{Ctx, Error, Event, Plugin, Result, Store, locora_plugin};
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

thread_local! {
    static HANDLERS: RefCell<HashMap<String, Vec<Handler>>> = RefCell::new(HashMap::new());
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
