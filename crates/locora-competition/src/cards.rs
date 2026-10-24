//! Cards: a standard deck of 52 without jokers, shuffled; the task is its first
//! `items` cards. One point per place that holds exactly the card that was there.
//!
//! Card id = `suit * 13 + rank`. Suits: 0 spades, 1 hearts, 2 diamonds, 3 clubs.
//! Ranks: 0 ace, 1–8 two to nine, 9 ten, 10 jack, 11 queen, 12 king. The names
//! here are used by screen readers, the CLI and the results (spec §3.4).

use crate::rng::{SplitMix64, shuffle};
use crate::{Info, Lang, Mark, Score};

/// Cards in a deck.
pub const DECK: usize = 52;

pub fn suit(card: u8) -> u8 {
    card / 13
}

pub fn rank(card: u8) -> u8 {
    card % 13
}

/// The card of `suit` (0–3) and `rank` (0–12), or `None` outside the deck.
pub fn card(suit: u8, rank: u8) -> Option<u8> {
    (suit < 4 && rank < 13).then(|| suit * 13 + rank)
}

/// Shuffles the deck and keeps the first `items` cards (at most 52).
pub(crate) fn generate(rng: &mut SplitMix64, items: u8) -> Vec<u8> {
    let mut deck: Vec<u8> = (0..DECK as u8).collect();
    shuffle(rng, &mut deck);
    deck.truncate(usize::from(items));
    deck
}

/// One point per place with its own card; also how far the recall is right
/// before its first mistake.
pub(crate) fn score(task: &[u8], recall: &[Option<u8>]) -> Score {
    let marks: Vec<Mark> = task
        .iter()
        .zip(recall)
        .map(|(&card, answer)| match *answer {
            None => Mark::Empty,
            Some(a) if a == card => Mark::Right,
            Some(_) => Mark::Wrong,
        })
        .collect();
    let points = marks.iter().filter(|&&m| m == Mark::Right).count() as u16;
    let first_error = marks.iter().take_while(|&&m| m == Mark::Right).count() as u16;
    crate::finish(
        points,
        marks,
        Vec::new(),
        Info {
            first_error: Some(first_error),
            ..Info::default()
        },
    )
}

const RANKS_EN: [&str; 13] = [
    "ace", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "jack", "queen",
    "king",
];
const RANKS_DE: [&str; 13] = [
    "Ass", "Zwei", "Drei", "Vier", "Fünf", "Sechs", "Sieben", "Acht", "Neun", "Zehn", "Bube",
    "Dame", "König",
];
const RANKS_FR: [&str; 13] = [
    "as", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf", "dix", "valet", "dame",
    "roi",
];
const RANKS_JA: [&str; 13] = [
    "エース",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "10",
    "ジャック",
    "クイーン",
    "キング",
];
const RANKS_ZH: [&str; 13] = [
    "A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K",
];

/// Suits in the order ♠ ♥ ♦ ♣.
const SUITS: [[&str; 4]; 5] = [
    ["spades", "hearts", "diamonds", "clubs"],
    ["Pik", "Herz", "Karo", "Kreuz"],
    ["pique", "cœur", "carreau", "trèfle"],
    ["スペード", "ハート", "ダイヤ", "クラブ"],
    ["黑桃", "红桃", "方块", "梅花"],
];
const SUIT_SYMBOLS: [char; 4] = ['♠', '♥', '♦', '♣'];

/// The name of a card inside a sentence: "seven of hearts", "Herz Sieben",
/// "sept de cœur", "ハートの7", "红桃7". Empty for an id outside the deck.
pub fn name(card: u8, lang: Lang) -> String {
    if usize::from(card) >= DECK {
        return String::new();
    }
    let (suit, rank) = (usize::from(suit(card)), usize::from(rank(card)));
    let suit_name = SUITS[lang as usize][suit];
    match lang {
        Lang::En => format!("{} of {suit_name}", RANKS_EN[rank]),
        Lang::De => format!("{suit_name} {}", RANKS_DE[rank]),
        Lang::Fr => format!("{} de {suit_name}", RANKS_FR[rank]),
        Lang::Ja => format!("{suit_name}の{}", RANKS_JA[rank]),
        Lang::Zh => format!("{suit_name}{}", RANKS_ZH[rank]),
    }
}

/// [`name`] at the start of a sentence: "Seven of hearts", "Sept de cœur".
pub fn name_capitalized(card: u8, lang: Lang) -> String {
    let name = name(card, lang);
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => name,
    }
}

