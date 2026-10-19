//! Showing media: pictures, videos and `.sply` animations. A `.sply` plays in a
//! sandboxed frame (scripts only, no access to the app); state changes reach
//! it as messages (see `src-tauri/src/sply.rs`).

use leptos::{html::Iframe, task::spawn_local};
use leptos::prelude::*;
use locora_app_api::{Media, MediaKind};
use wasm_bindgen::JsCast;

use crate::tauri::url;

/// Give the enclosing room stage the picture's own proportions. Markers are placed in
/// stage percentages, so the stage must be exactly as large as the visible picture.
pub fn fit_stage(picture: &web_sys::Element, width: f64, height: f64) {
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return;
    }
    let Ok(Some(stage)) = picture.closest(".stage") else {
        return;
    };
    if let Ok(stage) = stage.dyn_into::<web_sys::HtmlElement>() {
        let _ = stage.style().set_property("--stage-ratio", &(width / height).to_string());
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimedCommand {
    pub serial: u64,
    pub time: f64,
    pub start: Option<f64>,
    pub play: bool,
}

fn send_timed(frame: &web_sys::HtmlIFrameElement, action: &str, time: Option<f64>, start: Option<f64>) {
    let Some(window) = frame.content_window() else { return; };
    let message = js_sys::Object::new();
    let timed = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&message, &"locora".into(), &1.into());
    let _ = js_sys::Reflect::set(&timed, &"action".into(), &action.into());
    if let Some(time) = time {
        let _ = js_sys::Reflect::set(&timed, &"time".into(), &time.into());
    }
    if let Some(start) = start {
        let _ = js_sys::Reflect::set(&timed, &"start".into(), &start.into());
    }
    let _ = js_sys::Reflect::set(&message, &"timed".into(), &timed);
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
    // Decorative icons hold their first frame instead of animating on every frame.
    #[prop(optional)] still: bool,
    #[prop(optional)] timed: Option<RwSignal<TimedCommand>>,
    #[prop(optional)] on_timed_ready: Option<Box<dyn Fn(f64) + Send + Sync>>,
    #[prop(optional)] on_timed_at: Option<Box<dyn Fn(f64) + Send + Sync>>,
    #[prop(optional)] on_timed_position: Option<Box<dyn Fn(f64) + Send + Sync>>,
    #[prop(optional)] on_timed_error: Option<Box<dyn Fn() + Send + Sync>>,
) -> impl IntoView {
    let frame = NodeRef::<Iframe>::new();
    let ready = RwSignal::new(false);
    let failed = RwSignal::new(false);
    let timed_ready = RwSignal::new(false);
    let timed_ready_callback = StoredValue::new(on_timed_ready);
    let timed_at_callback = StoredValue::new(on_timed_at);
    let timed_position_callback = StoredValue::new(on_timed_position);
    let timed_error_callback = StoredValue::new(on_timed_error);
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
        if let Some(duration) = get("timedReady").and_then(|v| v.as_f64()) {
            timed_ready_callback.with_value(|callback| {
                if let Some(callback) = callback { callback(duration); }
            });
            timed_ready.set(true);
        }
        if let Some(time) = get("timedAt").and_then(|v| v.as_f64()) {
            timed_at_callback.with_value(|callback| {
                if let Some(callback) = callback { callback(time); }
            });
        }
        if let Some(time) = get("timedPosition").and_then(|v| v.as_f64()) {
            timed_position_callback.with_value(|callback| {
                if let Some(callback) = callback { callback(time); }
            });
        }
        if get("timedError").and_then(|v| v.as_string()).is_some() {
            failed.set(true);
        }
    });
    let pointer = window_event_listener(leptos::ev::pointermove, move |event| {
        // Touch and pen move the eyes too; they only send moves while they touch the screen.
        if !ready.get_untracked() {
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
    // Presses and releases travel as coordinates; the frame turns them into the same
    // pointer sequence a browser sends, so hover, clicks and drags behave alike.
    let pressed = StoredValue::new(false);
    let presses = window_event_listener(leptos::ev::pointerdown, move |event| {
        use wasm_bindgen::JsCast;
        if !ready.get_untracked() || event.button() != 0 {
            return;
        }
        let Some(target) = event
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        // Never duplicate an app command or interfere with editor gestures.
        if target.closest("button, a, input, select, textarea, [role=button], [contenteditable], .stage.editing").ok().flatten().is_some() {
            return;
        }
        let Some(frame) = frame.get_untracked() else {
            return;
        };
        let Some(window) = frame.content_window() else {
            return;
        };
        let rect = frame.get_bounding_client_rect();
        let x = f64::from(event.client_x()) - rect.left();
        let y = f64::from(event.client_y()) - rect.top();
        if rect.width() <= 0.0
            || rect.height() <= 0.0
            || x < 0.0
            || y < 0.0
            || x >= rect.width()
            || y >= rect.height()
        {
            return;
        }
        let point = js_sys::Object::new();
        let _ = js_sys::Reflect::set(
            &point,
            &"x".into(),
            &(x * f64::from(frame.client_width()) / rect.width()).into(),
        );
        let _ = js_sys::Reflect::set(
            &point,
            &"y".into(),
            &(y * f64::from(frame.client_height()) / rect.height()).into(),
        );
        let message = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&message, &"locora".into(), &1.into());
        let _ = js_sys::Reflect::set(&message, &"pointerDown".into(), &point);
        let _ = window.post_message(&message, "*");
        pressed.set_value(true);
    });
    // A press that started in the frame ends there, even when the pointer has left it.
    let releases = window_event_listener(leptos::ev::pointerup, move |event| {
        if !pressed.get_value() {
            return;
        }
        pressed.set_value(false);
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
        let x = f64::from(event.client_x()) - rect.left();
        let y = f64::from(event.client_y()) - rect.top();
        let point = js_sys::Object::new();
        let _ = js_sys::Reflect::set(
            &point,
            &"x".into(),
            &(x * f64::from(frame.client_width()) / rect.width()).into(),
        );
        let _ = js_sys::Reflect::set(
            &point,
            &"y".into(),
            &(y * f64::from(frame.client_height()) / rect.height()).into(),
        );
        let message = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&message, &"locora".into(), &1.into());
        let _ = js_sys::Reflect::set(&message, &"pointerUp".into(), &point);
        let _ = window.post_message(&message, "*");
        // A finger leaves no hover behind once it lifts.
        if event.pointer_type() != "mouse" {
            let leave = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&leave, &"locora".into(), &1.into());
            let _ = js_sys::Reflect::set(&leave, &"pointerLeave".into(), &true.into());
            let _ = window.post_message(&leave, "*");
        }
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
        presses.remove();
        releases.remove();
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
    Effect::new(move |_| {
        if timed_ready.get() {
            if let (Some(command), Some(frame)) = (timed, frame.get_untracked()) {
                let command = command.get();
                send_timed(&frame, if command.play { "playTo" } else { "seek" }, Some(command.time), command.start);
            }
        }
    });
    // Leptos may reuse an already loaded iframe after an editor save. Ask that
    // frame for its runtime state even when no new DOM load event fires.
    Effect::new(move |_| {
        if let Some(element) = frame.get()
            && let Some(window) = element.content_window()
        {
            let request = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&request, &"locora".into(), &1.into());
            let _ = js_sys::Reflect::set(&request, &"checkRuntime".into(), &true.into());
            let _ = window.post_message(&request, "*");
        }
    });
    Effect::new(move |_| {
        if ready.get() && !timed_ready.get() && timed.is_some()
            && let Some(element) = frame.get_untracked()
        {
            send_timed(&element, "init", None, None);
        }
    });
    Effect::new(move |_| {
        if failed.get() && timed.is_some() {
            timed_error_callback.with_value(|callback| {
                if let Some(callback) = callback { callback(); }
            });
        }
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
                ready.set(false);
                timed_ready.set(false);
                push();
                if still {
                    if let Some(window) = frame.get_untracked().and_then(|f| f.content_window()) {
                        let message = js_sys::Object::new();
                        let _ = js_sys::Reflect::set(&message, &"locora".into(), &1.into());
                        let _ = js_sys::Reflect::set(&message, &"still".into(), &true.into());
                        let _ = window.post_message(&message, "*");
                    }
                }
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
        MediaKind::Image => view! {
            <img class=class src=url(&media) alt="" on:load=move |ev| {
                let image = ev.target().and_then(|target| target.dyn_into::<web_sys::HtmlImageElement>().ok());
                if let Some(image) = image {
                    fit_stage(image.as_ref(), f64::from(image.natural_width()), f64::from(image.natural_height()));
                }
            }/>
        }
        .into_any(),
        MediaKind::Video => {
            view! { <VideoBackdrop media=media class=class controls=true muted=true/> }
                .into_any()
        }
    }
}

/// A stored video, played from a blob URL (see `tauri::video_url`): WebKitGTK's
/// media player refuses custom schemes, so the bytes are fetched over the
/// custom scheme and handed to `<video>` as a blob. `node_ref` receives the
/// element; `on_loaded`/`on_error` wrap the DOM events of the same name;
/// `on_timeupdate` runs on the periodic video clock. All callbacks may fire more
/// than once if the source changes.
#[component]
pub fn VideoBackdrop(
    media: Media,
    #[prop(optional, into)] class: String,
    #[prop(optional, into)] controls: MaybeProp<bool>,
    #[prop(default = true)] muted: bool,
    #[prop(default = "auto".into())] preload: String,
    #[prop(default = true)] autoplay: bool,
    #[prop(optional, into)] node_ref: NodeRef<leptos::html::Video>,
    #[prop(optional)] on_loaded: Option<Box<dyn Fn() + Send + Sync>>,
    #[prop(optional)] on_error: Option<Box<dyn Fn() + Send + Sync>>,
    #[prop(optional)] on_timeupdate: Option<Box<dyn Fn() + Send + Sync>>,
) -> impl IntoView {
    let source = RwSignal::new(Err("Video lädt…".to_string()));
    spawn_local(async move {
        let result = crate::tauri::video_url(&media).await;
        source.set(result);
    });
    on_cleanup(move || {
        // Revoke the object URL once the component is gone.
        if let Some(Ok(object)) = source.try_get_untracked() {
            crate::tauri::revoke_object_url(&object);
        }
    });
    let loaded = StoredValue::new(on_loaded);
    let failed = StoredValue::new(on_error);
    let ticked = StoredValue::new(on_timeupdate);
    let player = node_ref;
    let class = StoredValue::new(class);
    let preload = StoredValue::new(preload);
    let muted = StoredValue::new(muted);
    let autoplay = StoredValue::new(autoplay);
    view! {
        <Show when=move || source.get().is_ok()>
            <video
                node_ref=player
                class=class.with_value(|value| value.clone())
                controls=move || controls.get().unwrap_or(true)
                muted=muted.with_value(|value| *value)
                preload=preload.with_value(|value| value.clone())
                autoplay=autoplay.with_value(|value| *value)
                src=move || source.get().expect("checked by Show")
                on:loadedmetadata=move |_| loaded.with_value(|callback| if let Some(cb) = callback { cb(); })
                on:timeupdate=move |_| ticked.with_value(|callback| if let Some(cb) = callback { cb(); })
                on:error=move |_| failed.with_value(|callback| if let Some(cb) = callback { cb(); })
            ></video>
        </Show>
        <Show when=move || source.get().is_err()>
            <p class="notes" role="alert">{move || source.get().unwrap_err()}</p>
        </Show>
    }
}

/// A video's first frame as a palace thumbnail. A paused video often paints nothing
/// until something repaints it, so the frame is copied to a canvas, which always shows.
#[component]
pub fn VideoThumb(media: Media, #[prop(optional, into)] class: String) -> impl IntoView {
    let player = NodeRef::<leptos::html::Video>::new();
    let canvas = NodeRef::<leptos::html::Canvas>::new();
    let drawn = StoredValue::new(false);
    view! {
        <VideoBackdrop
            media=media
            class="palace-thumb-source"
            controls=false
            muted=true
            autoplay=false
            preload="auto".to_string()
            node_ref=player
            on_loaded=Box::new(move || {
                // Seeking past the first frame makes WebViews decode it.
                if let Some(video) = player.get_untracked() {
                    video.set_current_time(0.1);
                }
            })
            on_timeupdate=Box::new(move || {
                if drawn.get_value() {
                    return;
                }
                let (Some(video), Some(target)) = (player.get_untracked(), canvas.get_untracked()) else {
                    return;
                };
                if video.ready_state() < 2 || video.video_width() == 0 {
                    return;
                }
                target.set_width(video.video_width());
                target.set_height(video.video_height());
                let context = target.get_context("2d").ok().flatten()
                    .and_then(|context| context.dyn_into::<web_sys::CanvasRenderingContext2d>().ok());
                if let Some(context) = context
                    && context.draw_image_with_html_video_element(&video, 0.0, 0.0).is_ok()
                {
                    drawn.set_value(true);
                }
            })
        />
        <canvas node_ref=canvas class=format!("palace-thumb {class}")></canvas>
    }
}
