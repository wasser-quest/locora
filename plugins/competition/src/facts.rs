//! The contest's stored facts and everything that follows from them: today's challenge, records,
//! counts, trophies and the history. Pure: no storage, no clock (the time comes with each request).
//!
//! Keys (in the plugin's namespace, below the game epoch the runtime keeps):
//! - `g/<epoch>/r/<round_id>`: [`RoundFact`], written when a round is handed in
//! - `g/<epoch>/s/<date>/<discipline>/<level>/<round_id>`: [`MarkerFact`], written when a daily
//!   round begins. The key alone holds the level: a marker of a kind this version does not know
//!   (Locora Cloud's daily writes `cloud_daily` under the same keys), or one whose value does not
//!   read, is the day's try all the same.
//! - `g/<epoch>/t/<discipline>/<level>/<seed>/<round_id>`: [`SightingFact`], written when any
//!   other round begins: its task was shown, also if the round is never handed in
//! - `g/<epoch>/e/<round_id>/<evening_id>`: [`EveningFact`], written when an evening recall is
//!   handed in
//!
//! Every key is written once and never changed, so the sync (last writer wins per key) merges the
//! devices as a union. Values are JSON with a version `v`, so an older version reads the facts of
//! a newer one and leaves out the fields it does not know. Only facts of the current epoch count;
//! empty values are keys that were blanked. A round or an evening recall from the sync counts only
//! as this build scores it again from its seed, never with the points stored (see [`collect`]).
//!
//! Two devices may play the same daily offline: the round with the earliest start marker is the
//! day's try, the other counts as practice. A task counts for records only the first time any
//! device saw it (a round, a start marker or a sighting with its seed). Whether a task was new is
//! decided on the device when the round begins and stored with it (`first`), so a clock set back
//! never makes a replay the first play; the earliest start only settles tasks that two devices
//! saw first at the same time, offline. Rounds without a time limit never count for records or
//! trophies. A round dated after the learning date of a request (from a device whose clock ran
//! ahead) counts for records, trophies and the days with a daily only from its date on.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use locora_competition::{daily_seed, generate, schedule, score};
use locora_sdk::api::competition::{
    Best, Counted, DayPlan, EveningScore, EveningState, EveningToday, HISTORY_LIMIT, Hint,
    HistoryEntry, Kind, LevelRecord, LevelState, LevelToday, RECALL_GRACE_MS, RecordsResponse,
    TROPHIES, TodayResponse, Trophy,
};
use locora_sdk::api::competition::{
    Date, Discipline, EVENING_CLOSES_MS, EVENING_OPENS_MS, Lang, Level, Mark, Recall, Score, Seed,
    Spec, Task,
};
use serde::{Deserialize, Serialize};

/// The version of the facts this plugin writes.
pub const VERSION: u16 = 1;
/// The epoch before any reset.
pub const FIRST_EPOCH: &str = "0";

pub fn round_key(epoch: &str, round_id: &str) -> String {
    format!("g/{epoch}/r/{round_id}")
}

pub fn marker_key(
    epoch: &str,
    date: Date,
    discipline: Discipline,
    level: Level,
    round_id: &str,
) -> String {
    format!(
        "g/{epoch}/s/{date}/{}/{}/{round_id}",
        discipline.as_str(),
        level.get()
    )
}

pub fn sighting_key(
    epoch: &str,
    discipline: Discipline,
    level: Level,
    seed: Seed,
    round_id: &str,
) -> String {
    format!(
        "g/{epoch}/t/{}/{}/{seed}/{round_id}",
        discipline.as_str(),
        level.get()
    )
}

pub fn evening_key(epoch: &str, round_id: &str, evening_id: &str) -> String {
    format!("g/{epoch}/e/{round_id}/{evening_id}")
}

/// A round as it was handed in. Marks are not stored: they follow from the task, which the seed
/// gives again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundFact {
    pub v: u16,
    pub id: String,
    pub kind: Kind,
    pub discipline: Discipline,
    pub level: Level,
    pub rules: u16,
    pub content: u16,
    pub seed: Seed,
    /// The learning date the round began on.
    pub date: Date,
    pub lang: Lang,
    pub untimed: bool,
    /// The palace whose route the round was laid on.
    #[serde(default)]
    pub route: Option<String>,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    pub memorize_ms: u32,
    pub recall_ms: u32,
    pub recall: Recall,
    pub points: u16,
    pub max: u16,
    pub perfect: bool,
    /// Whether the task was new to the device when the round began, as its start marker or
    /// sighting says; `None` when no fact says.
    #[serde(default)]
    pub first: Option<bool>,
}

impl RoundFact {
    /// Whether a device of this version can have stored the round: a spec this build serves,
    /// times within the limits, and for a daily the date's discipline and task. Its points come
    /// from scoring the recall again ([`collect`]), never from the fact.
    fn plausible(&self) -> bool {
        let spec = Spec::standard(self.discipline, self.level);
        if (self.rules, self.content) != (spec.rules, spec.content) {
            return false;
        }
        let within = self.memorize_ms <= spec.memorize_ms
            && self.recall_ms <= spec.recall_ms.saturating_add(RECALL_GRACE_MS);
        if !self.untimed && !within {
            return false;
        }
        let daily = schedule::daily(self.date).discipline == self.discipline
            && self.seed == daily_seed(self.date, self.discipline, self.level);
        self.kind != Kind::Daily || (!self.untimed && daily)
    }
}

/// The kind of the start markers this version writes.
pub const DAILY: &str = "daily";

/// A daily round began (the date, discipline, level and round id are in the key). The kind is
/// text, so a marker of a newer kind still reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkerFact {
    pub v: u16,
    pub kind: String,
    pub started_at_ms: u64,
    /// Whether the task was new to this device when the round began.
    #[serde(default)]
    pub first: Option<bool>,
}

/// A round other than a daily began (the discipline, level, seed and round id are in the key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SightingFact {
    pub v: u16,
    pub started_at_ms: u64,
    /// Whether the task was new to this device when the round began.
    #[serde(default)]
    pub first: Option<bool>,
}

/// An evening recall as it was handed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EveningFact {
    pub v: u16,
    pub round_id: String,
    pub id: String,
    /// The learning date of the evening recall.
    pub date: Date,
    pub finished_at_ms: u64,
    pub recall: Recall,
    pub lang: Lang,
    pub points: u16,
    pub max: u16,
}

impl EveningFact {
    /// Whether this can be the evening recall of `morning`: a daily round, handed in within
    /// its window. Its points come from scoring the recall again against the morning's task.
    fn plausible(&self, morning: &RoundFact) -> bool {
        let (opens, closes) = evening_window(morning);
        morning.kind == Kind::Daily && (opens..closes).contains(&self.finished_at_ms)
    }
}

/// Points, the most points and whether all are right: a recall scored again.
type Points = (u16, u16, bool);

/// Scores `recall` against the task of `spec` and `seed`; `None` when it does not fit.
fn rescore(spec: &Spec, seed: Seed, recall: &Recall, lang: Lang) -> Option<Points> {
    let task = generate(spec, seed).ok()?;
    let score = score(&task, recall, lang).ok()?;
    Some((score.points, score.max, score.perfect))
}

/// The scores of stored recalls, by key and value: every key is written once, so a plugin
/// instance scores a round or an evening recall once, not each time the facts are read again.
#[derive(Debug, Default)]
pub struct Rescored(BTreeMap<String, (u64, Option<Points>)>);

