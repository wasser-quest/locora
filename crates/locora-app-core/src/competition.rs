//! The memory contest in the app: the one round that runs, how long its phases count, the routes
//! a round can be laid on, and the commands around them. The UI shows the task and collects the
//! recall; the `competition` plugin keeps what was handed in (see `locora_runtime::competition`).
//!
//! A running round lives only here, in memory: nothing half-finished is stored or synced. A daily
//! round wrote its start marker when it began, so a round that is dropped (or an app that was
//! killed) leaves the day's try begun and not finished.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use locora_api::competition::{EveningState, Kind, RECALL_GRACE_MS};
use locora_app_api::{
    CompeteChoice, CompeteEvening, CompeteEveningResult, CompeteEveningStarted, CompeteHandIn,
    CompeteHistoryEntry, CompeteHistoryQuery, CompeteRecall, CompeteRecords, CompeteResult,
    CompeteStart, CompeteStarted, CompeteToday, PlaceLabel, RouteChoice, StartKind,
};
use locora_competition::{
    Discipline, Lang, Level, Spec, Task, cards, code, daily_seed, schedule, words,
};
use locora_runtime::competition::{HandIn, Round, capped};
use locora_runtime::{Profile, built_in_kind};
use locora_store::Store;

/// The round or evening recall that runs, if any.
#[derive(Debug, Default)]
pub struct Contest {
    running: Option<Running>,
}

#[derive(Debug)]
enum Running {
    Round(RoundRun),
    Evening(EveningRun),
}

#[derive(Debug)]
struct RoundRun {
    round: Round,
    untimed: bool,
    route: Option<String>,
    /// When the task was handed out, and when memorizing ended.
    t0: Instant,
    done: Option<Instant>,
}

#[derive(Debug)]
struct EveningRun {
    round_id: String,
    spec: Spec,
    t0: Instant,
}

impl Running {
    /// Whether its time is over at `now`: past both limits and the grace for the hand-in. A
    /// round without a time limit never is.
    fn over(&self, now: Instant) -> bool {
        let ms = |ms: u32| Duration::from_millis(u64::from(ms));
        match self {
            Running::Round(run) if run.untimed => false,
            Running::Round(run) => {
                let spec = &run.round.spec;
                let recall_from = run
                    .done
                    .unwrap_or(run.t0 + ms(spec.memorize_ms))
                    .min(run.t0 + ms(spec.memorize_ms));
                now >= recall_from + ms(spec.recall_ms) + ms(RECALL_GRACE_MS)
            }
            Running::Evening(run) => now >= run.t0 + ms(run.spec.recall_ms) + ms(RECALL_GRACE_MS),
        }
    }
}

impl Contest {
    /// Takes the slot for a new round. One round runs at a time: a round still within its time
    /// must be handed in or dropped first; one whose time is over is dropped.
    fn take_slot(&mut self, now: Instant) -> Result<()> {
        if self
            .running
            .as_ref()
            .is_some_and(|running| !running.over(now))
        {
            bail!("Es läuft schon eine Runde. Gib sie ab oder brich sie ab.");
        }
        self.running = None;
        Ok(())
    }

    fn round(&mut self) -> Result<&mut RoundRun> {
        match self.running.as_mut() {
            Some(Running::Round(run)) => Ok(run),
            _ => bail!("Keine Wettkampfrunde aktiv"),
        }
    }

    /// Whether a round or an evening recall runs.
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }
}

/// The things per place a round is shown with: the given option when the discipline has it,
/// else its first.
fn per_place(discipline: Discipline, wanted: Option<u8>) -> u8 {
    let options = schedule::per_place_options(discipline);
    wanted.filter(|n| options.contains(n)).unwrap_or(options[0])
}

/// The place of each task item: with `per_place` things on a place, item `i` lies on place
/// `i / per_place + 1`, named by the route as far as it reaches.
pub fn place_labels(items: u8, per_place: u8, route: &[String]) -> Vec<PlaceLabel> {
    let per_place = usize::from(per_place.max(1));
    (0..usize::from(items))
        .map(|item| {
            let place = item / per_place;
            PlaceLabel {
                place: place as u16 + 1,
                name: route.get(place).filter(|name| !name.is_empty()).cloned(),
            }
        })
        .collect()
}

