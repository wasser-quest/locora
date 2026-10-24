//! Matching typed shopping-list answers against the word list (spec §8.7).
//!
//! Answers and accepted forms go through the same [`normalize`]: case,
//! full-width forms, katakana and Latin diacritics do not matter, and neither
//! do spaces, hyphens or apostrophes. A Latin form (pinyin too) of at least 5
//! characters or a kana form of at least 4 also accepts one typo: one letter
//! more, less, different, or two neighbours swapped. Ideographs must match
//! exactly; their readings are listed forms, so no IME choice costs a point.

use crate::Lang;
use crate::words::pool;

/// How an answer fits a concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Exact,
    /// One typo away from a form long enough to allow it.
    Tolerated,
    No,
}

/// Fixed fold table of Latin diacritics (after lowercasing). Beyond the spec's
/// table it folds the pinyin tone marks ǎ ě ī ǐ ǒ ǔ ǖ ǘ ǚ ǜ.
fn fold(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ǎ' => "a",
        'ç' => "c",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ě' => "e",
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ǐ' => "i",
        'ñ' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ǒ' => "o",
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ǔ' | 'ǖ' | 'ǘ' | 'ǚ' | 'ǜ' => "u",
        'ý' | 'ÿ' => "y",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        _ => return None,
    })
}

/// The comparable form of an answer or an accepted form:
///
/// 1. lowercase;
/// 2. full-width ASCII (U+FF01–FF5E) to ASCII, the ideographic space to a space;
/// 3. katakana (U+30A1–30F6) to hiragana;
/// 4. Latin diacritics folded with a fixed table (à…ā to a, ç to c, è…ē to e,
///    ì…ï to i, ñ to n, ò…ø to o, ù…ū to u, ý ÿ to y, and the pinyin tone
///    marks), ß to ss, æ to ae, œ to oe;
/// 5. everything that is not alphanumeric dropped (spaces, hyphens,
///    apostrophes, combining marks); the long-vowel mark ー stays.
///
/// The steps are the same in every language in content version 1; `lang` keeps
/// the signature stable for a later version.
pub fn normalize(lang: Lang, text: &str) -> String {
    let _ = lang;
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        let c = match u32::from(c) {
            0xff01..=0xff5e => char::from_u32(u32::from(c) - 0xfee0).unwrap_or(c),
            0x3000 => ' ',
            0x30a1..=0x30f6 => char::from_u32(u32::from(c) - 0x60).unwrap_or(c),
            _ => c,
        };
        match fold(c) {
            Some(folded) => out.push_str(folded),
            None if c.is_alphanumeric() => out.push(c),
            None => {}
        }
    }
    out
}

/// Whether a text holds no letter or digit, so it counts as an empty place.
pub(crate) fn blank(text: &str) -> bool {
    normalize(Lang::En, text).is_empty()
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{3096}' | 'ー')
}

/// Whether a normalized form accepts one typo: Latin (ASCII letters and digits)
/// from 5 characters, kana from 4; never a form with ideographs.
pub fn accepts_typo(normalized: &str) -> bool {
    let n = normalized.chars().count();
    if normalized.chars().all(|c| c.is_ascii_alphanumeric()) {
        n >= 5
    } else if normalized.chars().all(is_kana) {
        n >= 4
    } else {
        false
    }
}

/// Whether the optimal-string-alignment distance of `a` and `b` is at most 1:
/// equal, or one character inserted, deleted, replaced, or two neighbours swapped.
pub fn within_one(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    match long.len() - short.len() {
        0 => {
            let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
            match diff[..] {
                [] | [_] => true,
                [i, j] => j == i + 1 && a[i] == b[j] && a[j] == b[i],
                _ => false,
            }
        }
        1 => {
            let same = short
                .iter()
                .zip(long.iter())
                .take_while(|(x, y)| x == y)
                .count();
            short[same..] == long[same + 1..]
        }
        _ => false,
    }
}

/// How `normalized` (an answer after [`normalize`]) fits one of `forms`.
pub(crate) fn fit(forms: &[String], normalized: &str) -> Match {
    if forms.iter().any(|f| f == normalized) {
        Match::Exact
    } else if forms
        .iter()
        .any(|f| accepts_typo(f) && within_one(f, normalized))
    {
        Match::Tolerated
    } else {
        Match::No
    }
}

