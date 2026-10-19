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
        Self::ALL.into_iter().find(|lang| lang.code() == code).unwrap_or(Lang::En)
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

/// The saved language; English when nothing is saved or storage is unavailable.
pub fn load() -> Lang {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(KEY).ok().flatten())
        .map(|code| Lang::from_code(&code))
        .unwrap_or(Lang::En)
}

pub fn save(lang: Lang) {
    if let Some(storage) = web_sys::window().and_then(|window| window.local_storage().ok().flatten()) {
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

/// `t` with placeholders `{0}`, `{1}`, … filled in, in order.
pub fn tf(key: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let mut text = t(key).to_string();
    for (index, arg) in args.iter().enumerate() {
        text = text.replace(&format!("{{{index}}}"), &arg.to_string());
    }
    text
}
