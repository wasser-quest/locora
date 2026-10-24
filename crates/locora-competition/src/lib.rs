//! Rules of the memory contest: tasks, scoring, challenge codes and the texts
//! that go with them.
//!
//! The same code runs in the `competition` plugin, the app core, the CLI and on
//! Locora Cloud, so a seed gives the same task and a recall the same score on
//! every device and on the server. Every published version stays exactly as it
//! is ([`RULES`], [`CONTENT`], [`GENERATOR`]): a fix becomes version 2 next to
//! version 1, and the golden tests in `tests/golden` fail on any change to the
//! output of version 1.
//!
//! Three disciplines with three fixed levels each (spec §3): a shopping list
//! recalled in any order ([`words`], [`matcher`]), digits in rows of ten
//! ([`numbers`]), and the order of shuffled cards ([`cards`]).

pub mod cards;
pub mod code;
mod date;
pub mod floors;
mod list;
pub mod matcher;
pub mod numbers;
pub mod rng;
pub mod schedule;
pub mod texts;
pub mod words;
pub mod xp;

pub use date::Date;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Reverse;
use std::fmt;

/// Version of the limits, scoring, XP, schedule and codes.
pub const RULES: u16 = 1;
/// Version of the word list and the card names.
pub const CONTENT: u16 = 1;
/// Version of the seed derivation, the random generator and the draws.
pub const GENERATOR: u16 = 1;

/// Rules versions this build can score.
pub fn supported_rules() -> &'static [u16] {
    &[RULES]
}

/// Content versions this build can show and score.
pub fn supported_content() -> &'static [u16] {
    &[CONTENT]
}

/// The three disciplines; `"list"`, `"numbers"` and `"cards"` in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Discipline {
    List,
    Numbers,
    Cards,
}

impl Discipline {
    pub const ALL: [Discipline; 3] = [Discipline::List, Discipline::Numbers, Discipline::Cards];

    /// The name in keys, labels and JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Discipline::List => "list",
            Discipline::Numbers => "numbers",
            Discipline::Cards => "cards",
        }
    }

    pub fn parse(text: &str) -> Option<Discipline> {
        Discipline::ALL.into_iter().find(|d| d.as_str() == text)
    }
}

/// A level from 1 (starter) to 3 (expert); other numbers cannot be built or
/// deserialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Level(u8);

impl Level {
    pub const ALL: [Level; 3] = [Level(1), Level(2), Level(3)];

    pub fn new(level: u8) -> Option<Level> {
        (1..=3).contains(&level).then_some(Level(level))
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

impl Serialize for Level {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.0)
    }
}

impl<'de> Deserialize<'de> for Level {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Level, D::Error> {
        let level = u8::deserialize(deserializer)?;
        Level::new(level).ok_or_else(|| {
            serde::de::Error::custom(format_args!("Stufe {level} gibt es nicht (1 bis 3)"))
        })
    }
}

/// The five languages of the app; `"en"` … `"zh"` in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    De,
    Fr,
    Ja,
    Zh,
}

impl Lang {
    pub const ALL: [Lang; 5] = [Lang::En, Lang::De, Lang::Fr, Lang::Ja, Lang::Zh];

    /// The language code, as in the `lang` attribute.
    pub fn as_str(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
            Lang::Fr => "fr",
            Lang::Ja => "ja",
            Lang::Zh => "zh",
        }
    }

    pub fn parse(text: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.as_str() == text)
    }

    /// Picks the entry for this language from `[en, de, fr, ja, zh]`.
    pub(crate) fn pick<T: Copy>(self, texts: [T; 5]) -> T {
        texts[self as usize]
    }
}

/// A 60-bit seed. As text it is always 15 lowercase hex digits, also in JSON and
/// postcard, so it survives JavaScript (no integers above 2^53) and fits a
/// challenge code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Seed(u64);

impl Seed {
    pub const BITS: u32 = 60;
    pub const MASK: u64 = (1 << Seed::BITS) - 1;

    /// The seed, or `None` for a value of 2^60 or more.
    pub fn new(value: u64) -> Option<Seed> {
        (value <= Seed::MASK).then_some(Seed(value))
    }

