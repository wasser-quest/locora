//! Showing media: pictures, videos and `.sply` animations. A `.sply` plays in a
//! sandboxed frame (scripts only, no access to the app); state changes reach
//! it as messages (see `src-tauri/src/sply.rs`).

use crate::i18n::t;
use leptos::prelude::*;
use leptos::{html::Iframe, task::spawn_local};
use locora_app_api::{Media, MediaKind};
use wasm_bindgen::{JsCast, JsValue};

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
        let _ = stage
            .style()
            .set_property("--stage-ratio", &(width / height).to_string());
    }
}

/// Where the messages of a graphic go: the graphic in the page (the web app), or the frame of the
/// desktop app.
#[derive(Clone)]
enum Target {
    Inline(web_sys::Element),
    Frame(web_sys::HtmlIFrameElement),
}

impl Target {
    /// The element that shows the graphic: its box is the frame of reference for pointers.
    fn element(&self) -> web_sys::Element {
        match self {
            Target::Inline(element) => element.clone(),
            Target::Frame(frame) => frame.clone().unchecked_into(),
        }
    }

    fn post(&self, message: &JsValue) {
        match self {
            Target::Inline(element) => {
                let Some(window) = web_sys::window() else {
                    return;
                };
                let Ok(send) = js_sys::Reflect::get(&window, &"locoraSendGraphic".into()) else {
                    return;
                };
                if let Ok(send) = send.dyn_into::<js_sys::Function>() {
                    let element: &JsValue = element.as_ref();
                    let _ = send.call2(&JsValue::NULL, element, message);
                }
            }
            Target::Frame(frame) => {
                if let Some(window) = frame.content_window() {
                    let _ = window.post_message(message, "*");
                }
            }
        }
    }
}

fn send(target: &Target, machine: &str, event: &str) {
    let message = js_sys::Object::new();
    let set = |key: &str, value: JsValue| {
        let _ = js_sys::Reflect::set(&message, &key.into(), &value);
    };
    set("locora", 1.into());
    set("machine", machine.into());
    set("send", event.into());
    target.post(&message);
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimedCommand {
    pub serial: u64,
    pub time: f64,
    pub start: Option<f64>,
    pub play: bool,
}

fn send_timed(target: &Target, action: &str, time: Option<f64>, start: Option<f64>) {
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
    target.post(&message);
}

/// Whether the page mounts graphics inline (boot.mjs defines `locoraMountGraphic`). The desktop app
/// does not, and keeps the frame.
fn inline_available() -> bool {
    web_sys::window().is_some_and(|window| {
        js_sys::Reflect::has(&window, &"locoraMountGraphic".into()).unwrap_or(false)
    })
}

/// Mounts the graphic inline in `element` (boot.mjs: `locoraMountGraphic`): SVG in the page, like a
/// Rive artboard. `reply` receives the graphic's replies; `mounted` runs once it is in the page.
fn mount_inline(
    element: &web_sys::Element,
    path: &str,
    still: bool,
    reply: impl Fn(JsValue) + 'static,
    mounted: impl Fn() + 'static,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(mount) = js_sys::Reflect::get(&window, &"locoraMountGraphic".into()) else {
        return;
    };
    let Ok(mount) = mount.dyn_into::<js_sys::Function>() else {
        return;
    };
    let reply = wasm_bindgen::closure::Closure::<dyn Fn(JsValue)>::new(reply);
    let reply_fn: JsValue = reply.as_ref().clone();
    // The reply closure lives as long as the page: the graphic stops replying when it is unmounted.
    reply.forget();
    let element: &JsValue = element.as_ref();
    let Ok(promise) = mount.call4(
        &JsValue::NULL,
        element,
        &JsValue::from_str(path),
        &JsValue::from_bool(still),
        &reply_fn,
    ) else {
        return;
    };
    let Ok(promise) = promise.dyn_into::<js_sys::Promise>() else {
        return;
    };
    leptos::task::spawn_local(async move {
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
        mounted();
    });
}

