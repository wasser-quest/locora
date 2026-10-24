//! The memory contest around the `competition` plugin: round and evening ids, seeds, the time
//! limits of a round, and the plugin's services called with what only the runtime knows: the game
//! epoch (see `game`), the learning date (`Profile::learning_day`) and the time. The plugin has no
//! clock and takes an epoch only from these calls.
//!
//! A running round is the caller's (the app core, the CLI): nothing about it is stored until it
//! is handed in, except the start marker of a daily round.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use locora_api::Service;
use locora_api::competition::{
    Begin, BeginRequest, Evening, EveningFinish, EveningFinishRequest, EveningFinishResponse,
    EveningRequest, EveningResponse, Finish, FinishRequest, FinishResponse, History, HistoryEntry,
    HistoryRequest, Kind, RECALL_GRACE_MS, Records, RecordsRequest, RecordsResponse, Today,
    TodayRequest, TodayResponse,
};
use locora_competition::{
    Date, Discipline, Lang, Level, Recall, Seed, Spec, Task, code, daily_seed, schedule, texts,
};
use rand_core::{OsRng, RngCore};

use crate::Profile;

/// The plugin that keeps the contest.
pub const PLUGIN: &str = "competition";

/// A fresh id for a round or an evening recall: a UUID v7 as 32 lowercase hex characters, as
/// the store makes its ids.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}

/// A fresh 60-bit seed from the system's randomness.
pub fn new_seed() -> Seed {
    Seed::masked(OsRng.next_u64())
}

/// Now in milliseconds since the Unix epoch; `LOCORA_NOW` (seconds) overrides it, as for
/// [`crate::now`].
pub fn now_ms() -> u64 {
    match std::env::var("LOCORA_NOW")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        Some(seconds) => seconds.saturating_mul(1_000),
        None => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis().min(u128::from(u64::MAX)) as u64),
    }
}

/// How long the two phases count (spec §4): memorizing up to its limit, recall up to its limit
/// plus [`RECALL_GRACE_MS`] for the hand-in on its way. A round without a time limit keeps what
/// it took.
pub fn capped(spec: &Spec, untimed: bool, memorize: Duration, recall: Duration) -> (u32, u32) {
    let ms = |d: Duration| d.as_millis().min(u128::from(u32::MAX)) as u32;
    if untimed {
        return (ms(memorize), ms(recall));
    }
    (
        ms(memorize).min(spec.memorize_ms),
        ms(recall).min(spec.recall_ms.saturating_add(RECALL_GRACE_MS)),
    )
}

/// A round that began: what the plugin needs again to score it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Round {
    pub id: String,
    pub kind: Kind,
    pub spec: Spec,
    pub seed: Seed,
    pub task: Task,
    /// The learning date the round began on.
    pub date: Date,
    pub started_at_ms: u64,
}

impl Round {
    /// The challenge code of the task; a daily round has none (everyone has that task anyway).
    pub fn code(&self) -> Option<String> {
        (self.kind != Kind::Daily)
            .then(|| code::encode(self.spec.discipline, self.spec.level, self.seed))
    }
}

/// What a round brings when it is handed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandIn {
    pub recall: Recall,
    /// The language the round was played in (the shopping list is scored in it).
    pub lang: Lang,
    pub untimed: bool,
    /// The palace whose route the round was laid on.
    pub route: Option<String>,
    pub memorize_ms: u32,
    pub recall_ms: u32,
}

impl Profile {
    /// Whether the competition plugin runs.
    pub fn has_competition(&self) -> bool {
        self.host.plugin_provides(PLUGIN, Today::NAME)
    }

    /// The learning date now (see [`Profile::learning_day`]).
    pub fn learning_date(&self) -> Result<Date> {
        let (_, date) = self.learning_day()?;
        Date::parse(&date).with_context(|| format!("Kein gültiges Lerndatum: {date}"))
    }

    /// Calls a service of the plugin and keeps the tile it shows.
    fn contest<S: Service>(&mut self, request: &S::Request) -> Result<S::Response> {
        if !self.has_competition() {
            bail!("Dafür braucht es das Plugin `competition` (Gedächtnis-Wettkampf)");
        }
        let response = self.host.call_plugin::<S>(PLUGIN, request)?;
        let reported = self.report();
        self.notes.extend(reported);
        Ok(response)
    }

