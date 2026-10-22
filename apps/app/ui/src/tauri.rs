//! Calls into the Tauri backend and `locora://` URLs for media.

use locora_app_api::{Media, SCHEME};
use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::prelude::*;
use web_sys::Url;

use crate::i18n::{t, translate_message};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = convertFileSrc)]
    fn convert_file_src(path: &str, protocol: &str) -> String;
}

/// No arguments.
#[derive(Serialize)]
pub struct NoArgs {}

fn message(value: JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}

/// Call a backend command; errors are messages for people, in the current language.
pub async fn call<T: DeserializeOwned>(cmd: &str, args: impl Serialize) -> Result<T, String> {
    let args = args
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| e.to_string())?;
    let value = invoke(cmd, args)
        .await
        .map_err(|error| translate_message(&message(error)))?;
    serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())
}

/// A native drop exposes only a short-lived ticket. No path is trusted from JS.
/// Registers for the current component lifetime.
pub fn on_file_drop(callback: impl Fn(Result<String, String>) + 'static) {
    use leptos::prelude::*;
    let listener = window_event_listener_untyped("locora-file-drop", move |event| {
        let detail = js_sys::Reflect::get(&event, &"detail".into()).unwrap_or(JsValue::NULL);
        let field = |key: &str| {
            js_sys::Reflect::get(&detail, &key.into())
                .ok()
                .and_then(|v| v.as_string())
        };
        callback(match (field("ticket"), field("error")) {
            (_, Some(error)) => Err(translate_message(&error)),
            (Some(ticket), _) => Ok(ticket),
            _ => Err(t("Ungültige Dateifreigabe").into()),
        });
    });
    on_cleanup(move || listener.remove());
}

/// URL of a stored file (or a `.sply` as interactive SVG).
pub fn url(media: &Media) -> String {
    convert_file_src(&media.path(), SCHEME)
}

// GStreamer in WebKitGTK rejects custom schemes (`locora://`) as media
// sources; videos play from blob URLs fetched over the custom scheme instead.
// Byte ranges stay in the WebView: a blob needs no backend round-trip.
thread_local! {
    /// The video on stage as (media hash, blob URL). A new question re-mounts the player;
    /// reusing this URL avoids fetching the whole file again, which made the video reload.
    static LAST_VIDEO: std::cell::RefCell<Option<(String, String)>> = const { std::cell::RefCell::new(None) };
}

/// The stage's video URL when that video is already loaded; no fetch is needed.
pub fn cached_video_url(media: &Media) -> Option<String> {
    LAST_VIDEO.with(|last| {
        last.borrow()
            .as_ref()
            .filter(|(hash, _)| *hash == media.hash)
            .map(|(_, object)| object.clone())
    })
}

pub async fn video_url(media: &Media) -> Result<String, String> {
    if let Some(object) = cached_video_url(media) {
        return Ok(object);
    }
    let object = object_url(media, t("Video konnte nicht gelesen werden")).await?;
    // Another question for the same video may have fetched it meanwhile; keep that URL.
    let kept = LAST_VIDEO.with(|last| {
        let mut last = last.borrow_mut();
        if let Some((hash, cached)) = last.as_ref()
            && *hash == media.hash
        {
            let cached = cached.clone();
            revoke_object_url(&object);
            return cached;
        }
        if let Some((_, previous)) = last.replace((media.hash.clone(), object.clone())) {
            revoke_object_url(&previous);
        }
        object.clone()
    });
    Ok(kept)
}

/// A stored sound clip as a blob URL, for `<audio>`: like videos, WebKitGTK's media player
/// refuses the custom scheme. The caller revokes the URL when its element goes away.
pub async fn audio_url(media: &Media) -> Result<String, String> {
    object_url(media, t("Ton konnte nicht gelesen werden")).await
}

/// The stored bytes behind `media`, fetched over the custom scheme, as a new blob URL.
async fn object_url(media: &Media, read: &str) -> Result<String, String> {
    // Plain `fetch` needs CORS headers the custom scheme never sends; the
    // request still reaches the handler and dies on the response, so use
    // XMLHttpRequest, which WebKitGTK allows for custom schemes.
    let request = web_sys::XmlHttpRequest::new().map_err(|_| read.to_string())?;
    request
        .open_with_async("GET", &url(media), true)
        .map_err(|_| read.to_string())?;
    request.set_response_type(web_sys::XmlHttpRequestResponseType::Blob);
    let blob = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::new(
        &mut move |resolve: js_sys::Function, reject: js_sys::Function| {
            let done = wasm_bindgen::closure::Closure::once({
                let request = request.clone();
                let reject = reject.clone();
                move |_event: web_sys::Event| {
                    let response = request.response().unwrap_or_default();
                    if request.status() == Ok(200)
                        && response.clone().dyn_into::<web_sys::Blob>().is_ok()
                    {
                        let _ = resolve.call1(&wasm_bindgen::JsValue::NULL, &response);
                    } else {
                        let _ = reject
                            .call1(&wasm_bindgen::JsValue::NULL, &wasm_bindgen::JsValue::NULL);
                    }
                }
            });
            request.set_onload(Some(done.as_ref().unchecked_ref()));
            let failed = wasm_bindgen::closure::Closure::once({
                let reject = reject.clone();
                move |_event: web_sys::Event| {
                    let _ =
                        reject.call1(&wasm_bindgen::JsValue::NULL, &wasm_bindgen::JsValue::NULL);
                }
            });
            request.set_onerror(Some(failed.as_ref().unchecked_ref()));
            request.set_onabort(Some(failed.as_ref().unchecked_ref()));
            let sent = request.send();
            if sent.is_err() {
                let _ = reject.call1(&wasm_bindgen::JsValue::NULL, &wasm_bindgen::JsValue::NULL);
            }
            // Keep the load/error/abort closures alive until the promise settles.
            done.forget();
            failed.forget();
        },
    ))
    .await
    .map_err(|_| read.to_string())?;
    let blob: web_sys::Blob = blob.dyn_into().map_err(|_| read.to_string())?;
    let object = Url::create_object_url_with_blob(&blob).map_err(|_| read.to_string())?;
    BLOBS.with(|blobs| blobs.borrow_mut().insert(object.clone(), blob));
    Ok(object)
}

thread_local! {
    /// The blobs behind the live blob URLs, so a player that fails can read the file's format
    /// (`media_format`) without fetching the URL, which the app's content policy does not allow.
    static BLOBS: std::cell::RefCell<std::collections::HashMap<String, web_sys::Blob>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The blob behind a live blob URL made by `object_url`.
pub fn blob_of(object: &str) -> Option<web_sys::Blob> {
    BLOBS.with(|blobs| blobs.borrow().get(object).cloned())
}

/// Release a blob URL once no element uses it anymore.
pub fn revoke_object_url(object: &str) {
    BLOBS.with(|blobs| blobs.borrow_mut().remove(object));
    let _ = Url::revoke_object_url(object);
}
