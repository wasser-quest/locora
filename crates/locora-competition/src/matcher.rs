//! Matching typed shopping-list answers against the word list (spec §8.7, with
//! the additions of the word-list reviews in addendum 1).
//!
//! Answers and accepted forms go through the same [`normalize`]: case,
//! full-width and half-width forms, katakana, Latin diacritics, a leading
//! article (en, de, fr) and a quantity (zh) do not matter, and neither do
//! spaces, hyphens or apostrophes. A Latin form (pinyin too) of at least 5
//! characters or a kana form of at least 4 also accepts one typo: one letter
//! more, less, different, or two neighbours swapped. Ideographs must match
//! exactly; their readings are listed forms, so no IME choice costs a point.
//! An answer that is exactly a form of another concept is never a typo.

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

/// Fixed fold table of Latin diacritics (after lowercasing), including the
/// pinyin tone marks ǎ ě ī ǐ ǒ ǔ ǖ ǘ ǚ ǜ.
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

/// Full-width katakana for the half-width ones U+FF66–FF9D (ｦ … ﾝ, with ｰ as ー).
const HALF_WIDTH_KATAKANA: &str = concat!(
    "ヲァィゥェォャュョッー",
    "アイウエオカキクケコサシスセソタチツテト",
    "ナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン",
);

/// The voiced (゛) or semi-voiced (゜) sound mark joined with the hiragana before
/// it. Decomposed text (か + U+3099) and half-width katakana (ｶﾞ) type them apart.
fn join_mark(kana: char, semi: bool) -> Option<char> {
    match (kana, semi) {
        ('う', false) => Some('ゔ'),
        ('ゝ', false) => Some('ゞ'),
        (_, false) if "かきくけこさしすせそたちつてとはひふへほ".contains(kana) => {
            char::from_u32(u32::from(kana) + 1)
        }
        (_, true) if "はひふへほ".contains(kana) => char::from_u32(u32::from(kana) + 2),
        _ => None,
    }
}

/// Steps 1 to 4 of [`normalize`], character by character; nothing is dropped yet.
fn map_chars(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        let c = match u32::from(c) {
            0xff01..=0xff5e => char::from_u32(u32::from(c) - 0xfee0).unwrap_or(c),
            0x3000 => ' ',
            0xff66..=0xff9d => HALF_WIDTH_KATAKANA
                .chars()
                .nth((u32::from(c) - 0xff66) as usize)
                .unwrap_or(c),
            _ => c,
        };
        let c = match u32::from(c) {
            0x30a1..=0x30f6 => char::from_u32(u32::from(c) - 0x60).unwrap_or(c),
            _ => c,
        };
        let semi = match u32::from(c) {
            0x3099 | 0x309b | 0xff9e => Some(false),
            0x309a | 0x309c | 0xff9f => Some(true),
            _ => None,
        };
        if let Some(semi) = semi {
            // A mark that joins nothing is dropped, as step 5 drops every mark.
            if let Some(joined) = out.chars().last().and_then(|k| join_mark(k, semi)) {
                out.pop();
                out.push(joined);
            }
            continue;
        }
        match fold(c) {
            Some(folded) => out.push_str(folded),
            None => out.push(c),
        }
    }
    out
}

/// Articles an answer may start with, longest first where one begins another.
/// The French elided ones end in an apostrophe and need no space after them.
fn articles(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::En => &["a", "an", "the", "some"],
        Lang::De => &[
            "der", "die", "das", "den", "dem", "des", "ein", "eine", "einen", "einem", "einer",
            "eines",
        ],
        Lang::Fr => &[
            "de la", "de l'", "de l’", "des", "du", "de", "les", "le", "la", "l'", "l’", "une",
            "un", "d'", "d’",
        ],
        Lang::Ja | Lang::Zh => &[],
    }
}

/// `text` (after steps 1 to 4) without one leading article, when a word follows
/// it: "an apple", "die Milch", "l’huile", "de la farine". An article alone stays.
fn without_article(lang: Lang, text: &str) -> &str {
    let trimmed = text.trim_start();
    for article in articles(lang) {
        let Some(rest) = trimmed.strip_prefix(article) else {
            continue;
        };
        let elided = article.ends_with(['\'', '’']);
        if (elided || rest.starts_with(char::is_whitespace))
            && rest.chars().any(char::is_alphanumeric)
        {
            return rest;
        }
    }
    text
}

/// Numerals that open a Chinese quantity, besides ASCII digits.
const ZH_NUMERALS: &str = "〇零一二两三四五六七八九十百千万半几";

