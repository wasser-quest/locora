//! A modal dialog for the friends page and for sharing: a backdrop, and a card with
//! `role="dialog"` named by its title. It takes the focus, keeps Tab inside, closes on Escape, and
//! gives the focus back to the button that opened it. While it is open its height follows the part
//! of the window an on-screen keyboard leaves, so its fields and buttons stay reachable.

use std::cell::{Cell, RefCell};

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

/// What an open dialog keeps outside the reactive system (elements and listeners are not `Send`):
/// the element to focus again, and the listener that follows the keyboard.
struct Open {
    id: u32,
    opener: Option<web_sys::HtmlElement>,
    follow: Option<Closure<dyn Fn()>>,
}

thread_local! {
    static OPEN: RefCell<Vec<Open>> = const { RefCell::new(Vec::new()) };
    static NEXT: Cell<u32> = const { Cell::new(0) };
}

/// The element that has the focus now.
pub fn focused() -> Option<web_sys::HtmlElement> {
    web_sys::window()?
        .document()?
        .active_element()?
        .dyn_into::<web_sys::HtmlElement>()
        .ok()
}

/// The element a dialog opened from this click gives the focus back to: the clicked button itself
/// (in WebKit a click does not focus a button), else what has the focus.
pub fn opener_of(event: &web_sys::MouseEvent) -> Option<web_sys::HtmlElement> {
    event
        .current_target()
        .and_then(|target| target.dyn_into::<web_sys::HtmlElement>().ok())
        .or_else(focused)
}

/// Sets `--seen-height` to the height left by an on-screen keyboard (the visual viewport).
fn follow_keyboard() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let height = js_sys::Reflect::get(&window, &"visualViewport".into())
        .ok()
        .filter(|viewport| !viewport.is_undefined())
        .and_then(|viewport| js_sys::Reflect::get(&viewport, &"height".into()).ok())
        .and_then(|height| height.as_f64());
    if let (Some(height), Some(root)) = (
        height,
        window
            .document()
            .and_then(|document| document.document_element()),
    ) && let Ok(root) = root.dyn_into::<web_sys::HtmlElement>()
    {
        let _ = root
            .style()
            .set_property("--seen-height", &format!("{height}px"));
    }
}

/// The controls in `card` that Tab reaches.
fn focusable(card: &web_sys::Element) -> Vec<web_sys::HtmlElement> {
    let Ok(found) = card.query_selector_all(
        "button:not([disabled]), input:not([disabled]):not([type=hidden]), textarea:not([disabled]), select:not([disabled]), a[href], [tabindex=\"0\"]",
    ) else {
        return Vec::new();
    };
    (0..found.length())
        .filter_map(|index| found.item(index))
        .filter_map(|node| node.dyn_into::<web_sys::HtmlElement>().ok())
        .filter(|element| element.offset_parent().is_some())
        .collect()
}

/// The first focus in a dialog: on a touch screen its title (a field would bring up the keyboard
/// over the dialog before the person read it), else its first field, else its first button.
fn first_focus(card: &web_sys::Element, labelled: &str) {
    let title = card
        .query_selector(&format!("#{labelled}"))
        .ok()
        .flatten()
        .and_then(|title| title.dyn_into::<web_sys::HtmlElement>().ok());
    if crate::touch_screen()
        && let Some(title) = title
    {
        let _ = title.focus();
        return;
    }
    let field = card
        .query_selector("input:not([disabled]):not([readonly]):not([type=hidden]), textarea:not([disabled]):not([readonly]), select:not([disabled])")
        .ok()
        .flatten()
        .and_then(|field| field.dyn_into::<web_sys::HtmlElement>().ok());
    match field.or_else(|| focusable(card).into_iter().next()) {
        Some(element) => {
            let _ = element.focus();
        }
        None => {
            if let Some(title) = title {
                let _ = title.focus();
            }
        }
    }
}

