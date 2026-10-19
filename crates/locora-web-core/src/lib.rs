//! The Locora core for the browser. The host (JavaScript) calls `invoke` with a command name and
//! JSON arguments and gets JSON back: `{"ok": ...}` or `{"err": "..."}`. Data lives in the folder the
//! host preopens for WASI (`/locora`); nothing leaves the device. The commands are in commands.rs.
//!
//! Memory protocol: the host reserves input bytes with `alloc`, writes them, calls `invoke`, reads
//! the result from the returned (pointer, length) pair and frees it with `dealloc`.

mod commands;

use std::cell::RefCell;
use std::path::Path;

use locora_app_core::journey::Journey;
use locora_app_core::assets::Assets;
use locora_runtime::Profile;
use locora_store::SPLINELY;

pub struct Backend {
    profile: Profile,
    journey: Option<Journey>,
    /// Hash of the built-in locus marker in the store.
    default_marker: String,
    /// Hash of Simo (welcome and empty states) in the store.
    simo: String,
    /// The app's own graphics.
    assets: Assets,
}

thread_local! {
    static BACKEND: RefCell<Option<Backend>> = const { RefCell::new(None) };
}

/// The plugins of the browser host. The host provides the `locora_bridge` imports: `provides` says
/// whether a service exists, and `call` runs one and returns a packed (pointer, length) reply.
#[cfg(target_arch = "wasm32")]
mod plugins {
    use locora_host::ServiceBridge;

    // An empty plugin name means any plugin.
    #[link(wasm_import_module = "locora_bridge")]
    unsafe extern "C" {
        #[link_name = "provides"]
        fn host_provides(plugin: *const u8, plugin_len: usize, service: *const u8, service_len: usize) -> u32;
        #[link_name = "call"]
        fn host_call(
            plugin: *const u8,
            plugin_len: usize,
            service: *const u8,
            service_len: usize,
            request: *const u8,
            request_len: usize,
        ) -> u64;
    }

    pub struct Bridge;

    impl ServiceBridge for Bridge {
        fn provides(&self, plugin: Option<&str>, service: &str) -> bool {
            let plugin = plugin.unwrap_or("");
            unsafe { host_provides(plugin.as_ptr(), plugin.len(), service.as_ptr(), service.len()) == 1 }
        }

        fn call(&self, plugin: Option<&str>, service: &str, request: &[u8]) -> Result<Vec<u8>, String> {
            let plugin = plugin.unwrap_or("");
            let packed = unsafe {
                host_call(plugin.as_ptr(), plugin.len(), service.as_ptr(), service.len(), request.as_ptr(), request.len())
            };
            read_reply(packed)
        }
    }

    /// The reply is one tag byte (0 = ok, anything else = error), then the bytes or the message.
    /// The host reserved its memory with `alloc`, so it is taken back here.
    fn read_reply(packed: u64) -> Result<Vec<u8>, String> {
        let pointer = (packed >> 32) as *mut u8;
        let len = (packed & 0xffff_ffff) as usize;
        let reply = unsafe { Vec::from_raw_parts(pointer, len, len) };
        match reply.split_first() {
            Some((0, bytes)) => Ok(bytes.to_vec()),
            Some((_, message)) => Err(String::from_utf8_lossy(message).into_owned()),
            None => Err("the browser host sent no reply".to_owned()),
        }
    }
}

fn open(dir: &str) -> anyhow::Result<()> {
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_mut))]
    let mut profile = Profile::open(Some(Path::new(dir)))?;
    #[cfg(target_arch = "wasm32")]
    profile.host.set_bridge(Box::new(plugins::Bridge));
    let default_marker = profile.store.put_media(locora_runtime::tutorial::DEFAULT_MARKER, SPLINELY)?.hash;
    let simo = profile.store.put_media(locora_runtime::tutorial::SIMO, SPLINELY)?.hash;
    let assets = Assets::register(&profile.store)?;
    BACKEND.with(|cell| {
        *cell.borrow_mut() = Some(Backend {
            profile,
            journey: None,
            default_marker,
            simo,
            assets,
        })
    });
    Ok(())
}

fn with_backend<T>(f: impl FnOnce(&mut Backend) -> anyhow::Result<T>) -> anyhow::Result<T> {
    BACKEND.with(|cell| match cell.borrow_mut().as_mut() {
        Some(backend) => f(backend),
        None => anyhow::bail!("the profile is not open"),
    })
}

/// The events a command published, as topics and payload bytes for the page's plugin host.
#[cfg(target_arch = "wasm32")]
fn published(profile: &mut Profile) -> serde_json::Value {
    profile
        .host
        .take_events()
        .into_iter()
        .filter_map(|event| match event {
            locora_host::HostEvent::Published { topic, payload, .. } => {
                Some(serde_json::json!({ "topic": topic, "payload": payload }))
            }
            _ => None,
        })
        .collect()
}

/// Native builds have no browser plugins, so nothing is published to them.
#[cfg(not(target_arch = "wasm32"))]
fn published(_profile: &mut Profile) -> serde_json::Value {
    serde_json::json!([])
}

fn dispatch(command: &str, args: &str) -> String {
    let result: anyhow::Result<(serde_json::Value, serde_json::Value)> = (|| {
        if command == "open" {
            let dir: String = serde_json::from_str(args).unwrap_or_else(|_| "/locora".to_owned());
            open(&dir)?;
            return Ok((serde_json::Value::Null, serde_json::json!([])));
        }
        if !commands::is_known(command) {
            anyhow::bail!("unknown command {command}");
        }
        let args: serde_json::Value = if args.trim().is_empty() {
            serde_json::Value::Object(Default::default())
        } else {
            serde_json::from_str(args)?
        };
        // Events are taken in every case: the page delivers them to the plugins after the command.
        with_backend(|b| {
            let value = commands::run(b, command, args);
            let events = published(&mut b.profile);
            value.map(|value| (value, events))
        })
    })();
    match result {
        Ok((value, events)) => serde_json::json!({ "ok": value, "events": events }).to_string(),
        Err(error) => serde_json::json!({ "err": format!("{error:#}") }).to_string(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let pointer = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    pointer
}

/// # Safety
/// `pointer` and `len` must come from `alloc` (or `invoke`) and be freed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(pointer: *mut u8, len: usize) {
    unsafe { drop(Vec::from_raw_parts(pointer, 0, len)) };
}

/// # Safety
/// The two (pointer, length) pairs must describe valid UTF-8 bytes written by the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn invoke(command_ptr: *const u8, command_len: usize, args_ptr: *const u8, args_len: usize) -> u64 {
    let command = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(command_ptr, command_len)) };
    let args = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(args_ptr, args_len)) };
    let mut output = dispatch(command, args).into_bytes();
    output.shrink_to_fit();
    let (pointer, len) = (output.as_mut_ptr(), output.len());
    std::mem::forget(output);
    ((pointer as u64) << 32) | len as u64
}
