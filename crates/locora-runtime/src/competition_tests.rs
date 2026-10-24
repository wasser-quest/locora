//! Two devices with the real plugins play the contest offline and exchange their operations the
//! way the cloud does (the harness of `game_tests`): the history is the union, both derive the
//! same daily result, records and trophies, and deleting the game progress empties the contest on
//! both, also when old rounds arrive later.

use std::collections::BTreeSet;
use std::time::Duration;

use locora_api::competition::{Counted, LevelState, RecordsResponse};
use locora_competition::words;

use super::*;
use crate::game::game_tests::{Device, device, exchange, ship};

/// The recall that gets every place right; `miss` leaves the first place empty.
fn answers(task: &Task, miss: bool) -> Recall {
    let mut recall = match task {
        Task::List(items) => Recall::List(
            items
                .iter()
                .map(|id| words::display(*id, Lang::De).unwrap().to_owned())
                .collect(),
        ),
        Task::Numbers(digits) => Recall::Numbers(digits.iter().copied().map(Some).collect()),
        Task::Cards(cards) => Recall::Cards(cards.iter().copied().map(Some).collect()),
    };
    if miss {
        match &mut recall {
            Recall::List(answers) => answers[0].clear(),
            Recall::Numbers(cells) | Recall::Cards(cells) => cells[0] = None,
        }
    }
    recall
}

fn hand_in(device: &mut Device, round: &Round, miss: bool) -> FinishResponse {
    let hand_in = HandIn {
        recall: answers(&round.task, miss),
        lang: Lang::De,
        untimed: false,
        route: None,
        memorize_ms: 30_000,
        recall_ms: 40_000,
    };
    device.profile.compete_finish(round, hand_in).unwrap()
}

/// A practice round at level 1, handed in.
fn practice(device: &mut Device, discipline: Discipline) -> Round {
    let level = Level::new(1).unwrap();
    let round = device
        .profile
        .compete_begin(Kind::Practice, discipline, level, None)
        .unwrap();
    hand_in(device, &round, false);
    round
}

fn history(device: &mut Device) -> Vec<String> {
    let entries = device.profile.compete_history(0, None).unwrap();
    entries.into_iter().map(|e| e.round_id).collect()
}

fn records(device: &mut Device) -> RecordsResponse {
    device.profile.compete_records().unwrap()
}

#[test]
fn two_devices_play_the_same_daily_offline_and_agree_after_the_sync() {
    let mut a = device();
    let mut b = device();
    assert!(a.profile.has_competition(), "the contest is built in");
    let discipline = a.profile.compete_today().unwrap().today.discipline;
    let level = Level::new(1).unwrap();
    // A begins first; B, offline, a moment later. Both get the day's task.
    let on_a = a
        .profile
        .compete_begin(Kind::Daily, discipline, level, None)
        .unwrap();
    std::thread::sleep(Duration::from_millis(5));
    let on_b = b
        .profile
        .compete_begin(Kind::Daily, discipline, level, None)
        .unwrap();
    assert_eq!(on_a.task, on_b.task, "the same task for everyone");
    assert_eq!(on_a.code(), None, "a daily offers no code");
    // A misses a place, B gets all; A also practises another discipline.
    let first = hand_in(&mut a, &on_a, true);
    let second = hand_in(&mut b, &on_b, false);
    assert!(!first.counted_as_practice && !second.counted_as_practice);
    assert_eq!(first.trophies, ["compete-first"]);
    let other = Discipline::ALL.into_iter().find(|d| *d != discipline);
    let extra = practice(&mut a, other.unwrap());
    exchange(&mut a, &mut b);
    let union: BTreeSet<String> = [&on_a.id, &on_b.id, &extra.id]
        .into_iter()
        .cloned()
        .collect();
    let expected = records(&mut a);
    for device in [&mut a, &mut b] {
        let seen: BTreeSet<String> = history(device).into_iter().collect();
        assert_eq!(seen, union, "the history is the union");
        let today = device.profile.compete_today().unwrap();
        match &today.levels[0].state {
            LevelState::Done {
                round_id, counted, ..
            } => assert_eq!((round_id, *counted), (&on_a.id, Counted::Daily)),
            other => panic!("{other:?}"),
        }
        let entries = device.profile.compete_history(0, None).unwrap();
        let practice: Vec<&str> = entries
            .iter()
            .filter(|e| e.counted_as_practice)
            .map(|e| e.round_id.as_str())
            .collect();
        assert_eq!(practice, [on_b.id.as_str()], "B began later");
        assert_eq!(records(device), expected, "the same records and trophies");
    }
    let best = expected
        .levels
        .iter()
        .find(|l| l.discipline == discipline && l.level == level)
        .and_then(|l| l.best.clone())
        .unwrap();
    assert_eq!(best.round_id, on_a.id, "B saw the task second");
    assert_eq!(expected.daily_days, 1);
    // And the day's try cannot be begun again on either device.
    for device in [&mut a, &mut b] {
        let again = device
            .profile
            .compete_begin(Kind::Daily, discipline, level, None);
        assert!(again.is_err());
    }
}