/// Measure words and units after a numeral ("一瓶", "两盒", "500克"); the
/// two-character ones first, so "公斤" is not read as "斤".
const ZH_MEASURES: [&str; 48] = [
    "公斤", "千克", "毫升", "个", "只", "支", "枝", "条", "根", "块", "片", "瓶", "罐", "听", "盒",
    "包", "袋", "箱", "桶", "斤", "克", "升", "两", "把", "串", "颗", "粒", "棵", "头", "束", "朵",
    "份", "打", "卷", "板", "提", "双", "套", "节", "管", "杯", "盘", "张", "排", "捆", "扎", "些",
    "点",
];

fn zh_numeral(c: char) -> bool {
    c.is_ascii_digit() || ZH_NUMERALS.contains(c)
}

/// Step 7 for Chinese: a quantity before or after the item is dropped
/// ("一瓶牛奶", "一斤半苹果", "鸡蛋两盒", "牛奶x2"). A numeral needs a measure word
/// after it, so "三文鱼" and "四季豆" stay whole, and a quantity alone stays.
fn drop_quantity(text: String) -> String {
    let mut s = text.as_str();
    // Before the item: numerals, a measure word, perhaps 半 ("一斤半").
    let numerals: Vec<usize> = s
        .char_indices()
        .take_while(|&(_, c)| zh_numeral(c))
        .map(|(i, c)| i + c.len_utf8())
        .collect();
    for &end in numerals.iter().rev() {
        let rest = &s[end..];
        if let Some(m) = ZH_MEASURES.iter().find(|m| rest.starts_with(**m)) {
            let rest = &rest[m.len()..];
            let rest = rest
                .strip_prefix('半')
                .filter(|r| !r.is_empty())
                .unwrap_or(rest);
            if !rest.is_empty() {
                s = rest;
            }
            break;
        }
    }
    // After the item: numerals and a measure word, or digits after an optional x.
    if let Some(m) = ZH_MEASURES.iter().find(|m| s.ends_with(**m)) {
        let head = &s[..s.len() - m.len()];
        let item = head.trim_end_matches(zh_numeral);
        if item.len() < head.len() && !item.is_empty() {
            return item.to_string();
        }
    }
    let digits = s.trim_end_matches(|c: char| c.is_ascii_digit());
    if digits.len() < s.len() {
        let item = digits.strip_suffix('x').unwrap_or(digits);
        if !item.is_ascii() {
            return item.to_string();
        }
    }
    s.to_string()
}