/// The person's palaces, as routes for a round that needs `needed` places: built-in palaces
/// (the tutorial, the formula guide) are left out, they belong to other lessons.
pub fn routes(store: &Store, needed: u32) -> Result<Vec<RouteChoice>> {
    Ok(store
        .palaces()?
        .into_iter()
        .filter(|palace| built_in_kind(&palace.id).is_none())
        .map(|palace| RouteChoice {
            fits: palace.loci >= needed,
            places: palace.loci,
            palace_id: palace.id,
            name: palace.name,
        })
        .collect())
}

/// The place names of a palace's route, in order (an unnamed place is empty).
pub fn route_places(store: &Store, palace_id: &str) -> Result<Vec<String>> {
    ensure!(
        built_in_kind(palace_id).is_none(),
        "Die eingebauten Paläste sind keine Route für den Wettkampf"
    );
    let tree = store.palace(palace_id)?;
    ensure!(tree.palace.id == palace_id, "Den Palast gibt es nicht mehr");
    Ok(tree.route().map(|locus| locus.place.clone()).collect())
}

/// The task per place as people read it: things in `lang`, digits, card names.
fn readable(task: &Task, lang: Lang) -> Vec<String> {
    match task {
        Task::List(items) => items
            .iter()
            .map(|id| words::display(*id, lang).unwrap_or_default().to_owned())
            .collect(),
        Task::Numbers(digits) => digits.iter().map(u8::to_string).collect(),
        Task::Cards(deck) => deck.iter().map(|card| cards::name(*card, lang)).collect(),
    }
}

/// `CMD_COMPETE_TODAY`: today's challenge and the places each level needs.
pub fn today(profile: &mut Profile) -> Result<CompeteToday> {
    let today = profile.compete_today()?;
    let routes_needed = today
        .levels
        .iter()
        .map(|level| schedule::prep(&level.spec))
        .collect();
    Ok(CompeteToday {
        today,
        routes_needed,
    })
}

/// `CMD_COMPETE_START`: begins a round at `now`. Practice gets a fresh seed, an earlier day (one
/// of the last seven) that day's task, a code its own; the daily is today's discipline.
pub fn start(
    contest: &mut Contest,
    profile: &mut Profile,
    start: CompeteStart,
    now: Instant,
) -> Result<CompeteStarted> {
    contest.take_slot(now)?;
    let level = start.level.unwrap_or(Level::ALL[0]);
    let today = profile.learning_date()?;
    let (kind, discipline, level, seed) = match start.kind {
        StartKind::Practice => {
            let discipline = start.discipline.context("Welche Disziplin?")?;
            (Kind::Practice, discipline, level, None)
        }
        StartKind::Daily => (Kind::Daily, schedule::daily(today).discipline, level, None),
        StartKind::Earlier => {
            let date = start.date.context("Welcher Tag?")?;
            let back = today.days_since_epoch() - date.days_since_epoch();
            ensure!(
                (1..=7).contains(&back),
                "Als Übung gibt es die Aufgaben der letzten sieben Tage"
            );
            let discipline = schedule::daily(date).discipline;
            let seed = daily_seed(date, discipline, level);
            (Kind::Practice, discipline, level, Some(seed))
        }
        StartKind::Code => {
            let text = start.code.as_deref().unwrap_or_default();
            let (discipline, level, seed) =
                code::decode(text).map_err(|error| anyhow::anyhow!("{error}"))?;
            (Kind::Code, discipline, level, Some(seed))
        }
    };
    let untimed = start.untimed && kind != Kind::Daily;
    let route = match &start.route {
        Some(palace) => route_places(&profile.store, palace)?,
        None => Vec::new(),
    };
    let round = profile.compete_begin(kind, discipline, level, seed)?;
    let per_place = per_place(discipline, start.per_place);
    let started = CompeteStarted {
        round_id: round.id.clone(),
        kind,
        spec: round.spec,
        task: round.task.clone(),
        date: round.date,
        untimed,
        words: match &round.task {
            Task::List(_) => readable(&round.task, start.lang),
            _ => Vec::new(),
        },
        places: place_labels(round.spec.items, per_place, &route),
    };
    contest.running = Some(Running::Round(RoundRun {
        round,
        untimed,
        route: start.route,
        t0: now,
        done: None,
    }));
    Ok(started)
}