    /// The low 60 bits of `value`, e.g. of a hash or 64 random bits.
    pub fn masked(value: u64) -> Seed {
        Seed(value & Seed::MASK)
    }

    pub fn get(self) -> u64 {
        self.0
    }

    /// Exactly 15 lowercase hex digits.
    pub fn from_hex(text: &str) -> Option<Seed> {
        let lower_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
        if text.len() != 15 || !text.bytes().all(lower_hex) {
            return None;
        }
        u64::from_str_radix(text, 16).ok().map(Seed)
    }
}

impl fmt::Display for Seed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:015x}", self.0)
    }
}

impl Serialize for Seed {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Seed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Seed, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = Seed;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a seed as 15 lowercase hex digits")
            }
            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Seed, E> {
                Seed::from_hex(text).ok_or_else(|| {
                    E::custom(format_args!("kein Seed aus 15 kleinen Hex-Ziffern: {text}"))
                })
            }
        }
        deserializer.deserialize_str(Visitor)
    }
}

/// What a round asks for: the discipline, the level, the size and the time limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec {
    pub discipline: Discipline,
    pub level: Level,
    pub items: u8,
    pub memorize_ms: u32,
    pub recall_ms: u32,
    pub rules: u16,
    pub content: u16,
}

impl Spec {
    /// The fixed size and limits of a level (rules v1, spec §3.1).
    pub fn standard(discipline: Discipline, level: Level) -> Spec {
        const MIN: u32 = 60_000;
        let (items, memorize_min, recall_min) = match (discipline, level.get()) {
            (Discipline::List, 1) => (10, 2, 3),
            (Discipline::List, 2) => (20, 4, 5),
            (Discipline::List, _) => (30, 5, 8),
            (Discipline::Numbers, 1) => (20, 2, 3),
            (Discipline::Numbers, 2) => (40, 3, 5),
            (Discipline::Numbers, _) => (80, 5, 10),
            (Discipline::Cards, 1) => (13, 3, 5),
            (Discipline::Cards, 2) => (26, 4, 8),
            (Discipline::Cards, _) => (52, 5, 10),
        };
        Spec {
            discipline,
            level,
            items,
            memorize_ms: memorize_min * MIN,
            recall_ms: recall_min * MIN,
            rules: RULES,
            content: CONTENT,
        }
    }
}

/// The content of a round: concept ids of the word list, digits 0–9 or card ids
/// (`suit * 13 + rank`). JSON `{"items":[…]}`, `{"digits":[…]}` or `{"cards":[…]}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Task {
    #[serde(rename = "items")]
    List(Vec<u16>),
    #[serde(rename = "digits")]
    Numbers(Vec<u8>),
    #[serde(rename = "cards")]
    Cards(Vec<u8>),
}

