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

/// URL of a stored file (or a `.sply` as interactive SVG).
pub fn url(media: &Media) -> String {
    convert_file_src(&media.path(), SCHEME)
}