impl Rescored {
    /// The points of the recall stored under `key`, from `score` unless this value was scored.
    fn get(
        &mut self,
        key: &str,
        value: &[u8],
        score: impl FnOnce() -> Option<Points>,
    ) -> Option<Points> {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        let hash = hasher.finish();
        if let Some((scored, points)) = self.0.get(key)
            && *scored == hash
        {
            return *points;
        }
        let points = score();
        self.0.insert(key.to_owned(), (hash, points));
        points
    }
}

/// A start marker with what its key says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub date: Date,
    pub discipline: Discipline,
    pub level: Level,
    pub round_id: String,
    /// `None` when the value does not read: the marker still holds its level.
    pub fact: Option<MarkerFact>,
}

impl Marker {
    /// When its round began; `None` when the marker does not say.
    pub fn started_at_ms(&self) -> Option<u64> {
        self.fact.as_ref().map(|fact| fact.started_at_ms)
    }

    /// The order of the tries of one daily: the earliest start first, a marker that does not
    /// say when last.
    fn order(&self) -> (u64, &str) {
        (
            self.started_at_ms().unwrap_or(u64::MAX),
            self.round_id.as_str(),
        )
    }

    /// A daily round of this version: its task is the date's ([`daily_seed`]).
    fn local_daily(&self) -> bool {
        self.fact.as_ref().is_some_and(|fact| fact.kind == DAILY)
    }
}

/// A sighting with what its key says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    pub discipline: Discipline,
    pub level: Level,
    pub seed: Seed,
    pub round_id: String,
    pub fact: SightingFact,
}

/// A task: its discipline, level and seed.
pub type TaskKey = (Discipline, Level, Seed);

/// The readable facts of one epoch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// By round id.
    pub rounds: BTreeMap<String, RoundFact>,
    pub markers: Vec<Marker>,
    pub sightings: Vec<Sighting>,
    /// By the id of the morning round.
    pub evenings: BTreeMap<String, Vec<EveningFact>>,
}

/// Reads the facts of `epoch` from the stored keys. Blanked keys, keys of other epochs and
/// values that do not read (a kind or a level of a newer version) are left out, except start
/// markers: every one under a valid key counts. Rounds and evening recalls count as scored again
/// here (once per value, see [`Rescored`]); those that cannot be scored are left out too.
pub fn collect<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    epoch: &str,
    rescored: &mut Rescored,
) -> Facts {
    let mut facts = Facts::default();
    let mut evenings = Vec::new();
    let prefix = format!("g/{epoch}/");
    rescored.0.retain(|key, _| key.starts_with(&prefix));
    for (key, value) in entries {
        let Some(rest) = key.strip_prefix(&prefix) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let parts: Vec<&str> = rest.split('/').collect();
        match parts[..] {
            ["r", id] => {
                if let Ok(mut round) = serde_json::from_slice::<RoundFact>(value)
                    && round.id == id
                    && round.plausible()
                    && let Some(points) = rescored.get(key, value, || {
                        let spec = Spec::standard(round.discipline, round.level);
                        rescore(&spec, round.seed, &round.recall, round.lang)
                    })
                {
                    (round.points, round.max, round.perfect) = points;
                    facts.rounds.insert(round.id.clone(), round);
                }
            }
            ["s", date, discipline, level, id] => {
                let parsed = (
                    Date::parse(date),
                    Discipline::parse(discipline),
                    level.parse().ok().and_then(Level::new),
                );
                if let (Some(date), Some(discipline), Some(level)) = parsed {
                    facts.markers.push(Marker {
                        date,
                        discipline,
                        level,
                        round_id: id.to_owned(),
                        fact: serde_json::from_slice(value).ok(),
                    });
                }
            }
            ["t", discipline, level, seed, id] => {
                let parsed = (
                    Discipline::parse(discipline),
                    level.parse().ok().and_then(Level::new),
                    Seed::from_hex(seed),
                    serde_json::from_slice::<SightingFact>(value),
                );
                if let (Some(discipline), Some(level), Some(seed), Ok(fact)) = parsed {
                    facts.sightings.push(Sighting {
                        discipline,
                        level,
                        seed,
                        round_id: id.to_owned(),
                        fact,
                    });
                }
            }
            ["e", round_id, id] => {
                if let Ok(evening) = serde_json::from_slice::<EveningFact>(value)
                    && evening.round_id == round_id
                    && evening.id == id
                {
                    evenings.push((key, value, evening));
                }
            }
            _ => {}
        }
    }
    // An evening recall counts once its round is here, scored against the morning's task.
    for (key, value, mut evening) in evenings {
        if let Some(morning) = facts.rounds.get(&evening.round_id)
            && evening.plausible(morning)
            && let Some((points, max, _)) = rescored.get(key, value, || {
                let spec = Spec::standard(morning.discipline, morning.level);
                rescore(&spec, morning.seed, &evening.recall, evening.lang)
            })
        {
            (evening.points, evening.max) = (points, max);
            facts
                .evenings
                .entry(evening.round_id.clone())
                .or_default()
                .push(evening);
        }
    }
    facts
}

/// When the evening recall of a round opens and closes.
pub fn evening_window(round: &RoundFact) -> (u64, u64) {
    (
        round.finished_at_ms.saturating_add(EVENING_OPENS_MS),
        round.finished_at_ms.saturating_add(EVENING_CLOSES_MS),
    )
}

/// The plan of a day: its discipline, and whether it is a long day.
pub fn plan(date: Date) -> DayPlan {
    let daily = schedule::daily(date);
    DayPlan {
        date,
        discipline: daily.discipline,
        long_day: daily.long_day,
    }
}

/// The one hint a result gives, in this order: two or more neighbouring places missed (walk the
/// route again), two places that hold each other's card or digit (make them more different), any
/// place missed (bigger pictures). A tolerated typo is no miss.
pub fn hint(task: &Task, recall: &Recall, score: &Score) -> Option<Hint> {
    let missed = |mark: &Mark| !matches!(mark, Mark::Right | Mark::Tolerated);
    if score.marks.windows(2).any(|pair| pair.iter().all(missed)) {
        return Some(Hint::Route);
    }
    let swapped = match (task, recall) {
        (Task::Cards(task), Recall::Cards(recall))
        | (Task::Numbers(task), Recall::Numbers(recall)) => swapped(task, recall),
        _ => false,
    };
    if swapped {
        return Some(Hint::Loci);
    }
    score.marks.iter().any(missed).then_some(Hint::Exaggerate)
}

/// Whether two places hold each other's item.
fn swapped(task: &[u8], recall: &[Option<u8>]) -> bool {
    (0..task.len()).any(|i| {
        (i + 1..task.len()).any(|j| {
            task[i] != task[j]
                && recall.get(i).copied().flatten() == Some(task[j])
                && recall.get(j).copied().flatten() == Some(task[i])
        })
    })
}

/// Whether a round is the first play of its task.
fn first_play(first: &BTreeMap<TaskKey, Seen<'_>>, round: &RoundFact) -> bool {
    first
        .get(&(round.discipline, round.level, round.seed))
        .is_some_and(|(_, id)| *id == round.id)
}

/// Rounds in the order they were handed in.
fn by_finish(round: &RoundFact) -> (u64, &str) {
    (round.finished_at_ms, round.id.as_str())
}