    fn contest_epoch(&self) -> Result<String> {
        Ok(self.game_epoch()?.id)
    }

    /// Today's challenge: the discipline, each level's state, the evening recalls, tomorrow and
    /// the last seven days.
    pub fn compete_today(&mut self) -> Result<TodayResponse> {
        let request = TodayRequest {
            epoch: self.contest_epoch()?,
            date: self.learning_date()?,
            now_ms: now_ms(),
        };
        self.contest::<Today>(&request)
    }

    /// Begins a round: practice with `seed` (a fresh one without it), the task of a challenge
    /// code, or today's daily challenge, whose discipline is the day's and whose seed is the
    /// date's. A daily writes its start marker now: one try per level and learning day.
    pub fn compete_begin(
        &mut self,
        kind: Kind,
        discipline: Discipline,
        level: Level,
        seed: Option<Seed>,
    ) -> Result<Round> {
        let date = self.learning_date()?;
        let seed = if kind == Kind::Daily {
            let planned = schedule::daily(date).discipline;
            ensure!(
                planned == discipline,
                "Heute ist die Tages-Challenge {}, nicht {}",
                texts::discipline(Lang::De, planned),
                texts::discipline(Lang::De, discipline)
            );
            daily_seed(date, discipline, level)
        } else {
            seed.unwrap_or_else(new_seed)
        };
        let id = new_id();
        let started_at_ms = now_ms();
        let request = BeginRequest {
            epoch: self.contest_epoch()?,
            round_id: id.clone(),
            kind,
            discipline,
            level,
            seed,
            date,
            now_ms: started_at_ms,
        };
        let began = self.contest::<Begin>(&request)?;
        Ok(Round {
            id,
            kind,
            spec: began.spec,
            seed,
            task: began.task,
            date,
            started_at_ms,
        })
    }

    /// Hands a round in: the plugin scores it again from its seed and stores it.
    pub fn compete_finish(&mut self, round: &Round, hand_in: HandIn) -> Result<FinishResponse> {
        let request = FinishRequest {
            epoch: self.contest_epoch()?,
            round_id: round.id.clone(),
            kind: round.kind,
            discipline: round.spec.discipline,
            level: round.spec.level,
            seed: round.seed,
            date: round.date,
            lang: hand_in.lang,
            untimed: hand_in.untimed,
            route: hand_in.route,
            started_at_ms: round.started_at_ms,
            memorize_ms: hand_in.memorize_ms,
            recall_ms: hand_in.recall_ms,
            recall: hand_in.recall,
            now_ms: now_ms(),
        };
        self.contest::<Finish>(&request)
    }

    /// The evening recall of a daily round: its window and state, without the task.
    pub fn compete_evening(&mut self, round_id: &str) -> Result<EveningResponse> {
        let request = EveningRequest {
            epoch: self.contest_epoch()?,
            round_id: round_id.to_owned(),
            now_ms: now_ms(),
        };
        self.contest::<Evening>(&request)
    }

    /// Hands in the evening recall of a daily round while its window is open.
    pub fn compete_evening_finish(
        &mut self,
        round_id: &str,
        recall: Recall,
        lang: Lang,
    ) -> Result<EveningFinishResponse> {
        let request = EveningFinishRequest {
            epoch: self.contest_epoch()?,
            round_id: round_id.to_owned(),
            evening_id: new_id(),
            date: self.learning_date()?,
            recall,
            lang,
            now_ms: now_ms(),
        };
        self.contest::<EveningFinish>(&request)
    }

    /// Records, days with a daily challenge, and the trophies.
    pub fn compete_records(&mut self) -> Result<RecordsResponse> {
        let request = RecordsRequest {
            epoch: self.contest_epoch()?,
            date: self.learning_date()?,
        };
        self.contest::<Records>(&request)
    }

    /// The rounds handed in, newest first: at most `limit` (100 for 0), before `before_ms`, and
    /// with `before_id` after that round (the last of the page before).
    pub fn compete_history(
        &mut self,
        limit: u32,
        before_ms: Option<u64>,
        before_id: Option<String>,
    ) -> Result<Vec<HistoryEntry>> {
        let request = HistoryRequest {
            epoch: self.contest_epoch()?,
            limit,
            before_ms,
            before_id,
        };
        self.contest::<History>(&request)
    }
}

#[cfg(test)]
#[path = "competition_tests.rs"]
mod competition_tests;
