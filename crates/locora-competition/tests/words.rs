//! Integrity of the word list `groceries-v1` (spec §8.6) and its freeze.
//!
//! These checks keep matching fair: no two concepts share a form, and no typo
//! the matcher forgives can land on another concept, unless the two are in one
//! exclusivity group and so never in the same list.

use locora_competition::matcher::{Match, accepts_typo, matches, within_one};
use locora_competition::words::{Category, Concept, display, pool};
use locora_competition::{
    Discipline, Lang, Level, Mark, Recall, Seed, Spec, Task, generate, rng, score,
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

const TSV: &str = include_str!("../data/groceries-v1.tsv");

/// SHA-256 of `data/groceries-v1.tsv` (line ends as LF). The freeze is binding
/// from the first release that ships the contest; until then a reviewed
/// correction updates this hash and `tests/golden/list.json` in the same commit
/// (addendum D15). After that, corrections go into `groceries-v2.tsv`.
const FROZEN: &str = "54a12b704b22f0dc47f9704e9fdb1cea2c7a93a58e076b608d0ad5cf0490dd27";

#[test]
fn groceries_v1_is_frozen() {
    let text = TSV.replace("\r\n", "\n");
    let hash: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        hash, FROZEN,
        "groceries-v1.tsv changed: content version 1 is frozen (see data/REVIEW.md)"
    );
}

fn concepts() -> &'static [Concept] {
    pool().concepts()
}

#[test]
fn every_concept_has_five_languages_and_a_category() {
    assert!(concepts().len() >= 100, "{}", concepts().len());
    for c in concepts() {
        assert!(Category::ALL.contains(&c.category));
        for lang in Lang::ALL {
            assert!(!c.forms(lang).is_empty(), "{} {lang:?}", c.key);
            for (form, normalized) in c.forms(lang).iter().zip(c.normalized(lang)) {
                assert!(!normalized.is_empty(), "{} {lang:?}: {form}", c.key);
                assert_eq!(form.trim(), *form, "{} {lang:?}: {form:?}", c.key);
            }
        }
    }
}

#[test]
fn ids_are_unique_and_ascending_from_one_and_keys_unique() {
    let ids: Vec<u16> = concepts().iter().map(|c| c.id).collect();
    assert_eq!(ids[0], 1);
    assert!(ids.windows(2).all(|w| w[0] < w[1]));
    let keys: HashSet<&str> = concepts().iter().map(|c| c.key).collect();
    assert_eq!(keys.len(), concepts().len());
    assert!(keys.iter().all(|k| {
        k.bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit())
    }));
}

#[test]
fn every_category_has_three_concepts_outside_groups() {
    // A list of 30 takes 3 from each of the 10 categories.
    for category in Category::ALL {
        let free = concepts()
            .iter()
            .filter(|c| c.category == category && c.group.is_none())
            .count();
        assert!(free >= 3, "{category:?}: {free}");
    }
}

#[test]
fn groups_have_at_least_two_members() {
    let mut groups: HashMap<&str, usize> = HashMap::new();
    for group in concepts().iter().filter_map(|c| c.group) {
        *groups.entry(group).or_default() += 1;
    }
    for (group, members) in groups {
        assert!(members >= 2, "group {group} has one member");
    }
}

#[test]
fn display_forms_fit_the_screen() {
    for c in concepts() {
        for lang in Lang::ALL {
            let display = c.display(lang);
            let limit = match lang {
                Lang::Ja => 5,
                Lang::Zh => 3,
                Lang::En | Lang::De | Lang::Fr => 14,
            };
            let length = display.chars().count();
            assert!(length <= limit, "{} {lang:?}: {display} ({length})", c.key);
        }
    }
}

#[test]
fn readings_and_pinyin_are_listed() {
    // Typing the reading always counts, so no choice in an input method costs a point.
    let kana = |s: &str| {
        s.chars()
            .all(|ch| matches!(ch, '\u{3041}'..='\u{3096}' | 'ー'))
    };
    for c in concepts() {
        assert!(
            c.normalized(Lang::Ja).iter().any(|f| kana(f)),
            "{}: no reading in kana",
            c.key
        );
        assert!(
            c.normalized(Lang::Zh)
                .iter()
                .any(|f| f.chars().all(|ch| ch.is_ascii_lowercase())),
            "{}: no pinyin",
            c.key
        );
    }
}