/// The order in which tasks were seen: when a round began, then its id.
type Seen<'a> = (u64, &'a str);

impl Facts {
    /// The day's try of a daily challenge: its earliest start marker, of any kind. Once there
    /// is one, the level cannot be begun again.
    pub fn daily_try(&self, date: Date, discipline: Discipline, level: Level) -> Option<&Marker> {
        self.markers
            .iter()
            .filter(|m| m.date == date && m.discipline == discipline && m.level == level)
            .min_by_key(|m| m.order())
    }

    /// A daily round that is not the day's try: another round began the same daily earlier.
    pub fn counted_as_practice(&self, round: &RoundFact) -> bool {
        round.kind == Kind::Daily
            && self
                .daily_try(round.date, round.discipline, round.level)
                .is_some_and(|first| first.round_id != round.id)
    }

    /// Every time a task was shown: a round handed in, the start marker of a daily, the sighting
    /// of another round. With when and for which round, and whether the task was new then.
    fn shown(&self) -> impl Iterator<Item = (TaskKey, Seen<'_>, Option<bool>)> {
        let rounds = self.rounds.values().map(|round| {
            (
                (round.discipline, round.level, round.seed),
                (round.started_at_ms, round.id.as_str()),
                round.first,
            )
        });
        // A marker of another kind (Locora Cloud's daily) had another task.
        let markers = self.markers.iter().filter(|m| m.local_daily()).map(|m| {
            (
                (
                    m.discipline,
                    m.level,
                    daily_seed(m.date, m.discipline, m.level),
                ),
                m.order(),
                m.fact.as_ref().and_then(|fact| fact.first),
            )
        });
        let sightings = self.sightings.iter().map(|s| {
            (
                (s.discipline, s.level, s.seed),
                (s.fact.started_at_ms, s.round_id.as_str()),
                s.fact.first,
            )
        });
        rounds.chain(markers).chain(sightings)
    }

    /// Whether the task was shown for any round but `round_id`, here or on another device.
    pub fn seen(&self, task: TaskKey, round_id: &str) -> bool {
        self.shown()
            .any(|(shown, (_, id), _)| shown == task && id != round_id)
    }

    /// Whether the task was new when round `round_id` began, as its start marker or sighting
    /// says.
    pub fn first_when_shown(&self, task: TaskKey, round_id: &str) -> Option<bool> {
        self.shown()
            .find(|(shown, (_, id), _)| *shown == task && *id == round_id)
            .and_then(|(_, _, first)| first)
    }

