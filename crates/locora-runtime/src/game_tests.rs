//! Two devices with the real plugins, exchanging their operations the way the cloud does
//! (`Store::apply_remote`): progress made offline on both is merged without loss or double
//! counting, and deleting it wins on every device.

use std::collections::BTreeMap;

use locora_api::core::{Grade, JourneyCompleted, JourneyStarted};
use locora_api::gamification::{StatusV2Response, Trophy};
use locora_api::journey::{Answer, AnswerKind, QuizOption, Submission};
use locora_api::review::{CardState, Restore, RestoreRequest};
use locora_store::RemoteOp;
use serde::Serialize;
use tempfile::TempDir;

use super::*;
use crate::{AnswerRequest, Profile, now};

pub(crate) struct Device {
    _dir: TempDir,
    pub(crate) profile: Profile,
    /// How far the other device's log was taken over.
    pub(crate) seen: i64,
}

pub(crate) fn device() -> Device {
    let dir = TempDir::new().unwrap();
    let profile = Profile::open(Some(dir.path())).unwrap();
    assert!(profile.has_game(), "the gamification plugin is built in");
    Device {
        _dir: dir,
        profile,
        seen: 0,
    }
}

/// Everything `from` logged that `to` has not taken over yet, applied in order.
pub(crate) fn ship(from: &Device, to: &mut Device) {
    let ops = from.profile.store.ops_since(to.seen).unwrap();
    to.seen = ops.last().map_or(to.seen, |op| op.seq);
    let remote: Vec<RemoteOp> = ops
        .into_iter()
        .map(|op| RemoteOp {
            hlc: op.hlc,
            entity: op.entity,
            entity_id: op.entity_id,
            op: op.op,
            data: op.data,
        })
        .collect();
    let report = to.profile.store.apply_remote(&remote).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
}

pub(crate) fn exchange(a: &mut Device, b: &mut Device) {
    ship(a, b);
    ship(b, a);
}

fn card(due: u64) -> CardState {
    CardState {
        stability: 3.0,
        difficulty: 5.0,
        last_review: due.saturating_sub(3 * 86_400),
        due,
        reps: 2,
        lapses: 0,
    }
}

/// Review cards for loci `l<n>` of palace `p`, due an hour ago (or in a week when `early`).
fn plan(device: &mut Device, loci: std::ops::Range<u32>, early: bool) {
    let due = if early {
        now() + 7 * 86_400
    } else {
        now() - 3_600
    };
    let cards = loci.map(|n| (format!("l{n}"), card(due))).collect();
    device
        .profile
        .host
        .call::<Restore>(&RestoreRequest {
            palace_id: "p".into(),
            cards,
        })
        .unwrap();
}

fn answer_as(device: &mut Device, locus: &str, answer: &Answer, submission: Submission) {
    let answered = device
        .profile
        .answer(AnswerRequest {
            palace_id: "p",
            locus_id: locus,
            answer,
            expected: "Sonne",
            submission,
            elapsed_ms: 1_000,
        })
        .unwrap();
    assert!(answered.notes.is_empty(), "{:?}", answered.notes);
}

fn answer(device: &mut Device, locus: &str, grade: Grade) {
    answer_as(device, locus, &Answer::default(), Submission::Recall(grade));
}

fn status(device: &mut Device) -> StatusV2Response {
    device.profile.gamification_status(false).unwrap().unwrap()
}

/// What both devices must agree on.
fn standing(device: &mut Device) -> (u64, u32, (u32, u8, u32), Vec<String>) {
    let status = status(device);
    (
        status.xp,
        status.learning_days,
        (status.week.days, status.week.goal, status.week.streak),
        status.trophies.into_iter().map(|t| t.id).collect(),
    )
}

