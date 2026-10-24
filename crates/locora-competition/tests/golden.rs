//! Golden vectors of generator v1 and rules v1 (spec Appendix A, addendum D6).
//!
//! The files in `tests/golden` were computed with an independent reference
//! implementation and are compiled in, so the tests also run under wasmtime
//! without files. Locora Cloud reads the same files to check its scoring. If a
//! test here fails, version 1 changed: put the change into a version 2 instead.

use locora_competition::rng::{SplitMix64, fnv1a64};
use locora_competition::{
    Date, Discipline, Lang, Level, Recall, Score, Seed, Spec, Task, cards, code, daily_seed,
    generate, schedule, score, words, xp,
};
use serde::Deserialize;

fn hex(text: &str) -> u64 {
    u64::from_str_radix(text, 16).unwrap()
}

/// The notation of the golden files: rank `A 2 … 10 J Q K`, suit `S H D C`.
fn names(cards: &[u8]) -> String {
    let rank = |c: u8| match cards::rank(c) {
        0 => "A".to_string(),
        10 => "J".to_string(),
        11 => "Q".to_string(),
        12 => "K".to_string(),
        r => (r + 1).to_string(),
    };
    let suit = |c: u8| ["S", "H", "D", "C"][usize::from(cards::suit(c))];
    let names: Vec<String> = cards.iter().map(|&c| rank(c) + suit(c)).collect();
    names.join(" ")
}

fn digits(task: &Task) -> String {
    let Task::Numbers(digits) = task else {
        panic!("not a numbers task: {task:?}");
    };
    let text: String = digits.iter().map(|d| char::from(b'0' + d)).collect();
    let rows: Vec<&str> = text
        .as_bytes()
        .chunks(10)
        .map(|row| std::str::from_utf8(row).unwrap())
        .collect();
    rows.join(" ")
}

#[derive(Deserialize)]
struct Generator {
    fnv1a64: Vec<Hash>,
    splitmix64: Vec<Stream>,
    below: Draws,
    deck: Deck,
    digits: Digits,
}

#[derive(Deserialize)]
struct Hash {
    label: String,
    hash: String,
}

#[derive(Deserialize)]
struct Stream {
    seed: Seed,
    next: Vec<String>,
}

#[derive(Deserialize)]
struct Draws {
    seed: Seed,
    draws: Vec<Draw>,
}

#[derive(Deserialize)]
struct Draw {
    n: u64,
    value: u64,
}

#[derive(Deserialize)]
struct Deck {
    seed: Seed,
    cards: Vec<u8>,
    names: String,
}

#[derive(Deserialize)]
struct Digits {
    seed: Seed,
    digits: String,
}

fn generator() -> Generator {
    serde_json::from_str(include_str!("golden/generator.json")).unwrap()
}

fn level(n: u8) -> Level {
    Level::new(n).unwrap()
}

#[test]
fn fnv1a64_of_labels() {
    let golden = generator();
    assert_eq!(golden.fnv1a64.len(), 3);
    for case in golden.fnv1a64 {
        assert_eq!(
            fnv1a64(case.label.as_bytes()),
            hex(&case.hash),
            "{:?}",
            case.label
        );
    }
}

#[test]
fn splitmix64_streams() {
    for stream in generator().splitmix64 {
        let mut rng = SplitMix64::new(stream.seed);
        for expected in stream.next {
            assert_eq!(rng.next_u64(), hex(&expected));
        }
    }
}

#[test]
fn bounded_draws() {
    let golden = generator().below;
    let mut rng = SplitMix64::new(golden.seed);
    let draws: Vec<_> = golden.draws.iter().map(|d| rng.below(d.n)).collect();
    let expected: Vec<_> = golden.draws.iter().map(|d| d.value).collect();
    assert_eq!(draws, expected);
    assert_eq!(draws, [4, 8, 1]);
}

#[test]
fn the_full_deck_of_a_seed() {
    let golden = generator().deck;
    assert_eq!(golden.seed, Seed::masked(0x123_4567_89ab_cdef));
    let spec = Spec {
        items: 52,
        ..Spec::standard(Discipline::Cards, level(3))
    };
    let task = generate(&spec, golden.seed).unwrap();
    assert_eq!(task, Task::Cards(golden.cards.clone()));
    assert_eq!(names(&golden.cards), golden.names);
    // Smaller levels take the start of the same deck.
    let Ok(Task::Cards(first)) =
        generate(&Spec::standard(Discipline::Cards, level(1)), golden.seed)
    else {
        panic!("cards");
    };
    assert_eq!(first[..], golden.cards[..13]);
}

#[test]
fn the_digits_of_a_seed() {
    let golden = generator().digits;
    let task = generate(&Spec::standard(Discipline::Numbers, level(1)), golden.seed).unwrap();
    assert_eq!(digits(&task), golden.digits);
    assert_eq!(golden.digits, "0816007591 8455281058");
}

#[derive(Deserialize)]
struct DailyCase {
    date: Date,
    discipline: Discipline,
    level: Level,
    seed: Seed,
    task: Option<Task>,
    names: Option<String>,
}

