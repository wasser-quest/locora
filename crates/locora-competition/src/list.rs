//! The shopping list: concepts drawn from `groceries-v1`, recalled in any order.
//!
//! The generator spreads a list over the categories and takes at most one
//! concept of each exclusivity group. Scoring gives one point per item that
//! some answer matches; order shows only in `in_order` (spec §3.2, §8.5, §8.7).

use crate::matcher::{Match, fit, normalize};
use crate::rng::{SplitMix64, shuffle};
use crate::words::{Category, pool};
use crate::{Info, Lang, Mark, Score};

/// Shuffles the concept ids (ascending before the shuffle) and walks them,
/// taking an id unless its group is already taken or its category already has
/// `ceil(items / 10)` ids. The list keeps the order taken.
pub(crate) fn generate(rng: &mut SplitMix64, items: u8) -> Vec<u16> {
    let pool = pool();
    let mut ids: Vec<u16> = pool.concepts().iter().map(|c| c.id).collect();
    shuffle(rng, &mut ids);
    let cap = usize::from(items).div_ceil(Category::ALL.len());
    let mut per_category = [0usize; Category::ALL.len()];
    let mut groups: Vec<&str> = Vec::new();
    let mut list = Vec::with_capacity(usize::from(items));
    for concept in ids.into_iter().filter_map(|id| pool.get(id)) {
        if list.len() == usize::from(items) {
            break;
        }
        let count = &mut per_category[concept.category as usize];
        if *count >= cap || concept.group.is_some_and(|g| groups.contains(&g)) {
            continue;
        }
        *count += 1;
        groups.extend(concept.group);
        list.push(concept.id);
    }
    list
}

