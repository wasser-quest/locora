//! Challenge codes: the discipline, the level and a 60-bit seed in a form that
//! survives being read out or typed on a phone (spec §6). Two people who play
//! the same code get the same task and compare their scores themselves.
//!
//! Format `<D><L>-XXXX-XXXX-XXXX-K`: `D` is `S` (shopping list), `N` or `C`,
//! `L` the level, then the seed as 12 Crockford base32 characters (most
//! significant first) and a check character `alphabet[Σ (2i+1)·vᵢ mod 32]`. The
//! odd weights are units modulo 32, so every single-character typo in the seed
//! or the check character changes the sum and is caught.
//!
//! What the check character does not catch (format v1, spec §6): a typo in the
//! prefix, which is not part of the sum, so `C3-…` for `C2-…` plays the same
//! seed at another level and an unknown letter or level reads as
//! [`CodeError::Newer`]; and two neighbouring seed characters swapped whose
//! values differ by 16 (`H` and `1`, `Z` and `F`), about 3 % of all swaps,
//! which plays another seed. The round shows its discipline and level before
//! it starts.

use crate::{Discipline, Lang, Level, Seed};
use std::fmt;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const SEED_CHARS: usize = 12;

/// Why a code cannot be played.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeError {
    /// Not a code, or a typo in it.
    Invalid,
    /// A well-formed code for a discipline or level this version does not know.
    Newer,
}

impl fmt::Display for CodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => write!(f, "Dieser Code ist ungültig"),
            Self::Newer => write!(f, "Dieser Code braucht eine neuere Version von Locora"),
        }
    }
}

impl std::error::Error for CodeError {}

impl CodeError {
    /// The message for people, in their language.
    pub fn text(&self, lang: Lang) -> &'static str {
        lang.pick(match self {
            Self::Invalid => [
                "This code is not valid. Check it for typos.",
                "Dieser Code ist ungültig. Prüfe ihn auf Tippfehler.",
                "Ce code n’est pas valide. Vérifie qu’il ne contient pas de faute de frappe.",
                "このチャレンジコードは無効です。入力ミスがないか確認してください。",
                "这个挑战码无效。请检查是否有输入错误。",
            ],
            Self::Newer => [
                "This code needs a newer version of Locora.",
                "Für diesen Code brauchst du eine neuere Version von Locora.",
                "Ce code nécessite une version plus récente de Locora.",
                "このチャレンジコードには新しいバージョンの Locora が必要です。",
                "这个挑战码需要更新版本的 Locora。",
            ],
        })
    }
}

fn letter(discipline: Discipline) -> char {
    match discipline {
        Discipline::List => 'S',
        Discipline::Numbers => 'N',
        Discipline::Cards => 'C',
    }
}

fn check(values: &[u8; SEED_CHARS]) -> u8 {
    let sum: u32 = (0..SEED_CHARS)
        .map(|i| (2 * i as u32 + 1) * u32::from(values[i]))
        .sum();
    (sum % 32) as u8
}

/// The code of a task, e.g. `C2-28T5-CY4T-QKFF-K`.
pub fn encode(discipline: Discipline, level: Level, seed: Seed) -> String {
    let values: [u8; SEED_CHARS] =
        std::array::from_fn(|i| ((seed.get() >> (5 * (SEED_CHARS - 1 - i))) & 31) as u8);
    let chars: String = values
        .iter()
        .map(|&v| char::from(ALPHABET[usize::from(v)]))
        .collect();
    format!(
        "{}{}-{}-{}-{}-{}",
        letter(discipline),
        level.get(),
        &chars[0..4],
        &chars[4..8],
        &chars[8..12],
        char::from(ALPHABET[usize::from(check(&values))])
    )
}

