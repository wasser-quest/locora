//! The Locora core for the browser. The host (JavaScript) calls `invoke` with a command name and
//! JSON arguments and gets JSON back: `{"ok": ...}` or `{"err": "..."}`. Data lives in the folder the
//! host preopens for WASI (`/locora`); nothing leaves the device. The commands are in commands.rs.
//!
//! Memory protocol: the host reserves input bytes with `alloc`, writes them, calls `invoke`, reads
//! the result from the returned (pointer, length) pair and frees it with `dealloc`.

mod cloud;
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
    /// The store, for the plugins' calls back into the core. Those calls arrive while a command is
    /// running and the backend is borrowed, so they use this handle instead.
    static STORE: RefCell<Option<std::sync::Arc<locora_store::Store>>> = const { RefCell::new(None) };
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
        #[link_name = "filter"]
        fn host_filter(hook: *const u8, hook_len: usize, value: *const u8, value_len: usize) -> u64;
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

        fn filter(&self, hook: &str, value: &[u8]) -> Result<Vec<u8>, String> {
            let packed = unsafe { host_filter(hook.as_ptr(), hook.len(), value.as_ptr(), value.len()) };
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
    let store = profile.store.clone();
    STORE.with(|cell| *cell.borrow_mut() = Some(store));
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

pub(crate) fn with_backend<T>(f: impl FnOnce(&mut Backend) -> anyhow::Result<T>) -> anyhow::Result<T> {
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
        // The cloud commands take the backend themselves, and may wait for the page's calls.
        if command.starts_with("cloud_") {
            return Ok((cloud::command(command, &args)?, serde_json::json!([])));
        }
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

/// The output of a call from a plugin: one tag byte (0 = ok, 1 = error), then the bytes or the message.
#[cfg(target_arch = "wasm32")]
fn tagged(result: Result<Vec<u8>, String>) -> u64 {
    let mut output = match result {
        Ok(bytes) => std::iter::once(0).chain(bytes).collect::<Vec<u8>>(),
        Err(message) => std::iter::once(1).chain(message.into_bytes()).collect(),
    };
    output.shrink_to_fit();
    let (pointer, len) = (output.as_mut_ptr(), output.len());
    std::mem::forget(output);
    ((pointer as u64) << 32) | len as u64
}

/// # Safety
/// The pair must describe bytes the host wrote with `alloc`.
#[cfg(target_arch = "wasm32")]
unsafe fn input(pointer: *const u8, len: usize) -> Vec<u8> {
    unsafe { std::slice::from_raw_parts(pointer, len) }.to_vec()
}

/// # Safety
/// The pair must describe UTF-8 the host wrote with `alloc`.
#[cfg(target_arch = "wasm32")]
unsafe fn text(pointer: *const u8, len: usize) -> String {
    String::from_utf8_lossy(&unsafe { input(pointer, len) }).into_owned()
}

/// A plugin's palace read (`core.palaces@1`), answered by the store while the core is busy.
/// # Safety
/// The pair must describe a postcard request the host wrote with `alloc`.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn palace_service(pointer: *const u8, len: usize) -> u64 {
    let request = unsafe { input(pointer, len) };
    let store = STORE.with(|cell| cell.borrow().clone());
    tagged(match store {
        Some(store) => locora_host::nested_palace(&store, &request),
        None => Err("the palace service is not bound".to_owned()),
    })
}

/// A tour step a plugin shows (`ui.highlight@1`).
/// # Safety
/// Both pairs must describe bytes the host wrote with `alloc`.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plugin_highlight(
    plugin_pointer: *const u8,
    plugin_len: usize,
    request_pointer: *const u8,
    request_len: usize,
) -> u64 {
    let plugin = unsafe { text(plugin_pointer, plugin_len) };
    let request = unsafe { input(request_pointer, request_len) };
    tagged(locora_host::nested_highlight(&plugin, &request))
}

/// An event a plugin published (its UI declarations reach the core this way).
/// # Safety
/// The three pairs must describe bytes the host wrote with `alloc`.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plugin_publish(
    plugin_pointer: *const u8,
    plugin_len: usize,
    topic_pointer: *const u8,
    topic_len: usize,
    payload_pointer: *const u8,
    payload_len: usize,
) -> u64 {
    let plugin = unsafe { text(plugin_pointer, plugin_len) };
    let topic = unsafe { text(topic_pointer, topic_len) };
    let payload = unsafe { input(payload_pointer, payload_len) };
    locora_host::nested_publish(&plugin, &topic, payload);
    tagged(Ok(Vec::new()))
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

/// The page's reply to the call of a cloud command: the status, and the body it read.
///
/// # Safety
/// The pair must describe bytes the host wrote with `alloc`; the host frees them after the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cloud_reply(status: u32, pointer: *const u8, length: usize) -> u64 {
    let body = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
    let output = match cloud::reply(status, body) {
        Ok(value) => serde_json::json!({ "ok": value, "events": [] }).to_string(),
        Err(error) => serde_json::json!({ "err": format!("{error:#}") }).to_string(),
    };
    pack_bytes(output.into_bytes())
}

/// The body of the call the cloud command last asked for (empty when it had none).
#[unsafe(no_mangle)]
pub extern "C" fn cloud_body() -> u64 {
    pack_bytes(cloud::body())
}

/// Hands bytes to the host as a packed (pointer, length) pair; the host frees them with `dealloc`.
fn pack_bytes(mut output: Vec<u8>) -> u64 {
    output.shrink_to_fit();
    let (pointer, len) = (output.as_mut_ptr(), output.len());
    std::mem::forget(output);
    ((pointer as u64) << 32) | len as u64
}