    /// The first play of every task on any device: of the rounds, markers and sightings whose
    /// device had not seen the task, the earliest. One that had seen it is a replay, whatever
    /// its clock says.
    fn first_plays(&self) -> BTreeMap<TaskKey, Seen<'_>> {
        let mut first: BTreeMap<TaskKey, Seen<'_>> = BTreeMap::new();
        for (task, seen, new) in self.shown() {
            if new == Some(false) {
                continue;
            }
            let slot = first.entry(task).or_insert(seen);
            if seen < *slot {
                *slot = seen;
            }
        }
        first
    }

    /// Whether the round's task was seen before it, here or on another device.
    pub fn repeat_seed(&self, round: &RoundFact) -> bool {
        !first_play(&self.first_plays(), round)
    }

    /// The rounds of learning dates up to `date`: a round from a device whose clock ran ahead
    /// waits for its date.
    fn rounds_until(&self, date: Date) -> impl Iterator<Item = &RoundFact> {
        self.rounds.values().filter(move |round| round.date <= date)
    }

    /// Rounds that count for records on `date`: with the time limit, and the first play of
    /// their task.
    fn record_rounds(&self, date: Date) -> Vec<&RoundFact> {
        let first = self.first_plays();
        self.rounds_until(date)
            .filter(|round| !round.untimed && first_play(&first, round))
            .collect()
    }

    /// The record of a discipline and level on `date`: most points, then the shortest
    /// memorizing time; at equal results the earlier round keeps it.
    pub fn best(&self, discipline: Discipline, level: Level, date: Date) -> Option<Best> {
        self.record_rounds(date)
            .into_iter()
            .filter(|round| round.discipline == discipline && round.level == level)
            .min_by_key(|round| {
                (
                    locora_competition::rank_key(round.points, round.memorize_ms),
                    by_finish(round),
                )
            })
            .map(|round| Best {
                round_id: round.id.clone(),
                points: round.points,
                max: round.max,
                memorize_ms: round.memorize_ms,
                date: round.date,
            })
    }

    /// Records, counts and trophies on the learning date `date`.
    pub fn records(&self, date: Date) -> RecordsResponse {
        let mut levels = Vec::new();
        for discipline in Discipline::ALL {
            for level in Level::ALL {
                levels.push(LevelRecord {
                    discipline,
                    level,
                    best: self.best(discipline, level, date),
                    rounds: self
                        .rounds_until(date)
                        .filter(|r| r.discipline == discipline && r.level == level)
                        .count() as u32,
                });
            }
        }
        let earned = self.trophies(date);
        RecordsResponse {
            levels,
            daily_days: self.daily_dates(date).len() as u32,
            trophies: TROPHIES
                .iter()
                .map(|id| Trophy {
                    id: (*id).to_owned(),
                    unlocked: earned.get(id).copied(),
                })
                .collect(),
        }
    }

    /// Learning dates up to `date` with a daily round handed in.
    fn daily_dates(&self, date: Date) -> BTreeSet<Date> {
        self.rounds_until(date)
            .filter(|round| round.kind == Kind::Daily)
            .map(|round| round.date)
            .collect()
    }

    /// The evening recall that counts for a round: the first handed in.
    pub fn evening(&self, round_id: &str) -> Option<&EveningFact> {
        self.evenings
            .get(round_id)?
            .iter()
            .min_by_key(|e| (e.finished_at_ms, e.id.as_str()))
    }

    pub fn evening_state(&self, round: &RoundFact, now_ms: u64) -> EveningState {
        let (opens, closes) = evening_window(round);
        match self.evening(&round.id) {
            Some(evening) => EveningState::Done {
                points: evening.points,
                max: evening.max,
            },
            None if now_ms < opens => EveningState::Locked,
            None if now_ms < closes => EveningState::Open,
            None => EveningState::Closed,
        }
    }

    /// Trophies earned up to the learning date `date`, and the date each was earned on. Only
    /// rounds with the time limit count, in the order they were handed in.
    pub fn trophies(&self, date: Date) -> BTreeMap<&'static str, Date> {
        let mut rounds: Vec<&RoundFact> = self.rounds_until(date).filter(|r| !r.untimed).collect();
        rounds.sort_by_key(|round| by_finish(round));
        let mut earned = BTreeMap::new();
        let first = |earned: &mut BTreeMap<&'static str, Date>, id: &'static str, date: Date| {
            earned.entry(id).or_insert(date);
        };
        let mut dates = BTreeSet::new();
        // Per discipline: the position of its first perfect round.
        let mut perfect: BTreeMap<Discipline, usize> = BTreeMap::new();
        for (n, round) in rounds.iter().enumerate() {
            first(&mut earned, "compete-first", round.date);
            if round.kind == Kind::Daily && dates.insert(round.date) {
                match dates.len() {
                    7 => first(&mut earned, "compete-daily-7", round.date),
                    30 => first(&mut earned, "compete-daily-30", round.date),
                    _ => {}
                }
            }
            if !round.perfect {
                continue;
            }
            perfect.entry(round.discipline).or_insert(n);
            let level = round.level.get();
            match round.discipline {
                Discipline::List if level == 3 => first(&mut earned, "compete-list-30", round.date),
                Discipline::Numbers if level >= 2 => {
                    first(&mut earned, "compete-numbers-40", round.date)
                }
                Discipline::Cards if level == 3 => first(&mut earned, "compete-deck", round.date),
                _ => {}
            }
        }
        if perfect.len() == Discipline::ALL.len()
            && let Some(last) = perfect.values().max()
        {
            first(&mut earned, "compete-triple", rounds[*last].date);
        }
        let kept = self
            .evenings
            .values()
            .flatten()
            .filter(|e| e.max >= 10 && e.points == e.max && e.date <= date)
            .filter(|e| self.rounds.get(&e.round_id).is_some_and(|r| !r.untimed))
            .min_by_key(|e| (e.finished_at_ms, e.id.as_str()));
        if let Some(evening) = kept {
            first(&mut earned, "compete-evening", evening.date);
        }
        earned
    }

    /// Rounds handed in before `before_ms` (all without it), newest first; at most `limit`
    /// ([`HISTORY_LIMIT`] for 0 or more). With `before_id` too, the rounds before that one in
    /// the order of the history, so a page goes on after its last entry also when other rounds
    /// were handed in in the same millisecond.
    pub fn history(
        &self,
        limit: u32,
        before_ms: Option<u64>,
        before_id: Option<&str>,
    ) -> Vec<HistoryEntry> {
        let limit = match limit {
            0 => HISTORY_LIMIT,
            n => n.min(HISTORY_LIMIT),
        };
        let before = |round: &RoundFact| match (before_ms, before_id) {
            (None, _) => true,
            (Some(ms), None) => round.finished_at_ms < ms,
            (Some(ms), Some(id)) => by_finish(round) < (ms, id),
        };
        let mut rounds: Vec<&RoundFact> = self.rounds.values().filter(|r| before(r)).collect();
        rounds.sort_by_key(|round| std::cmp::Reverse(by_finish(round)));
        rounds
            .into_iter()
            .take(limit as usize)
            .map(|round| HistoryEntry {
                round_id: round.id.clone(),
                kind: round.kind,
                discipline: round.discipline,
                level: round.level,
                date: round.date,
                points: round.points,
                max: round.max,
                memorize_ms: round.memorize_ms,
                untimed: round.untimed,
                finished_at_ms: round.finished_at_ms,
                counted_as_practice: self.counted_as_practice(round),
                evening: self.evening(&round.id).map(|e| EveningScore {
                    points: e.points,
                    max: e.max,
                }),
            })
            .collect()
    }

    /// How one level of a daily challenge stands at `now_ms`.
    fn level_state(
        &self,
        date: Date,
        discipline: Discipline,
        level: Level,
        now_ms: u64,
    ) -> LevelState {
        let Some(first) = self.daily_try(date, discipline, level) else {
            return LevelState::Open;
        };
        let done = |round: &RoundFact, counted| LevelState::Done {
            round_id: round.id.clone(),
            points: round.points,
            max: round.max,
            memorize_ms: round.memorize_ms,
            counted,
        };
        if let Some(round) = self.rounds.get(&first.round_id) {
            return done(round, Counted::Daily);
        }
        let spec = Spec::standard(discipline, level);
        let ends = first.started_at_ms().map(|started| {
            started
                .saturating_add(u64::from(spec.memorize_ms) + u64::from(spec.recall_ms))
                .saturating_add(u64::from(RECALL_GRACE_MS))
        });
        if ends.is_some_and(|ends| now_ms < ends) {
            return LevelState::Started {
                round_id: first.round_id.clone(),
            };
        }
        // The day's try was never handed in; a later round of the same daily may have been.
        let other = self
            .rounds
            .values()
            .filter(|r| {
                r.kind == Kind::Daily
                    && r.date == date
                    && r.discipline == discipline
                    && r.level == level
            })
            .min_by_key(|round| by_finish(round));
        match other {
            Some(round) => done(round, Counted::Practice),
            None => LevelState::Unfinished {
                round_id: first.round_id.clone(),
            },
        }
    }

    /// Today's challenge on the learning date `date`.
    pub fn today(&self, date: Date, now_ms: u64) -> TodayResponse {
        let today = plan(date);
        let levels = Level::ALL
            .into_iter()
            .map(|level| LevelToday {
                spec: Spec::standard(today.discipline, level),
                state: self.level_state(date, today.discipline, level, now_ms),
            })
            .collect();
        let mut dailies: Vec<&RoundFact> = self
            .rounds
            .values()
            .filter(|r| r.kind == Kind::Daily)
            // Today's, and those handed in before now whose recall is not closed yet: a round
            // from a device whose clock ran ahead is no evening recall of today.
            .filter(|r| {
                r.date == date || (r.finished_at_ms <= now_ms && now_ms < evening_window(r).1)
            })
            .collect();
        dailies.sort_by_key(|round| by_finish(round));
        let evenings = dailies
            .into_iter()
            .map(|round| {
                let (opens_at_ms, closes_at_ms) = evening_window(round);
                EveningToday {
                    round_id: round.id.clone(),
                    discipline: round.discipline,
                    level: round.level,
                    items: round.max.min(u16::from(u8::MAX)) as u8,
                    opens_at_ms,
                    closes_at_ms,
                    state: self.evening_state(round, now_ms),
                }
            })
            .collect();
        TodayResponse {
            today,
            levels,
            evenings,
            tomorrow: plan(date.add_days(1)),
            earlier: (1..=7).map(|n| plan(date.add_days(-n))).collect(),
        }
    }
}