#[test]
fn no_two_concepts_share_a_form() {
    for lang in Lang::ALL {
        let mut owner: HashMap<&str, &str> = HashMap::new();
        for c in concepts() {
            for form in c.normalized(lang) {
                if let Some(other) = owner.insert(form, c.key) {
                    assert_eq!(other, c.key, "{lang:?}: {form} in {other} and {}", c.key);
                }
            }
        }
    }
}

#[test]
fn a_typo_never_reaches_another_concept() {
    for lang in Lang::ALL {
        let forms: Vec<(&Concept, &str)> = concepts()
            .iter()
            .flat_map(|c| c.normalized(lang).iter().map(move |f| (c, f.as_str())))
            .collect();
        for (i, &(a, fa)) in forms.iter().enumerate() {
            for &(b, fb) in &forms[i + 1..] {
                let same_group = a.group.is_some() && a.group == b.group;
                if a.id == b.id || same_group || !(accepts_typo(fa) || accepts_typo(fb)) {
                    continue;
                }
                assert!(
                    !within_one(fa, fb),
                    "{lang:?}: {} „{fa}“ and {} „{fb}“ are one typo apart",
                    a.key,
                    b.key
                );
            }
        }
    }
}

#[test]
fn the_first_palace_items_are_in_the_list() {
    for key in ["milk", "bread", "apple", "cheese", "coffee"] {
        assert!(concepts().iter().any(|c| c.key == key), "{key}");
    }
}

#[test]
fn lists_keep_their_rules_and_reach_every_concept() {
    let mut seen: HashSet<u16> = HashSet::new();
    let mut seeds = rng::SplitMix64::new(Seed::masked(8));
    for _ in 0..10_000 {
        let seed = Seed::masked(seeds.next_u64());
        for level in Level::ALL {
            let spec = Spec::standard(Discipline::List, level);
            let Task::List(ids) = generate(&spec, seed) else {
                panic!("not a list");
            };
            assert_eq!(ids.len(), usize::from(spec.items));
            let unique: HashSet<u16> = ids.iter().copied().collect();
            assert_eq!(unique.len(), ids.len(), "repeated concept");
            let cap = usize::from(spec.items).div_ceil(Category::ALL.len());
            let mut per_category: HashMap<Category, usize> = HashMap::new();
            let mut groups: HashSet<&str> = HashSet::new();
            for &id in &ids {
                let c = pool().get(id).unwrap();
                let count = per_category.entry(c.category).or_default();
                *count += 1;
                assert!(*count <= cap, "{:?} over {cap}", c.category);
                if let Some(group) = c.group {
                    assert!(groups.insert(group), "two of group {group}");
                }
            }
            seen.extend(ids);
        }
    }
    let missing: Vec<&str> = concepts()
        .iter()
        .filter(|c| !seen.contains(&c.id))
        .map(|c| c.key)
        .collect();
    assert!(missing.is_empty(), "never drawn: {missing:?}");
}

fn concept(key: &str) -> &'static Concept {
    concepts()
        .iter()
        .find(|c| c.key == key)
        .unwrap_or_else(|| panic!("no concept {key}"))
}