#[test]
fn offline_progress_on_two_devices_adds_up_once_in_either_order() {
    let mut a = device();
    let mut b = device();
    plan(&mut a, 0..10, false);
    exchange(&mut a, &mut b);
    // Offline: A reviews five due loci, B two others.
    for n in 0..5 {
        answer(&mut a, &format!("l{n}"), Grade::Good);
    }
    for n in 5..7 {
        answer(&mut b, &format!("l{n}"), Grade::Again);
    }
    assert_eq!(status(&mut a).xp, 50);
    assert_eq!(status(&mut b).xp, 20);
    let mut c = Device {
        seen: 0,
        ..device()
    };
    // B first, then A, on a third device; A and B both ways.
    ship(&b, &mut c);
    ship(&a, &mut c);
    exchange(&mut a, &mut b);
    let expected = standing(&mut a);
    assert_eq!(expected.0, 70, "no XP lost, none counted twice");
    assert_eq!(expected.1, 1, "one learning day, not two");
    assert_eq!(standing(&mut b), expected);
    assert_eq!(standing(&mut c), expected);
    // The same operations again change nothing.
    a.seen = 0;
    b.seen = 0;
    exchange(&mut a, &mut b);
    assert_eq!(standing(&mut a), expected);
    assert_eq!(standing(&mut b), expected);
}

#[test]
fn the_same_locus_on_both_devices_on_one_day_counts_once() {
    let mut a = device();
    let mut b = device();
    plan(&mut a, 0..3, false);
    exchange(&mut a, &mut b);
    answer(&mut a, "l0", Grade::Good);
    answer(&mut b, "l0", Grade::Hard);
    exchange(&mut a, &mut b);
    assert_eq!(status(&mut a).xp, 10);
    assert_eq!(status(&mut b).xp, 10);
}

#[test]
fn due_reviews_earn_new_loci_less_early_practice_nothing_whatever_the_grade() {
    let mut a = device();
    // One due locus stays open, so the day is not cleared (that bonus is tested in the ledger).
    plan(&mut a, 0..3, false);
    plan(&mut a, 10..11, true);
    answer(&mut a, "l0", Grade::Again);
    assert_eq!(status(&mut a).xp, 10, "a due review: 10 whatever the grade");
    answer(&mut a, "l1", Grade::Easy);
    assert_eq!(status(&mut a).xp, 20);
    answer(&mut a, "l10", Grade::Easy);
    assert_eq!(status(&mut a).xp, 20, "practising early earns nothing");
    answer(&mut a, "l20", Grade::Good);
    assert_eq!(status(&mut a).xp, 26, "a new locus: 6");
    // Again on the same day, after its card moved: no more XP.
    a.profile
        .publish(&JourneyStarted {
            palace_ids: vec!["p".into()],
        })
        .unwrap();
    answer(&mut a, "l20", Grade::Good);
    answer(&mut a, "l0", Grade::Good);
    let status = status(&mut a);
    assert_eq!(status.xp, 26);
    let session = status.session.unwrap();
    assert_eq!((session.xp, session.repeat, session.early), (0, 2, 0));
}

#[test]
fn a_review_due_later_today_is_due_all_day() {
    let mut a = device();
    let day = a.profile.due_day().unwrap();
    let later = (day.end - 60).max(now() + 30);
    if later >= day.end {
        return; // A minute before the day turns: nothing to tell apart.
    }
    let cards = [("l0".to_owned(), card(later))].into_iter().collect();
    a.profile
        .host
        .call::<Restore>(&RestoreRequest {
            palace_id: "p".into(),
            cards,
        })
        .unwrap();
    let before = status(&mut a);
    assert_eq!(before.today.due_open, 1, "due today, though later");
    assert_eq!(before.today.goal, locora_api::gamification::Goal::Open);
    answer(&mut a, "l0", Grade::Good);
    let after = status(&mut a);
    assert_eq!(after.xp, 10 + 20, "a due review, and all due done");
    assert_eq!(after.today.goal, locora_api::gamification::Goal::AllDone);
}