/// A `.sply` animation. With `state`, the event is sent to the state machine
/// `machine` whenever it changes (and once the graphic is there).
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
    // The web app mounts the graphic inline; the desktop and Android apps keep the frame.
    let inline = inline_available();
    let path = media.path();
    let frame = NodeRef::<Iframe>::new();
    let host = NodeRef::<leptos::html::Div>::new();
    let ready = RwSignal::new(false);
    let failed = RwSignal::new(false);
    let timed_ready = RwSignal::new(false);
    let timed_ready_callback = StoredValue::new(on_timed_ready);
    let timed_at_callback = StoredValue::new(on_timed_at);
    let timed_position_callback = StoredValue::new(on_timed_position);
    let timed_error_callback = StoredValue::new(on_timed_error);
    // Where the messages go now, if the graphic is there.
    let target = move || -> Option<Target> {
        if inline {
            host.try_get_untracked()
                .flatten()
                .map(|element| Target::Inline(element.unchecked_into::<web_sys::Element>()))
        } else {
            frame.try_get_untracked().flatten().map(Target::Frame)
        }
    };
    // The graphic's replies: its runtime started, and the timed playback reports.
    let handle = move |data: JsValue| {
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
                if let Some(callback) = callback {
                    callback(duration);
                }
            });
            timed_ready.set(true);
        }
        if let Some(time) = get("timedAt").and_then(|v| v.as_f64()) {
            timed_at_callback.with_value(|callback| {
                if let Some(callback) = callback {
                    callback(time);
                }
            });
        }
        if let Some(time) = get("timedPosition").and_then(|v| v.as_f64()) {
            timed_position_callback.with_value(|callback| {
                if let Some(callback) = callback {
                    callback(time);
                }
            });
        }
        if get("timedError").and_then(|v| v.as_string()).is_some() {
            failed.set(true);
        }
    };
    let messages = window_event_listener(leptos::ev::message, move |event| {
        // Inline graphics reply through the page (see mount_inline); only a frame's replies come here.
        if inline {
            return;
        }
        let Some(window) = frame.get_untracked().and_then(|f| f.content_window()) else {
            return;
        };
        // Sandboxed frames have opaque origins: authenticate by source window.
        let source = js_sys::Reflect::get(event.as_ref(), &"source".into()).ok();
        if source != Some(window.into()) {
            return;
        }
        handle(event.data());
    });
    // Converts a viewport position into the graphic's coordinates and sends it to the runtime.
    let post_pointer = move |graphic: Target, client_x: f64, client_y: f64| {
        let element = graphic.element();
        let rect = element.get_bounding_client_rect();
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }
        // Convert viewport coordinates into the graphic's box, also accounting for CSS-scaled
        // locus pictures. Coordinates may be outside the box so followers can look toward a cursor
        // elsewhere in the app.
        let x = (client_x - rect.left()) * f64::from(element.client_width()) / rect.width();
        let y = (client_y - rect.top()) * f64::from(element.client_height()) / rect.height();
        let message = js_sys::Object::new();
        let point = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&point, &"x".into(), &x.into());
        let _ = js_sys::Reflect::set(&point, &"y".into(), &y.into());
        let _ = js_sys::Reflect::set(&message, &"locora".into(), &1.into());
        let _ = js_sys::Reflect::set(&message, &"pointer".into(), &point);
        graphic.post(&message);
    };
    // Touch and pen send several moves per frame; only the newest position is sent, once per frame.
    let pending = StoredValue::new(None::<(f64, f64)>);
    let pointer = window_event_listener(leptos::ev::pointermove, move |event| {
        // Touch and pen move the eyes too; they only send moves while they touch the screen.
        if !ready.get_untracked() {
            return;
        }
        let waiting = pending.with_value(Option::is_some);
        pending.set_value(Some((
            f64::from(event.client_x()),
            f64::from(event.client_y()),
        )));
        if waiting {
            return;
        }
        use wasm_bindgen::JsCast;
        // The frame runs after the animation frame: the graphic may be gone by then, so the
        // values are read with the checked accessors.
        let frame_task = wasm_bindgen::closure::Closure::once_into_js(move || {
            let Some(Some((client_x, client_y))) = pending.try_get_value() else {
                return;
            };
            let _ = pending.try_set_value(None);
            if let Some(graphic) = target() {
                post_pointer(graphic, client_x, client_y);
            }
        });
        if let Some(window) = web_sys::window() {
            let _ = window.request_animation_frame(frame_task.unchecked_ref());
        }
    });
    // Presses and releases travel as coordinates; the graphic turns them into the same
    // pointer sequence a browser sends, so hover, clicks and drags behave alike.
    let pressed = StoredValue::new(false);
    let presses = window_event_listener(leptos::ev::pointerdown, move |event| {
        use wasm_bindgen::JsCast;
        if !ready.get_untracked() || event.button() != 0 {
            return;
        }
        let Some(hit) = event
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        // Never duplicate an app command or interfere with editor gestures.
        if hit.closest("button, a, input, select, textarea, [role=button], [contenteditable], .stage.editing").ok().flatten().is_some() {
            return;
        }
        let Some(graphic) = target() else {
            return;
        };
        let element = graphic.element();
        let rect = element.get_bounding_client_rect();
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
            &(x * f64::from(element.client_width()) / rect.width()).into(),
        );
        let _ = js_sys::Reflect::set(
            &point,
            &"y".into(),
            &(y * f64::from(element.client_height()) / rect.height()).into(),
        );
        let message = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&message, &"locora".into(), &1.into());
        let _ = js_sys::Reflect::set(&message, &"pointerDown".into(), &point);
        graphic.post(&message);
        pressed.set_value(true);
    });
    // A press that started in the graphic ends there, even when the pointer has left it.
    let releases = window_event_listener(leptos::ev::pointerup, move |event| {
        if !pressed.get_value() {
            return;
        }
        pressed.set_value(false);
        let Some(graphic) = target() else {
            return;
        };
        let element = graphic.element();
        let rect = element.get_bounding_client_rect();
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }
        let x = f64::from(event.client_x()) - rect.left();
        let y = f64::from(event.client_y()) - rect.top();
        let point = js_sys::Object::new();
        let _ = js_sys::Reflect::set(
            &point,
            &"x".into(),
            &(x * f64::from(element.client_width()) / rect.width()).into(),
        );
        let _ = js_sys::Reflect::set(
            &point,
            &"y".into(),
            &(y * f64::from(element.client_height()) / rect.height()).into(),
        );
        let message = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&message, &"locora".into(), &1.into());
        let _ = js_sys::Reflect::set(&message, &"pointerUp".into(), &point);
        graphic.post(&message);
        // A finger leaves no hover behind once it lifts.
        if event.pointer_type() != "mouse" {
            let leave = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&leave, &"locora".into(), &1.into());
            let _ = js_sys::Reflect::set(&leave, &"pointerLeave".into(), &true.into());
            graphic.post(&leave);
        }
    });
    let blur = window_event_listener(leptos::ev::blur, move |_| {
        if let Some(graphic) = target() {
            let message = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&message, &"locora".into(), &1.into());
            let _ = js_sys::Reflect::set(&message, &"pointerLeave".into(), &true.into());
            graphic.post(&message);
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
        if let (Some(state), Some(graphic)) = (state, target()) {
            send(&graphic, machine, state.get_untracked());
        }
    };
    Effect::new(move |_| {
        if let Some(state) = state {
            state.track();
        }
        push();
    });
    Effect::new(move |_| {
        if timed_ready.get()
            && let (Some(command), Some(graphic)) = (timed, target()) {
                let command = command.get();
                send_timed(
                    &graphic,
                    if command.play { "playTo" } else { "seek" },
                    Some(command.time),
                    command.start,
                );
            }
    });
    // The graphic is there (its frame loaded, or it is mounted inline): it reports its runtime state.
    let mounted = move || {
        ready.set(false);
        timed_ready.set(false);
        push();
        if let Some(graphic) = target() {
            if still {
                let message = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&message, &"locora".into(), &1.into());
                let _ = js_sys::Reflect::set(&message, &"still".into(), &true.into());
                graphic.post(&message);
            }
            let request = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&request, &"locora".into(), &1.into());
            let _ = js_sys::Reflect::set(&request, &"checkRuntime".into(), &true.into());
            graphic.post(&request);
        }
        set_timeout(
            move || {
                if ready.try_get_untracked() == Some(false) {
                    failed.try_set(true);
                }
            },
            std::time::Duration::from_secs(5),
        );
    };
    if inline {
        Effect::new(move |_| {
            let Some(element) = host.get() else { return };
            let element: web_sys::Element = element.unchecked_into();
            mount_inline(&element, &path, still, handle, mounted);
        });
    } else {
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
    }
    Effect::new(move |_| {
        if ready.get()
            && !timed_ready.get()
            && timed.is_some()
            && let Some(graphic) = target()
        {
            send_timed(&graphic, "init", None, None);
        }
    });
    Effect::new(move |_| {
        if failed.get() && timed.is_some() {
            timed_error_callback.with_value(|callback| {
                if let Some(callback) = callback {
                    callback();
                }
            });
        }
    });
    let graphic_view = if inline {
        view! { <div node_ref=host class=format!("sply-inline {class}")></div> }.into_any()
    } else {
        view! {
            <iframe
                node_ref=frame
                class=format!("sply {class}")
                sandbox="allow-scripts"
                src=url(&media)
                title=t("Animation")
                // A focused frame would swallow every key (Ctrl/⌘+K, Space, 1–4).
                tabindex="-1"
                on:load=move |_| mounted()
            ></iframe>
        }
        .into_any()
    };
    view! {
        {graphic_view}
        <Show when=move || failed.get()>
            <span class="notes" role="alert">{t("Splinely-Runtime nicht gestartet – Animation konnte nicht geladen werden.")}</span>
        </Show>
    }
    .into_any()
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
    // A video that is already loaded shows at once; otherwise the frame waits for the fetch.
    let cached = crate::tauri::cached_video_url(&media);
    let fetching = cached.is_none();
    let source = RwSignal::new(cached.map_or_else(|| Err(t("Video lädt…").to_string()), Ok));
    if fetching {
        spawn_local(async move {
            let result = crate::tauri::video_url(&media).await;
            source.set(result);
        });
    }
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
                on:error=move |_| {
                    // Usually a format the browser cannot decode (often HEVC/H.265 from a phone): say so
                    // instead of an empty frame.
                    source.set(Err(t("Dieses Video kann der Browser nicht abspielen (häufig HEVC/H.265 vom Handy). Als MP4 (H.264) oder WebM speichern.").to_string()));
                    failed.with_value(|callback| if let Some(cb) = callback { cb(); });
                }
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