/// The value of a seed or check character; `O` reads as `0`, `I` and `L` as `1`.
fn value(c: char) -> Option<u8> {
    let c = match c {
        'O' => '0',
        'I' | 'L' => '1',
        c => c,
    };
    ALPHABET
        .iter()
        .position(|&a| char::from(a) == c)
        .map(|v| v as u8)
}

/// Whether `c` separates the groups of a code or is invisible, so it is
/// ignored: whitespace, hyphens and dashes, the Japanese long-vowel mark and
/// middle dot that a kana keyboard types for `-`, and the format characters
/// that copying from a messenger or web page adds (soft hyphen, zero-width
/// space and joiners, direction marks, word joiner, byte order mark).
fn ignored(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '-' | '‐' | '‑' | '‒' | '–' | '—' | '−' | 'ー' | 'ｰ' | '・' | '･'
                | '\u{ad}'
                | '\u{180e}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206f}'
                | '\u{feff}'
        )
}

/// Reads a code as people type or paste it: case does not matter, and spaces,
/// hyphens and invisible characters are ignored. A pasted share text ends with
/// its code ("… Same task for you: C2-28T5-CY4T-QKFF-K"), so when more than a
/// code is left, the last 15 letters and digits are read; the check character
/// still refuses anything else.
pub fn decode(text: &str) -> Result<(Discipline, Level, Seed), CodeError> {
    let mut chars: Vec<char> = text
        .chars()
        .map(|c| match u32::from(c) {
            // Full-width ASCII, as a Japanese or Chinese keyboard may type it.
            0xff01..=0xff5e => char::from_u32(u32::from(c) - 0xfee0).unwrap_or(c),
            _ => c,
        })
        .filter(|&c| !ignored(c))
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let code_len = 2 + SEED_CHARS + 1;
    if chars.len() > code_len {
        // The code at the end of a longer text: after trailing punctuation, the
        // run of letters and digits that ends it, if it is long enough.
        let end = chars.len()
            - chars
                .iter()
                .rev()
                .take_while(|c| !c.is_ascii_alphanumeric())
                .count();
        let run = chars[..end]
            .iter()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric())
            .count();
        if run >= code_len {
            chars = chars[end - code_len..end].to_vec();
        }
    }
    let [d, l, rest @ ..] = chars.as_slice() else {
        return Err(CodeError::Invalid);
    };
    if rest.len() != SEED_CHARS + 1 || !d.is_ascii_uppercase() || !l.is_ascii_digit() {
        return Err(CodeError::Invalid);
    }
    let mut values = [0u8; SEED_CHARS];
    for (v, &c) in values.iter_mut().zip(rest) {
        *v = value(c).ok_or(CodeError::Invalid)?;
    }
    if value(rest[SEED_CHARS]) != Some(check(&values)) {
        return Err(CodeError::Invalid);
    }
    let seed = values
        .iter()
        .fold(0u64, |acc, &v| (acc << 5) | u64::from(v));
    let discipline = Discipline::ALL.into_iter().find(|&dd| letter(dd) == *d);
    let level = l.to_digit(10).map_or(0, |n| n as u8);
    match (discipline, Level::new(level)) {
        (Some(discipline), Some(level)) => Ok((discipline, level, Seed::masked(seed))),
        _ if level == 0 => Err(CodeError::Invalid),
        _ => Err(CodeError::Newer),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(n: u8) -> Level {
        Level::new(n).unwrap()
    }

    const EXAMPLE: &str = "C2-28T5-CY4T-QKFF-K";

    fn example() -> (Discipline, Level, Seed) {
        (
            Discipline::Cards,
            level(2),
            Seed::masked(0x123_4567_89ab_cdef),
        )
    }

    #[test]
    fn the_example_of_the_spec() {
        let (d, l, s) = example();
        assert_eq!(encode(d, l, s), EXAMPLE);
        assert_eq!(decode(EXAMPLE), Ok(example()));
    }

    #[test]
    fn codes_round_trip() {
        let mut rng = crate::rng::SplitMix64::new(Seed::masked(3));
        for d in Discipline::ALL {
            for l in Level::ALL {
                for seed in [0, Seed::MASK, rng.next_u64()] {
                    let seed = Seed::masked(seed);
                    let code = encode(d, l, seed);
                    assert_eq!(code.len(), 19);
                    assert_eq!(decode(&code), Ok((d, l, seed)), "{code}");
                }
            }
        }
        assert_eq!(
            encode(Discipline::List, level(1), Seed::masked(0)),
            "S1-0000-0000-0000-0"
        );
        assert_eq!(
            encode(Discipline::Numbers, level(3), Seed::masked(Seed::MASK)),
            "N3-ZZZZ-ZZZZ-ZZZZ-G"
        );
    }

    #[test]
    fn every_single_character_substitution_is_refused() {
        let positions: Vec<usize> = EXAMPLE
            .char_indices()
            .filter(|&(i, c)| i >= 3 && c != '-')
            .map(|(i, _)| i)
            .collect();
        assert_eq!(positions.len(), SEED_CHARS + 1);
        for i in positions {
            let original = value(EXAMPLE[i..].chars().next().unwrap());
            for &a in ALPHABET.iter().chain(b"OIL") {
                let typo = char::from(a);
                if value(typo) == original {
                    continue; // the same character, or one read as it
                }
                let mut code = EXAMPLE.to_string();
                code.replace_range(i..i + 1, &typo.to_string());
                assert_eq!(decode(&code), Err(CodeError::Invalid), "{code}");
            }
        }
    }

    #[test]
    fn neighbours_16_apart_swap_unnoticed() {
        // H is 17 and 1 is 1: the weights 3 and 5 move the sum by 2 · 16 ≡ 0.
        let (d, l, seed) = decode("C1-WH1H-Z8Y8-1PR6-D").unwrap();
        let (_, _, swapped) = decode("C1-W1HH-Z8Y8-1PR6-D").unwrap();
        assert_eq!((d, l), (Discipline::Cards, level(1)));
        assert_ne!(seed, swapped);
        // Other neighbours swapped are refused.
        assert_eq!(decode("C2-82T5-CY4T-QKFF-K"), Err(CodeError::Invalid));
    }

    #[test]
    fn case_spaces_hyphens_and_look_alikes() {
        for text in [
            "c2-28t5-cy4t-qkff-k",
            "C2 28T5 CY4T QKFF K",
            "C228T5CY4TQKFFK",
            " C2–28T5—CY4T−QKFF‐K\n",
            "Ｃ２－２８Ｔ５－ＣＹ４Ｔ－ＱＫＦＦ－Ｋ",
            "C2ー28T5ーCY4TーQKFFーK",
            "C2ｰ28T5ｰCY4TｰQKFFｰK",
            "C2・28T5・CY4T・QKFF・K",
            // Invisible characters that copying adds: zero-width space, byte order
            // mark, direction marks, word joiner, soft hyphen.
            "C2-28T5-CY4T-QKFF-K\u{200b}",
            "\u{feff}C2-28T5-CY4T-QKFF-K",
            "\u{200e}C2-28T5-CY4T-QKFF-K\u{200f}",
            "C2\u{2060}-28T5-CY4T-QKFF-K",
            "C2\u{ad}28T5\u{ad}CY4T\u{ad}QKFF\u{ad}K",
        ] {
            assert_eq!(decode(text), Ok(example()), "{text}");
        }
        // O reads as 0, I and L as 1 in the seed and check characters.
        let seed = Seed::masked(1 << 55);
        assert_eq!(
            encode(Discipline::List, level(1), seed),
            "S1-1000-0000-0000-1"
        );
        let expected = Ok((Discipline::List, level(1), seed));
        assert_eq!(decode("S1-1000-0000-0000-1"), expected);
        assert_eq!(decode("s1-iooo-oooo-oooo-l"), expected);
        assert_eq!(decode("S1-L0O0-0000-0000-I"), expected);
        // The prefix is not mapped: "SL" is not level 1, "5" is not "S".
        assert_eq!(decode("SL-1000-0000-0000-1"), Err(CodeError::Invalid));
        assert_eq!(decode("51-1000-0000-0000-1"), Err(CodeError::Invalid));
    }

    #[test]
    fn a_pasted_share_text_gives_its_code() {
        for text in [
            "I scored 24/26 in the Locora memory contest. Same task for you: C2-28T5-CY4T-QKFF-K",
            "Gleiche Aufgabe für dich: c2 28t5 cy4t qkff k.",
            "同じ課題に挑戦してね：Ｃ２－２８Ｔ５－ＣＹ４Ｔ－ＱＫＦＦ－Ｋ。",
            "同样的题目给你C2-28T5-CY4T-QKFF-K！",
            "Play C2-28T5-CY4T-QKFF-K",
            "XC2-28T5-CY4T-QKFF-K",
        ] {
            assert_eq!(decode(text), Ok(example()), "{text}");
        }
        // Only the end counts, and the check character still refuses the rest.
        for text in [
            "C2-28T5-CY4T-QKFF-K is my code, play it",
            "Same task for you: C2-28T5-CY4T-QKFF-J",
            "C2-28T5-CY4T-QKFF-KK",
            "C2-28T5-CY4T-QKFFF-K",
            "I scored 24/26 in the Locora memory contest.",
        ] {
            assert_eq!(decode(text), Err(CodeError::Invalid), "{text}");
        }
    }

    #[test]
    fn prefixes_and_levels() {
        let rest = &EXAMPLE[2..];
        assert_eq!(
            decode(&format!("N3{rest}")).map(|c| (c.0, c.1)),
            Ok((Discipline::Numbers, level(3)))
        );
        assert_eq!(
            decode(&format!("S1{rest}")).map(|c| (c.0, c.1)),
            Ok((Discipline::List, level(1)))
        );
        // The prefix is not part of the check: a typo there plays the same seed.
        assert_eq!(
            decode(&format!("C3{rest}")),
            Ok((Discipline::Cards, level(3), example().2))
        );
        // A letter or level of a later version.
        assert_eq!(decode(&format!("W2{rest}")), Err(CodeError::Newer));
        assert_eq!(decode(&format!("C4{rest}")), Err(CodeError::Newer));
        // Not a code at all.
        assert_eq!(decode(&format!("C0{rest}")), Err(CodeError::Invalid));
        assert_eq!(decode(&format!("2C{rest}")), Err(CodeError::Invalid));
        assert_eq!(decode(&format!("Ü2{rest}")), Err(CodeError::Invalid));
        for text in [
            "",
            "C2",
            "C2-28T5-CY4T-QKFF",
            "C2-28T5-CY4T-QKFF-KK",
            "C2-28T5-CY4U-QKFF-K",
        ] {
            assert_eq!(decode(text), Err(CodeError::Invalid), "{text}");
        }
    }

    #[test]
    fn a_seed_of_61_bits_cannot_be_encoded() {
        // Seeds below 2^60 are all a `Seed` can hold, and 12 characters hold exactly 60 bits.
        assert_eq!(Seed::new(1 << 60), None);
        let biggest = encode(Discipline::Cards, level(1), Seed::masked(u64::MAX));
        assert_eq!(decode(&biggest).map(|c| c.2.get()), Ok(Seed::MASK));
    }

    #[test]
    fn error_texts() {
        for error in [CodeError::Invalid, CodeError::Newer] {
            assert!(!error.to_string().is_empty());
            for lang in Lang::ALL {
                assert!(!error.text(lang).is_empty());
            }
        }
        assert_eq!(
            CodeError::Invalid.text(Lang::En),
            "This code is not valid. Check it for typos."
        );
    }
}