#[test]
fn opening_the_app_on_a_day_with_nothing_due_adds_no_learning_day() {
    let mut a = device();
    plan(&mut a, 0..3, true);
    for _ in 0..3 {
        let status = status(&mut a);
        assert_eq!(status.learning_days, 0);
        assert_eq!(status.week.days, 0);
        assert_eq!(status.today.goal, locora_api::gamification::Goal::Free);
        assert!(status.trophies.is_empty());
        // Nothing is due today: the week needs one learning day less.
        assert_eq!(status.week.free, 1);
        assert_eq!(status.week.target, 3);
    }
    let xp = status(&mut a).xp;
    assert_eq!(xp, 0);
}

#[test]
fn the_answer_is_classified_before_the_review_plugin_moves_its_card() {
    let mut a = device();
    plan(&mut a, 0..2, false);
    a.profile
        .publish(&JourneyStarted {
            palace_ids: vec!["p".into()],
        })
        .unwrap();
    answer(&mut a, "l0", Grade::Good);
    answer(&mut a, "l9", Grade::Good);
    let session = status(&mut a).session.unwrap();
    assert_eq!((session.due, session.new, session.early), (1, 1, 0));
    assert_eq!(session.xp, 16);
    // The review plugin has moved both cards by now.
    let cards = a.profile.schedule(Some("p")).unwrap();
    assert!(cards["l0"].due > now() && cards["l9"].due > now());
}

#[test]
fn only_quiz_and_guess_answers_make_a_run() {
    let mut a = device();
    a.profile
        .publish(&JourneyStarted {
            palace_ids: vec!["p".into()],
        })
        .unwrap();
    let quiz = Answer {
        kind: AnswerKind::Quiz,
        options: vec![
            QuizOption {
                text: "Sonne".into(),
                correct: true,
            },
            QuizOption {
                text: "Mond".into(),
                correct: false,
            },
        ],
        accept: Vec::new(),
    };
    answer_as(&mut a, "q0", &quiz, Submission::Quiz(vec![0]));
    answer_as(&mut a, "q1", &quiz, Submission::Quiz(vec![0]));
    answer(&mut a, "r0", Grade::Again);
    assert_eq!(
        status(&mut a).session.unwrap().combo,
        2,
        "recall never breaks it"
    );
    answer_as(&mut a, "q2", &quiz, Submission::Quiz(vec![1]));
    assert_eq!(status(&mut a).session.unwrap().combo, 0);
}

/// The progress blob of the versions before the sync, in its layout.
#[derive(Serialize)]
struct OldProgress {
    xp: u64,
    max_combo: u32,
    trophies: Vec<Trophy>,
    sessions: BTreeMap<String, (u32, u32, u8)>,
    last_palace: Option<String>,
    daily_day: Option<u64>,
    daily_correct: u32,
}

fn old_device(xp: u64) -> Device {
    let dir = TempDir::new().unwrap();
    let blob = OldProgress {
        xp,
        max_combo: 4,
        trophies: vec![Trophy::FirstJourney, Trophy::PerfectJourney],
        sessions: BTreeMap::new(),
        last_palace: None,
        daily_day: None,
        daily_correct: 3,
    };
    {
        let store = locora_store::Store::open(dir.path()).unwrap();
        store
            .kv_set(
                "gamification",
                "progress",
                &locora_api::encode(&blob).unwrap(),
            )
            .unwrap();
    }
    let profile = Profile::open(Some(dir.path())).unwrap();
    Device {
        _dir: dir,
        profile,
        seen: 0,
    }
}

#[test]
fn existing_progress_is_taken_over_once_without_double_counting() {
    let mut a = old_device(340);
    let status = status(&mut a);
    assert_eq!(status.xp, 340);
    let trophies: Vec<&str> = status.trophies.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(trophies, ["first-journey", "hundred-xp", "perfect-journey"]);
    // Opening again changes nothing.
    let dir = a._dir;
    let ops = a.profile.store.ops_since(0).unwrap().len();
    drop(a.profile);
    let mut again = Device {
        profile: Profile::open(Some(dir.path())).unwrap(),
        _dir: dir,
        seen: 0,
    };
    assert_eq!(again.profile.store.ops_since(0).unwrap().len(), ops);
    assert_eq!(self::status(&mut again).xp, 340);
    // Two devices that synced the same blob before have 340, not 680.
    let mut b = old_device(340);
    exchange(&mut again, &mut b);
    assert_eq!(self::status(&mut again).xp, 340);
    assert_eq!(self::status(&mut b).xp, 340);
}

