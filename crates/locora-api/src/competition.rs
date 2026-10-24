//! Services of the `competition` plugin: the memory contest's rounds, records, trophies and
//! history (see `docs/PLAN.md`, section 4.8).
//!
//! Tasks, recalls and scores are the types of `locora-competition`, the rules the plugin, the app
//! and Locora Cloud share. The runtime supplies the game epoch, the learning date and the time
//! with every request, so the plugin needs no clock, and it takes an epoch only from these
//! requests. There are no events: contest XP reaches gamification through a call of the runtime,
//! never through a topic that any plugin could publish.

use super::*;

pub use locora_competition::{
    Date, Discipline, Info, Lang, Level, Mark, Recall, Score, Seed, Spec, Task,
};

/// The ids of the contest trophies, in the order they are shown.
pub const TROPHIES: [&str; 8] = [
    "compete-first",
    "compete-daily-7",
    "compete-daily-30",
    "compete-list-30",
    "compete-numbers-40",
    "compete-deck",
    "compete-evening",
    "compete-triple",
];

/// Recall may run this much past its limit before it counts as over (the hand-in on the way).
pub const RECALL_GRACE_MS: u32 = 10_000;
/// The evening recall of a daily round opens this long after the round was handed in …
pub const EVENING_OPENS_MS: u64 = 6 * 3_600_000;
/// … and closes this long after it.
pub const EVENING_CLOSES_MS: u64 = 24 * 3_600_000;
/// Most entries `history@1` answers at once.
pub const HISTORY_LIMIT: u32 = 100;

/// What kind of round: practice (a fresh seed, or an earlier day's), the daily challenge (one try
/// per level and learning day), or a challenge code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Practice,
    Daily,
    Code,
}

/// The discipline of a day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayPlan {
    pub date: Date,
    pub discipline: Discipline,
    /// Sunday: the evening recall comes first.
    pub long_day: bool,
}

/// How a daily round of today stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LevelState {
    Open,
    /// Begun and still within its time: it can be handed in.
    Started {
        round_id: String,
    },
    /// Begun and never handed in: it counts as played and cannot be started again.
    Unfinished {
        round_id: String,
    },
    /// Handed in. `counted` is `Practice` when another device began the same daily earlier and
    /// that round was never handed in: these points stand, but the earlier try is the day's.
    Done {
        round_id: String,
        points: u16,
        max: u16,
        memorize_ms: u32,
        counted: Counted,
    },
}

/// Whether a daily round is the day's try or counts as practice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Counted {
    Daily,
    Practice,
}

/// One level of today's challenge: its size, limits and state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelToday {
    pub spec: Spec,
    pub state: LevelState,
}

/// Where an evening recall stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EveningState {
    /// Opens at `opens_at_ms`.
    Locked,
    Open,
    Done {
        points: u16,
        max: u16,
    },
    Closed,
}

/// The evening recall of a daily round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EveningToday {
    pub round_id: String,
    pub discipline: Discipline,
    pub level: Level,
    pub items: u8,
    pub opens_at_ms: u64,
    pub closes_at_ms: u64,
    pub state: EveningState,
}

