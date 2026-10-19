//! The Locora core for the browser. The host (JavaScript) calls `invoke` with a command name and
//! JSON arguments and gets JSON back: `{"ok": ...}` or `{"err": "..."}`. Data lives in the folder the
//! host preopens for WASI (`/locora`); nothing leaves the device.
//!
//! Memory protocol: the host reserves input bytes with `alloc`, writes them, calls `invoke`, reads
//! the result from the returned (pointer, length) pair and frees it with `dealloc`.

use std::cell::RefCell;
use std::path::Path;

use locora_app_core::journey::Journey;
use locora_app_core::views;
use locora_app_api::{JourneyStart, Media, MediaKind, Reply};
use locora_runtime::Profile;
use locora_store::SPLINELY;
use serde::Serialize;
use serde::de::DeserializeOwned;

struct Backend {
    profile: Profile,
    default_marker: String,
    simo: String,
    journey: Option<Journey>,
}

/// The arguments of `journey_action`, as the UI sends them.
#[derive(serde::Deserialize)]
struct ActionArgs {
    key: String,
    revealed: bool,
    after: Option<bool>,
}

fn parse<T: DeserializeOwned>(args: &str) -> anyhow::Result<T> {
    Ok(serde_json::from_str(args)?)
}

fn value<T: Serialize>(value: T) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::to_value(value)?)
}

/// The journey that is running (not finished): the commands that answer need one.
fn running(journey: Option<&mut Journey>) -> anyhow::Result<&mut Journey> {
    match journey {
        Some(j) if !j.done() => Ok(j),
        Some(_) => anyhow::bail!("the journey is finished"),
        None => anyhow::bail!("no journey is running"),
    }
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
    BACKEND.with(|cell| *cell.borrow_mut() = Some(Backend { profile, default_marker, simo, journey: None }));
    Ok(())
}

fn with_backend<T>(f: impl FnOnce(&mut Backend) -> anyhow::Result<T>) -> anyhow::Result<T> {
    BACKEND.with(|cell| match cell.borrow_mut().as_mut() {
        Some(backend) => f(backend),
        None => anyhow::bail!("the profile is not open"),
    })
}

fn home() -> anyhow::Result<serde_json::Value> {
    with_backend(|b| {
        let mut home = views::home(&mut b.profile)?;
        if home.palaces.is_empty() {
            home.welcome = Some(Media { hash: b.simo.clone(), kind: MediaKind::Sply });
        }
        Ok(serde_json::to_value(home)?)
    })
}

fn palace(id: &str) -> anyhow::Result<serde_json::Value> {
    with_backend(|b| {
        let marker = b.default_marker.clone();
        value(views::palace(&mut b.profile, id, &marker)?)
    })
}

fn tutorial(lang: &str) -> anyhow::Result<serde_json::Value> {
    with_backend(|b| {
        // Built once per language: an existing palace keeps its progress.
        if let Some(existing) = locora_runtime::tutorial::find_in(&b.profile, lang)? {
            return value(existing.id);
        }
        let (tree, _) = locora_runtime::tutorial::install_in(&mut b.profile, lang)?;
        value(tree.palace.id)
    })
}

fn formula_guide(lang: &str) -> anyhow::Result<serde_json::Value> {
    with_backend(|b| {
        if let Some(existing) = locora_runtime::formula_guide::find_in(&b.profile, lang)? {
            return value(existing.id);
        }
        let (tree, _) = locora_runtime::formula_guide::install_in(&mut b.profile, lang)?;
        value(tree.palace.id)
    })
}

fn journey_start(start: JourneyStart) -> anyhow::Result<serde_json::Value> {
    with_backend(|b| {
        if let Some(mut old) = b.journey.take() {
            old.finish(&mut b.profile)?;
        }
        match Journey::start(&mut b.profile, start)? {
            None => value(locora_app_api::Step::Done(Default::default())),
            Some(journey) => {
                let question = journey.question(&b.profile, &b.default_marker);
                b.journey = Some(journey);
                value(locora_app_api::Step::Question(question))
            }
        }
    })
}

fn journey_inputs(start: JourneyStart) -> anyhow::Result<serde_json::Value> {
    with_backend(|b| value(locora_app_core::journey::inputs(&b.profile, &start)?))
}

fn journey_action(args: ActionArgs) -> anyhow::Result<serde_json::Value> {
    with_backend(|b| {
        if args.after.unwrap_or(false) {
            let journey = b.journey.as_mut().ok_or_else(|| anyhow::anyhow!("no journey is running"))?;
            value(journey.action_after(&b.profile, &args.key, &b.default_marker)?)
        } else {
            value(running(b.journey.as_mut())?.action(&b.profile, &args.key, args.revealed, &b.default_marker)?)
        }
    })
}

fn journey_reveal() -> anyhow::Result<serde_json::Value> {
    with_backend(|b| {
        let revealed = running(b.journey.as_mut())?.reveal(&b.profile);
        value(revealed)
    })
}

fn journey_answer(reply: Reply) -> anyhow::Result<serde_json::Value> {
    with_backend(|b| {
        let feedback = running(b.journey.as_mut())?.answer(&mut b.profile, reply, &b.default_marker)?;
        value(feedback)
    })
}

fn dispatch(command: &str, args: &str) -> String {
    let result: anyhow::Result<serde_json::Value> = match command {
        "open" => {
            let dir: String = parse(args).unwrap_or_else(|_| "/locora".to_owned());
            open(&dir).map(|()| serde_json::Value::Null)
        }
        "home" => home(),
        "palace" => parse::<String>(args).and_then(|id| palace(&id)),
        "tutorial" => parse::<String>(args).and_then(|lang| tutorial(&lang)),
        "formula_guide" => parse::<String>(args).and_then(|lang| formula_guide(&lang)),
        "journey_start" => parse(args).and_then(journey_start),
        "journey_inputs" => parse(args).and_then(journey_inputs),
        "journey_action" => parse(args).and_then(journey_action),
        "journey_reveal" => journey_reveal(),
        "journey_answer" => parse(args).and_then(journey_answer),
        other => Err(anyhow::anyhow!("unknown command {other}")),
    };
    match result {
        Ok(value) => serde_json::json!({ "ok": value }).to_string(),
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
