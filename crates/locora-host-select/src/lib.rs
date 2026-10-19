//! The plugin host of the current target, under the name `locora_host` that the runtime uses.
#[cfg(not(target_arch = "wasm32"))]
pub use locora_host::*;

#[cfg(target_arch = "wasm32")]
pub use locora_host_browser::*;
