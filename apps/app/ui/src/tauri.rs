//! Calls into the Tauri backend and `locora://` URLs for media.

use locora_app_api::{Media, SCHEME};
use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::prelude::*;

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

/// Call a backend command; errors are messages for people.
pub async fn call<T: DeserializeOwned>(cmd: &str, args: impl Serialize) -> Result<T, String> {
    let args = args
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| e.to_string())?;
    let value = invoke(cmd, args).await.map_err(message)?;
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
            (_, Some(error)) => Err(error),
            (Some(ticket), _) => Ok(ticket),
            _ => Err("Ungültige Dateifreigabe".into()),
        });
    });
    on_cleanup(move || listener.remove());
}

pub fn marker_url(media: &Media) -> String {
    convert_file_src(&format!("markers/{}", media.hash), SCHEME)
}

/// URL of a stored file (or a `.sply` as interactive SVG).
pub fn url(media: &Media) -> String {
    convert_file_src(&media.path(), SCHEME)
}