/// How `answer` fits concept `id` in `lang`; `No` for an id the list does not have.
pub fn matches(id: u16, lang: Lang, answer: &str) -> Match {
    let Some(concept) = pool().get(id) else {
        return Match::No;
    };
    let answer = normalize(lang, answer);
    if answer.is_empty() {
        return Match::No;
    }
    fit(concept.normalized(lang), &answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(text: &str) -> String {
        normalize(Lang::En, text)
    }

    #[test]
    fn normalize_follows_the_five_steps() {
        assert_eq!(n("Äpfel"), "apfel");
        assert_eq!(n("ÆBLE œuf Straße"), "aebleoeufstrasse");
        assert_eq!(n("pomme de terre"), "pommedeterre");
        assert_eq!(n("chewing-gum"), "chewinggum");
        assert_eq!(n("huile d’olive"), "huiledolive");
        assert_eq!(n("huile d'olive"), "huiledolive");
        assert_eq!(n("Ｍｉｌｋ"), "milk");
        assert_eq!(n("ミルク"), "みるく");
        assert_eq!(n("ヨーグルト"), "よーぐると");
        assert_eq!(n("玉ねぎ"), "玉ねぎ");
        assert_eq!(n("牛乳\u{3000}"), "牛乳");
        assert_eq!(n("ﾐﾙｸ"), "ﾐﾙｸ"); // half-width katakana are not mapped
        // Combining accents are dropped, so decomposed input folds too.
        assert_eq!(n("cafe\u{301}"), "cafe");
        assert_eq!(n("píngguǒ"), "pingguo");
        assert_eq!(n("lǜchá"), "lucha");
        assert_eq!(n(" 2 Liter! "), "2liter");
        assert_eq!(n("…"), "");
    }

    #[test]
    fn which_forms_accept_a_typo() {
        assert!(accepts_typo("lemon"));
        assert!(!accepts_typo("pear"));
        assert!(accepts_typo("pingguo"));
        assert!(accepts_typo("きゅうり"));
        assert!(!accepts_typo("りんご"));
        assert!(accepts_typo("よーぐると"));
        assert!(!accepts_typo("玉ねぎ"));
        assert!(!accepts_typo("西红柿"));
        assert!(!accepts_typo("kürbis")); // not folded: not plain Latin
    }

    #[test]
    fn one_edit_at_most() {
        for (a, b) in [
            ("lemon", "lemon"),
            ("lemon", "lemn"),
            ("lemon", "lemons"),
            ("lemon", "lemen"),
            ("lemon", "lmeon"),
            ("lemon", "elmon"),
            ("lemon", "lemno"),
            ("", "a"),
        ] {
            assert!(within_one(a, b), "{a} {b}");
            assert!(within_one(b, a), "{b} {a}");
        }
        for (a, b) in [
            ("lemon", "melon"),
            ("lemon", "lem"),
            ("lemon", "lmeno"),
            ("lemon", "nomel"),
            ("abc", "cab"),
            ("", "ab"),
        ] {
            assert!(!within_one(a, b), "{a} {b}");
        }
        assert!(within_one("きゅうり", "きゅり"));
    }

    #[test]
    fn matches_against_the_word_list() {
        let id = |key: &str| pool().concepts().iter().find(|c| c.key == key).unwrap().id;
        let apple = id("apple");
        assert_eq!(matches(apple, Lang::En, "Apples"), Match::Exact);
        assert_eq!(matches(apple, Lang::En, "appel"), Match::Tolerated);
        assert_eq!(matches(apple, Lang::En, "aple"), Match::Tolerated);
        assert_eq!(matches(apple, Lang::En, "Apfel"), Match::No); // German in an English round
        assert_eq!(matches(apple, Lang::De, "aepfel"), Match::Tolerated);
        assert_eq!(matches(apple, Lang::Ja, "リンゴ"), Match::Exact);
        assert_eq!(matches(apple, Lang::Zh, "蘋果"), Match::Exact);
        assert_eq!(matches(apple, Lang::Zh, "pingguo"), Match::Exact);
        assert_eq!(matches(apple, Lang::Zh, "pinguo"), Match::Tolerated);
        assert_eq!(matches(apple, Lang::En, ""), Match::No);
        assert_eq!(matches(0, Lang::En, "apple"), Match::No);
        // Too short for a typo: "pear" is four letters.
        assert_eq!(matches(id("pear"), Lang::En, "peat"), Match::No);
        // Ideographs need the exact character.
        assert_eq!(matches(id("tomato"), Lang::Zh, "西红"), Match::No);
    }
}