impl Task {
    pub fn discipline(&self) -> Discipline {
        match self {
            Task::List(_) => Discipline::List,
            Task::Numbers(_) => Discipline::Numbers,
            Task::Cards(_) => Discipline::Cards,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Task::List(items) => items.len(),
            Task::Numbers(digits) => digits.len(),
            Task::Cards(cards) => cards.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// What a person handed in, one entry per place: typed text (empty when left
/// blank), a digit or `None`, a card or `None`. Same JSON keys as [`Task`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recall {
    #[serde(rename = "items")]
    List(Vec<String>),
    #[serde(rename = "digits")]
    Numbers(Vec<Option<u8>>),
    #[serde(rename = "cards")]
    Cards(Vec<Option<u8>>),
}

impl Recall {
    /// A recall with every place empty, as a round's recall phase starts.
    pub fn blank(discipline: Discipline, items: u8) -> Recall {
        let n = usize::from(items);
        match discipline {
            Discipline::List => Recall::List(vec![String::new(); n]),
            Discipline::Numbers => Recall::Numbers(vec![None; n]),
            Discipline::Cards => Recall::Cards(vec![None; n]),
        }
    }

    pub fn discipline(&self) -> Discipline {
        match self {
            Recall::List(_) => Discipline::List,
            Recall::Numbers(_) => Discipline::Numbers,
            Recall::Cards(_) => Discipline::Cards,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Recall::List(answers) => answers.len(),
            Recall::Numbers(digits) => digits.len(),
            Recall::Cards(cards) => cards.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Places that hold an answer. Text without a letter or digit counts as
    /// empty, as it does in the score.
    pub fn answered(&self) -> usize {
        match self {
            Recall::List(answers) => answers.iter().filter(|a| !matcher::blank(a)).count(),
            Recall::Numbers(digits) => digits.iter().flatten().count(),
            Recall::Cards(cards) => cards.iter().flatten().count(),
        }
    }
}

/// The verdict on one place; lowercase in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mark {
    Right,
    /// Right with one typo (shopping list only).
    Tolerated,
    Wrong,
    Empty,
    /// The item was already counted for an earlier place (shopping list only).
    Duplicate,
}

/// The score of a recall. `marks` has one entry per place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Score {
    pub points: u16,
    pub max: u16,
    pub perfect: bool,
    pub marks: Vec<Mark>,
    /// Shopping list only (empty otherwise): per place, the index in the task of
    /// the item that this place's answer counted for.
    pub matched: Vec<Option<u16>>,
    pub info: Info,
}

/// The line under a result, for information only; it never changes the points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Info {
    /// Shopping list: places whose answer is the item at that same position.
    pub in_order: Option<u16>,
    /// Numbers: digits right, wherever they are.
    pub digits_right: Option<u16>,
    /// Cards: places right up to the first mistake.
    pub first_error: Option<u16>,
}

/// Why a recall cannot be scored. `place` counts from 1, as people see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecallError {
    /// The recall belongs to another discipline than the task.
    Discipline,
    Length {
        expected: usize,
        got: usize,
    },
    /// Longer than [`MAX_ANSWER_CHARS`] characters or [`MAX_ANSWER_BYTES`] bytes.
    TooLong {
        place: usize,
    },
    Digit {
        place: usize,
    },
    Card {
        place: usize,
    },
    /// The card already lies on an earlier place.
    RepeatedCard {
        place: usize,
    },
}

/// Longest answer of a shopping list in characters …
pub const MAX_ANSWER_CHARS: usize = 64;
/// … and in bytes.
pub const MAX_ANSWER_BYTES: usize = 256;

impl fmt::Display for RecallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discipline => write!(f, "Die Antworten gehören zu einer anderen Disziplin"),
            Self::Length { expected, got } => {
                write!(f, "Erwartet waren {expected} Antworten, nicht {got}")
            }
            Self::TooLong { place } => write!(
                f,
                "Die Antwort an Ort {place} ist länger als {MAX_ANSWER_CHARS} Zeichen oder {MAX_ANSWER_BYTES} Bytes"
            ),
            Self::Digit { place } => write!(f, "An Stelle {place} steht keine Ziffer von 0 bis 9"),
            Self::Card { place } => write!(f, "An Ort {place} liegt keine gültige Karte"),
            Self::RepeatedCard { place } => {
                write!(
                    f,
                    "Die Karte an Ort {place} liegt schon an einem anderen Ort"
                )
            }
        }
    }
}

impl std::error::Error for RecallError {}