/// A browser from before the sync: its own progress (never synced) and the desktop's synced blob
/// in the same store, the browser's kept for the migration (see `keep_own_game_progress`).
fn old_browser(own: u64, synced: u64) -> Device {
    let dir = TempDir::new().unwrap();
    let blob = |xp| OldProgress {
        xp,
        max_combo: 0,
        trophies: vec![Trophy::FirstJourney],
        sessions: BTreeMap::new(),
        last_palace: None,
        daily_day: None,
        daily_correct: 0,
    };
    {
        let store = locora_store::Store::open(dir.path()).unwrap();
        store
            .kv_set(
                "gamification",
                "progress",
                &locora_api::encode(&blob(synced)).unwrap(),
            )
            .unwrap();
        let bytes = locora_api::encode(&blob(own)).unwrap();
        let profile = Profile::from_parts(
            std::sync::Arc::new(store),
            dir.path().to_owned(),
            locora_host::PluginHost::new(None).unwrap(),
        );
        assert!(profile.keep_own_game_progress(&bytes).unwrap());
    }
    let profile = Profile::open(Some(dir.path())).unwrap();
    Device {
        _dir: dir,
        profile,
        seen: 0,
    }
}

#[test]
fn a_browsers_own_progress_is_added_to_the_synced_one_once() {
    // The browser had 300 XP of its own; the desktop's synced blob (500) had already arrived.
    let mut web = old_browser(300, 500);
    assert_eq!(status(&mut web).xp, 800);
    let mut desktop = old_device(500);
    assert_eq!(status(&mut desktop).xp, 500);
    exchange(&mut web, &mut desktop);
    assert_eq!(status(&mut web).xp, 800, "nothing lost, nothing twice");
    assert_eq!(status(&mut desktop).xp, 800);
    // The desktop's blob never changed: an older desktop still reads its own numbers.
    let blob = desktop
        .profile
        .store
        .kv_get("gamification", "progress")
        .unwrap()
        .unwrap();
    assert_eq!(
        locora_api::decode::<OldProgressRead>(&blob).unwrap().xp,
        500
    );
    // Kept only before the migration.
    assert!(!web.profile.keep_own_game_progress(&[1, 2]).unwrap());
}

/// The first field of the old blob, enough to read its XP back.
#[derive(serde::Deserialize)]
struct OldProgressRead {
    xp: u64,
}

#[test]
fn deleting_the_progress_wins_against_old_facts_that_arrive_later() {
    let mut a = device();
    let mut b = device();
    plan(&mut a, 0..10, false);
    for n in 0..3 {
        answer(&mut a, &format!("l{n}"), Grade::Good);
    }
    unlock_app_medal(&a.profile.store, "tour-done", "2026-10-20").unwrap();
    exchange(&mut a, &mut b);
    assert_eq!(status(&mut b).xp, 30);
    let due_before = due_now(&mut a);
    // A deletes; B, offline, reviews five more under the old epoch.
    let epoch = a.profile.reset_game().unwrap();
    for n in 3..8 {
        answer(&mut b, &format!("l{n}"), Grade::Good);
    }
    let reset = standing(&mut a);
    assert_eq!(
        reset,
        (0, 0, (0, 4, 0), Vec::new()),
        "the day of the deletion is no free learning day"
    );
    assert_eq!(due_now(&mut a), due_before, "the review schedule stays");
    assert!(
        app_medals(&a.profile.store)
            .unwrap()
            .contains_key("tour-done"),
        "the tour, the lessons and the tutorial are learning progress: they stay"
    );
    // B's old facts arrive first, then B gets the reset: both end with A's state.
    ship(&b, &mut a);
    assert_eq!(standing(&mut a), reset);
    ship(&a, &mut b);
    assert_eq!(standing(&mut b), reset);
    assert_eq!(b.profile.game_epoch().unwrap(), epoch);
    // B's old keys are empty now, on B and after the next exchange on A.
    exchange(&mut a, &mut b);
    for device in [&a, &b] {
        let old = device
            .profile
            .store
            .kv_list("gamification", "g/0/", 0, 1000)
            .unwrap();
        assert!(!old.entries.is_empty());
        assert!(old.entries.iter().all(|(_, value)| value.is_empty()));
    }
    // B's next answer counts, on both.
    answer(&mut b, "l8", Grade::Good);
    exchange(&mut a, &mut b);
    assert_eq!(status(&mut a).xp, 10);
    assert_eq!(status(&mut b).xp, 10);
}