/// Gives the focus back when the dialog `id` closes: to its opener while it is on the page, else
/// to the page's heading.
fn close_open(id: u32) {
    let Some(open) = OPEN.with(|all| {
        let mut all = all.borrow_mut();
        let at = all.iter().position(|open| open.id == id)?;
        Some(all.remove(at))
    }) else {
        return;
    };
    let window = web_sys::window();
    if let (Some(follow), Some(window)) = (&open.follow, &window)
        && let Ok(viewport) = js_sys::Reflect::get(window, &"visualViewport".into())
        && let Ok(viewport) = viewport.dyn_into::<web_sys::EventTarget>()
    {
        let _ =
            viewport.remove_event_listener_with_callback("resize", follow.as_ref().unchecked_ref());
    }
    let opener = open.opener.filter(|opener| opener.is_connected());
    let target = opener.or_else(|| {
        window
            .and_then(|window| window.document())
            .and_then(|document| document.query_selector("main h1").ok().flatten())
            .and_then(|heading| heading.dyn_into::<web_sys::HtmlElement>().ok())
    });
    if let Some(target) = target {
        let _ = target.focus();
    }
}

/// A modal dialog. `labelled` (and `described`) are the ids of its title (and its explanation),
/// which the children carry; the title gets `tabindex="-1"` to take the focus on a touch screen.
/// `opener` is focused again on close (see `opener_of`). While `dirty` holds, a tap beside the
/// dialog does not close it (the person typed something); Escape and its buttons still do.
#[component]
pub fn Modal(
    labelled: &'static str,
    #[prop(optional)] described: Option<&'static str>,
    #[prop(optional)] class: &'static str,
    #[prop(optional, into)] dirty: Option<Signal<bool>>,
    opener: Option<web_sys::HtmlElement>,
    on_close: Callback<()>,
    children: Children,
) -> impl IntoView {
    let id = NEXT.with(|next| {
        let id = next.get().wrapping_add(1);
        next.set(id);
        id
    });
    // The height left by a keyboard, now and whenever it changes.
    follow_keyboard();
    let follow = Closure::<dyn Fn()>::new(follow_keyboard);
    if let Some(window) = web_sys::window()
        && let Ok(viewport) = js_sys::Reflect::get(&window, &"visualViewport".into())
        && let Ok(viewport) = viewport.dyn_into::<web_sys::EventTarget>()
    {
        let _ =
            viewport.add_event_listener_with_callback("resize", follow.as_ref().unchecked_ref());
    }
    OPEN.with(|all| {
        all.borrow_mut().push(Open {
            id,
            opener: opener.or_else(focused),
            follow: Some(follow),
        })
    });
    on_cleanup(move || close_open(id));
    let card = NodeRef::<leptos::html::Section>::new();
    Effect::new(move |_| {
        if let Some(card) = card.get() {
            first_focus(&card, labelled);
        }
    });
    // On the window: after a tap beside the dialog the focus may be outside it.
    let keys = move |event: web_sys::KeyboardEvent| match event.key().as_str() {
        "Escape" => {
            event.prevent_default();
            event.stop_propagation();
            on_close.run(());
        }
        "Tab" => {
            let Some(card) = card.get_untracked() else {
                return;
            };
            let controls = focusable(&card);
            let (Some(first), Some(last)) = (controls.first(), controls.last()) else {
                return;
            };
            let now = focused();
            let inside = now.as_ref().is_some_and(|now| card.contains(Some(now)));
            if event.shift_key() && (!inside || now.as_ref() == Some(first)) {
                event.prevent_default();
                let _ = last.focus();
            } else if !event.shift_key() && (!inside || now.as_ref() == Some(last)) {
                event.prevent_default();
                let _ = first.focus();
            }
        }
        _ => {}
    };
    let listener = window_event_listener(leptos::ev::keydown, keys);
    on_cleanup(move || listener.remove());
    view! {
        <div class="modal-backdrop" on:focusin=move |_| follow_keyboard()
            on:click=move |event| {
                // Only a tap on the backdrop itself, and only while nothing was typed.
                let beside = event.target().is_some() && event.target() == event.current_target();
                if beside && !dirty.is_some_and(|dirty| dirty.get_untracked()) {
                    on_close.run(());
                }
            }>
            <section class=format!("card modal {class}") role="dialog" aria-modal="true"
                aria-labelledby=labelled aria-describedby=described node_ref=card>
                {children()}
            </section>
        </div>
    }
}
