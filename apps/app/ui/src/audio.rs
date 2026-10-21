//! A locus sound clip: a player with controls and a play button.

use std::sync::{Arc, Mutex};

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::Media;

use crate::i18n::t;

/// The sound clip of a locus. With `autoplay` it starts as soon as it is loaded: it is shown when
/// the answer is revealed, which the person started with a click or a key.
#[component]
pub fn LocusAudio(media: Media, #[prop(optional)] autoplay: bool) -> impl IntoView {
    let source: RwSignal<Option<Result<String, String>>> = RwSignal::new(None);
    // The blob URL, released when the player goes away (or at once if it is already gone).
    let current = Arc::new(Mutex::new(None::<String>));
    spawn_local({
        let current = current.clone();
        async move {
            let result = crate::tauri::audio_url(&media)
                .await
                .map_err(|_| t("Ton konnte nicht abgespielt werden").to_string());
            if let Ok(object) = &result
                && let Ok(mut slot) = current.lock()
            {
                *slot = Some(object.clone());
            }
            if source.try_set(Some(result)).is_some() {
                release(&current);
            }
        }
    });
    on_cleanup(move || release(&current));
    let player = NodeRef::<leptos::html::Audio>::new();
    let play = move |_| {
        if let Some(audio) = player.get_untracked() {
            audio.set_current_time(0.0);
            let _ = audio.play();
        }
    };
    view! {
        <div class="locus-audio">
            {move || match source.get() {
                None => view! { <span class="notes">{t("Ton lädt…")}</span> }.into_any(),
                Some(Err(error)) => view! { <span class="notes" role="alert">{error}</span> }.into_any(),
                Some(Ok(url)) => view! {
                    <button type="button" on:click=play>{t("▶ Audio")}</button>
                    <audio
                        node_ref=player
                        controls=true
                        preload="auto"
                        autoplay=autoplay
                        src=url
                        aria-label=t("Ton zur Antwort")
                        on:error=move |_| source.set(Some(Err(t("Ton konnte nicht abgespielt werden").to_string())))
                    ></audio>
                }
                .into_any(),
            }}
        </div>
    }
}

fn release(current: &Mutex<Option<String>>) {
    if let Some(object) = current.lock().ok().and_then(|mut slot| slot.take()) {
        crate::tauri::revoke_object_url(&object);
    }
}