/// The home tile: an open evening recall first, then today's best level handed in, or that
/// today's challenge waits. Templates the app translates (see `localized`).
pub fn tile(today: &TodayResponse) -> (&'static str, Vec<String>) {
    if today.evenings.iter().any(|e| e.state == EveningState::Open) {
        return ("Die Abendabfrage ist offen", Vec::new());
    }
    let played = today
        .levels
        .iter()
        .rev()
        .find_map(|level| match level.state {
            LevelState::Done { points, max, .. } => Some((points, max)),
            _ => None,
        });
    let discipline = today.today.discipline;
    match played {
        Some((points, max)) => (
            match discipline {
                Discipline::List => "Heute: Einkaufsliste – {0}/{1}",
                Discipline::Numbers => "Heute: Zahlen – {0}/{1}",
                Discipline::Cards => "Heute: Karten – {0}/{1}",
            },
            vec![points.to_string(), max.to_string()],
        ),
        None => (
            match discipline {
                Discipline::List => "Heute: Einkaufsliste – noch nicht gespielt",
                Discipline::Numbers => "Heute: Zahlen – noch nicht gespielt",
                Discipline::Cards => "Heute: Karten – noch nicht gespielt",
            },
            Vec::new(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: u64 = 3_600_000;

    fn date(text: &str) -> Date {
        Date::parse(text).unwrap()
    }

    fn level(n: u8) -> Level {
        Level::new(n).unwrap()
    }

    /// A learning date after every round of these tests.
    fn later() -> Date {
        date("2027-12-31")
    }

    /// A round handed in: `points` of the spec's items, `started` and `finished` in hours.
    fn round(id: &str, kind: Kind, discipline: Discipline, n: u8, points: u16) -> RoundFact {
        let spec = Spec::standard(discipline, level(n));
        let max = u16::from(spec.items);
        RoundFact {
            v: VERSION,
            id: id.into(),
            kind,
            discipline,
            level: level(n),
            rules: spec.rules,
            content: spec.content,
            seed: Seed::new(0x123).unwrap(),
            date: date("2026-10-28"),
            lang: Lang::De,
            untimed: false,
            route: None,
            started_at_ms: H,
            finished_at_ms: 2 * H,
            memorize_ms: 60_000,
            recall_ms: 60_000,
            recall: Recall::blank(discipline, spec.items),
            points,
            max,
            perfect: points == max,
            first: Some(true),
        }
    }

    /// The recall that gets every place of the round's task right.
    fn all_right(round: &RoundFact) -> Recall {
        let spec = Spec::standard(round.discipline, round.level);
        match generate(&spec, round.seed).unwrap() {
            Task::List(items) => Recall::List(
                items
                    .iter()
                    .map(|id| {
                        locora_competition::words::display(*id, round.lang)
                            .unwrap()
                            .into()
                    })
                    .collect(),
            ),
            Task::Numbers(digits) => Recall::Numbers(digits.into_iter().map(Some).collect()),
            Task::Cards(cards) => Recall::Cards(cards.into_iter().map(Some).collect()),
        }
    }

    fn json<T: Serialize>(value: &T) -> Vec<u8> {
        serde_json::to_vec(value).unwrap()
    }

    fn facts(entries: &[(String, Vec<u8>)]) -> Facts {
        let entries = entries.iter().map(|(k, v)| (k.as_str(), v.as_slice()));
        collect(entries, "0", &mut Rescored::default())
    }

    #[test]
    fn only_readable_facts_of_the_epoch_are_collected() {
        let fact = round("a", Kind::Practice, Discipline::Cards, 1, 0);
        let marker = MarkerFact {
            v: VERSION,
            kind: DAILY.into(),
            started_at_ms: 5,
            first: Some(true),
        };
        let mut newer =
            serde_json::to_value(round("e", Kind::Code, Discipline::List, 1, 0)).unwrap();
        newer["kind"] = serde_json::json!("duel");
        let entries = vec![
            (round_key("0", "a"), json(&fact)),
            // Another epoch, a blanked key, a key that does not match its id, a newer kind.
            (
                round_key("1-x", "b"),
                json(&round("b", Kind::Code, Discipline::List, 1, 0)),
            ),
            (round_key("0", "c"), Vec::new()),
            (
                round_key("0", "d"),
                json(&round("x", Kind::Practice, Discipline::Cards, 1, 13)),
            ),
            (round_key("0", "e"), serde_json::to_vec(&newer).unwrap()),
            (
                marker_key("0", date("2026-10-28"), Discipline::Cards, level(2), "a"),
                json(&marker),
            ),
            ("g/0/s/2026-10-28/cards/4/x".into(), json(&marker)),
            (
                sighting_key("0", Discipline::List, level(3), Seed::new(9).unwrap(), "a"),
                br#"{"v":1,"started_at_ms":7,"first":true}"#.to_vec(),
            ),
            ("g/0/t/list/3/9/b".into(), json(&marker)),
            ("legacy/x".into(), b"1".to_vec()),
        ];
        let facts = facts(&entries);
        assert_eq!(facts.rounds.len(), 1);
        assert_eq!(facts.rounds["a"], fact);
        assert!(!facts.rounds.contains_key("x"), "stored under another id");
        assert_eq!(facts.markers.len(), 1, "level 4 does not exist");
        assert_eq!(facts.markers[0].level, level(2));
        assert_eq!(facts.sightings.len(), 1, "a seed is 15 hex digits");
        assert_eq!(facts.sightings[0].fact.started_at_ms, 7);
    }

    #[test]
    fn facts_with_unknown_fields_still_load() {
        let fact = round("a", Kind::Daily, Discipline::Numbers, 1, 20);
        // The daily of Tuesday 2026-10-27 (numbers), every digit right.
        let mut daily = fact.clone();
        daily.date = date("2026-10-27");
        daily.seed = daily_seed(daily.date, Discipline::Numbers, level(1));
        daily.recall = all_right(&daily);
        let mut value: serde_json::Value = serde_json::to_value(&daily).unwrap();
        value["server"] = serde_json::json!({"round_id": "gr_1"});
        value["v"] = serde_json::json!(2);
        // A round from before `route` was written.
        value.as_object_mut().unwrap().remove("route");
        let mut evening = serde_json::to_value(EveningFact {
            v: VERSION,
            round_id: "a".into(),
            id: "e".into(),
            date: date("2026-10-28"),
            finished_at_ms: 9 * H,
            recall: Recall::Numbers(vec![None; 20]),
            lang: Lang::De,
            points: 0,
            max: 20,
        })
        .unwrap();
        evening["xp"] = serde_json::json!(5);
        let entries = vec![
            (round_key("0", "a"), serde_json::to_vec(&value).unwrap()),
            (
                evening_key("0", "a", "e"),
                serde_json::to_vec(&evening).unwrap(),
            ),
        ];
        let facts = facts(&entries);
        assert_eq!(facts.rounds["a"].points, 20);
        assert_eq!(facts.rounds["a"].route, None);
        assert_eq!(facts.evening("a").unwrap().max, 20);
        // The JSON keeps the shapes of the rules crate.
        let text = String::from_utf8(json(&fact)).unwrap();
        assert!(text.contains(r#""seed":"000000000000123""#), "{text}");
        assert!(text.contains(r#""recall":{"digits":[null"#), "{text}");
        assert!(text.contains(r#""kind":"daily""#), "{text}");
    }

    #[test]
    fn rounds_and_evening_recalls_count_only_as_scored_again() {
        let day = date("2026-10-28");
        let mut daily = round("d", Kind::Daily, Discipline::Cards, 1, 0);
        daily.seed = daily_seed(day, Discipline::Cards, level(1));
        daily.recall = all_right(&daily);
        // A deck that claims more points than it has places, with every place empty.
        let mut inflated = round("i", Kind::Practice, Discipline::Cards, 3, 0);
        (inflated.points, inflated.max, inflated.perfect) = (60_000, 1, true);
        // No recall at all; a timed round slower than its limit; a daily that is not the
        // date's task; rules this build does not know.
        let mut empty = round("e", Kind::Practice, Discipline::Cards, 3, 52);
        empty.recall = Recall::Cards(Vec::new());
        let mut slow = round("s", Kind::Practice, Discipline::Cards, 1, 0);
        slow.memorize_ms = 9_000_000;
        let mut untimed = slow.clone();
        (untimed.id, untimed.untimed) = ("u".into(), true);
        let mut wrong = round("w", Kind::Daily, Discipline::Cards, 2, 0);
        wrong.seed = daily_seed(day, Discipline::Cards, level(1));
        let mut newer = round("n", Kind::Practice, Discipline::Cards, 1, 0);
        newer.rules = 2;
        let evening = |id: &str, finished_at_ms, recall| EveningFact {
            v: VERSION,
            round_id: "d".into(),
            id: id.into(),
            date: day,
            finished_at_ms,
            recall,
            lang: Lang::De,
            points: 13,
            max: 13,
        };
        let blank = Recall::blank(Discipline::Cards, 13);
        let mut entries: Vec<(String, Vec<u8>)> = [&daily, &inflated, &empty, &slow, &untimed]
            .into_iter()
            .chain([&wrong, &newer])
            .map(|fact| (round_key("0", &fact.id), json(fact)))
            .collect();
        for fact in [
            evening("x", 9 * H, blank.clone()),
            evening("y", 7 * H, all_right(&daily)),
        ] {
            entries.push((evening_key("0", "d", &fact.id), json(&fact)));
        }
        let facts = facts(&entries);
        assert_eq!(
            facts.rounds.keys().map(String::as_str).collect::<Vec<_>>(),
            ["d", "i", "u"]
        );
        let inflated = &facts.rounds["i"];
        assert_eq!(
            (inflated.points, inflated.max, inflated.perfect),
            (0, 52, false)
        );
        assert_eq!(facts.rounds["d"].points, 13);
        // Only the recall within the window counts, with its own points.
        assert_eq!(facts.evenings["d"].len(), 1);
        assert_eq!(facts.evening("d").map(|e| (e.points, e.max)), Some((0, 13)));
        let earned = facts.trophies(later());
        assert!(!earned.contains_key("compete-deck"));
        assert!(!earned.contains_key("compete-evening"));
        assert_eq!(
            facts
                .best(Discipline::Cards, level(3), later())
                .map(|b| b.points),
            Some(0)
        );
        // Each value is scored once; other bytes under the same key are scored anew.
        let mut rescored = Rescored::default();
        let read = |entries: &[(String, Vec<u8>)], rescored: &mut Rescored| {
            let entries = entries.iter().map(|(k, v)| (k.as_str(), v.as_slice()));
            collect(entries, "0", rescored)
        };
        assert_eq!(read(&entries, &mut rescored), facts);
        assert_eq!(read(&entries, &mut rescored), facts);
        daily.recall = blank;
        entries[0].1 = json(&daily);
        assert_eq!(read(&entries, &mut rescored).rounds["d"].points, 0);
    }

    fn marker(day: &str, n: u8, id: &str, started_at_ms: u64) -> Marker {
        Marker {
            date: date(day),
            discipline: plan(date(day)).discipline,
            level: level(n),
            round_id: id.into(),
            fact: Some(MarkerFact {
                v: VERSION,
                kind: DAILY.into(),
                started_at_ms,
                first: Some(true),
            }),
        }
    }

    /// A daily round of 2026-10-28 (cards) at level `n`, with its marker.
    fn daily(facts: &mut Facts, id: &str, n: u8, points: u16, started: u64) -> RoundFact {
        let mut fact = round(id, Kind::Daily, Discipline::Cards, n, points);
        fact.seed = daily_seed(fact.date, Discipline::Cards, level(n));
        fact.started_at_ms = started;
        fact.finished_at_ms = started + H / 4;
        facts.markers.push(marker("2026-10-28", n, id, started));
        facts.rounds.insert(id.into(), fact.clone());
        fact
    }

    #[test]
    fn the_earliest_marker_is_the_days_try_and_the_other_round_counts_as_practice() {
        let mut facts = Facts::default();
        // The round that began first has the greater id: the start counts, never the id.
        let late = daily(&mut facts, "a", 1, 13, 2 * H);
        let early = daily(&mut facts, "b", 1, 9, H);
        assert!(facts.counted_as_practice(&late));
        assert!(!facts.counted_as_practice(&early));
        let today = facts.today(date("2026-10-28"), 3 * H);
        assert_eq!(
            today.levels[0].state,
            LevelState::Done {
                round_id: "b".into(),
                points: 9,
                max: 13,
                memorize_ms: 60_000,
                counted: Counted::Daily,
            }
        );
        assert_eq!(today.levels[1].state, LevelState::Open);
        // The later round saw a task seen before: it is no record.
        assert!(facts.repeat_seed(&late));
        assert_eq!(
            facts
                .best(Discipline::Cards, level(1), later())
                .unwrap()
                .round_id,
            "b"
        );
        let history = facts.history(0, None, None);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].round_id, "a", "newest first");
        assert!(history[0].counted_as_practice && !history[1].counted_as_practice);
    }

    #[test]
    fn a_start_marker_of_any_kind_or_one_that_does_not_read_holds_its_level() {
        let day = date("2026-10-28");
        let key = |n, id| marker_key("0", day, Discipline::Cards, level(n), id);
        // Locora Cloud's daily (stage 2) began level 1 first; this device's daily later.
        let cloud = serde_json::json!({"v": 2, "kind": "cloud_daily", "started_at_ms": H});
        let mut local = round("l", Kind::Daily, Discipline::Cards, 1, 13);
        local.seed = daily_seed(day, Discipline::Cards, level(1));
        local.started_at_ms = 2 * H;
        local.finished_at_ms = 2 * H + H / 4;
        let marker = MarkerFact {
            v: VERSION,
            kind: DAILY.into(),
            started_at_ms: 2 * H,
            first: Some(true),
        };
        let mut entries = vec![
            (key(1, "c"), serde_json::to_vec(&cloud).unwrap()),
            (key(1, "l"), json(&marker)),
            // Level 2: a marker that does not read, and one without its start.
            (key(2, "x"), b"garbage".to_vec()),
            (key(2, "y"), br#"{"v":3,"kind":"duel"}"#.to_vec()),
        ];
        let state = |entries: &[(String, Vec<u8>)], n: usize, now| {
            facts(entries).today(day, now).levels[n].state.clone()
        };
        let started = LevelState::Started {
            round_id: "c".into(),
        };
        assert_eq!(state(&entries, 0, H + 60_000), started);
        let unfinished = |id: &str| LevelState::Unfinished {
            round_id: id.into(),
        };
        assert_eq!(state(&entries, 0, 2 * H), unfinished("c"));
        // A marker that does not say when it began sorts last and is never running.
        assert_eq!(state(&entries, 1, 0), unfinished("x"));
        assert_eq!(state(&entries, 2, 0), LevelState::Open);
        // The local round, handed in, counts as practice; its task (the date's) was new.
        entries.push((round_key("0", "l"), json(&local)));
        let facts = facts(&entries);
        assert!(facts.counted_as_practice(&local));
        assert!(matches!(
            facts.today(day, 3 * H).levels[0].state,
            LevelState::Done {
                counted: Counted::Practice,
                ..
            }
        ));
        assert!(!facts.repeat_seed(&local), "the cloud's task is another");
        assert_eq!(facts.markers.len(), 4);
    }

    #[test]
    fn a_daily_never_handed_in_stays_unfinished_after_its_time() {
        let mut facts = Facts::default();
        facts.markers.push(marker("2026-10-28", 2, "a", H));
        let spec = Spec::standard(Discipline::Cards, level(2));
        let ends = H + u64::from(spec.memorize_ms + spec.recall_ms + RECALL_GRACE_MS);
        let state =
            |facts: &Facts, now| facts.today(date("2026-10-28"), now).levels[1].state.clone();
        assert_eq!(
            state(&facts, ends - 1),
            LevelState::Started {
                round_id: "a".into()
            }
        );
        assert_eq!(
            state(&facts, ends),
            LevelState::Unfinished {
                round_id: "a".into()
            }
        );
        // Another device played the same daily later and handed it in: its points show, as
        // practice.
        daily(&mut facts, "b", 2, 20, 2 * H);
        assert!(matches!(
            state(&facts, ends),
            LevelState::Done {
                points: 20,
                counted: Counted::Practice,
                ..
            }
        ));
        // A marker seen first counts as a sighting of the task.
        assert!(facts.repeat_seed(&facts.rounds["b"]));
        assert_eq!(facts.best(Discipline::Cards, level(2), later()), None);
    }

    #[test]
    fn records_come_only_from_a_tasks_first_play_with_the_time_limit() {
        let mut facts = Facts::default();
        // The first play has the greater id: what was seen first counts, never the id.
        let mut first = round("b", Kind::Code, Discipline::List, 1, 7);
        first.started_at_ms = H;
        let mut again = round("a", Kind::Code, Discipline::List, 1, 10);
        again.started_at_ms = 3 * H;
        again.finished_at_ms = 4 * H;
        let mut untimed = round("c", Kind::Practice, Discipline::List, 1, 10);
        untimed.seed = Seed::new(0x456).unwrap();
        untimed.untimed = true;
        for fact in [&first, &again, &untimed] {
            facts.rounds.insert(fact.id.clone(), fact.clone());
        }
        let best = facts.best(Discipline::List, level(1), later()).unwrap();
        assert_eq!((best.round_id.as_str(), best.points), ("b", 7));
        assert!(facts.repeat_seed(&again) && !facts.repeat_seed(&first));
        // A replay with the clock set back: its device had seen the task, whatever the time.
        let mut back = round("f", Kind::Code, Discipline::List, 1, 10);
        back.started_at_ms = 0;
        back.first = Some(false);
        facts.rounds.insert("f".into(), back.clone());
        assert!(facts.repeat_seed(&back) && !facts.repeat_seed(&first));
        // The same for a round that was looked at and left; one seen first takes the task.
        let sighting = |id: &str, started_at_ms, first| Sighting {
            discipline: Discipline::List,
            level: level(1),
            seed: Seed::new(0x123).unwrap(),
            round_id: id.into(),
            fact: SightingFact {
                v: VERSION,
                started_at_ms,
                first: Some(first),
            },
        };
        facts.sightings.push(sighting("g", 1, false));
        assert_eq!(
            facts
                .best(Discipline::List, level(1), later())
                .unwrap()
                .round_id,
            "b"
        );
        facts.sightings.push(sighting("h", 2, true));
        assert_eq!(facts.best(Discipline::List, level(1), later()), None);
        assert!(facts.repeat_seed(&first));
        facts.sightings.clear();
        facts.rounds.remove("f");
        // A faster round with the same points takes the record; a slower one does not.
        let mut faster = round("d", Kind::Practice, Discipline::List, 1, 7);
        faster.seed = Seed::new(0x789).unwrap();
        faster.memorize_ms = 30_000;
        facts.rounds.insert("d".into(), faster.clone());
        assert_eq!(
            facts
                .best(Discipline::List, level(1), later())
                .unwrap()
                .round_id,
            "d"
        );
        let records = facts.records(later());
        let list_1 = &records.levels[0];
        assert_eq!(
            (list_1.discipline, list_1.level, list_1.rounds),
            (Discipline::List, level(1), 4)
        );
        assert_eq!(records.levels.len(), 9);
        assert!(records.levels[1..].iter().all(|l| l.best.is_none()));
        assert_eq!(records.daily_days, 0);
    }

    #[test]
    fn the_history_is_newest_first_in_pages_of_at_most_a_hundred() {
        let mut facts = Facts::default();
        for n in 0..120u64 {
            let mut fact = round(
                &format!("r{n:03}"),
                Kind::Practice,
                Discipline::Numbers,
                1,
                0,
            );
            fact.seed = Seed::new(n).unwrap();
            fact.finished_at_ms = (n + 1) * H;
            facts.rounds.insert(fact.id.clone(), fact);
        }
        assert_eq!(facts.history(500, None, None).len(), 100, "at most 100");
        assert_eq!(facts.history(0, None, None).len(), 100);
        let newest = facts.history(1, None, None);
        assert_eq!(newest.len(), 1);
        assert_eq!(newest[0].round_id, "r119");
        // The next page: rounds handed in before the last one shown.
        let page = facts.history(3, Some(newest[0].finished_at_ms), None);
        let ids: Vec<&str> = page.iter().map(|e| e.round_id.as_str()).collect();
        assert_eq!(ids, ["r118", "r117", "r116"]);
        assert!(facts.history(0, Some(H), None).is_empty());
        // Rounds handed in in the same millisecond: a page goes on after its last round.
        for id in ["s1", "s2", "s3"] {
            let mut fact = round(id, Kind::Practice, Discipline::Numbers, 1, 0);
            fact.finished_at_ms = 500 * H;
            facts.rounds.insert(id.into(), fact);
        }
        let mut seen = Vec::new();
        let mut cursor: Option<(u64, String)> = None;
        while seen.len() < 5 {
            let (ms, id) = cursor.clone().unzip();
            let page = facts.history(1, ms, id.as_deref());
            let last = page.last().unwrap();
            seen.push(last.round_id.clone());
            cursor = Some((last.finished_at_ms, last.round_id.clone()));
        }
        assert_eq!(seen, ["s3", "s2", "s1", "r119", "r118"]);
    }

    fn finished_daily(day: &str, points: u16, finished: u64) -> RoundFact {
        let mut fact = round(day, Kind::Daily, plan(date(day)).discipline, 1, points);
        fact.date = date(day);
        fact.finished_at_ms = finished;
        fact
    }

    #[test]
    fn the_evening_window_opens_after_six_hours_and_closes_after_a_day() {
        let mut facts = Facts::default();
        let round = finished_daily("2026-10-28", 13, 10 * H);
        facts.rounds.insert(round.id.clone(), round.clone());
        let at = |ms| facts.evening_state(&round, 10 * H + ms);
        assert_eq!(at(6 * H - 1_000), EveningState::Locked, "5:59:59");
        assert_eq!(at(6 * H), EveningState::Open, "6:00");
        assert_eq!(at(24 * H - 1), EveningState::Open);
        assert_eq!(at(24 * H), EveningState::Closed, "24:00");
        // Today shows it while it can still be done, and on its own day after that.
        let today = facts.today(date("2026-10-28"), 10 * H + 6 * H);
        assert_eq!(today.evenings.len(), 1);
        assert_eq!(today.evenings[0].state, EveningState::Open);
        assert_eq!(today.evenings[0].items, 13);
        assert_eq!(tile(&today).0, "Die Abendabfrage ist offen");
        let tomorrow = facts.today(date("2026-10-29"), 10 * H + 25 * H);
        assert!(tomorrow.evenings.is_empty());
        // Two evening recalls of one round (two devices): the first handed in counts.
        for (id, points, finished) in [("late", 13, 20 * H), ("early", 11, 19 * H)] {
            facts
                .evenings
                .entry(round.id.clone())
                .or_default()
                .push(EveningFact {
                    v: VERSION,
                    round_id: round.id.clone(),
                    id: id.into(),
                    date: date("2026-10-28"),
                    finished_at_ms: finished,
                    recall: Recall::blank(Discipline::Cards, 13),
                    lang: Lang::De,
                    points,
                    max: 13,
                });
        }
        let at = |ms| facts.evening_state(&round, 10 * H + ms);
        assert_eq!(
            at(25 * H),
            EveningState::Done {
                points: 11,
                max: 13
            }
        );
        assert_eq!(
            facts.history(1, None, None)[0].evening,
            Some(EveningScore {
                points: 11,
                max: 13
            })
        );
    }

    #[test]
    fn a_round_from_a_clock_that_ran_ahead_waits_for_its_date() {
        let mut facts = Facts::default();
        let day = date("2026-10-28");
        // A daily played on a device a year ahead (a Thursday: a list).
        let ahead = finished_daily("2027-10-28", 10, 9_000 * H);
        facts.rounds.insert(ahead.id.clone(), ahead.clone());
        // Today, tomorrow and months later its evening recall is none of today's.
        for (n, now) in [(0, 10 * H), (1, 34 * H), (120, 2_900 * H)] {
            assert!(facts.today(day.add_days(n), now).evenings.is_empty(), "{n}");
        }
        assert!(facts.trophies(day).is_empty());
        let records = facts.records(day);
        assert_eq!(records.daily_days, 0);
        assert!(
            records
                .levels
                .iter()
                .all(|l| l.best.is_none() && l.rounds == 0)
        );
        // From its date on it counts.
        assert_eq!(
            facts.trophies(ahead.date)["compete-first"],
            date("2027-10-28")
        );
        assert_eq!(facts.records(ahead.date).daily_days, 1);
        assert!(facts.best(Discipline::List, level(1), ahead.date).is_some());
        // A round of today handed in an hour ahead of this clock: its recall is today's.
        let soon = finished_daily("2026-10-28", 13, 11 * H);
        facts.rounds.insert(soon.id.clone(), soon);
        let evenings = facts.today(day, 10 * H).evenings;
        assert_eq!(evenings.len(), 1);
        assert_eq!(evenings[0].state, EveningState::Locked);
    }

    #[test]
    fn today_shows_the_week_ahead_and_behind_and_the_tile_follows_it() {
        let mut facts = Facts::default();
        // 2026-10-28 is a Wednesday: cards; Thursday a list; Sunday 2026-10-25 a list too.
        let today = facts.today(date("2026-10-28"), 0);
        assert_eq!(today.today.discipline, Discipline::Cards);
        assert_eq!(today.tomorrow.discipline, Discipline::List);
        assert_eq!(today.earlier.len(), 7);
        assert_eq!(today.earlier[0].date, date("2026-10-27"));
        assert!(today.earlier[2].long_day, "Sunday");
        assert_eq!(
            today
                .levels
                .iter()
                .map(|l| l.spec.items)
                .collect::<Vec<_>>(),
            [13, 26, 52]
        );
        assert_eq!(
            tile(&today),
            ("Heute: Karten – noch nicht gespielt", Vec::new())
        );
        daily(&mut facts, "a", 1, 9, H);
        daily(&mut facts, "b", 2, 20, 2 * H);
        let today = facts.today(date("2026-10-28"), 3 * H);
        assert_eq!(
            tile(&today),
            (
                "Heute: Karten – {0}/{1}",
                vec!["20".to_owned(), "26".to_owned()]
            )
        );
    }

    #[test]
    fn trophies_follow_the_rounds_with_the_time_limit_and_keep_their_first_date() {
        let mut facts = Facts::default();
        assert!(facts.trophies(later()).is_empty());
        // Six daily days, then an untimed perfect deck: neither the 7th day nor the deck.
        for day in 0..6 {
            let mut fact = finished_daily(
                &date("2026-10-01").add_days(day).to_string(),
                0,
                day as u64 * 24 * H,
            );
            fact.id = format!("d{day}");
            facts.rounds.insert(fact.id.clone(), fact);
        }
        let mut deck = round("deck", Kind::Practice, Discipline::Cards, 3, 52);
        deck.untimed = true;
        deck.finished_at_ms = 200 * H;
        facts.rounds.insert("deck".into(), deck.clone());
        let earned = facts.trophies(later());
        assert_eq!(
            earned.keys().copied().collect::<Vec<_>>(),
            ["compete-first"]
        );
        assert_eq!(earned["compete-first"], date("2026-10-01"));
        // The 7th day, a timed deck, a perfect list and numbers at level 2: all three too.
        let mut seventh = finished_daily("2026-10-20", 0, 300 * H);
        seventh.id = "d6".into();
        facts.rounds.insert("d6".into(), seventh);
        deck.untimed = false;
        deck.date = date("2026-10-21");
        deck.finished_at_ms = 301 * H;
        facts.rounds.insert("deck".into(), deck);
        let mut list = round("list", Kind::Code, Discipline::List, 1, 10);
        list.finished_at_ms = 302 * H;
        list.date = date("2026-10-22");
        facts.rounds.insert("list".into(), list);
        let mut digits = round("digits", Kind::Code, Discipline::Numbers, 2, 40);
        digits.finished_at_ms = 303 * H;
        digits.date = date("2026-10-23");
        facts.rounds.insert("digits".into(), digits);
        let earned = facts.trophies(later());
        assert_eq!(earned["compete-daily-7"], date("2026-10-20"));
        assert_eq!(earned["compete-deck"], date("2026-10-21"));
        assert_eq!(earned["compete-numbers-40"], date("2026-10-23"));
        assert_eq!(
            earned["compete-triple"],
            date("2026-10-23"),
            "the third discipline's date"
        );
        assert!(
            !earned.contains_key("compete-list-30"),
            "a list of 10 is no list of 30"
        );
        assert!(!earned.contains_key("compete-daily-30"));
        // An evening recall of at least 10, all kept.
        facts
            .evenings
            .entry("d6".into())
            .or_default()
            .push(EveningFact {
                v: VERSION,
                round_id: "d6".into(),
                id: "e".into(),
                date: date("2026-10-24"),
                finished_at_ms: 310 * H,
                recall: Recall::blank(Discipline::List, 10),
                lang: Lang::De,
                points: 10,
                max: 10,
            });
        assert_eq!(
            facts.trophies(later())["compete-evening"],
            date("2026-10-24")
        );
        let records = facts.records(later());
        assert_eq!(records.trophies.len(), TROPHIES.len());
        assert_eq!(
            records
                .trophies
                .iter()
                .filter(|t| t.unlocked.is_some())
                .count(),
            6
        );
        assert_eq!(records.daily_days, 7);
    }

    #[test]
    fn the_hint_names_neighbouring_misses_then_swaps_then_any_miss() {
        let task = Task::Cards(vec![0, 1, 2, 3, 4, 5]);
        let hint_for = |recall: Vec<Option<u8>>| {
            let recall = Recall::Cards(recall);
            let score = locora_competition::score(&task, &recall, Lang::En).unwrap();
            hint(&task, &recall, &score)
        };
        let all = |cards: [Option<u8>; 6]| hint_for(cards.to_vec());
        let s = Some;
        assert_eq!(all([s(0), s(1), s(2), s(3), s(4), s(5)]), None);
        assert_eq!(all([s(0), None, None, s(3), s(4), s(5)]), Some(Hint::Route));
        assert_eq!(all([s(4), s(1), s(2), s(3), s(0), s(5)]), Some(Hint::Loci));
        assert_eq!(
            all([s(0), None, s(2), s(3), s(4), s(5)]),
            Some(Hint::Exaggerate)
        );
        // A shopping list knows no swaps.
        let task = Task::List(vec![1, 2]);
        let recall = Recall::List(vec![String::new(), String::new()]);
        let score = locora_competition::score(&task, &recall, Lang::De).unwrap();
        assert_eq!(hint(&task, &recall, &score), Some(Hint::Route));
    }
}