/// Today's challenge (`competition.today@1`).
pub struct Today;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodayRequest {
    pub epoch: String,
    /// The learning date now.
    pub date: Date,
    /// Now, milliseconds since the Unix epoch.
    pub now_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodayResponse {
    pub today: DayPlan,
    /// Levels 1 to 3.
    pub levels: Vec<LevelToday>,
    /// The evening recalls of today's daily rounds and of those still open, oldest first.
    pub evenings: Vec<EveningToday>,
    pub tomorrow: DayPlan,
    /// The seven days before today, newest first: each can be played as practice.
    pub earlier: Vec<DayPlan>,
}

impl Service for Today {
    const NAME: &'static str = "competition.today@1";
    type Request = TodayRequest;
    type Response = TodayResponse;
}

/// A round begins (`competition.begin@1`): its task. A daily round writes its start marker here,
/// and a second daily of the same level and learning day is refused.
pub struct Begin;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeginRequest {
    pub epoch: String,
    /// 32 lowercase hex characters, from the runtime.
    pub round_id: String,
    pub kind: Kind,
    pub discipline: Discipline,
    pub level: Level,
    /// For a daily round, the day's seed (`locora_competition::daily_seed`).
    pub seed: Seed,
    /// The learning date the round belongs to.
    pub date: Date,
    pub now_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeginResponse {
    pub spec: Spec,
    pub task: Task,
}

impl Service for Begin {
    const NAME: &'static str = "competition.begin@1";
    type Request = BeginRequest;
    type Response = BeginResponse;
}

/// A round is handed in (`competition.finish@1`): the plugin scores it again from its seed and
/// stores it.
pub struct Finish;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishRequest {
    pub epoch: String,
    pub round_id: String,
    pub kind: Kind,
    pub discipline: Discipline,
    pub level: Level,
    pub seed: Seed,
    /// The learning date the round began on.
    pub date: Date,
    /// The language the round was played in (the shopping list is scored in it).
    pub lang: Lang,
    /// Practice without a time limit: never counts for records or trophies.
    pub untimed: bool,
    /// The palace whose route the round was laid on; it stays on the devices.
    pub route: Option<String>,
    pub started_at_ms: u64,
    pub memorize_ms: u32,
    pub recall_ms: u32,
    pub recall: Recall,
    pub now_ms: u64,
}

/// The best round of a discipline and level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Best {
    pub round_id: String,
    pub points: u16,
    pub max: u16,
    pub memorize_ms: u32,
    pub date: Date,
}

/// The record after a round: `new` when this round set it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordLine {
    pub new: bool,
    pub best: Option<Best>,
}

/// The one hint a result may give, with the lesson it points to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hint {
    /// Several places in a row were empty or wrong.
    Route,
    /// Two places hold each other's item (cards and numbers).
    Loci,
    /// Some place was wrong or empty.
    Exaggerate,
}