impl RecallError {
    /// The message for people, in their language.
    pub fn text(&self, lang: Lang) -> &'static str {
        lang.pick(match self {
            Self::Discipline => [
                "These answers belong to another discipline.",
                "Die Antworten gehören zu einer anderen Disziplin.",
                "Ces réponses appartiennent à une autre discipline.",
                "この解答は別の種目のものです。",
                "这些答案属于另一个项目。",
            ],
            Self::Length { .. } => [
                "The number of answers does not match the task.",
                "Die Zahl der Antworten passt nicht zur Aufgabe.",
                "Le nombre de réponses ne correspond pas à la tâche.",
                "解答の数が課題と合いません。",
                "答案数量与题目不符。",
            ],
            Self::TooLong { .. } => [
                "An answer is too long (at most 64 characters).",
                "Eine Antwort ist zu lang (höchstens 64 Zeichen).",
                "Une réponse est trop longue (64 caractères au plus).",
                "長すぎる解答があります（64 文字まで）。",
                "有答案太长（最多 64 个字符）。",
            ],
            Self::Digit { .. } => [
                "Only the digits 0 to 9 count as answers.",
                "Als Antwort zählen nur die Ziffern 0 bis 9.",
                "Seuls les chiffres de 0 à 9 comptent comme réponses.",
                "解答にできるのは 0〜9 の数字だけです。",
                "只能用数字 0 到 9 作答。",
            ],
            Self::Card { .. } => [
                "A place holds a card that does not exist.",
                "An einem Ort liegt eine Karte, die es nicht gibt.",
                "Un lieu contient une carte qui n’existe pas.",
                "存在しないカードが置かれた場所があります。",
                "有位置放了不存在的牌。",
            ],
            Self::RepeatedCard { .. } => [
                "Each card can be on one place only.",
                "Jede Karte kann nur an einem Ort liegen.",
                "Chaque carte ne peut occuper qu’un seul lieu.",
                "同じカードは一つの場所にしか置けません。",
                "同一张牌只能放在一个位置上。",
            ],
        })
    }
}

/// Checks a recall against what the task asks for: the discipline, the number
/// of places, the length of texts, the digits and the cards.
fn check(discipline: Discipline, places: usize, recall: &Recall) -> Result<(), RecallError> {
    if recall.discipline() != discipline {
        return Err(RecallError::Discipline);
    }
    if recall.len() != places {
        return Err(RecallError::Length {
            expected: places,
            got: recall.len(),
        });
    }
    match recall {
        Recall::List(answers) => {
            for (i, answer) in answers.iter().enumerate() {
                if answer.len() > MAX_ANSWER_BYTES || answer.chars().count() > MAX_ANSWER_CHARS {
                    return Err(RecallError::TooLong { place: i + 1 });
                }
            }
        }
        Recall::Numbers(digits) => {
            if let Some(i) = digits.iter().position(|d| d.is_some_and(|d| d > 9)) {
                return Err(RecallError::Digit { place: i + 1 });
            }
        }
        Recall::Cards(cards) => {
            let mut seen = [false; cards::DECK];
            for (i, card) in cards.iter().enumerate() {
                let Some(card) = *card else { continue };
                let slot = seen
                    .get_mut(usize::from(card))
                    .ok_or(RecallError::Card { place: i + 1 })?;
                if *slot {
                    return Err(RecallError::RepeatedCard { place: i + 1 });
                }
                *slot = true;
            }
        }
    }
    Ok(())
}

/// The seed of a daily task: FNV-1a-64 of
/// `locora/competition/v1/daily/{YYYY-MM-DD}/{discipline}/{level}`, low 60 bits.
pub fn daily_seed(date: Date, discipline: Discipline, level: Level) -> Seed {
    let label = format!(
        "locora/competition/v1/daily/{date}/{}/{}",
        discipline.as_str(),
        level.get()
    );
    Seed::masked(rng::fnv1a64(label.as_bytes()))
}

/// The task of `spec` for `seed`, drawn with a fresh generator. Only the
/// discipline and the number of items matter.
pub fn generate(spec: &Spec, seed: Seed) -> Task {
    let mut rng = rng::SplitMix64::new(seed);
    match spec.discipline {
        Discipline::List => Task::List(list::generate(&mut rng, spec.items)),
        Discipline::Numbers => Task::Numbers(numbers::generate(&mut rng, spec.items)),
        Discipline::Cards => Task::Cards(cards::generate(&mut rng, spec.items)),
    }
}

/// Checks a recall before it is handed in: one entry per item, texts at most
/// 64 characters and 256 bytes, digits 0–9, cards below 52 and each card once.
pub fn validate(spec: &Spec, recall: &Recall) -> Result<(), RecallError> {
    check(spec.discipline, usize::from(spec.items), recall)
}

/// Scores a recall against its task (spec §3). `lang` is the language the
/// shopping list was played in; numbers and cards ignore it.
pub fn score(task: &Task, recall: &Recall, lang: Lang) -> Result<Score, RecallError> {
    check(task.discipline(), task.len(), recall)?;
    match (task, recall) {
        (Task::List(items), Recall::List(answers)) => Ok(list::score(items, answers, lang)),
        (Task::Numbers(digits), Recall::Numbers(answers)) => Ok(numbers::score(digits, answers)),
        (Task::Cards(cards), Recall::Cards(answers)) => Ok(cards::score(cards, answers)),
        _ => Err(RecallError::Discipline),
    }
}

