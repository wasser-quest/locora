//! A small ⓘ button that shows a short explanation of the field next to it.

use leptos::ev::{KeyboardEvent, MouseEvent};
use leptos::prelude::*;

use crate::i18n::t;

/// An ⓘ button and, once tapped, its explanation; `text` is a German catalog key.
///
/// It usually sits inside a `<label>`, right after the label text. A real `<button>` there would
/// become the label's control in place of the input that follows, so it is a span with the
/// button role. A click in the label would also toggle its checkbox or focus its input, so the
/// button and the box keep their clicks to themselves. The glyph is hidden from screen readers;
/// the button is named „Erklärung“ instead.
#[component]
pub fn Info(text: &'static str) -> impl IntoView {
    let open = RwSignal::new(false);
    let keep = |event: MouseEvent| {
        event.prevent_default();
        event.stop_propagation();
    };
    view! {
        <span class="info-button" role="button" tabindex="0" aria-label=t("Erklärung")
            aria-expanded=move || if open.get() { "true" } else { "false" }
            on:click=move |event| {
                keep(event);
                open.update(|shown| *shown = !*shown);
            }
            on:keydown=move |event: KeyboardEvent| {
                let key = event.key();
                let toggle = key == "Enter" || key == " ";
                if toggle || (key == "Escape" && open.get_untracked()) {
                    event.prevent_default();
                    event.stop_propagation();
                    open.set(toggle && !open.get_untracked());
                }
            }>
            <span aria-hidden="true">"i"</span>
        </span>
        <Show when=move || open.get()>
            <span class="info-box" role="note" on:click=keep>{t(text)}</span>
        </Show>
    }
}
