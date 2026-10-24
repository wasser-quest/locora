//! Sharing: copying a text, and how this device shares (`ShareWays`, asked once per run).

use std::cell::Cell;

use locora_app_api::{CMD_SHARE_WAYS, ShareWay, ShareWays};
use wasm_bindgen::JsCast;

use crate::tauri::{NoArgs, call};

thread_local! {
    static WAYS: Cell<Option<ShareWays>> = const { Cell::new(None) };
}

/// How sharing works here; a device that cannot tell saves the picture and copies the text.
pub async fn ways() -> ShareWays {
    if let Some(ways) = WAYS.with(Cell::get) {
        return ways;
    }
    let ways = call::<ShareWays>(CMD_SHARE_WAYS, NoArgs {})
        .await
        .unwrap_or(ShareWays {
            card: ShareWay::Save,
            text_sheet: false,
        });
    WAYS.with(|cell| cell.set(Some(ways)));
    ways
}

/// Copies `text` to the clipboard and says whether it worked. The copy starts at once, before the
/// future is awaited, so it runs inside the click that asked for it (browsers allow it only there).
pub fn copy_text(text: &str) -> impl std::future::Future<Output = bool> + use<> {
    let started = web_sys::window()
        .and_then(|window| js_sys::Reflect::get(&window.navigator(), &"clipboard".into()).ok())
        .filter(|clipboard| !clipboard.is_undefined())
        .and_then(|clipboard| {
            let write = js_sys::Reflect::get(&clipboard, &"writeText".into()).ok()?;
            let write: js_sys::Function = write.dyn_into().ok()?;
            let promise = write.call1(&clipboard, &text.into()).ok()?;
            promise.dyn_into::<js_sys::Promise>().ok()
        });
    async move {
        match started {
            Some(promise) => wasm_bindgen_futures::JsFuture::from(promise).await.is_ok(),
            None => false,
        }
    }
}