/// Scores answers in any order, deterministically:
///
/// 1. in place order, an answer that matches an item exactly takes it; one
///    that matches an item already taken is a duplicate;
/// 2. in place order, every other answer takes the one untaken item it matches
///    with a typo; with no such item or several, it is wrong;
/// 3. empty answers are empty.
///
/// An answer that is exactly a form of another concept of the word list counts
/// as that concept and never as a typo of an item: "ドーナツ" (doughnut) is not
/// "ピーナツ" (peanuts) misspelt.
pub(crate) fn score(task: &[u16], answers: &[String], lang: Lang) -> Score {
    let pool = pool();
    let forms: Vec<&[String]> = task
        .iter()
        .map(|&id| pool.get(id).map_or(&[][..], |c| c.normalized(lang)))
        .collect();
    let answers: Vec<String> = answers.iter().map(|a| normalize(lang, a)).collect();
    let mut taken = vec![false; task.len()];
    let mut matched: Vec<Option<u16>> = vec![None; answers.len()];
    let mut marks: Vec<Option<Mark>> = vec![None; answers.len()];

    for (place, answer) in answers.iter().enumerate() {
        if answer.is_empty() {
            marks[place] = Some(Mark::Empty);
            continue;
        }
        let exact = forms.iter().position(|f| f.contains(answer));
        if let Some(item) = exact {
            if taken[item] {
                marks[place] = Some(Mark::Duplicate);
            } else {
                taken[item] = true;
                matched[place] = Some(item as u16);
                marks[place] = Some(Mark::Right);
            }
        } else if pool.find(lang, answer).is_some() {
            marks[place] = Some(Mark::Wrong);
        }
    }

    for (place, answer) in answers.iter().enumerate() {
        if marks[place].is_some() {
            continue;
        }
        let mut candidates = (0..task.len())
            .filter(|&item| !taken[item] && fit(forms[item], answer) == Match::Tolerated);
        marks[place] = Some(match (candidates.next(), candidates.next()) {
            (Some(item), None) => {
                taken[item] = true;
                matched[place] = Some(item as u16);
                Mark::Tolerated
            }
            _ => Mark::Wrong,
        });
    }

    let marks: Vec<Mark> = marks
        .into_iter()
        .map(|m| m.unwrap_or(Mark::Wrong))
        .collect();
    let points = marks
        .iter()
        .filter(|m| matches!(m, Mark::Right | Mark::Tolerated))
        .count() as u16;
    let in_order = matched
        .iter()
        .enumerate()
        .filter(|&(place, m)| *m == Some(place as u16))
        .count() as u16;
    crate::finish(
        points,
        marks,
        matched,
        Info {
            in_order: Some(in_order),
            ..Info::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::words::pool;

    fn id(key: &str) -> u16 {
        pool()
            .concepts()
            .iter()
            .find(|c| c.key == key)
            .unwrap_or_else(|| panic!("no concept {key}"))
            .id
    }

    fn task(keys: &[&str]) -> Vec<u16> {
        keys.iter().map(|k| id(k)).collect()
    }

    fn answers(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn exact_answers_in_any_order() {
        let task = task(&["milk", "bread", "apple", "cheese", "coffee"]);
        let s = score(
            &task,
            &answers(&["Kaffee", "Milch", "Äpfel", "Brot", "Käse"]),
            Lang::De,
        );
        assert_eq!((s.points, s.max, s.perfect), (5, 5, true));
        assert_eq!(s.marks, vec![Mark::Right; 5]);
        assert_eq!(s.matched, [Some(4), Some(0), Some(2), Some(1), Some(3)]);
        assert_eq!(s.info.in_order, Some(1)); // only the apples are on their own place
        let s = score(
            &task,
            &answers(&["milk", "bread", "apples", "cheese", "coffee"]),
            Lang::En,
        );
        assert_eq!((s.points, s.info.in_order), (5, Some(5)));
    }

    #[test]
    fn a_typo_counts_from_five_letters() {
        let task = task(&["lemon", "pear"]);
        let s = score(&task, &answers(&["lemno", "paer"]), Lang::En);
        assert_eq!(s.marks, [Mark::Tolerated, Mark::Wrong]);
        assert_eq!(s.points, 1);
        assert!(!s.perfect);
        assert_eq!(s.matched, [Some(0), None]);
    }

    #[test]
    fn duplicates_count_once() {
        let task = task(&["milk", "bread"]);
        let s = score(&task, &answers(&["Milch", "milch"]), Lang::De);
        assert_eq!(s.marks, [Mark::Right, Mark::Duplicate]);
        assert_eq!(s.points, 1);
        // A typo of an item already counted is wrong, not a duplicate.
        let task = self::task(&["lemon", "bread"]);
        let s = score(&task, &answers(&["lemon", "lemno"]), Lang::En);
        assert_eq!(s.marks, [Mark::Right, Mark::Wrong]);
    }

    #[test]
    fn an_exact_answer_wins_over_an_earlier_typo() {
        // The typo at place 1 would fit the lemons, but place 2 names them exactly.
        let task = task(&["lemon", "bread"]);
        let s = score(&task, &answers(&["lemno", "lemon"]), Lang::En);
        assert_eq!(s.marks, [Mark::Wrong, Mark::Right]);
        assert_eq!(s.matched, [None, Some(0)]);
    }

    #[test]
    fn an_ambiguous_typo_is_wrong() {
        // "lelon" is one letter from "lemon" and from "melon".
        let task = task(&["lemon", "melon"]);
        let s = score(&task, &answers(&["lelon", ""]), Lang::En);
        assert_eq!(s.marks, [Mark::Wrong, Mark::Empty]);
        // With one of them taken, the typo has a single candidate left.
        let s = score(&task, &answers(&["lemon", "lelon"]), Lang::En);
        assert_eq!(s.marks, [Mark::Right, Mark::Tolerated]);
        assert_eq!(s.points, 2);
    }

    #[test]
    fn another_concept_is_never_a_typo() {
        let task = task(&["peanuts", "milk"]);
        let s = score(&task, &answers(&["ドーナツ", "ピーナツ"]), Lang::Ja);
        assert_eq!(s.marks, [Mark::Wrong, Mark::Right]);
        let s = score(&task, &answers(&["ぴーなっつ", "ぎゅうにゅう"]), Lang::Ja);
        assert_eq!(s.points, 2);
    }

    #[test]
    fn kana_katakana_and_kanji() {
        let task = task(&["cucumber", "apple", "onion", "yoghurt"]);
        let s = score(
            &task,
            &answers(&["キュウリ", "林檎", "たまねぎ", "よーぐるど"]),
            Lang::Ja,
        );
        assert_eq!(
            s.marks,
            [Mark::Right, Mark::Right, Mark::Right, Mark::Tolerated]
        );
        // Kanji need the exact characters; three kana are too short for a typo.
        let s = score(&task, &answers(&["胡", "りんこ", "玉葱", ""]), Lang::Ja);
        assert_eq!(
            s.marks,
            [Mark::Wrong, Mark::Wrong, Mark::Right, Mark::Empty]
        );
    }

    #[test]
    fn pinyin_and_traditional_characters() {
        let task = task(&["apple", "carrot", "milk"]);
        let s = score(&task, &answers(&["蘋果", "huluobo", "niunai"]), Lang::Zh);
        assert_eq!(s.points, 3);
        let s = score(&task, &answers(&["pingguo", "胡蘿蔔", "niunia"]), Lang::Zh);
        assert_eq!(s.marks, [Mark::Right, Mark::Right, Mark::Tolerated]);
        // Full-width letters from a Chinese keyboard.
        let s = score(&task, &answers(&["ｐｉｎｇｇｕｏ", "", ""]), Lang::Zh);
        assert_eq!(s.marks[0], Mark::Right);
    }

    #[test]
    fn diacritics_fold_and_plurals_are_listed() {
        let task = task(&["eggs", "spinach", "celery", "sweet_potato"]);
        let s = score(
            &task,
            &answers(&["oeufs", "EPINARDS", "celeri", "Patates douces"]),
            Lang::Fr,
        );
        assert_eq!(s.points, 4);
        let s = score(
            &task,
            &answers(&["Eier", "Spinat", "Sellerie", "Suesskartoffel"]),
            Lang::De,
        );
        assert_eq!(
            s.marks,
            [Mark::Right, Mark::Right, Mark::Right, Mark::Tolerated]
        );
    }

    #[test]
    fn nothing_but_wrong_and_empty() {
        let task = task(&["milk", "bread", "apple"]);
        let s = score(&task, &answers(&["", "Fahrrad", "  "]), Lang::De);
        assert_eq!(s.marks, [Mark::Empty, Mark::Wrong, Mark::Empty]);
        assert_eq!((s.points, s.max, s.info.in_order), (0, 3, Some(0)));
        assert_eq!(s.matched, [None, None, None]);
    }

    #[test]
    fn a_list_of_ten_has_one_of_each_category() {
        let mut rng = SplitMix64::new(crate::Seed::masked(11));
        let list = generate(&mut rng, 10);
        assert_eq!(list.len(), 10);
        let mut categories: Vec<_> = list
            .iter()
            .map(|&id| crate::words::category(id).unwrap())
            .collect();
        categories.sort();
        assert_eq!(categories, Category::ALL);
    }
}
