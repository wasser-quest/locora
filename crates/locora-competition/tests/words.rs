//! Integrity of the word list `groceries-v1` (spec §8.6) and its freeze.
//!
//! These checks keep matching fair: no two concepts share a form, and no typo
//! the matcher forgives can land on another concept, unless the two are in one
//! exclusivity group and so never in the same list.

use locora_competition::matcher::{accepts_typo, within_one};
use locora_competition::words::{Category, Concept, pool};
use locora_competition::{Discipline, Lang, Level, Seed, Spec, Task, generate, rng};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

const TSV: &str = include_str!("../data/groceries-v1.tsv");

/// SHA-256 of `data/groceries-v1.tsv` (line ends as LF). The freeze is binding
/// from the first release that ships the contest; until then a reviewed
/// correction updates this hash and `tests/golden/list.json` in the same commit
/// (addendum D15). After that, corrections go into `groceries-v2.tsv`.
const FROZEN: &str = "8615d517867ac11a57fab1b167ece376ebde8b1b873ba2c7f68910fe31288202";

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