fn due_now(device: &mut Device) -> usize {
    let now = now();
    device
        .profile
        .schedule(None)
        .unwrap()
        .values()
        .filter(|card| card.due <= now)
        .count()
}

#[test]
fn deletions_on_two_devices_at_once_end_in_one_epoch() {
    let mut a = device();
    let mut b = device();
    let first = a.profile.reset_game().unwrap();
    let second = b.profile.reset_game().unwrap();
    assert_ne!(first.id, second.id);
    exchange(&mut a, &mut b);
    let winner = a.profile.game_epoch().unwrap();
    assert_eq!(b.profile.game_epoch().unwrap(), winner);
    plan(&mut a, 0..2, false);
    answer(&mut a, "l0", Grade::Good);
    exchange(&mut a, &mut b);
    assert_eq!(status(&mut a).xp, 10);
    assert_eq!(status(&mut b).xp, 10);
}

#[test]
fn after_a_deletion_and_erasing_the_cloud_a_new_device_starts_at_zero() {
    let mut a = device();
    plan(&mut a, 0..5, false);
    for n in 0..4 {
        answer(&mut a, &format!("l{n}"), Grade::Good);
    }
    a.profile.reset_game().unwrap();
    // „Delete cloud data“: the device sends everything again, left-out operations excepted.
    let skip = a.profile.store.superseded_after(0).unwrap();
    let ops: Vec<RemoteOp> = a
        .profile
        .store
        .ops_since(0)
        .unwrap()
        .into_iter()
        .filter(|op| !skip.contains(&op.seq))
        .map(|op| RemoteOp {
            hlc: op.hlc,
            entity: op.entity,
            entity_id: op.entity_id,
            op: op.op,
            data: op.data,
        })
        .collect();
    assert!(
        ops.iter()
            .any(|op| op.entity == "kv" && op.entity_id == "host/game-epoch")
    );
    let mut fresh = device();
    fresh.profile.store.apply_remote(&ops).unwrap();
    let status = status(&mut fresh);
    assert_eq!((status.xp, status.learning_days), (0, 0));
}

#[test]
fn the_weekly_goal_and_a_dismissed_hint_reach_the_other_device() {
    let mut a = device();
    let mut b = device();
    a.profile.set_week_goal(2).unwrap();
    a.profile.game_seen("game-intro").unwrap();
    assert!(a.profile.set_week_goal(9).is_err());
    exchange(&mut a, &mut b);
    let status = status(&mut b);
    assert_eq!(status.week.goal, 2);
    assert!(status.intro_seen);
}

#[test]
fn a_finished_journey_earns_the_first_trophy_and_both_devices_see_its_day() {
    let mut a = device();
    let mut b = device();
    a.profile
        .publish(&JourneyStarted {
            palace_ids: vec!["p".into()],
        })
        .unwrap();
    answer(&mut a, "l0", Grade::Good);
    a.profile
        .publish(&JourneyCompleted {
            palace_id: "p".into(),
            correct: 1,
            total: 1,
            duration_secs: 3,
        })
        .unwrap();
    exchange(&mut a, &mut b);
    let (_, day) = a.profile.learning_day().unwrap();
    for device in [&mut a, &mut b] {
        let status = status(device);
        assert_eq!(status.trophies.len(), 1);
        assert_eq!(status.trophies[0].id, "first-journey");
        assert_eq!(status.trophies[0].day.as_deref(), Some(day.as_str()));
    }
}