/// The short form with the rank's letter in `lang` and the suit symbol: "7♥",
/// "10♠", "B♥" (German Bube). Empty for an id outside the deck.
pub fn short(card: u8, lang: Lang) -> String {
    if usize::from(card) >= DECK {
        return String::new();
    }
    let rank = rank(card);
    let symbol = SUIT_SYMBOLS[usize::from(suit(card))];
    if rank == 9 {
        format!("10{symbol}")
    } else {
        format!("{}{symbol}", rank_key(rank, lang))
    }
}

/// The key that picks a rank during recall: `A` for ace, `2`–`9`, `0` for ten,
/// and jack, queen and king by their letters in `lang` (EN J Q K, DE B D K,
/// FR V D R; JA and ZH use J Q K). `?` for a rank outside 0–12.
pub fn rank_key(rank: u8, lang: Lang) -> char {
    let court = match lang {
        Lang::De => ['B', 'D', 'K'],
        Lang::Fr => ['V', 'D', 'R'],
        Lang::En | Lang::Ja | Lang::Zh => ['J', 'Q', 'K'],
    };
    match rank {
        0 => 'A',
        1..=8 => char::from(b'1' + rank),
        9 => '0',
        10..=12 => court[usize::from(rank - 10)],
        _ => '?',
    }
}

