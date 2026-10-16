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
    let ready = RwSignal::new(false);
    let failed = RwSignal::new(false);
    let messages = window_event_listener(leptos::ev::message, move |event| {
        let Some(window) = frame.get_untracked().and_then(|f| f.content_window()) else {
            return;
        };
        // Sandboxed frames have opaque origins: authenticate by source window.
        let source = js_sys::Reflect::get(event.as_ref(), &"source".into()).ok();
        if source != Some(window.into()) {
            return;
        }
        let data = event.data();
        let get = |key: &str| js_sys::Reflect::get(&data, &key.into()).ok();
        if get("locora").and_then(|v| v.as_f64()) != Some(1.0) {
            return;
        }
        if let Some(value) = get("runtimeReady").and_then(|v| v.as_bool()) {
            ready.set(value);
            failed.set(!value);
        }
    });
    let pointer = window_event_listener(leptos::ev::pointermove, move |event| {
        if !ready.get_untracked() || event.pointer_type() != "mouse" {
            return;
        }
        let Some(frame) = frame.get_untracked() else {
            return;
        };
        let Some(window) = frame.content_window() else {
            return;
        };
        let rect = frame.get_bounding_client_rect();
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }
        // Convert parent viewport coordinates into the iframe viewport, also
        // accounting for CSS-scaled locus pictures. Coordinates may be outside
        // the frame so followers can look toward a cursor elsewhere in the app.
        let x = (f64::from(event.client_x()) - rect.left()) * f64::from(frame.client_width())
            / rect.width();
        let y = (f64::from(event.client_y()) - rect.top()) * f64::from(frame.client_height())
            / rect.height();
        let message = js_sys::Object::new();
        let point = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&point, &"x".into(), &x.into());
        let _ = js_sys::Reflect::set(&point, &"y".into(), &y.into());
        let _ = js_sys::Reflect::set(&message, &"locora".into(), &1.into());
        let _ = js_sys::Reflect::set(&message, &"pointer".into(), &point);
        let _ = window.post_message(&message, "*");
    });
    let blur = window_event_listener(leptos::ev::blur, move |_| {
        if let Some(window) = frame.get_untracked().and_then(|f| f.content_window()) {
            let message = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&message, &"locora".into(), &1.into());
            let _ = js_sys::Reflect::set(&message, &"pointerLeave".into(), &true.into());
            let _ = window.post_message(&message, "*");
        }
    });
    on_cleanup(move || {
        messages.remove();
        pointer.remove();
        blur.remove();
    });
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
            // A focused frame would swallow every key (Ctrl/⌘+K, Space, 1–4).
            tabindex="-1"
            on:load=move |_| {
                push();
                if let Some(window) = frame.get_untracked().and_then(|f| f.content_window()) {
                    let request = js_sys::Object::new();
                    let _ = js_sys::Reflect::set(&request, &"locora".into(), &1.into());
                    let _ = js_sys::Reflect::set(&request, &"checkRuntime".into(), &true.into());
                    let _ = window.post_message(&request, "*");
                }
                set_timeout(move || {
                    if ready.try_get_untracked() == Some(false) {
                        failed.try_set(true);
                    }
                }, std::time::Duration::from_secs(5));
            }
        ></iframe>
        <Show when=move || failed.get()>
            <span class="notes" role="alert">"Splinely-Runtime nicht gestartet – Animation konnte nicht geladen werden."</span>
        </Show>
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