/// `CMD_COMPETE_RECALL`: memorizing ends at `now` (it counts up to its limit).
pub fn recall(contest: &mut Contest, now: Instant) -> Result<CompeteRecall> {
    let run = contest.round()?;
    let done = *run.done.get_or_insert(now);
    let (memorize_ms, _) = capped(
        &run.round.spec,
        run.untimed,
        done.saturating_duration_since(run.t0),
        Duration::ZERO,
    );
    let recall_ms_left = (!run.untimed).then(|| {
        let used = now.saturating_duration_since(done).as_millis();
        let left = u128::from(run.round.spec.recall_ms).saturating_sub(used);
        left.min(u128::from(u32::MAX)) as u32
    });
    Ok(CompeteRecall {
        memorize_ms,
        recall_ms_left,
    })
}

/// `CMD_COMPETE_FINISH`: hands the round in at `now`. Memorizing that never ended explicitly
/// ends here; both phases count up to their limits.
pub fn finish(
    contest: &mut Contest,
    profile: &mut Profile,
    hand_in: CompeteHandIn,
    now: Instant,
) -> Result<CompeteResult> {
    let run = contest.round()?;
    let done = run.done.unwrap_or(now);
    let (memorize_ms, recall_ms) = capped(
        &run.round.spec,
        run.untimed,
        done.saturating_duration_since(run.t0),
        now.saturating_duration_since(done),
    );
    let round = run.round.clone();
    let request = HandIn {
        recall: hand_in.recall,
        lang: hand_in.lang,
        untimed: run.untimed,
        route: run.route.clone(),
        memorize_ms,
        recall_ms,
    };
    let finished = profile.compete_finish(&round, request)?;
    contest.running = None;
    Ok(CompeteResult {
        finished,
        correct: readable(&round.task, hand_in.lang),
        memorize_ms,
        recall_ms,
        code: round.code(),
    })
}

/// `CMD_COMPETE_ABANDON`: drops the running round or evening recall. A daily stays begun.
pub fn abandon(contest: &mut Contest) {
    contest.running = None;
}

/// `CMD_COMPETE_EVENING_START`: begins the evening recall of a daily round while it is open.
pub fn evening_start(
    contest: &mut Contest,
    profile: &mut Profile,
    evening: CompeteEvening,
    now: Instant,
) -> Result<CompeteEveningStarted> {
    contest.take_slot(now)?;
    let view = profile.compete_evening(&evening.round_id)?;
    match view.state {
        EveningState::Open => {}
        EveningState::Locked => bail!("Die Abendabfrage öffnet erst sechs Stunden nach der Runde"),
        EveningState::Closed => bail!("Diese Abendabfrage ist geschlossen"),
        EveningState::Done { .. } => bail!("Diese Abendabfrage ist schon abgegeben"),
    }
    let route = match &view.route {
        // The palace may be gone since the morning: the places keep their numbers then.
        Some(palace) => route_places(&profile.store, palace).unwrap_or_default(),
        None => Vec::new(),
    };
    let per_place = per_place(view.spec.discipline, evening.per_place);
    let places = place_labels(view.spec.items, per_place, &route);
    contest.running = Some(Running::Evening(EveningRun {
        round_id: view.round_id.clone(),
        spec: view.spec,
        t0: now,
    }));
    Ok(CompeteEveningStarted {
        evening: view,
        places,
    })
}

/// `CMD_COMPETE_EVENING_FINISH`: hands the evening recall in.
pub fn evening_finish(
    contest: &mut Contest,
    profile: &mut Profile,
    hand_in: CompeteHandIn,
) -> Result<CompeteEveningResult> {
    let Some(Running::Evening(run)) = contest.running.as_ref() else {
        bail!("Keine Abendabfrage aktiv");
    };
    let round_id = run.round_id.clone();
    let finished = profile.compete_evening_finish(&round_id, hand_in.recall, hand_in.lang)?;
    contest.running = None;
    // The morning's task, for the result table.
    let correct = match profile.compete_evening(&round_id) {
        Ok(view) => {
            let seed = daily_seed(view.date, view.spec.discipline, view.spec.level);
            match locora_competition::generate(&view.spec, seed) {
                Ok(task) => readable(&task, hand_in.lang),
                Err(_) => Vec::new(),
            }
        }
        Err(_) => Vec::new(),
    };
    Ok(CompeteEveningResult { finished, correct })
}