/// The rank a key picks in `lang`, case-insensitive: [`rank_key`] backwards, and
/// `1` also for the ace.
pub fn rank_from_key(key: char, lang: Lang) -> Option<u8> {
    let key = key.to_ascii_uppercase();
    if key == '1' {
        return Some(0);
    }
    (0..13).find(|&rank| rank_key(rank, lang) == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_split_into_suit_and_rank() {
        assert_eq!((suit(36), rank(36)), (2, 10)); // jack of diamonds
        assert_eq!(card(2, 10), Some(36));
        assert_eq!(card(4, 0), None);
        assert_eq!(card(0, 13), None);
        for id in 0..DECK as u8 {
            assert_eq!(card(suit(id), rank(id)), Some(id));
        }
    }

    #[test]
    fn names_follow_the_pattern_of_each_language() {
        let seven_of_hearts = card(1, 6).unwrap();
        let names = Lang::ALL.map(|l| name(seven_of_hearts, l));
        assert_eq!(
            names,
            [
                "seven of hearts",
                "Herz Sieben",
                "sept de cœur",
                "ハートの7",
                "红桃7"
            ]
        );
        assert_eq!(name(0, Lang::En), "ace of spades");
        assert_eq!(name(51, Lang::De), "Kreuz König");
        assert_eq!(name(card(2, 9).unwrap(), Lang::Fr), "dix de carreau");
        assert_eq!(name(card(3, 11).unwrap(), Lang::Ja), "クラブのクイーン");
        assert_eq!(name(card(0, 0).unwrap(), Lang::Ja), "スペードのエース");
        assert_eq!(name(card(2, 12).unwrap(), Lang::Zh), "方块K");
        assert_eq!(name(52, Lang::En), "");
    }

    #[test]
    fn every_language_names_52_different_cards() {
        for lang in Lang::ALL {
            let mut names: Vec<String> = (0..DECK as u8).map(|c| name(c, lang)).collect();
            let mut shorts: Vec<String> = (0..DECK as u8).map(|c| short(c, lang)).collect();
            assert!(names.iter().all(|n| !n.is_empty() && !n.contains('{')));
            names.sort();
            names.dedup();
            shorts.sort();
            shorts.dedup();
            assert_eq!((names.len(), shorts.len()), (DECK, DECK), "{lang:?}");
        }
    }

    #[test]
    fn english_and_french_names_start_a_sentence_capitalised() {
        let seven_of_hearts = card(1, 6).unwrap();
        assert_eq!(
            name_capitalized(seven_of_hearts, Lang::En),
            "Seven of hearts"
        );
        assert_eq!(name_capitalized(seven_of_hearts, Lang::Fr), "Sept de cœur");
        assert_eq!(name_capitalized(seven_of_hearts, Lang::De), "Herz Sieben");
        assert_eq!(name_capitalized(seven_of_hearts, Lang::Ja), "ハートの7");
        assert_eq!(name_capitalized(52, Lang::En), "");
    }

    #[test]
    fn short_forms_and_keys() {
        assert_eq!(short(card(1, 6).unwrap(), Lang::En), "7♥");
        assert_eq!(short(card(0, 9).unwrap(), Lang::En), "10♠");
        assert_eq!(short(card(1, 10).unwrap(), Lang::De), "B♥");
        assert_eq!(short(card(3, 12).unwrap(), Lang::Fr), "R♣");
        assert_eq!(short(card(2, 0).unwrap(), Lang::Zh), "A♦");
        let keys: String = (0..13).map(|r| rank_key(r, Lang::En)).collect();
        assert_eq!(keys, "A234567890JQK");
        let keys: String = (0..13).map(|r| rank_key(r, Lang::De)).collect();
        assert_eq!(keys, "A234567890BDK");
        let keys: String = (0..13).map(|r| rank_key(r, Lang::Fr)).collect();
        assert_eq!(keys, "A234567890VDR");
        assert_eq!(rank_key(13, Lang::En), '?');
        for lang in Lang::ALL {
            for rank in 0..13 {
                assert_eq!(rank_from_key(rank_key(rank, lang), lang), Some(rank));
            }
            assert_eq!(rank_from_key('1', lang), Some(0));
            assert_eq!(rank_from_key('x', lang), None);
        }
        assert_eq!(rank_from_key('q', Lang::En), Some(11));
        assert_eq!(rank_from_key('q', Lang::De), None);
        assert_eq!(rank_from_key('b', Lang::De), Some(10));
        assert_eq!(rank_from_key('r', Lang::Fr), Some(12));
    }

    #[test]
    fn a_shuffled_deck_is_a_permutation_prefix() {
        let mut rng = SplitMix64::new(crate::Seed::masked(5));
        let full = generate(&mut rng, 52);
        let mut sorted = full.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..52).collect::<Vec<u8>>());
        for items in [0, 13, 26, 52, 60] {
            let mut rng = SplitMix64::new(crate::Seed::masked(5));
            let cards = generate(&mut rng, items);
            assert_eq!(cards.len(), usize::from(items).min(DECK));
            assert_eq!(cards[..], full[..cards.len()]);
        }
    }

    #[test]
    fn shuffles_are_uniform() {
        // Chi-square of position × card counts over 100,000 shuffles: 52 positions with
        // 51 degrees of freedom each, 2,652 in all, standard deviation about 73.
        const SHUFFLES: u32 = 100_000;
        let mut counts = vec![[0u32; DECK]; DECK];
        let mut seeds = SplitMix64::new(crate::Seed::masked(2026));
        for _ in 0..SHUFFLES {
            let mut rng = SplitMix64::new(crate::Seed::masked(seeds.next_u64()));
            for (position, card) in generate(&mut rng, 52).into_iter().enumerate() {
                counts[position][usize::from(card)] += 1;
            }
        }
        let expected = f64::from(SHUFFLES) / DECK as f64;
        let chi_square = |row: &[u32; DECK]| -> f64 {
            row.iter()
                .map(|&n| (f64::from(n) - expected).powi(2) / expected)
                .sum()
        };
        let total: f64 = counts.iter().map(chi_square).sum();
        assert!(
            (2_652.0 - 6.0 * 73.0..2_652.0 + 6.0 * 73.0).contains(&total),
            "{total}"
        );
        // No single position is far off either (51 degrees of freedom: p < 1e-5 above 110).
        for (position, row) in counts.iter().enumerate() {
            assert!(
                chi_square(row) < 110.0,
                "position {position}: {}",
                chi_square(row)
            );
        }
    }

    #[test]
    fn a_swap_costs_two_places_and_a_gap_one() {
        let task = [36, 18, 7, 44];
        let swap = score(&task, &[Some(36), Some(7), Some(18), Some(44)]);
        assert_eq!((swap.points, swap.max, swap.perfect), (2, 4, false));
        assert_eq!(
            swap.marks,
            [Mark::Right, Mark::Wrong, Mark::Wrong, Mark::Right]
        );
        assert_eq!(swap.info.first_error, Some(1));
        // A forgotten card does not shift the ones after it.
        let gap = score(&task, &[Some(36), None, Some(7), Some(44)]);
        assert_eq!(gap.points, 3);
        assert_eq!(gap.marks[1], Mark::Empty);
        assert_eq!(gap.info.first_error, Some(1));
        let perfect = score(&task, &[Some(36), Some(18), Some(7), Some(44)]);
        assert!(perfect.perfect);
        assert_eq!(perfect.info.first_error, Some(4));
        assert_eq!(perfect.info.in_order, None);
        assert!(perfect.matched.is_empty());
        let empty = score(&task, &[None; 4]);
        assert_eq!((empty.points, empty.info.first_error), (0, Some(0)));
    }
}