#[test]
fn deleting_the_game_progress_empties_the_contest_on_both_devices() {
    let mut a = device();
    let mut b = device();
    practice(&mut a, Discipline::Cards);
    exchange(&mut a, &mut b);
    assert_eq!(history(&mut b).len(), 1);
    // A deletes; B, offline, practises under the old epoch.
    a.profile.reset_game().unwrap();
    assert!(history(&mut a).is_empty());
    assert!(
        records(&mut a)
            .trophies
            .iter()
            .all(|t| t.unlocked.is_none())
    );
    practice(&mut b, Discipline::Cards);
    // B's old round arrives first, then B learns of the deletion: both end empty.
    ship(&b, &mut a);
    assert!(history(&mut a).is_empty());
    ship(&a, &mut b);
    assert!(history(&mut b).is_empty());
    exchange(&mut a, &mut b);
    for device in [&a, &b] {
        let old = device
            .profile
            .store
            .kv_list(PLUGIN, "g/0/", 0, 1000)
            .unwrap();
        assert_eq!(old.entries.len(), 4, "two rounds and their sightings");
        assert!(old.entries.iter().all(|(_, value)| value.is_empty()));
    }
    // B's next round counts, on both.
    let next = practice(&mut b, Discipline::Numbers);
    exchange(&mut a, &mut b);
    assert_eq!(history(&mut a), history(&mut b));
    assert_eq!(history(&mut a), [next.id]);
}

#[test]
fn ids_are_32_hex_characters_and_seeds_stay_below_2_to_the_60() {
    let mut ids = BTreeSet::new();
    for _ in 0..1_000 {
        let id = new_id();
        assert_eq!(id.len(), 32);
        assert!(
            id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
            "{id}"
        );
        ids.insert(id);
        assert!(new_seed().get() < 1 << 60);
    }
    assert_eq!(ids.len(), 1_000);
    // The seeds differ too, and a practice round's code plays its task again.
    let seeds: BTreeSet<u64> = (0..100).map(|_| new_seed().get()).collect();
    assert_eq!(seeds.len(), 100);
    let mut a = device();
    let round = practice(&mut a, Discipline::List);
    let (discipline, level, seed) = code::decode(&round.code().unwrap()).unwrap();
    assert_eq!(
        (discipline, level, seed),
        (round.spec.discipline, round.spec.level, round.seed)
    );
}

#[test]
fn the_phases_are_capped_at_their_limits() {
    let spec = Spec::standard(Discipline::List, Level::new(1).unwrap());
    let minutes = |m: u64| Duration::from_secs(60 * m);
    assert_eq!(
        capped(&spec, false, minutes(1), minutes(1)),
        (60_000, 60_000)
    );
    assert_eq!(
        capped(&spec, false, minutes(9), minutes(9)),
        (120_000, 180_000 + RECALL_GRACE_MS),
        "memorize ≤ limit, recall ≤ limit + 10 s"
    );
    assert_eq!(
        capped(&spec, true, minutes(9), minutes(9)),
        (540_000, 540_000)
    );
}