impl Hint {
    /// The lesson of „Mnemotechnik lernen“ the hint opens.
    pub fn lesson(self) -> &'static str {
        match self {
            Hint::Route => "route",
            Hint::Loci => "loci",
            Hint::Exaggerate => "exaggerate",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishResponse {
    pub score: Score,
    pub record: RecordLine,
    /// Trophies this round earned, on this device.
    pub trophies: Vec<String>,
    pub hint: Option<Hint>,
    /// A daily round another device began earlier: that one is the day's try.
    pub counted_as_practice: bool,
    /// The task was played before (on any device): it does not count for records.
    pub repeat_seed: bool,
}

impl Service for Finish {
    const NAME: &'static str = "competition.finish@1";
    type Request = FinishRequest;
    type Response = FinishResponse;
}

/// The evening recall of a daily round (`competition.evening@1`): its window and state, without
/// the task.
pub struct Evening;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EveningRequest {
    pub epoch: String,
    pub round_id: String,
    pub now_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EveningResponse {
    pub round_id: String,
    pub spec: Spec,
    /// The learning date of the morning round.
    pub date: Date,
    /// The language and route of the morning round.
    pub lang: Lang,
    pub route: Option<String>,
    pub opens_at_ms: u64,
    pub closes_at_ms: u64,
    pub state: EveningState,
}

impl Service for Evening {
    const NAME: &'static str = "competition.evening@1";
    type Request = EveningRequest;
    type Response = EveningResponse;
}

/// An evening recall is handed in (`competition.evening_finish@1`).
pub struct EveningFinish;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EveningFinishRequest {
    pub epoch: String,
    pub round_id: String,
    /// 32 lowercase hex characters, from the runtime.
    pub evening_id: String,
    /// The learning date now.
    pub date: Date,
    pub recall: Recall,
    pub lang: Lang,
    pub now_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EveningFinishResponse {
    pub score: Score,
    /// The points of the morning round.
    pub morning_points: u16,
    pub trophies: Vec<String>,
}

impl Service for EveningFinish {
    const NAME: &'static str = "competition.evening_finish@1";
    type Request = EveningFinishRequest;
    type Response = EveningFinishResponse;
}

/// Records, the days with a daily challenge and the trophies (`competition.records@1`).
pub struct Records;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordsRequest {
    pub epoch: String,
    /// The learning date now: a round dated later (a device whose clock ran ahead) counts
    /// from its date on.
    pub date: Date,
}

/// The record of one discipline and level, and how many rounds were handed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelRecord {
    pub discipline: Discipline,
    pub level: Level,
    pub best: Option<Best>,
    pub rounds: u32,
}

/// A contest trophy and the learning date it was first earned on (on any device).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trophy {
    pub id: String,
    pub unlocked: Option<Date>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordsResponse {
    /// Every discipline and level, list first.
    pub levels: Vec<LevelRecord>,
    /// Learning dates with a daily round handed in.
    pub daily_days: u32,
    /// Every trophy of [`TROPHIES`], in that order.
    pub trophies: Vec<Trophy>,
}

impl Service for Records {
    const NAME: &'static str = "competition.records@1";
    type Request = RecordsRequest;
    type Response = RecordsResponse;
}

/// The rounds handed in, newest first (`competition.history@1`).
pub struct History;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRequest {
    pub epoch: String,
    /// At most [`HISTORY_LIMIT`].
    pub limit: u32,
    /// Only rounds handed in before this time (the next page).
    pub before_ms: Option<u64>,
}

/// The points of an evening recall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EveningScore {
    pub points: u16,
    pub max: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub round_id: String,
    pub kind: Kind,
    pub discipline: Discipline,
    pub level: Level,
    pub date: Date,
    pub points: u16,
    pub max: u16,
    pub memorize_ms: u32,
    pub untimed: bool,
    pub finished_at_ms: u64,
    pub counted_as_practice: bool,
    pub evening: Option<EveningScore>,
}

impl Service for History {
    const NAME: &'static str = "competition.history@1";
    type Request = HistoryRequest;
    type Response = Vec<HistoryEntry>;
}

/// The game progress was deleted (`competition.reset@1`): `epoch` is the new epoch, and rounds
/// of older ones are emptied. Answers how many stored facts were emptied.
pub struct Reset;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResetRequest {
    pub epoch: String,
}

impl Service for Reset {
    const NAME: &'static str = "competition.reset@1";
    type Request = ResetRequest;
    type Response = u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_answers_round_trip_through_postcard() {
        let date = Date::parse("2026-10-28").unwrap();
        let level = Level::new(2).unwrap();
        let request = FinishRequest {
            epoch: "0".into(),
            round_id: "0".repeat(32),
            kind: Kind::Code,
            discipline: Discipline::Cards,
            level,
            seed: Seed::new(0x123_4567_89ab_cdef).unwrap(),
            date,
            lang: Lang::De,
            untimed: false,
            route: None,
            started_at_ms: 1,
            memorize_ms: 2,
            recall_ms: 3,
            recall: Recall::Cards(vec![Some(36), None]),
            now_ms: 4,
        };
        assert_eq!(
            decode::<FinishRequest>(&encode(&request).unwrap()).unwrap(),
            request
        );
        let today = TodayResponse {
            today: DayPlan {
                date,
                discipline: Discipline::Cards,
                long_day: false,
            },
            levels: vec![LevelToday {
                spec: Spec::standard(Discipline::Cards, level),
                state: LevelState::Done {
                    round_id: "r".into(),
                    points: 24,
                    max: 26,
                    memorize_ms: 181_000,
                    counted: Counted::Practice,
                },
            }],
            evenings: vec![EveningToday {
                round_id: "r".into(),
                discipline: Discipline::Cards,
                level,
                items: 26,
                opens_at_ms: 5,
                closes_at_ms: 6,
                state: EveningState::Done { points: 1, max: 2 },
            }],
            tomorrow: DayPlan {
                date: date.add_days(1),
                discipline: Discipline::List,
                long_day: false,
            },
            earlier: Vec::new(),
        };
        assert_eq!(
            decode::<TodayResponse>(&encode(&today).unwrap()).unwrap(),
            today
        );
    }

    #[test]
    fn every_hint_opens_a_lesson() {
        let lessons = [Hint::Route, Hint::Loci, Hint::Exaggerate].map(Hint::lesson);
        assert_eq!(lessons, ["route", "loci", "exaggerate"]);
    }
}