/// Sort key of results: more points first, then the shorter memorizing time.
/// Equal keys share a place. Recall time never counts.
pub fn rank_key(points: u16, memorize_ms: u32) -> (Reverse<u16>, u32) {
    (Reverse(points), memorize_ms)
}

/// `points` out of `max`, with `perfect` derived.
pub(crate) fn finish(
    points: u16,
    marks: Vec<Mark>,
    matched: Vec<Option<u16>>,
    info: Info,
) -> Score {
    let max = marks.len() as u16;
    Score {
        points,
        max,
        perfect: points == max,
        marks,
        matched,
        info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(n: u8) -> Level {
        Level::new(n).unwrap()
    }

    #[test]
    fn standard_specs_follow_the_table() {
        let sizes: Vec<_> = Discipline::ALL
            .into_iter()
            .flat_map(|d| Level::ALL.map(|l| Spec::standard(d, l)))
            .map(|s| (s.items, s.memorize_ms / 1_000, s.recall_ms / 1_000))
            .collect();
        assert_eq!(
            sizes,
            [
                (10, 120, 180),
                (20, 240, 300),
                (30, 300, 480),
                (20, 120, 180),
                (40, 180, 300),
                (80, 300, 600),
                (13, 180, 300),
                (26, 240, 480),
                (52, 300, 600),
            ]
        );
        let spec = Spec::standard(Discipline::Cards, level(2));
        assert_eq!((spec.rules, spec.content), (RULES, CONTENT));
        // Numbers come in whole rows.
        for l in Level::ALL {
            assert_eq!(Spec::standard(Discipline::Numbers, l).items % 10, 0);
        }
        assert_eq!(supported_rules(), [1]);
        assert_eq!(supported_content(), [1]);
    }

    #[test]
    fn levels_and_seeds_are_checked() {
        assert_eq!(Level::new(0), None);
        assert_eq!(Level::new(4), None);
        assert_eq!(level(3).get(), 3);
        assert_eq!(Seed::new(Seed::MASK).map(Seed::get), Some((1 << 60) - 1));
        assert_eq!(Seed::new(1 << 60), None);
        assert_eq!(Seed::masked(u64::MAX).get(), Seed::MASK);
        assert_eq!(
            Seed::masked(0x123_4567_89ab_cdef).to_string(),
            "123456789abcdef"
        );
        assert_eq!(Seed::masked(1).to_string(), "000000000000001");
        for bad in [
            "23456789abcdef",
            "0123456789abcdef",
            "123456789ABCDEF",
            "123456789abcdeg",
            " 23456789abcdef",
            "+23456789abcdef",
        ] {
            assert_eq!(Seed::from_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn names_parse_back() {
        for d in Discipline::ALL {
            assert_eq!(Discipline::parse(d.as_str()), Some(d));
        }
        for l in Lang::ALL {
            assert_eq!(Lang::parse(l.as_str()), Some(l));
        }
        assert_eq!(Discipline::parse("List"), None);
        assert_eq!(Lang::parse("xx"), None);
    }

    #[test]
    fn json_shapes() {
        fn json<T: Serialize>(value: &T) -> String {
            serde_json::to_string(value).unwrap()
        }
        assert_eq!(json(&Discipline::List), "\"list\"");
        assert_eq!(json(&Lang::Zh), "\"zh\"");
        assert_eq!(json(&level(2)), "2");
        assert_eq!(json(&Seed::masked(0xabc)), "\"000000000000abc\"");
        assert_eq!(json(&Task::List(vec![1, 2])), r#"{"items":[1,2]}"#);
        assert_eq!(json(&Task::Numbers(vec![3, 1])), r#"{"digits":[3,1]}"#);
        assert_eq!(json(&Task::Cards(vec![36, 18])), r#"{"cards":[36,18]}"#);
        assert_eq!(
            json(&Recall::List(vec!["Milch".into(), String::new()])),
            r#"{"items":["Milch",""]}"#
        );
        assert_eq!(
            json(&Recall::Numbers(vec![Some(3), None])),
            r#"{"digits":[3,null]}"#
        );
        assert_eq!(
            json(&Recall::Cards(vec![None, Some(7)])),
            r#"{"cards":[null,7]}"#
        );
        assert_eq!(json(&Mark::Tolerated), "\"tolerated\"");
        assert_eq!(
            json(&Spec::standard(Discipline::Cards, level(2))),
            r#"{"discipline":"cards","level":2,"items":26,"memorize_ms":240000,"recall_ms":480000,"rules":1,"content":1}"#
        );
        assert_eq!(
            json(&Info {
                digits_right: Some(18),
                ..Info::default()
            }),
            r#"{"in_order":null,"digits_right":18,"first_error":null}"#
        );
    }

    #[test]
    fn serde_refuses_what_cannot_exist() {
        for bad in ["0", "4", "-1", "\"2\""] {
            assert!(serde_json::from_str::<Level>(bad).is_err(), "{bad}");
        }
        for bad in [
            "\"23456789abcdef\"",
            "\"0123456789abcdef\"",
            "\"123456789ABCDEF\"",
            "123456789",
        ] {
            assert!(serde_json::from_str::<Seed>(bad).is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<Lang>("\"xx\"").is_err());
        assert!(serde_json::from_str::<Discipline>("\"words\"").is_err());
        assert!(serde_json::from_str::<Task>(r#"{"list":[1]}"#).is_err());
    }

    #[test]
    fn every_public_type_survives_postcard() {
        fn round_trip<T>(value: &T)
        where
            T: Serialize + serde::de::DeserializeOwned + PartialEq + fmt::Debug,
        {
            let bytes = postcard::to_allocvec(value).unwrap();
            assert_eq!(&postcard::from_bytes::<T>(&bytes).unwrap(), value);
        }
        round_trip(&Discipline::Numbers);
        round_trip(&level(3));
        round_trip(&Lang::Ja);
        round_trip(&Seed::masked(u64::MAX));
        round_trip(&Date::parse("2026-10-28").unwrap());
        round_trip(&Spec::standard(Discipline::List, level(1)));
        round_trip(&Task::List(vec![1, 200]));
        round_trip(&Task::Numbers(vec![0, 9]));
        round_trip(&Task::Cards(vec![51, 0]));
        round_trip(&Recall::List(vec!["りんご".into(), String::new()]));
        round_trip(&Recall::Numbers(vec![Some(0), None]));
        round_trip(&Recall::Cards(vec![None, Some(51)]));
        round_trip(&Mark::Duplicate);
        let task = Task::Cards(vec![1, 2, 3]);
        let recall = Recall::Cards(vec![Some(2), Some(1), None]);
        round_trip(&score(&task, &recall, Lang::En).unwrap());
        round_trip(&schedule::daily(Date::parse("2026-10-25").unwrap()));
        round_trip(&schedule::prep(&Spec::standard(
            Discipline::Numbers,
            level(2),
        )));
        // The seed is text in postcard too: a length byte and 15 digits.
        assert_eq!(postcard::to_allocvec(&Seed::masked(1)).unwrap().len(), 16);
    }

    #[test]
    fn validate_checks_every_limit() {
        let list = Spec::standard(Discipline::List, level(1));
        let mut answers = vec![String::new(); 10];
        assert_eq!(validate(&list, &Recall::List(answers.clone())), Ok(()));
        answers[3] = "x".repeat(64);
        assert_eq!(validate(&list, &Recall::List(answers.clone())), Ok(()));
        answers[3] = "x".repeat(65);
        assert_eq!(
            validate(&list, &Recall::List(answers.clone())),
            Err(RecallError::TooLong { place: 4 })
        );
        // A character takes at most 4 bytes, so 64 of them always fit 256 bytes.
        answers[3] = "𝄞".repeat(64);
        assert_eq!(validate(&list, &Recall::List(answers.clone())), Ok(()));
        answers[3] = "𝄞".repeat(65);
        assert_eq!(
            validate(&list, &Recall::List(answers.clone())),
            Err(RecallError::TooLong { place: 4 })
        );
        answers.pop();
        assert_eq!(
            validate(&list, &Recall::List(answers)),
            Err(RecallError::Length {
                expected: 10,
                got: 9
            })
        );
        assert_eq!(
            validate(&list, &Recall::Numbers(vec![None; 10])),
            Err(RecallError::Discipline)
        );

        let numbers = Spec::standard(Discipline::Numbers, level(1));
        let mut digits = vec![Some(9); 20];
        assert_eq!(validate(&numbers, &Recall::Numbers(digits.clone())), Ok(()));
        digits[17] = Some(10);
        assert_eq!(
            validate(&numbers, &Recall::Numbers(digits)),
            Err(RecallError::Digit { place: 18 })
        );

        let cards = Spec::standard(Discipline::Cards, level(1));
        let mut placed: Vec<Option<u8>> = (0..13).map(|c| Some(c * 4)).collect();
        assert_eq!(validate(&cards, &Recall::Cards(placed.clone())), Ok(()));
        placed[0] = None;
        placed[1] = None;
        assert_eq!(validate(&cards, &Recall::Cards(placed.clone())), Ok(()));
        placed[5] = Some(52);
        assert_eq!(
            validate(&cards, &Recall::Cards(placed.clone())),
            Err(RecallError::Card { place: 6 })
        );
        placed[5] = Some(48);
        assert_eq!(
            validate(&cards, &Recall::Cards(placed)),
            Err(RecallError::RepeatedCard { place: 13 })
        );
    }

    #[test]
    fn score_refuses_a_recall_that_does_not_fit_the_task() {
        let task = Task::Cards(vec![1, 2, 3]);
        assert_eq!(
            score(
                &task,
                &Recall::Cards(vec![Some(1), Some(1), None]),
                Lang::En
            ),
            Err(RecallError::RepeatedCard { place: 2 })
        );
        assert_eq!(
            score(&task, &Recall::Cards(vec![Some(1)]), Lang::En),
            Err(RecallError::Length {
                expected: 3,
                got: 1
            })
        );
        assert_eq!(
            score(&task, &Recall::Numbers(vec![None; 3]), Lang::En),
            Err(RecallError::Discipline)
        );
    }

    #[test]
    fn recall_counts_its_answers() {
        assert_eq!(
            Recall::blank(Discipline::Cards, 13),
            Recall::Cards(vec![None; 13])
        );
        assert_eq!(Recall::blank(Discipline::List, 2).answered(), 0);
        assert_eq!(Recall::List(vec![" ".into(), "Brot".into()]).answered(), 1);
        assert_eq!(Recall::List(vec!["…".into(), "- ?".into()]).answered(), 0);
        assert_eq!(Recall::Numbers(vec![None, Some(0), Some(4)]).answered(), 2);
        assert_eq!(Recall::Cards(vec![None, Some(51)]).answered(), 1);
    }

    #[test]
    fn ranking_prefers_points_then_the_shorter_memorizing_time() {
        let mut results = [(24, 181_000), (26, 240_000), (24, 150_000), (26, 240_000)];
        results.sort_by_key(|&(points, ms)| rank_key(points, ms));
        assert_eq!(
            results,
            [(26, 240_000), (26, 240_000), (24, 150_000), (24, 181_000)]
        );
    }

    #[test]
    fn error_texts_exist_in_every_language() {
        let errors = [
            RecallError::Discipline,
            RecallError::Length {
                expected: 10,
                got: 9,
            },
            RecallError::TooLong { place: 1 },
            RecallError::Digit { place: 1 },
            RecallError::Card { place: 1 },
            RecallError::RepeatedCard { place: 1 },
        ];
        for error in errors {
            assert!(!error.to_string().is_empty());
            let texts: Vec<_> = Lang::ALL.map(|l| error.text(l)).into();
            for (i, text) in texts.iter().enumerate() {
                assert!(!text.is_empty() && !text.contains('{'), "{error:?}");
                assert!(!texts[..i].contains(text), "{error:?}: {text}");
            }
        }
    }
}
