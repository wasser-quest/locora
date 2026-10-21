//! Interface languages. The German text is the key of every message; English is the default.
//! A message without a translation shows its German text, so a gap never shows an empty label.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::i18n_catalog::MESSAGES;

const KEY: &str = "locora-language-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Fr,
    Ja,
    Zh,
}

impl Lang {
    pub const ALL: [Lang; 5] = [Lang::En, Lang::De, Lang::Fr, Lang::Ja, Lang::Zh];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
            Lang::Fr => "fr",
            Lang::Ja => "ja",
            Lang::Zh => "zh",
        }
    }

    /// The language's own name, so people find it whatever the current language is.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::De => "Deutsch",
            Lang::Fr => "Français",
            Lang::Ja => "日本語",
            Lang::Zh => "中文",
        }
    }

    pub fn from_code(code: &str) -> Lang {
        Self::ALL
            .into_iter()
            .find(|lang| lang.code() == code)
            .unwrap_or(Lang::En)
    }

    fn column(self) -> usize {
        match self {
            Lang::En => 0,
            Lang::De => 1,
            Lang::Fr => 2,
            Lang::Ja => 3,
            Lang::Zh => 4,
        }
    }
}

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::En) };
    static TABLE: RefCell<Option<HashMap<&'static str, [&'static str; 5]>>> = const { RefCell::new(None) };
}

pub fn current() -> Lang {
    CURRENT.with(|lang| lang.get())
}

/// Set once at start-up; changing the language reloads the page, so every text is rebuilt.
pub fn set_current(lang: Lang) {
    CURRENT.with(|current| current.set(lang));
}

/// The language as a signal. The app's root watches it and builds the screens again when it changes.
#[derive(Clone, Copy)]
pub struct Language(pub leptos::prelude::RwSignal<Lang>);

/// Switch the language on the running app: saved, then every text is looked up again.
pub fn switch(lang: Lang) {
    use leptos::prelude::Set;
    save(lang);
    set_current(lang);
    if let Some(Language(signal)) = leptos::prelude::use_context::<Language>() {
        signal.set(lang);
    }
}

/// The saved language; English when nothing is saved or storage is unavailable.
pub fn load() -> Lang {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(KEY).ok().flatten())
        .map(|code| Lang::from_code(&code))
        .unwrap_or(Lang::En)
}

pub fn save(lang: Lang) {
    if let Some(storage) =
        web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    {
        let _ = storage.set_item(KEY, lang.code());
    }
}

/// The text for `key` (its German original) in the current language.
pub fn t(key: &'static str) -> &'static str {
    let column = current().column();
    TABLE.with(|table| {
        let mut table = table.borrow_mut();
        let map = table.get_or_insert_with(|| MESSAGES.iter().copied().collect());
        match map.get(key) {
            Some(texts) if !texts[column].is_empty() => texts[column],
            _ => key,
        }
    })
}

/// Text from a plugin in the current language. Plain text is translated when a translation
/// exists. A template comes as `@@template␟arg␟arg…` (␟ is U+001F): the template is translated
/// and its `{0}`, `{1}`, … are filled with the arguments, so the numbers sit in the right place.
pub fn plugin_text(text: &str) -> String {
    let Some(rest) = text.strip_prefix("@@") else {
        return translate(text);
    };
    let mut parts = rest.split('\u{1f}');
    let mut out = translate(parts.next().unwrap_or_default());
    for (index, arg) in parts.enumerate() {
        out = out.replace(&format!("{{{index}}}"), arg);
    }
    out
}

/// The translation of a text known only at run time, or the text itself.
pub fn translate(text: &str) -> String {
    let column = current().column();
    TABLE.with(|table| {
        let mut table = table.borrow_mut();
        let map = table.get_or_insert_with(|| MESSAGES.iter().copied().collect());
        match map.get(text) {
            Some(texts) if !texts[column].is_empty() => texts[column].to_string(),
            _ => text.to_string(),
        }
    })
}

/// `t` with placeholders `{0}`, `{1}`, … filled in, in order.
pub fn tf(key: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let mut text = t(key).to_string();
    for (index, arg) in args.iter().enumerate() {
        text = text.replace(&format!("{{{index}}}"), &arg.to_string());
    }
    text
}
