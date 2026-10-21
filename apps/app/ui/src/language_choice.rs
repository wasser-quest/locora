//! On the very first start: which language the app speaks. Shown before the app, in all languages
//! at once, the one the system suggests first.

use leptos::prelude::*;

use crate::i18n::{self, Lang};

/// The language the browser or system prefers, if the app has it.
fn suggested() -> Option<Lang> {
    let preferred = web_sys::window()?.navigator().language()?;
    let code = preferred.split(['-', '_']).next()?.to_ascii_lowercase();
    Lang::ALL.into_iter().find(|lang| lang.code() == code)
}

#[component]
pub fn LanguageChoice(chosen: RwSignal<bool>) -> impl IntoView {
    let mut order: Vec<Lang> = Lang::ALL.to_vec();
    if let Some(first) = suggested() {
        order.retain(|lang| *lang != first);
        order.insert(0, first);
    }
    let buttons = order
        .iter()
        .enumerate()
        .map(|(index, lang)| {
            let lang = *lang;
            view! {
                <button type="button" class:primary=index == 0 autofocus=index == 0
                    lang=lang.code()
                    on:click=move |_| {
                        i18n::switch(lang);
                        chosen.set(true);
                    }>
                    {lang.name()}
                </button>
            }
        })
        .collect_view();
    view! {
        <div class="palette-backdrop language-choice-backdrop">
            <div class="card language-choice" role="dialog" aria-modal="true" aria-labelledby="language-choice-title">
                <h2 id="language-choice-title">"Sprache · Language · Langue · 言語 · 语言"</h2>
                <p class="meta">"Wähle die Sprache der App · Choose the app's language · Choisis la langue de l’app"</p>
                <div class="language-choice-buttons">{buttons}</div>
            </div>
        </div>
    }
}
