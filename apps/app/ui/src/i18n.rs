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

/// Whether a language was chosen (saved) on this device. Without storage, it counts as chosen.
pub fn is_saved() -> bool {
    match web_sys::window().and_then(|window| window.local_storage().ok().flatten()) {
        Some(storage) => storage.get_item(KEY).ok().flatten().is_some(),
        None => true,
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

/// A message from the backend (an error, a note) in the current language. An error chain
/// `context: cause: cause` is translated part by part when the whole has no translation, so a
/// known cause (such as a Locora Cloud message) is translated under any context.
pub fn translate_message(text: &str) -> String {
    let whole = translate(text);
    if whole != text || !text.contains(": ") {
        return whole;
    }
    text.split(": ")
        .map(translate)
        .collect::<Vec<_>>()
        .join(": ")
}

/// `t` with placeholders `{0}`, `{1}`, … filled in, in order.
pub fn tf(key: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let mut text = t(key).to_string();
    for (index, arg) in args.iter().enumerate() {
        text = text.replace(&format!("{{{index}}}"), &arg.to_string());
    }
    text
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::i18n_catalog::MESSAGES;

    /// Every UI source except the catalog itself.
    const SOURCES: &[(&str, &str)] = &[
        ("achievements.rs", include_str!("achievements.rs")),
        ("activity.rs", include_str!("activity.rs")),
        ("audio.rs", include_str!("audio.rs")),
        ("auto_sync.rs", include_str!("auto_sync.rs")),
        ("cloud_sync_form.rs", include_str!("cloud_sync_form.rs")),
        ("cloud_settings.rs", include_str!("cloud_settings.rs")),
        ("crash.rs", include_str!("crash.rs")),
        ("dashboard.rs", include_str!("dashboard.rs")),
        ("dashboard_editor.rs", include_str!("dashboard_editor.rs")),
        ("editor.rs", include_str!("editor.rs")),
        ("formulas.rs", include_str!("formulas.rs")),
        ("i18n.rs", include_str!("i18n.rs")),
        ("info.rs", include_str!("info.rs")),
        ("journey.rs", include_str!("journey.rs")),
        ("language_choice.rs", include_str!("language_choice.rs")),
        ("locus_display.rs", include_str!("locus_display.rs")),
        ("look.rs", include_str!("look.rs")),
        ("main.rs", include_str!("main.rs")),
        ("media_format.rs", include_str!("media_format.rs")),
        ("palette.rs", include_str!("palette.rs")),
        ("settings.rs", include_str!("settings.rs")),
        ("sply.rs", include_str!("sply.rs")),
        ("startup.rs", include_str!("startup.rs")),
        ("tauri.rs", include_str!("tauri.rs")),
        ("theme.rs", include_str!("theme.rs")),
        ("video_frames.rs", include_str!("video_frames.rs")),
    ];

    /// The string literal at the start of `text` (after its opening quote), unescaped.
    fn literal(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' => return out,
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    // A line continuation drops the line break and the next line's indent.
                    Some('\n') => while chars.next_if(|c| c.is_whitespace()).is_some() {},
                    Some('u') => {
                        let code: String =
                            chars.by_ref().skip(1).take_while(|c| *c != '}').collect();
                        out.extend(u32::from_str_radix(&code, 16).ok().and_then(char::from_u32));
                    }
                    Some(other) => out.push(other),
                    None => break,
                },
                _ => out.push(c),
            }
        }
        panic!("unterminated string literal: {out}");
    }

    /// The German keys a source looks up literally: a string literal as the first argument of
    /// `t` or `tf` (also on the next line) and the `text` of an `Info` explanation.
    fn keys(source: &str) -> Vec<String> {
        let mut keys = Vec::new();
        let calls = [
            String::from("t") + "(",
            String::from("tf") + "(",
            concat!("<Info", " text=").into(),
        ];
        for call in &calls {
            for (at, _) in source.match_indices(call.as_str()) {
                let word = call.starts_with(|c: char| c.is_alphabetic());
                let before = source[..at].chars().next_back();
                if word && before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let rest = source[at + call.len()..].trim_start();
                if let Some(rest) = rest.strip_prefix('"') {
                    keys.push(literal(rest));
                }
            }
        }
        keys
    }

    #[test]
    fn every_literal_key_has_a_catalog_row() {
        let catalog: HashSet<&str> = MESSAGES.iter().map(|(key, _)| *key).collect();
        let missing: Vec<String> = SOURCES
            .iter()
            .flat_map(|(file, source)| {
                keys(source)
                    .into_iter()
                    .filter(|key| !catalog.contains(key.as_str()))
                    .map(move |key| format!("{file}: {key:?}"))
            })
            .collect();
        assert!(
            missing.is_empty(),
            "keys without a catalog row:\n{}",
            missing.join("\n")
        );
    }

    #[test]
    fn the_key_scan_finds_split_and_escaped_calls() {
        let source = concat!(
            "x(t(\"A\"), tf(\n  \"B {0}\", &[&1]), est(\"no\"), ",
            "<Info text=\"C \\\"D\\\"\" />, t(\"E \\\n     F\"))"
        );
        assert_eq!(keys(source), ["A", "E F", "B {0}", "C \"D\""]);
    }

    #[test]
    fn catalog_rows_are_unique_and_complete() {
        let mut seen = HashSet::new();
        for (key, texts) in MESSAGES {
            assert!(seen.insert(*key), "duplicate catalog row: {key:?}");
            assert_eq!(texts[1], *key, "the German column must equal the key");
            assert!(
                texts.iter().all(|text| !text.is_empty()),
                "empty translation: {key:?}"
            );
        }
    }

    #[test]
    fn an_error_chain_is_translated_part_by_part() {
        super::set_current(super::Lang::En);
        assert_eq!(
            super::translate_message("Unbekannt: Locora Cloud nicht erreichbar"),
            "Unbekannt: Locora Cloud is not reachable"
        );
        assert_eq!(super::translate_message("Unbekannt"), "Unbekannt");
    }
}