/// `CMD_COMPETE_RECORDS`.
pub fn records(profile: &mut Profile) -> Result<CompeteRecords> {
    profile.compete_records()
}

/// `CMD_COMPETE_HISTORY`.
pub fn history(
    profile: &mut Profile,
    query: CompeteHistoryQuery,
) -> Result<Vec<CompeteHistoryEntry>> {
    profile.compete_history(query.limit, query.before_ms, query.before_id)
}

/// `CMD_COMPETE_CODE`: a new challenge code with a fresh seed.
pub fn new_code(choice: CompeteChoice) -> String {
    code::encode(
        choice.discipline,
        choice.level,
        locora_runtime::competition::new_seed(),
    )
}

/// `CMD_COMPETE_ROUTES`: the palaces, and whether their route is long enough for the round.
pub fn route_choices(store: &Store, choice: CompeteChoice) -> Result<Vec<RouteChoice>> {
    let spec = Spec::standard(choice.discipline, choice.level);
    let per_place = per_place(choice.discipline, choice.per_place);
    routes(store, u32::from(schedule::places(spec.items, per_place)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_api::competition::{LevelState, Mark};
    use locora_competition::Recall;
    use locora_store::{NewLocus, RoomKind};

    fn profile() -> (tempfile::TempDir, Profile) {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile::open(Some(dir.path())).unwrap();
        assert!(profile.has_competition(), "the contest is built in");
        (dir, profile)
    }

    fn practice(discipline: Discipline) -> CompeteStart {
        CompeteStart {
            kind: StartKind::Practice,
            discipline: Some(discipline),
            level: None,
            code: None,
            date: None,
            untimed: false,
            route: None,
            per_place: None,
            lang: Lang::De,
        }
    }

    /// Every place right.
    fn answers(task: &Task) -> Recall {
        match task {
            Task::List(items) => Recall::List(
                items
                    .iter()
                    .map(|id| words::display(*id, Lang::De).unwrap().to_owned())
                    .collect(),
            ),
            Task::Numbers(digits) => Recall::Numbers(digits.iter().copied().map(Some).collect()),
            Task::Cards(deck) => Recall::Cards(deck.iter().copied().map(Some).collect()),
        }
    }

    fn hand_in(recall: Recall) -> CompeteHandIn {
        CompeteHandIn {
            recall,
            lang: Lang::De,
        }
    }

    #[test]
    fn memorizing_counts_up_to_its_limit_and_recall_up_to_ten_seconds_more() {
        let (_dir, mut profile) = profile();
        let mut contest = Contest::default();
        let t0 = Instant::now();
        let secs = Duration::from_secs;
        let started = start(&mut contest, &mut profile, practice(Discipline::List), t0).unwrap();
        assert_eq!(
            (started.spec.memorize_ms, started.spec.recall_ms),
            (120_000, 180_000)
        );
        assert_eq!(started.words.len(), 10);
        // Memorizing for 5 minutes counts as the 2 allowed; recall then has its full 3.
        let ended = recall(&mut contest, t0 + secs(300)).unwrap();
        assert_eq!(ended.memorize_ms, 120_000);
        assert_eq!(ended.recall_ms_left, Some(180_000));
        // Recall for 10 minutes counts as 3 minutes and 10 seconds.
        let task = started.task.clone();
        let result = finish(
            &mut contest,
            &mut profile,
            hand_in(answers(&task)),
            t0 + secs(900),
        )
        .unwrap();
        assert_eq!((result.memorize_ms, result.recall_ms), (120_000, 190_000));
        assert!(result.finished.score.perfect);
        assert_eq!(result.correct, started.words);
        assert!(result.code.is_some(), "practice offers its code");
        assert!(!contest.is_running());
        // Within the limits the times are as taken; without a limit they are never cut.
        let mut untimed = practice(Discipline::Numbers);
        untimed.untimed = true;
        let started = start(&mut contest, &mut profile, untimed, t0).unwrap();
        assert!(started.untimed);
        assert_eq!(
            recall(&mut contest, t0 + secs(3_600))
                .unwrap()
                .recall_ms_left,
            None
        );
        let blank = Recall::blank(started.spec.discipline, started.spec.items);
        let result = finish(&mut contest, &mut profile, hand_in(blank), t0 + secs(7_200)).unwrap();
        assert_eq!(
            (result.memorize_ms, result.recall_ms),
            (3_600_000, 3_600_000)
        );
        assert!(
            result
                .finished
                .score
                .marks
                .iter()
                .all(|m| *m == Mark::Empty)
        );
    }

    #[test]
    fn one_round_runs_at_a_time_and_an_abandoned_one_is_gone() {
        let (_dir, mut profile) = profile();
        let mut contest = Contest::default();
        let t0 = Instant::now();
        start(&mut contest, &mut profile, practice(Discipline::Cards), t0).unwrap();
        let second = start(&mut contest, &mut profile, practice(Discipline::List), t0);
        assert!(second.unwrap_err().to_string().contains("schon eine Runde"));
        // Dropped: nothing is stored, and a new round can begin.
        abandon(&mut contest);
        assert!(recall(&mut contest, t0).is_err());
        assert!(profile.compete_history(0, None, None).unwrap().is_empty());
        let started = start(&mut contest, &mut profile, practice(Discipline::List), t0).unwrap();
        // A round whose time is long over gives way to the next.
        let later = t0 + Duration::from_secs(started.spec.memorize_ms as u64 / 1000 + 200);
        start(
            &mut contest,
            &mut profile,
            practice(Discipline::Cards),
            later,
        )
        .unwrap();
        // A dropped daily stays begun: the day's try is taken.
        abandon(&mut contest);
        let mut daily = practice(Discipline::List);
        daily.kind = StartKind::Daily;
        daily.discipline = None;
        let started = start(&mut contest, &mut profile, daily.clone(), t0).unwrap();
        assert_eq!(started.kind, Kind::Daily);
        abandon(&mut contest);
        let today = today(&mut profile).unwrap();
        assert!(matches!(
            today.today.levels[0].state,
            LevelState::Started { .. }
        ));
        assert_eq!(today.routes_needed.len(), 3);
        assert!(
            start(&mut contest, &mut profile, daily, t0).is_err(),
            "one try"
        );
    }

    #[test]
    fn codes_and_earlier_days_play_their_own_tasks() {
        let (_dir, mut profile) = profile();
        let mut contest = Contest::default();
        let t0 = Instant::now();
        let code = new_code(CompeteChoice {
            discipline: Discipline::Cards,
            level: Level::new(2).unwrap(),
            per_place: None,
        });
        let mut by_code = practice(Discipline::List);
        by_code.kind = StartKind::Code;
        by_code.code = Some("C2-28T5-CY4T-QKFF-K".into());
        let started = start(&mut contest, &mut profile, by_code.clone(), t0).unwrap();
        assert_eq!((started.kind, started.spec.items), (Kind::Code, 26));
        let Task::Cards(deck) = &started.task else {
            panic!("{:?}", started.task);
        };
        assert_eq!(deck[..3], [36, 18, 7], "the golden deck");
        abandon(&mut contest);
        by_code.code = Some(code.replace('-', "").to_lowercase());
        assert_eq!(
            start(&mut contest, &mut profile, by_code.clone(), t0)
                .unwrap()
                .spec
                .items,
            26
        );
        abandon(&mut contest);
        by_code.code = Some("C2-28T5-CY4T-QKFF-X".into());
        let error = start(&mut contest, &mut profile, by_code, t0).unwrap_err();
        assert_eq!(error.to_string(), "Dieser Code ist ungültig");
        // Yesterday's daily, as practice; a week and a day ago is too long.
        let today = profile.learning_date().unwrap();
        let mut earlier = practice(Discipline::List);
        earlier.kind = StartKind::Earlier;
        earlier.discipline = None;
        earlier.date = Some(today.add_days(-1));
        let started = start(&mut contest, &mut profile, earlier.clone(), t0).unwrap();
        assert_eq!(started.kind, Kind::Practice);
        assert_eq!(
            started.spec.discipline,
            schedule::daily(today.add_days(-1)).discipline
        );
        abandon(&mut contest);
        earlier.date = Some(today.add_days(-8));
        assert!(start(&mut contest, &mut profile, earlier.clone(), t0).is_err());
        earlier.date = Some(today);
        assert!(
            start(&mut contest, &mut profile, earlier, t0).is_err(),
            "today is the daily"
        );
    }

    fn palace(store: &Store, name: &str, places: usize) -> String {
        let palace = store.create_palace(name).unwrap();
        let room = store
            .add_room(&palace.id, "Wohnung", RoomKind::Blank, None)
            .unwrap();
        for n in 0..places {
            let locus = NewLocus {
                audio: None,
                place: format!("Ort {}", n + 1),
                item: String::new(),
                x: 0.5,
                y: 0.5,
                time: None,
                answer: Default::default(),
                image: None,
                note: String::new(),
                image_size: 0.25,
                zoom: 1.0,
            };
            store.add_locus(&room.id, locus).unwrap();
        }
        palace.id
    }

    #[test]
    fn a_route_fits_when_it_has_the_places_a_round_needs() {
        let (_dir, mut profile) = profile();
        let home = palace(&profile.store, "Wohnung", 13);
        let office = palace(&profile.store, "Büro", 12);
        locora_runtime::tutorial::ensure_in(&mut profile, "de").unwrap();
        let cards = |level: u8, per_place| CompeteChoice {
            discipline: Discipline::Cards,
            level: Level::new(level).unwrap(),
            per_place,
        };
        let fits = |choice| {
            let mut routes = route_choices(&profile.store, choice).unwrap();
            routes.sort_by(|a, b| a.name.cmp(&b.name));
            routes
                .into_iter()
                .map(|r| (r.name, r.places, r.fits))
                .collect::<Vec<_>>()
        };
        // 13 cards, one per place: 13 places. The tutorial palace is no route.
        assert_eq!(
            fits(cards(1, None)),
            [("Büro".into(), 12, false), ("Wohnung".into(), 13, true)]
        );
        // 26 cards, two per place: 13 places.
        assert!(fits(cards(2, Some(2)))[1].2);
        assert!(!fits(cards(2, None))[1].2);
        let walk = route_places(&profile.store, &home).unwrap();
        assert_eq!(walk.len(), 13);
        assert_eq!(walk[0], "Ort 1");
        assert!(route_places(&profile.store, &office).is_ok());
        let tutorial = profile.store.palaces().unwrap();
        let built_in = tutorial
            .iter()
            .find(|p| built_in_kind(&p.id).is_some())
            .unwrap();
        assert!(route_places(&profile.store, &built_in.id).is_err());
    }

    #[test]
    fn place_labels_follow_the_things_per_place_and_the_route() {
        let route: Vec<String> = ["Tür", "Sofa", ""].map(String::from).to_vec();
        let labels = |items, per_place| {
            place_labels(items, per_place, &route)
                .into_iter()
                .map(|label| (label.place, label.name))
                .collect::<Vec<_>>()
        };
        let named = |n: &str| Some(n.to_owned());
        assert_eq!(
            labels(4, 1),
            [(1, named("Tür")), (2, named("Sofa")), (3, None), (4, None)],
            "an unnamed place and one beyond the route have only their number"
        );
        assert_eq!(
            labels(5, 2),
            [
                (1, named("Tür")),
                (1, named("Tür")),
                (2, named("Sofa")),
                (2, named("Sofa")),
                (3, None)
            ]
        );
        assert_eq!(
            labels(6, 3).iter().map(|l| l.0).collect::<Vec<_>>(),
            [1, 1, 1, 2, 2, 2]
        );
        // Numbers: 2, 3, 4 or 6 digits per place; an option the discipline lacks is its first.
        assert_eq!(per_place(Discipline::Numbers, Some(4)), 4);
        assert_eq!(per_place(Discipline::Numbers, Some(1)), 2);
        assert_eq!(per_place(Discipline::List, None), 1);
        let digits = place_labels(20, per_place(Discipline::Numbers, Some(6)), &[]);
        assert_eq!(digits.last().unwrap().place, 4);
        // A round started on a route labels its places with the route's names.
        let (_dir, mut profile) = profile();
        let id = palace(&profile.store, "Wohnung", 6);
        let mut contest = Contest::default();
        let mut start_on_route = practice(Discipline::Cards);
        start_on_route.route = Some(id);
        start_on_route.per_place = Some(3);
        let started = start(&mut contest, &mut profile, start_on_route, Instant::now()).unwrap();
        assert_eq!(started.places.len(), 13);
        assert_eq!(started.places[12].place, 5);
        assert_eq!(started.places[12].name.as_deref(), Some("Ort 5"));
    }
}
