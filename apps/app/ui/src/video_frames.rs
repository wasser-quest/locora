//! Observe decoded video frames when the WebView supports the browser API.
//! The timeupdate and interval handlers remain available for older WebViews.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use js_sys::Function;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::HtmlVideoElement;

#[wasm_bindgen(inline_js = r#"
export function watchVideoFrames(video, onFrame) {
  if (typeof video.requestVideoFrameCallback !== 'function') return null;
  let cancelled = false;
  let requestId;
  const tick = () => {
    if (cancelled) return;
    onFrame();
    if (!cancelled) requestId = video.requestVideoFrameCallback(tick);
  };
  requestId = video.requestVideoFrameCallback(tick);
  return () => {
    cancelled = true;
    video.cancelVideoFrameCallback(requestId);
    onFrame = null;
  };
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = watchVideoFrames)]
    fn watch_video_frames(video: &HtmlVideoElement, on_frame: &Function) -> JsValue;
}

struct FrameWatch {
    _callback: Closure<dyn FnMut()>,
    cancel: Function,
}

thread_local! {
    static WATCHES: RefCell<HashMap<u64, FrameWatch>> = RefCell::new(HashMap::new());
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn new_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Replace the watch after each metadata load; `None` means the API is absent.
pub fn start(id: u64, video: &HtmlVideoElement, on_frame: impl FnMut() + 'static) -> bool {
    stop(id);
    let callback = Closure::wrap(Box::new(on_frame) as Box<dyn FnMut()>);
    let cancel = watch_video_frames(video, callback.as_ref().unchecked_ref());
    let Some(cancel) = cancel.dyn_into::<Function>().ok() else {
        return false;
    };
    WATCHES.with_borrow_mut(|watches| {
        watches.insert(
            id,
            FrameWatch {
                _callback: callback,
                cancel,
            },
        );
    });
    true
}

pub fn stop(id: u64) {
    WATCHES.with_borrow_mut(|watches| {
        if let Some(watch) = watches.remove(&id) {
            let _ = watch.cancel.call0(&JsValue::NULL);
        }
    });
}
