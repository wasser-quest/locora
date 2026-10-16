//! Showing media: pictures, videos and `.sply` animations. A `.sply` plays in a
//! sandboxed frame (scripts only, no access to the app); state changes reach
//! it as messages (see `src-tauri/src/sply.rs`).

use leptos::html::Iframe;
use leptos::prelude::*;
use locora_app_api::{Media, MediaKind};

use crate::tauri::url;

fn send(frame: &web_sys::HtmlIFrameElement, machine: &str, event: &str) {
    let Some(window) = frame.content_window() else {
        return;
    };
    let message = js_sys::Object::new();
    let set = |key: &str, value: wasm_bindgen::JsValue| {
        let _ = js_sys::Reflect::set(&message, &key.into(), &value);
    };
    set("locora", 1.into());
    set("machine", machine.into());
    set("send", event.into());
    let _ = window.post_message(&message, "*");
}

/// A `.sply` animation. With `state`, the event is sent to the state machine
/// `machine` whenever it changes (and once the frame has loaded).
#[component]
pub fn Sply(
    media: Media,
    #[prop(optional, into)] class: String,
    #[prop(optional, into)] state: Option<Signal<&'static str>>,
    #[prop(default = "status")] machine: &'static str,
) -> impl IntoView {
    let frame = NodeRef::<Iframe>::new();
    let push = move || {
        if let (Some(state), Some(el)) = (state, frame.get_untracked()) {
            send(&el, machine, state.get_untracked());
        }
    };
    Effect::new(move |_| {
        if let Some(state) = state {
            state.track();
        }
        push();
    });
    view! {
        <iframe
            node_ref=frame
            class=format!("sply {class}")
            sandbox="allow-scripts"
            src=url(&media)
            title="Animation"
            on:load=move |_| push()
        ></iframe>
    }
}

/// Any stored media, filling its box.
#[component]
pub fn Shown(media: Media, #[prop(optional, into)] class: String) -> impl IntoView {
    match media.kind {
        MediaKind::Sply => view! { <Sply media=media class=class/> }.into_any(),
        MediaKind::Image => view! { <img class=class src=url(&media) alt=""/> }.into_any(),
        MediaKind::Video => {
            view! { <video class=class src=url(&media) controls=true muted=true></video> }
                .into_any()
        }
    }
}