/// The comparable form of an answer or an accepted form:
///
/// 1. lowercase;
/// 2. full-width ASCII (U+FF01–FF5E) to ASCII, the ideographic space to a
///    space, half-width katakana (U+FF66–FF9D) to full-width;
/// 3. katakana (U+30A1–30F6) to hiragana, and a voiced or semi-voiced sound
///    mark joined with the kana before it (か with U+3099, ｶﾞ and か゛ are が);
/// 4. Latin diacritics folded with a fixed table (à…ā ǎ to a, ç to c, è…ē ě
///    to e, ì…ï ī ǐ to i, ñ to n, ò…ø ō ǒ to o, ù…ū ǔ ǖ ǘ ǚ ǜ to u, ý ÿ to y),
///    ß to ss, æ to ae, œ to oe;
/// 5. en, de, fr: one leading article dropped when a word follows it (en a, an,
///    the, some; de der, die, das, den, dem, des, ein, eine, einen, einem,
///    einer, eines; fr le, la, les, l’, un, une, des, du, de, de la, de l’, d’);
/// 6. everything that is not alphanumeric dropped (spaces, hyphens,
///    apostrophes, combining marks); the long-vowel mark ー stays;
/// 7. de: "ae", "oe" and "ue" read as "a", "o" and "u", the way ä, ö and ü are
///    typed without those keys; zh: a quantity dropped (see `drop_quantity`).
pub fn normalize(lang: Lang, text: &str) -> String {
    let mapped = map_chars(text);
    let kept: String = without_article(lang, &mapped)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    match lang {
        // "ae", "oe", "ue" are how ä, ö, ü are typed without them.
        Lang::De => kept
            .replace("ae", "a")
            .replace("oe", "o")
            .replace("ue", "u"),
        Lang::Zh => drop_quantity(kept),
        Lang::En | Lang::Fr | Lang::Ja => kept,
    }
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

/// How `answer` fits concept `id` in `lang`; `No` for an id the list does not
/// have, and for an answer that is exactly a form of another concept.
pub fn matches(id: u16, lang: Lang, answer: &str) -> Match {
    let Some(concept) = pool().get(id) else {
        return Match::No;
    };
    let answer = normalize(lang, answer);
    if answer.is_empty() {
        return Match::No;
    }
    match fit(concept.normalized(lang), &answer) {
        // Exactly another concept counts as that one, never as a typo of this one,
        // as in the score of a list: "ドーナツ" (doughnut) is not "ピーナツ" misspelt.
        Match::Tolerated if pool().find(lang, &answer).is_some_and(|other| other != id) => {
            Match::No
        }
        fit => fit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(text: &str) -> String {
        normalize(Lang::En, text)
    }

    #[test]
    fn normalize_follows_the_steps() {
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
        // Combining accents are dropped, so decomposed Latin input folds too.
        assert_eq!(n("cafe\u{301}"), "cafe");
        assert_eq!(n("píngguǒ"), "pingguo");
        assert_eq!(n("lǜchá"), "lucha");
        assert_eq!(n(" 2 Liter! "), "2liter");
        assert_eq!(n("…"), "");
    }

    #[test]
    fn kana_marks_join_and_half_width_katakana_widen() {
        // Decomposed kana: the sound mark joins the kana before it.
        assert_eq!(n("り\u{3093}こ\u{3099}"), "りんご");
        assert_eq!(n("き\u{3099}ゅうにゅう"), "ぎゅうにゅう");
        assert_eq!(n("ハ\u{3099}ナナ"), "ばなな");
        assert_eq!(n("ハ\u{309a}ン"), "ぱん");
        assert_eq!(n("か゛"), "が");
        assert_eq!(n("ウ\u{3099}"), "ゔ");
        // Half-width katakana, as a desktop input method types them with F8.
        assert_eq!(HALF_WIDTH_KATAKANA.chars().count(), 0xff9d - 0xff66 + 1);
        assert_eq!(n("ﾐﾙｸ"), "みるく");
        assert_eq!(n("ﾘﾝｺﾞ"), "りんご");
        assert_eq!(n("ﾊﾟﾝ"), "ぱん");
        assert_eq!(n("ﾖｰｸﾞﾙﾄ"), "よーぐると");
        // A mark with nothing to join is dropped, as before.
        assert_eq!(n("ん\u{3099}"), "ん");
        assert_eq!(n("a\u{3099}"), "a");
        assert_eq!(n("ﾞ"), "");
    }

    #[test]
    fn a_leading_article_is_dropped_in_its_language() {
        assert_eq!(normalize(Lang::En, "an apple"), "apple");
        assert_eq!(normalize(Lang::En, "A Pear"), "pear");
        assert_eq!(normalize(Lang::En, "some milk"), "milk");
        assert_eq!(normalize(Lang::En, "the eggs"), "eggs");
        // Not an article: no word after it, or part of the word.
        assert_eq!(normalize(Lang::En, "a"), "a");
        assert_eq!(normalize(Lang::En, "the ..."), "the");
        assert_eq!(normalize(Lang::En, "anchovies"), "anchovies");
        assert_eq!(normalize(Lang::De, "die Milch"), "milch");
        assert_eq!(normalize(Lang::De, "ein Brot"), "brot");
        assert_eq!(normalize(Lang::De, "eine Gurke"), "gurke");
        assert_eq!(normalize(Lang::De, "Eis"), "eis");
        assert_eq!(normalize(Lang::Fr, "du lait"), "lait");
        assert_eq!(normalize(Lang::Fr, "des pommes"), "pommes");
        assert_eq!(normalize(Lang::Fr, "les œufs"), "oeufs");
        assert_eq!(normalize(Lang::Fr, "l’ail"), "ail");
        assert_eq!(normalize(Lang::Fr, "L'huile"), "huile");
        assert_eq!(normalize(Lang::Fr, "de la farine"), "farine");
        assert_eq!(normalize(Lang::Fr, "de l’huile"), "huile");
        assert_eq!(normalize(Lang::Fr, "la"), "la");
        // Only one article, and only in its own language.
        assert_eq!(normalize(Lang::En, "a an apple"), "anapple");
        assert_eq!(normalize(Lang::En, "die Milch"), "diemilch");
        assert_eq!(normalize(Lang::Ja, "the milk"), "themilk");
    }

    #[test]
    fn no_form_of_the_list_begins_with_an_article() {
        for concept in pool().concepts() {
            for lang in Lang::ALL {
                for form in concept.forms(lang) {
                    let mapped = map_chars(form);
                    assert_eq!(without_article(lang, &mapped), mapped, "{lang:?} {form}");
                }
            }
        }
    }

    #[test]
    fn german_reads_ae_oe_ue_as_umlauts() {
        let de = |text| normalize(Lang::De, text);
        assert_eq!(de("Aepfel"), de("Äpfel"));
        assert_eq!(de("Broetchen"), de("Brötchen"));
        assert_eq!(de("Zuendhoelzer"), de("Zündhölzer"));
        assert_eq!(de("Muellsaecke"), "mullsacke");
        assert_eq!(de("Feuerzeug"), "feurzeug");
        // Only German reads them so.
        assert_eq!(normalize(Lang::En, "Aepfel"), "aepfel");
    }

    #[test]
    fn chinese_drops_a_quantity() {
        let zh = |text| normalize(Lang::Zh, text);
        for (answer, item) in [
            ("一瓶牛奶", "牛奶"),
            ("两盒鸡蛋", "鸡蛋"),
            ("鸡蛋两盒", "鸡蛋"),
            ("３个苹果", "苹果"),
            ("3个苹果", "苹果"),
            ("苹果两个", "苹果"),
            ("牛奶×2", "牛奶"),
            ("牛奶x2", "牛奶"),
            ("牛奶 2瓶", "牛奶"),
            ("二两牛肉", "牛肉"),
            ("一斤半苹果", "苹果"),
            ("2公斤苹果", "苹果"),
            ("500克牛肉", "牛肉"),
            ("一束花", "花"),
        ] {
            assert_eq!(zh(answer), item, "{answer}");
        }
        // A numeral without a measure word is part of the item, and a quantity
        // alone stays what it is.
        for whole in ["三文鱼", "四季豆", "打火机", "一个", "2", "pingguo2"] {
            assert_eq!(zh(whole), n(whole), "{whole}");
        }
        for concept in pool().concepts() {
            for form in concept.normalized(Lang::Zh) {
                assert_eq!(drop_quantity(form.clone()), *form, "{}", concept.key);
            }
        }
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
        assert_eq!(matches(apple, Lang::De, "aepfel"), Match::Exact);
        assert_eq!(matches(apple, Lang::En, "an apple"), Match::Exact);
        assert_eq!(matches(apple, Lang::Fr, "des pommes"), Match::Exact);
        assert_eq!(matches(apple, Lang::Ja, "ﾘﾝｺﾞ"), Match::Exact);
        assert_eq!(matches(apple, Lang::Zh, "3个苹果"), Match::Exact);
        assert_eq!(matches(id("milk"), Lang::De, "die Milch"), Match::Exact);
        assert_eq!(matches(id("milk"), Lang::En, "die Milch"), Match::No);
        assert_eq!(
            matches(id("matches"), Lang::De, "Zuendhoelzer"),
            Match::Exact
        );
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

    #[test]
    fn another_concept_is_never_a_typo_and_score_agrees() {
        // Concepts one typo apart share a group, so they never meet in a list, but
        // an answer that is exactly one of them still never counts for the other.
        let concepts = pool().concepts();
        let mut pairs = 0;
        for a in concepts {
            for b in concepts {
                if a.id == b.id || a.group.is_none() || a.group != b.group {
                    continue;
                }
                for lang in Lang::ALL {
                    for form in a.forms(lang) {
                        assert_eq!(matches(b.id, lang, form), Match::No, "{lang:?} {form}");
                        let task = crate::Task::List(vec![b.id]);
                        let recall = crate::Recall::List(vec![form.to_string()]);
                        let score = crate::score(&task, &recall, lang).unwrap();
                        assert_eq!(score.marks, [crate::Mark::Wrong], "{lang:?} {form}");
                        let fits = fit(b.normalized(lang), &normalize(lang, form));
                        pairs += usize::from(fits == Match::Tolerated);
                    }
                }
            }
        }
        // The pairs this guards: œuf and bœuf, ドーナツ and ピーナツ, mangguo and tangguo …
        assert!(pairs >= 4, "{pairs}");
        let id = |key: &str| concepts.iter().find(|c| c.key == key).unwrap().id;
        assert_eq!(matches(id("peanuts"), Lang::Ja, "ドーナツ"), Match::No);
        assert_eq!(matches(id("beef"), Lang::Fr, "oeuf"), Match::No);
        assert_eq!(matches(id("sweets"), Lang::Zh, "mangguo"), Match::No);
    }
}