/// Answers that the reviews of 2026-10-24 found wrong, or right only as a typo,
/// and that now count exactly for their own concept.
#[test]
fn answers_from_the_reviews_count() {
    for (lang, key, answer) in [
        (Lang::En, "apple", "an apple"),
        (Lang::En, "pear", "a pear"),
        (Lang::En, "milk", "some milk"),
        (Lang::En, "sweets", "candies"),
        (Lang::En, "jam", "jams"),
        (Lang::En, "wine", "wines"),
        (Lang::En, "light_bulb", "bulbs"),
        (Lang::En, "bread", "loaves"),
        (Lang::En, "bread_rolls", "bun"),
        (Lang::En, "mandarin", "satsumas"),
        (Lang::En, "bell_pepper", "red peppers"),
        (Lang::En, "cream", "heavy cream"),
        (Lang::En, "mince", "beef mince"),
        (Lang::En, "fizzy_water", "seltzer"),
        (Lang::En, "clothes_pegs", "pegs"),
        (Lang::En, "sun_cream", "sunblock"),
        (Lang::En, "sliced_bread", "sandwich bread"),
        (Lang::De, "milk", "die Milch"),
        (Lang::De, "bread", "ein Brot"),
        (Lang::De, "bread_rolls", "Broetchen"),
        (Lang::De, "cereal", "Fruehstuecksflocken"),
        (Lang::De, "bin_bags", "Muellsaecke"),
        (Lang::De, "matches", "Zündholz"),
        (Lang::De, "juice", "O-Saft"),
        (Lang::De, "pizza", "TK-Pizza"),
        (Lang::De, "champagne", "Prosecco"),
        (Lang::De, "tomato", "Paradeiser"),
        (Lang::De, "potato", "Erdäpfel"),
        (Lang::De, "cream", "Schlagobers"),
        (Lang::De, "tuna", "Tunfisch"),
        (Lang::De, "mayonnaise", "Majonäse"),
        (Lang::Fr, "milk", "du lait"),
        (Lang::Fr, "flour", "de la farine"),
        (Lang::Fr, "water", "l’eau"),
        (Lang::Fr, "bread", "pains"),
        (Lang::Fr, "lettuce", "salades"),
        (Lang::Fr, "cream", "crème fraîche"),
        (Lang::Fr, "corn", "maïs doux"),
        (Lang::Fr, "mince", "bœuf haché"),
        (Lang::Fr, "biscuits", "gâteau sec"),
        (Lang::Fr, "watermelon", "melon d’eau"),
        (Lang::Fr, "crisps", "croustilles"),
        (Lang::Fr, "doughnut", "beigne"),
        (Lang::Fr, "cola", "coca"),
        (Lang::Fr, "marshmallows", "chamallows"),
        (Lang::Ja, "potato", "じゃが芋"),
        (Lang::Ja, "leek", "長葱"),
        (Lang::Ja, "chicken", "とり肉"),
        (Lang::Ja, "olive_oil", "オリーブゆ"),
        (Lang::Ja, "oil", "さらだゆ"),
        (Lang::Ja, "flour", "はくりきこ"),
        (Lang::Ja, "avocado", "アボガド"),
        (Lang::Ja, "kiwi", "キウイフルーツ"),
        (Lang::Ja, "tuna", "ツナ缶"),
        (Lang::Ja, "soap", "石鹼"),
        (Lang::Zh, "milk", "一瓶牛奶"),
        (Lang::Zh, "sweet_potato", "digua"),
        (Lang::Zh, "coriander", "yansui"),
        (Lang::Zh, "cherry", "车厘子"),
        (Lang::Zh, "juice", "chengzhi"),
        (Lang::Zh, "cabbage", "baocai"),
        (Lang::Zh, "cream", "dannaiyou"),
        (Lang::Zh, "chicken", "雞"),
        (Lang::Zh, "nappies", "尿不湿"),
    ] {
        let c = concept(key);
        assert_eq!(
            matches(c.id, lang, answer),
            Match::Exact,
            "{lang:?} {key}: {answer}"
        );
    }
    let task = Task::List(
        ["bread", "milk", "lettuce", "garlic"]
            .map(|k| concept(k).id)
            .into(),
    );
    let recall = Recall::List(
        ["des pains", "Du lait", "la salade", "l’ail"]
            .map(String::from)
            .into(),
    );
    assert_eq!(
        score(&task, &recall, Lang::Fr).unwrap().marks,
        [Mark::Right; 4]
    );
}

#[test]
fn display_forms_and_groups_from_the_reviews() {
    for (lang, key, shown) in [
        (Lang::En, "apple", "apples"),
        (Lang::En, "sweet_potato", "sweet potatoes"),
        (Lang::En, "cucumber", "cucumber"),
        (Lang::En, "sesame", "sesame seeds"),
        (Lang::De, "strawberry", "Erdbeeren"),
        (Lang::De, "cherry", "Kirschen"),
        (Lang::Zh, "cream", "淡奶油"),
        (Lang::Zh, "clothes_pegs", "晾衣夹"),
    ] {
        assert_eq!(
            display(concept(key).id, lang),
            Some(shown),
            "{lang:?} {key}"
        );
    }
    // Near-synonyms and words one letter apart never meet in a list.
    for keys in [
        &["mango", "sweets", "candles", "sugar"][..],
        &["wine", "champagne"],
        &["flour", "flowers"],
        &["watermelon", "melon", "sweet_potato"],
        &["orange", "mandarin", "juice"],
        &["spinach", "cabbage"],
        &["coriander", "parsley"],
    ] {
        let group = concept(keys[0]).group;
        assert!(group.is_some(), "{keys:?}");
        for key in keys {
            assert_eq!(concept(key).group, group, "{key}");
        }
    }
}