#[test]
fn daily_seeds_and_their_tasks() {
    let cases: Vec<DailyCase> = serde_json::from_str(include_str!("golden/daily.json")).unwrap();
    assert_eq!(cases.len(), 5);
    for case in cases {
        assert_eq!(
            daily_seed(case.date, case.discipline, case.level),
            case.seed
        );
        let spec = Spec::standard(case.discipline, case.level);
        if let Some(task) = case.task {
            assert_eq!(
                generate(&spec, case.seed),
                Ok(task.clone()),
                "{}",
                case.date
            );
            if let (Some(expected), Task::Cards(cards)) = (case.names, &task) {
                assert_eq!(names(cards), expected);
            }
        }
    }
    // The values of the appendix, spelled out.
    let date = |text| Date::parse(text).unwrap();
    let seed = daily_seed(date("2026-10-27"), Discipline::Numbers, level(1));
    assert_eq!(seed.to_string(), "906193cffb3b5ed");
    let task = generate(&Spec::standard(Discipline::Numbers, level(1)), seed).unwrap();
    assert_eq!(digits(&task), "3378122732 1509657859");
    assert_eq!(
        daily_seed(date("2026-10-26"), Discipline::List, level(1)).to_string(),
        "e8e9eb445f8a08c"
    );
}

#[derive(Deserialize)]
struct ListCase {
    seed: Seed,
    items: u8,
    ids: Vec<u16>,
    keys: String,
    date: Option<Date>,
    level: Option<Level>,
}

#[test]
fn shopping_lists_of_a_seed() {
    let cases: Vec<ListCase> = serde_json::from_str(include_str!("golden/list.json")).unwrap();
    assert_eq!(cases.len(), 4);
    for case in cases {
        let spec = Spec {
            items: case.items,
            ..Spec::standard(Discipline::List, case.level.unwrap_or(level(1)))
        };
        assert_eq!(generate(&spec, case.seed), Ok(Task::List(case.ids.clone())));
        let keys: Vec<&str> = case
            .ids
            .iter()
            .map(|&id| words::pool().get(id).unwrap().key)
            .collect();
        assert_eq!(keys.join(" "), case.keys);
        if let (Some(date), Some(level)) = (case.date, case.level) {
            assert_eq!(daily_seed(date, Discipline::List, level), case.seed);
        }
    }
}

#[derive(Deserialize)]
struct ScheduleCase {
    date: Date,
    days: i64,
    weekday: u8,
    daily: schedule::Daily,
}

#[test]
fn the_weekly_schedule() {
    let cases: Vec<ScheduleCase> =
        serde_json::from_str(include_str!("golden/schedule.json")).unwrap();
    assert_eq!(cases.len(), 11);
    for case in cases {
        assert_eq!(case.date.days_since_epoch(), case.days, "{}", case.date);
        assert_eq!(case.date.weekday(), case.weekday, "{}", case.date);
        assert_eq!(schedule::daily(case.date), case.daily, "{}", case.date);
    }
}

#[derive(Deserialize)]
struct CodeCase {
    discipline: Discipline,
    level: Level,
    seed: Seed,
    code: String,
}

#[test]
fn challenge_codes() {
    let cases: Vec<CodeCase> = serde_json::from_str(include_str!("golden/codes.json")).unwrap();
    assert_eq!(cases[0].code, "C2-28T5-CY4T-QKFF-K");
    for case in cases {
        assert_eq!(
            code::encode(case.discipline, case.level, case.seed),
            case.code
        );
        assert_eq!(
            code::decode(&case.code),
            Ok((case.discipline, case.level, case.seed))
        );
    }
}

#[derive(Deserialize)]
struct ScoreCase {
    name: String,
    task: Task,
    recall: Recall,
    lang: Lang,
    score: Score,
}

#[test]
fn scoring_examples() {
    let cases: Vec<ScoreCase> = serde_json::from_str(include_str!("golden/scoring.json")).unwrap();
    assert_eq!(cases.len(), 15);
    for case in cases {
        assert_eq!(
            score(&case.task, &case.recall, case.lang).as_ref(),
            Ok(&case.score),
            "{}",
            case.name
        );
    }
}

#[derive(Deserialize)]
struct XpCase {
    what: String,
    level: Level,
    evening: bool,
    answered: bool,
    xp: u32,
}

/// What gamification adds for a contest round (addendum D6 and D8).
fn contest_xp(level: Level, evening: bool, answered: bool) -> u32 {
    match (answered, evening) {
        (false, _) => 0,
        (true, true) => xp::EVENING,
        (true, false) => xp::daily(level),
    }
}

#[test]
fn xp_for_taking_part() {
    let cases: Vec<XpCase> = serde_json::from_str(include_str!("golden/xp.json")).unwrap();
    assert_eq!(cases.len(), 6);
    for case in cases {
        assert_eq!(
            contest_xp(case.level, case.evening, case.answered),
            case.xp,
            "{}",
            case.what
        );
    }
    // `answered` is what `xp::earns` says about the recall.
    assert!(!xp::earns(&Recall::blank(Discipline::Cards, 26)));
}
