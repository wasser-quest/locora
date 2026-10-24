//! The memory contest: rounds of a shopping list, numbers or cards against the clock, the daily
//! challenge, challenge codes, records, trophies and the history, the same on every device.
//!
//! The rules are `locora-competition`'s, shared with the app and Locora Cloud. This plugin keeps
//! one fact per round under a key of its own (see `facts`), holds them in a memory mirror
//! refreshed with `storage.list@1`, and derives everything else from the merged facts. The
//! runtime supplies the game epoch, the learning date and the time with every request, so the
//! plugin needs no clock; it takes an epoch only from these requests, and a new one (the game
//! progress was deleted) empties the facts of older ones. A running round lives in the app, never
//! in storage, so nothing half-finished syncs.

mod facts;

use std::cell::RefCell;
use std::collections::BTreeMap;

use facts::{EveningFact, FIRST_EPOCH, Facts, MarkerFact, RoundFact, SightingFact, VERSION};
use locora_competition::{
    Date, Discipline, Lang, Level, Seed, Spec, daily_seed, generate, schedule, score, texts,
    validate,
};
use locora_sdk::api::competition::{
    Begin, BeginRequest, BeginResponse, Evening, EveningFinish, EveningFinishRequest,
    EveningFinishResponse, EveningRequest, EveningResponse, EveningState, Finish, FinishRequest,
    FinishResponse, History, HistoryEntry, HistoryRequest, Kind, RECALL_GRACE_MS, RecordLine,
    Records, RecordsRequest, RecordsResponse, Reset, TROPHIES, Today, TodayRequest, TodayResponse,
};
use locora_sdk::api::storage::ListResponse;
use locora_sdk::prelude::*;

#[locora_plugin(name = "Wettkampf", permissions = [storage])]
struct Competition;

/// The title of the home tile.
const TITLE: &str = "Gedächtnis-Wettkampf";

/// The plugin's storage as the services use it: the host's store in the app, a map in the tests.
trait Storage {
    /// `storage.list@1`: keys changed after `after_rev`.
    fn list(&mut self, prefix: &str, after_rev: u64) -> Result<ListResponse>;
    /// Writes a key (empty bytes blank it).
    fn set(&mut self, key: &str, value: &[u8]) -> Result;
}

impl Storage for Store {
    fn list(&mut self, prefix: &str, after_rev: u64) -> Result<ListResponse> {
        Store::list(self, prefix, after_rev)
    }

    fn set(&mut self, key: &str, value: &[u8]) -> Result {
        self.set_raw(key, value)
    }
}

/// What the plugin keeps in memory: a mirror of its storage and the facts read from it.
#[derive(Default)]
struct Contest {
    mirror: BTreeMap<String, Vec<u8>>,
    rev: u64,
    /// The epoch of the last request; facts of other epochs were emptied for it.
    epoch: Option<String>,
    /// The facts of `epoch` as read from the mirror; `None` once the mirror changed.
    facts: Option<Facts>,
}

thread_local! {
    static CONTEST: RefCell<Contest> = RefCell::new(Contest::default());
}

fn with<T>(f: impl FnOnce(&mut Contest) -> T) -> T {
    CONTEST.with_borrow_mut(f)
}

/// An epoch from the runtime: `0` or `<n>-<device>`, never anything that could leave its key.
fn check_epoch(epoch: &str) -> Result {
    let fine = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    if epoch.is_empty() || epoch.len() > 64 || !epoch.chars().all(fine) {
        return Err(Error::msg(format!("Ungültige Spiel-Epoche „{epoch}“")));
    }
    Ok(())
}

/// Round and evening ids: 32 lowercase hex characters, as the runtime makes them.
fn check_id(id: &str) -> Result {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::msg(format!("Ungültige Runden-Id „{id}“")));
    }
    Ok(())
}

fn json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(value)?)
}

/// What a service shows on the home tile: a template and its arguments.
type TileText = (&'static str, Vec<String>);

impl Contest {
    /// Takes in what changed in storage since the last look, here or through a sync.
    fn refresh(&mut self, kv: &mut impl Storage) -> Result {
        loop {
            let page = kv.list("", self.rev)?;
            if !page.entries.is_empty() {
                self.facts = None;
            }
            for entry in page.entries {
                if entry.value.is_empty() {
                    self.mirror.remove(&entry.key);
                } else {
                    self.mirror.insert(entry.key, entry.value);
                }
            }
            self.rev = self.rev.max(page.rev);
            if !page.more {
                return Ok(());
            }
        }
    }

    /// Writes a key unless it already holds these bytes (empty bytes blank it).
    fn write(&mut self, kv: &mut impl Storage, key: &str, bytes: Vec<u8>) -> Result<bool> {
        let same = match self.mirror.get(key) {
            Some(current) => *current == bytes,
            None => bytes.is_empty(),
        };
        if same {
            return Ok(false);
        }
        kv.set(key, &bytes)?;
        if bytes.is_empty() {
            self.mirror.remove(key);
        } else {
            self.mirror.insert(key.to_owned(), bytes);
        }
        self.facts = None;
        Ok(true)
    }

    /// Follows the epoch of a request. A new epoch means the game progress was deleted (here or
    /// on another device): rounds of other epochs are emptied on this device too, once. Answers
    /// how many keys were emptied.
    fn adopt_epoch(&mut self, kv: &mut impl Storage, epoch: &str) -> Result<u32> {
        if self.epoch.as_deref() == Some(epoch) {
            return Ok(0);
        }
        let mut emptied = 0;
        if epoch != FIRST_EPOCH {
            let current = format!("g/{epoch}/");
            let old: Vec<String> = self
                .mirror
                .keys()
                .filter(|key| key.starts_with("g/") && !key.starts_with(&current))
                .cloned()
                .collect();
            for key in old {
                emptied += u32::from(self.write(kv, &key, Vec::new())?);
            }
        }
        self.epoch = Some(epoch.to_owned());
        self.facts = None;
        Ok(emptied)
    }

    /// Every service starts here: what the sync brought, then the epoch of the request.
    fn enter(&mut self, kv: &mut impl Storage, epoch: &str) -> Result {
        check_epoch(epoch)?;
        self.refresh(kv)?;
        self.adopt_epoch(kv, epoch)?;
        Ok(())
    }

    /// The facts of the current epoch, read once per change of the mirror.
    fn facts(&mut self) -> &Facts {
        let epoch = self.epoch.clone().unwrap_or_else(|| FIRST_EPOCH.to_owned());
        let mirror = &self.mirror;
        self.facts.get_or_insert_with(|| {
            facts::collect(
                mirror
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_slice())),
                &epoch,
            )
        })
    }

    /// The earned trophies, in the order of [`TROPHIES`].
    fn trophies(&mut self) -> Vec<&'static str> {
        let earned = self.facts().trophies();
        TROPHIES
            .into_iter()
            .filter(|id| earned.contains_key(id))
            .collect()
    }

    fn tile(&mut self, date: Date, now_ms: u64) -> TileText {
        facts::tile(&self.facts().today(date, now_ms))
    }

    fn today(
        &mut self,
        kv: &mut impl Storage,
        request: TodayRequest,
    ) -> Result<(TodayResponse, TileText)> {
        self.enter(kv, &request.epoch)?;
        let today = self.facts().today(request.date, request.now_ms);
        let tile = facts::tile(&today);
        Ok((today, tile))
    }

    /// A round begins. A daily round must be the day's discipline with the day's seed, and a
    /// learning day has one try per level: its start marker is written now. Any other round
    /// writes a sighting: its task is shown now, also if it is never handed in. Both keep
    /// whether the task was new to this device.
    fn begin(
        &mut self,
        kv: &mut impl Storage,
        request: BeginRequest,
    ) -> Result<(BeginResponse, Option<TileText>)> {
        self.enter(kv, &request.epoch)?;
        check_id(&request.round_id)?;
        let spec = Spec::standard(request.discipline, request.level);
        let task = generate(&spec, request.seed)?;
        let epoch = request.epoch.as_str();
        let shown = (request.discipline, request.level, request.seed);
        let first = !self.facts().seen(shown, &request.round_id);
        if request.kind != Kind::Daily {
            let sighting = SightingFact {
                v: VERSION,
                started_at_ms: request.now_ms,
                first: Some(first),
            };
            let key = facts::sighting_key(
                epoch,
                request.discipline,
                request.level,
                request.seed,
                &request.round_id,
            );
            self.write(kv, &key, json(&sighting)?)?;
            return Ok((BeginResponse { spec, task }, None));
        }
        check_daily(
            request.date,
            request.discipline,
            request.level,
            request.seed,
        )?;
        // The same markers as `today@1` reads, so a level shown open can be begun.
        let played = self
            .facts()
            .daily_try(request.date, request.discipline, request.level)
            .is_some();
        if played {
            return Err(Error::msg(
                "Diese Tages-Challenge hast du heute schon gespielt",
            ));
        }
        let marker = MarkerFact {
            v: VERSION,
            kind: facts::DAILY.into(),
            started_at_ms: request.now_ms,
            first: Some(first),
        };
        let key = facts::marker_key(
            epoch,
            request.date,
            request.discipline,
            request.level,
            &request.round_id,
        );
        self.write(kv, &key, json(&marker)?)?;
        let tile = self.tile(request.date, request.now_ms);
        Ok((BeginResponse { spec, task }, Some(tile)))
    }

    /// A round is handed in: scored again from its seed, stored once, and compared with what
    /// the facts held before.
    fn finish(
        &mut self,
        kv: &mut impl Storage,
        request: FinishRequest,
    ) -> Result<(FinishResponse, TileText)> {
        self.enter(kv, &request.epoch)?;
        check_id(&request.round_id)?;
        let epoch = request.epoch.clone();
        let key = facts::round_key(&epoch, &request.round_id);
        if self.mirror.contains_key(&key) {
            return Err(Error::msg("Diese Runde ist schon abgegeben"));
        }
        let spec = Spec::standard(request.discipline, request.level);
        if request.kind == Kind::Daily {
            if request.untimed {
                return Err(Error::msg("Die Tages-Challenge läuft immer mit Zeitlimit"));
            }
            check_daily(
                request.date,
                request.discipline,
                request.level,
                request.seed,
            )?;
            let marker = facts::marker_key(
                &epoch,
                request.date,
                request.discipline,
                request.level,
                &request.round_id,
            );
            if !self.mirror.contains_key(&marker) {
                return Err(Error::msg("Diese Tages-Challenge wurde nicht begonnen"));
            }
        }
        let task = generate(&spec, request.seed)?;
        validate(&spec, &request.recall)?;
        let score = score(&task, &request.recall, request.lang)?;
        // The app caps both phases already; a round with the limits never runs past them here.
        let (memorize_ms, recall_ms) = if request.untimed {
            (request.memorize_ms, request.recall_ms)
        } else {
            (
                request.memorize_ms.min(spec.memorize_ms),
                request
                    .recall_ms
                    .min(spec.recall_ms.saturating_add(RECALL_GRACE_MS)),
            )
        };
        // Whether the task was new when the round began, as its marker or sighting says: a
        // clock set back since then changes nothing.
        let shown = (request.discipline, request.level, request.seed);
        let facts = self.facts();
        let first = facts
            .first_when_shown(shown, &request.round_id)
            .unwrap_or_else(|| !facts.seen(shown, &request.round_id));
        let before = self.trophies();
        let fact = RoundFact {
            v: VERSION,
            id: request.round_id.clone(),
            kind: request.kind,
            discipline: request.discipline,
            level: request.level,
            rules: spec.rules,
            content: spec.content,
            seed: request.seed,
            date: request.date,
            lang: request.lang,
            untimed: request.untimed,
            route: request.route,
            started_at_ms: request.started_at_ms,
            finished_at_ms: request.now_ms,
            memorize_ms,
            recall_ms,
            recall: request.recall.clone(),
            points: score.points,
            max: score.max,
            perfect: score.perfect,
            first: Some(first),
        };
        self.write(kv, &key, json(&fact)?)?;
        let trophies = new_trophies(&before, self.trophies());
        let facts = self.facts();
        let best = facts.best(fact.discipline, fact.level);
        let round = &facts.rounds[&fact.id];
        let finished = FinishResponse {
            record: RecordLine {
                new: best.as_ref().is_some_and(|best| best.round_id == fact.id),
                best,
            },
            trophies,
            hint: facts::hint(&task, &request.recall, &score),
            counted_as_practice: facts.counted_as_practice(round),
            repeat_seed: facts.repeat_seed(round),
            score,
        };
        let tile = self.tile(request.date, request.now_ms);
        Ok((finished, tile))
    }

    /// The daily round of an evening recall.
    fn morning(&mut self, round_id: &str) -> Result<RoundFact> {
        self.facts()
            .rounds
            .get(round_id)
            .filter(|round| round.kind == Kind::Daily)
            .cloned()
            .ok_or_else(|| Error::msg("Zu dieser Runde gibt es keine Abendabfrage"))
    }

    fn evening(
        &mut self,
        kv: &mut impl Storage,
        request: EveningRequest,
    ) -> Result<EveningResponse> {
        self.enter(kv, &request.epoch)?;
        let round = self.morning(&request.round_id)?;
        let (opens_at_ms, closes_at_ms) = facts::evening_window(&round);
        Ok(EveningResponse {
            round_id: round.id.clone(),
            spec: Spec::standard(round.discipline, round.level),
            date: round.date,
            lang: round.lang,
            route: round.route.clone(),
            opens_at_ms,
            closes_at_ms,
            state: self.facts().evening_state(&round, request.now_ms),
        })
    }

    /// An evening recall is handed in while its window is open, once per round on this device;
    /// it is scored against the morning's task.
    fn evening_finish(
        &mut self,
        kv: &mut impl Storage,
        request: EveningFinishRequest,
    ) -> Result<(EveningFinishResponse, TileText)> {
        self.enter(kv, &request.epoch)?;
        check_id(&request.evening_id)?;
        let round = self.morning(&request.round_id)?;
        match self.facts().evening_state(&round, request.now_ms) {
            EveningState::Open => {}
            EveningState::Locked => {
                return Err(Error::msg(
                    "Die Abendabfrage öffnet erst sechs Stunden nach der Runde",
                ));
            }
            EveningState::Closed => {
                return Err(Error::msg(
                    "Die Abendabfrage ist geschlossen: sie bleibt einen Tag nach der Runde offen",
                ));
            }
            EveningState::Done { .. } => {
                return Err(Error::msg("Diese Abendabfrage ist schon abgegeben"));
            }
        }
        let spec = Spec::standard(round.discipline, round.level);
        let task = generate(&spec, round.seed)?;
        validate(&spec, &request.recall)?;
        let score = score(&task, &request.recall, request.lang)?;
        let before = self.trophies();
        let evening = EveningFact {
            v: VERSION,
            round_id: round.id.clone(),
            id: request.evening_id.clone(),
            date: request.date,
            finished_at_ms: request.now_ms,
            recall: request.recall,
            lang: request.lang,
            points: score.points,
            max: score.max,
        };
        let epoch = self.epoch.clone().unwrap_or_else(|| FIRST_EPOCH.to_owned());
        let key = facts::evening_key(&epoch, &round.id, &request.evening_id);
        self.write(kv, &key, json(&evening)?)?;
        let trophies = new_trophies(&before, self.trophies());
        let tile = self.tile(request.date, request.now_ms);
        Ok((
            EveningFinishResponse {
                score,
                morning_points: round.points,
                trophies,
            },
            tile,
        ))
    }

    fn records(
        &mut self,
        kv: &mut impl Storage,
        request: RecordsRequest,
    ) -> Result<RecordsResponse> {
        self.enter(kv, &request.epoch)?;
        Ok(self.facts().records())
    }

    fn history(
        &mut self,
        kv: &mut impl Storage,
        request: HistoryRequest,
    ) -> Result<Vec<HistoryEntry>> {
        self.enter(kv, &request.epoch)?;
        Ok(self.facts().history(request.limit, request.before_ms))
    }

    /// The game progress was deleted: the rounds of every older epoch are emptied, also when
    /// this device had seen the new epoch already.
    fn reset(&mut self, kv: &mut impl Storage, epoch: &str) -> Result<u32> {
        check_epoch(epoch)?;
        self.refresh(kv)?;
        self.epoch = None;
        self.adopt_epoch(kv, epoch)
    }
}

/// A daily round plays the discipline of its date, with the date's seed.
fn check_daily(date: Date, discipline: Discipline, level: Level, seed: Seed) -> Result {
    let planned = schedule::daily(date).discipline;
    if planned != discipline {
        return Err(Error::msg(format!(
            "Die Tages-Challenge am {date} ist {}, nicht {}",
            texts::discipline(Lang::De, planned),
            texts::discipline(Lang::De, discipline)
        )));
    }
    if seed != daily_seed(date, discipline, level) {
        return Err(Error::msg("Das ist nicht die Aufgabe der Tages-Challenge"));
    }
    Ok(())
}

/// Trophies earned now that were not earned before.
fn new_trophies(before: &[&'static str], after: Vec<&'static str>) -> Vec<String> {
    after
        .into_iter()
        .filter(|id| !before.contains(id))
        .map(str::to_owned)
        .collect()
}

/// A tile text the app fills in the current language: a template and its arguments,
/// separated by U+001F (see `plugin_text` in the app).
fn localized((template, args): &TileText) -> String {
    args.iter().fold(format!("@@{template}"), |text, arg| {
        format!("{text}\u{1f}{arg}")
    })
}

fn show(ctx: &mut Ctx, tile: &TileText) -> Result {
    ctx.tile(TITLE, localized(tile))
}

impl Plugin for Competition {
    /// No storage is read here: on the web, storage is connected after the plugins start.
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.provide::<Today>(|ctx, request| {
            let (today, tile) = with(|contest| contest.today(&mut ctx.store(), request))?;
            show(ctx, &tile)?;
            Ok(today)
        })?;
        ctx.provide::<Begin>(|ctx, request| {
            let (began, tile) = with(|contest| contest.begin(&mut ctx.store(), request))?;
            if let Some(tile) = tile {
                show(ctx, &tile)?;
            }
            Ok(began)
        })?;
        ctx.provide::<Finish>(|ctx, request| {
            let (finished, tile) = with(|contest| contest.finish(&mut ctx.store(), request))?;
            show(ctx, &tile)?;
            Ok(finished)
        })?;
        ctx.provide::<Evening>(|ctx, request| {
            with(|contest| contest.evening(&mut ctx.store(), request))
        })?;
        ctx.provide::<EveningFinish>(|ctx, request| {
            let (finished, tile) =
                with(|contest| contest.evening_finish(&mut ctx.store(), request))?;
            show(ctx, &tile)?;
            Ok(finished)
        })?;
        ctx.provide::<Records>(|ctx, request| {
            with(|contest| contest.records(&mut ctx.store(), request))
        })?;
        ctx.provide::<History>(|ctx, request| {
            with(|contest| contest.history(&mut ctx.store(), request))
        })?;
        ctx.provide::<Reset>(|ctx, request| {
            with(|contest| contest.reset(&mut ctx.store(), &request.epoch))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_competition::{Recall, Task};
    use locora_sdk::api::competition::{Counted, LevelState};
    use locora_sdk::api::storage::ListEntry;

    /// Plugin storage as the host keeps it: every write gets the next revision, and a list
    /// answers what changed after one, `page` entries at a time.
    struct Memory {
        keys: BTreeMap<String, (u64, Vec<u8>)>,
        rev: u64,
        page: usize,
        lists: usize,
    }

    impl Memory {
        fn new(page: usize) -> Self {
            Memory {
                keys: BTreeMap::new(),
                rev: 0,
                page,
                lists: 0,
            }
        }

        fn value(&self, key: &str) -> Option<&[u8]> {
            self.keys.get(key).map(|(_, value)| value.as_slice())
        }

        /// What a sync brings: the other store's keys that differ here, as new writes.
        fn merge(&mut self, other: &Memory) {
            for (key, (_, value)) in &other.keys {
                if self.value(key) != Some(value.as_slice()) {
                    self.set(key, value).unwrap();
                }
            }
        }
    }

    impl Storage for Memory {
        fn list(&mut self, prefix: &str, after_rev: u64) -> Result<ListResponse> {
            self.lists += 1;
            let mut changed: Vec<(&String, &(u64, Vec<u8>))> = self
                .keys
                .iter()
                .filter(|(key, (rev, _))| key.starts_with(prefix) && *rev > after_rev)
                .collect();
            changed.sort_by_key(|(_, (rev, _))| *rev);
            let more = changed.len() > self.page;
            changed.truncate(self.page);
            Ok(ListResponse {
                rev: changed.last().map_or(after_rev, |(_, (rev, _))| *rev),
                entries: changed
                    .into_iter()
                    .map(|(key, (_, value))| ListEntry {
                        key: key.clone(),
                        value: value.clone(),
                    })
                    .collect(),
                more,
            })
        }

        fn set(&mut self, key: &str, value: &[u8]) -> Result {
            self.rev += 1;
            self.keys.insert(key.to_owned(), (self.rev, value.to_vec()));
            Ok(())
        }
    }

    const H: u64 = 3_600_000;

    /// A Wednesday: cards.
    fn wednesday() -> Date {
        Date::parse("2026-10-28").unwrap()
    }

    fn id(n: u32) -> String {
        format!("{n:032x}")
    }

    /// A task to play: the kind, the discipline, the level and the seed.
    #[derive(Clone, Copy)]
    struct Play {
        kind: Kind,
        discipline: Discipline,
        level: Level,
        seed: Seed,
    }

    impl Play {
        fn new(kind: Kind, discipline: Discipline, level: u8, seed: u64) -> Self {
            Play {
                kind,
                discipline,
                level: Level::new(level).unwrap(),
                seed: Seed::new(seed).unwrap(),
            }
        }

        /// Wednesday's daily challenge at a level.
        fn daily(level: u8) -> Self {
            let level = Level::new(level).unwrap();
            Play {
                kind: Kind::Daily,
                discipline: Discipline::Cards,
                level,
                seed: daily_seed(wednesday(), Discipline::Cards, level),
            }
        }

        fn begin(
            self,
            contest: &mut Contest,
            kv: &mut Memory,
            round: &str,
            now_ms: u64,
        ) -> Result<BeginResponse> {
            let request = BeginRequest {
                epoch: "0".into(),
                round_id: round.into(),
                kind: self.kind,
                discipline: self.discipline,
                level: self.level,
                seed: self.seed,
                date: wednesday(),
                now_ms,
            };
            contest.begin(kv, request).map(|(began, _)| began)
        }

        /// Handed in at `now_ms`, begun a quarter of an hour before.
        fn hand_in(self, round: &str, recall: Recall, now_ms: u64) -> FinishRequest {
            FinishRequest {
                epoch: "0".into(),
                round_id: round.into(),
                kind: self.kind,
                discipline: self.discipline,
                level: self.level,
                seed: self.seed,
                date: wednesday(),
                lang: Lang::De,
                untimed: false,
                route: None,
                started_at_ms: now_ms.saturating_sub(H / 4),
                memorize_ms: 61_000,
                recall_ms: 99_000,
                recall,
                now_ms,
            }
        }

        fn blank(self) -> Recall {
            Recall::blank(
                self.discipline,
                Spec::standard(self.discipline, self.level).items,
            )
        }
    }

    /// The recall that gets every place right.
    fn perfect(task: &Task, lang: Lang) -> Recall {
        match task {
            Task::List(items) => Recall::List(
                items
                    .iter()
                    .map(|id| {
                        locora_competition::words::display(*id, lang)
                            .unwrap()
                            .to_owned()
                    })
                    .collect(),
            ),
            Task::Numbers(digits) => Recall::Numbers(digits.iter().copied().map(Some).collect()),
            Task::Cards(cards) => Recall::Cards(cards.iter().copied().map(Some).collect()),
        }
    }

    fn today(contest: &mut Contest, kv: &mut Memory, epoch: &str, now_ms: u64) -> TodayResponse {
        let request = TodayRequest {
            epoch: epoch.into(),
            date: wednesday(),
            now_ms,
        };
        contest.today(kv, request).unwrap().0
    }

    fn history(contest: &mut Contest, kv: &mut Memory, epoch: &str) -> Vec<HistoryEntry> {
        let request = HistoryRequest {
            epoch: epoch.into(),
            limit: 0,
            before_ms: None,
        };
        contest.history(kv, request).unwrap()
    }

    fn records(contest: &mut Contest, kv: &mut Memory) -> RecordsResponse {
        let request = RecordsRequest { epoch: "0".into() };
        contest.records(kv, request).unwrap()
    }

    fn stored(kv: &Memory, epoch: &str, round: &str) -> RoundFact {
        serde_json::from_slice(kv.value(&facts::round_key(epoch, round)).unwrap()).unwrap()
    }

    #[test]
    fn a_round_of_each_kind_begins_and_is_handed_in() {
        let mut contest = Contest::default();
        let mut kv = Memory::new(5000);
        // The first round earns „compete-first“; a perfect 20 digits is no „compete-numbers-40“;
        // the third discipline perfect earns „compete-triple“.
        let plays = [
            (
                Play::new(Kind::Practice, Discipline::List, 1, 7),
                &["compete-first"][..],
            ),
            (
                Play::new(Kind::Code, Discipline::Numbers, 1, 0x123_4567_89ab_cdef),
                &[],
            ),
            (Play::daily(1), &["compete-triple"]),
        ];
        for (n, (play, trophies)) in plays.into_iter().enumerate() {
            let round = id(n as u32 + 1);
            let began = play.begin(&mut contest, &mut kv, &round, H).unwrap();
            assert_eq!(began.spec, Spec::standard(play.discipline, play.level));
            assert_eq!(began.task.len(), usize::from(began.spec.items));
            let recall = perfect(&began.task, Lang::De);
            let (finished, tile) = contest
                .finish(&mut kv, play.hand_in(&round, recall, 2 * H))
                .unwrap();
            assert!(finished.score.perfect, "{:?}", play.kind);
            assert!(finished.record.new);
            assert!(!finished.counted_as_practice && !finished.repeat_seed);
            assert_eq!(finished.hint, None);
            assert_eq!(finished.trophies, trophies, "{:?}", play.kind);
            let fact = stored(&kv, "0", &round);
            assert_eq!(
                (fact.kind, fact.points, fact.memorize_ms),
                (play.kind, finished.score.max, 61_000)
            );
            // A daily writes a start marker, any other round a sighting; the tile shows today's
            // daily.
            let markers = kv.keys.keys().filter(|k| k.starts_with("g/0/s/")).count();
            assert_eq!(markers, usize::from(play.kind == Kind::Daily));
            let sightings = kv.keys.keys().filter(|k| k.starts_with("g/0/t/")).count();
            assert_eq!(sightings, n + 1 - markers);
            let expected = if play.kind == Kind::Daily {
                "Heute: Karten – {0}/{1}"
            } else {
                "Heute: Karten – noch nicht gespielt"
            };
            assert_eq!(tile.0, expected);
        }
        // Today's daily task again as practice: a repeat, but never „counted as practice“, which
        // only a second daily round is.
        let again = Play {
            kind: Kind::Practice,
            ..Play::daily(1)
        };
        let task = again
            .begin(&mut contest, &mut kv, &id(4), 3 * H)
            .unwrap()
            .task;
        let request = again.hand_in(&id(4), perfect(&task, Lang::De), 4 * H);
        let (finished, _) = contest.finish(&mut kv, request).unwrap();
        assert!(finished.repeat_seed && !finished.counted_as_practice);
        let entries = history(&mut contest, &mut kv, "0");
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].round_id, id(4));
        assert!(entries.iter().all(|e| !e.counted_as_practice));
        // A round is handed in once.
        let practice = Play::new(Kind::Practice, Discipline::List, 1, 7);
        let again = practice.hand_in(&id(1), practice.blank(), 3 * H);
        assert_eq!(
            contest.finish(&mut kv, again).unwrap_err().to_string(),
            "Diese Runde ist schon abgegeben"
        );
    }

    #[test]
    fn a_daily_is_one_try_per_level_and_learning_day() {
        let mut contest = Contest::default();
        let mut kv = Memory::new(5000);
        let level_2 = Play::daily(2);
        level_2.begin(&mut contest, &mut kv, &id(1), H).unwrap();
        let again = level_2.begin(&mut contest, &mut kv, &id(2), 2 * H);
        assert_eq!(
            again.unwrap_err().to_string(),
            "Diese Tages-Challenge hast du heute schon gespielt"
        );
        // Another level is a try of its own; another discipline or seed is not this date's daily.
        Play::daily(1)
            .begin(&mut contest, &mut kv, &id(3), H)
            .unwrap();
        let list = Play {
            discipline: Discipline::List,
            seed: daily_seed(wednesday(), Discipline::List, Level::new(1).unwrap()),
            ..Play::daily(1)
        };
        let error = list.begin(&mut contest, &mut kv, &id(4), H).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Die Tages-Challenge am 2026-10-28 ist Karten, nicht Einkaufsliste"
        );
        let other_seed = Play {
            seed: Play::daily(1).seed,
            ..Play::daily(3)
        };
        assert!(other_seed.begin(&mut contest, &mut kv, &id(5), H).is_err());
        // The same task as practice is fine (an earlier day, or after the daily).
        let practice = Play {
            kind: Kind::Practice,
            ..level_2
        };
        practice.begin(&mut contest, &mut kv, &id(6), H).unwrap();
        // A daily that was never begun cannot be handed in, nor one without the time limit.
        let level_3 = Play::daily(3);
        let unknown = level_3.hand_in(&id(7), level_3.blank(), H);
        assert!(contest.finish(&mut kv, unknown).is_err());
        let mut untimed = level_2.hand_in(&id(1), level_2.blank(), H);
        untimed.untimed = true;
        assert!(contest.finish(&mut kv, untimed).is_err());
        // While it runs it is started; after its time it stays „not finished“.
        let state = |contest: &mut Contest, kv: &mut Memory, now_ms| {
            today(contest, kv, "0", now_ms).levels[1].state.clone()
        };
        assert_eq!(
            state(&mut contest, &mut kv, H + 60_000),
            LevelState::Started { round_id: id(1) }
        );
        assert_eq!(
            state(&mut contest, &mut kv, H + 13 * 60_000),
            LevelState::Unfinished { round_id: id(1) }
        );
    }

    #[test]
    fn a_level_shown_open_can_be_begun_and_any_start_marker_holds_one() {
        let mut contest = Contest::default();
        let mut kv = Memory::new(5000);
        let key = |n, round| {
            let level = Level::new(n).unwrap();
            facts::marker_key("0", wednesday(), Discipline::Cards, level, &id(round))
        };
        // A marker of a newer kind (Locora Cloud's daily) and one that does not read.
        let cloud = br#"{"v":2,"kind":"cloud_daily","started_at_ms":3600000}"#;
        kv.set(&key(1, 8), cloud).unwrap();
        kv.set(&key(2, 9), b"garbage").unwrap();
        let levels = today(&mut contest, &mut kv, "0", 2 * H).levels;
        assert_eq!(levels[0].state, LevelState::Unfinished { round_id: id(8) });
        assert_eq!(levels[1].state, LevelState::Unfinished { round_id: id(9) });
        assert_eq!(levels[2].state, LevelState::Open);
        for (n, round) in [(1, 1), (2, 2)] {
            let error = Play::daily(n)
                .begin(&mut contest, &mut kv, &id(round), 2 * H)
                .unwrap_err();
            assert_eq!(
                error.to_string(),
                "Diese Tages-Challenge hast du heute schon gespielt"
            );
        }
        Play::daily(3)
            .begin(&mut contest, &mut kv, &id(3), 2 * H)
            .unwrap();
    }

    #[test]
    fn the_earliest_marker_wins_and_the_other_device_counts_as_practice() {
        let (mut a, mut b) = (Contest::default(), Contest::default());
        let (mut kv_a, mut kv_b) = (Memory::new(5000), Memory::new(5000));
        // Offline, both play level 1: A began first (with the greater id), B scores more and
        // hands in later.
        let play = Play::daily(1);
        let task = play.begin(&mut a, &mut kv_a, &id(2), H).unwrap().task;
        play.begin(&mut b, &mut kv_b, &id(1), H + 1).unwrap();
        let mut weak = perfect(&task, Lang::De);
        if let Recall::Cards(cards) = &mut weak {
            cards[0] = None;
        }
        let at = H + H / 4;
        let (on_a, _) = a.finish(&mut kv_a, play.hand_in(&id(2), weak, at)).unwrap();
        let strong = perfect(&task, Lang::De);
        let (on_b, _) = b
            .finish(&mut kv_b, play.hand_in(&id(1), strong, at + 1))
            .unwrap();
        assert!(
            !on_a.counted_as_practice && !on_b.counted_as_practice,
            "neither knows yet"
        );
        assert_eq!((on_a.score.points, on_b.score.points), (12, 13));
        kv_a.merge(&kv_b);
        kv_b.merge(&kv_a);
        for (contest, kv) in [(&mut a, &mut kv_a), (&mut b, &mut kv_b)] {
            let today = today(contest, kv, "0", 2 * H);
            assert_eq!(
                today.levels[0].state,
                LevelState::Done {
                    round_id: id(2),
                    points: 12,
                    max: 13,
                    memorize_ms: 61_000,
                    counted: Counted::Daily,
                }
            );
            let practice: Vec<bool> = history(contest, kv, "0")
                .iter()
                .map(|e| e.counted_as_practice)
                .collect();
            assert_eq!(practice, [true, false], "B's round, then A's");
            let records = records(contest, kv);
            let cards_1 = &records.levels[6];
            let best = cards_1.best.clone().unwrap();
            assert_eq!(
                (cards_1.discipline, best.round_id, best.points),
                (Discipline::Cards, id(2), 12),
                "B saw the task second"
            );
            assert_eq!(records.daily_days, 1);
        }
    }

    #[test]
    fn a_task_counts_for_records_once_also_when_left_or_replayed_with_the_clock_set_back() {
        let mut contest = Contest::default();
        let mut kv = Memory::new(5000);
        let code = Play::new(Kind::Code, Discipline::Cards, 2, 0x123_4567_89ab_cdef);
        // Looked at and left without a result; played again with the answers seen.
        let task = code
            .begin(&mut contest, &mut kv, &id(1), 2 * H)
            .unwrap()
            .task;
        code.begin(&mut contest, &mut kv, &id(2), 3 * H).unwrap();
        let replay = code.hand_in(&id(2), perfect(&task, Lang::De), 3 * H + H / 4);
        let (finished, _) = contest.finish(&mut kv, replay).unwrap();
        assert!(finished.repeat_seed && !finished.record.new);
        assert_eq!(finished.record.best, None);
        assert_eq!(stored(&kv, "0", &id(2)).first, Some(false));
        // Played honestly first, then again with the clock an hour earlier.
        let other = Play::new(Kind::Code, Discipline::Cards, 2, 0x42);
        let task = other
            .begin(&mut contest, &mut kv, &id(3), 4 * H)
            .unwrap()
            .task;
        let honest = other.hand_in(&id(3), other.blank(), 4 * H + H / 4);
        let (finished, _) = contest.finish(&mut kv, honest).unwrap();
        assert!(finished.record.new && !finished.repeat_seed);
        other.begin(&mut contest, &mut kv, &id(4), 3 * H).unwrap();
        let back = other.hand_in(&id(4), perfect(&task, Lang::De), 3 * H + H / 4);
        let (finished, _) = contest.finish(&mut kv, back).unwrap();
        assert!(finished.repeat_seed && !finished.record.new);
        let best = finished.record.best.unwrap();
        assert_eq!((best.round_id, best.points), (id(3), 0));
        // Nor does looking at it again with the clock set back take the record away.
        other.begin(&mut contest, &mut kv, &id(5), H).unwrap();
        let cards_2 = &records(&mut contest, &mut kv).levels[7];
        assert_eq!(cards_2.best.as_ref().unwrap().round_id, id(3));
        assert_eq!(cards_2.rounds, 3);
    }

    #[test]
    fn the_evening_recall_opens_after_six_hours_and_is_handed_in_once() {
        let mut contest = Contest::default();
        let mut kv = Memory::new(5000);
        let play = Play::daily(1);
        let task = play.begin(&mut contest, &mut kv, &id(1), H).unwrap().task;
        let morning = play.hand_in(&id(1), perfect(&task, Lang::De), 2 * H);
        contest.finish(&mut kv, morning).unwrap();
        let mut evening = |contest: &mut Contest, n, now_ms| {
            let request = EveningFinishRequest {
                epoch: "0".into(),
                round_id: id(1),
                evening_id: id(n),
                date: wednesday(),
                recall: perfect(&task, Lang::En),
                lang: Lang::En,
                now_ms,
            };
            contest.evening_finish(&mut kv, request)
        };
        assert!(evening(&mut contest, 2, 8 * H - 1_000).is_err(), "5:59:59");
        assert!(evening(&mut contest, 2, 26 * H).is_err(), "24:00");
        let (kept, _) = evening(&mut contest, 2, 8 * H).unwrap();
        assert_eq!((kept.score.points, kept.morning_points), (13, 13));
        assert_eq!(kept.trophies, ["compete-evening"], "all of 10 or more kept");
        let error = evening(&mut contest, 3, 9 * H).unwrap_err();
        assert_eq!(error.to_string(), "Diese Abendabfrage ist schon abgegeben");
        let request = EveningRequest {
            epoch: "0".into(),
            round_id: id(1),
            now_ms: 8 * H - 1_000,
        };
        let view = contest.evening(&mut kv, request).unwrap();
        assert_eq!((view.opens_at_ms, view.closes_at_ms), (8 * H, 26 * H));
        assert_eq!((view.spec.items, view.lang), (13, Lang::De));
        assert_eq!(
            view.state,
            EveningState::Done {
                points: 13,
                max: 13
            }
        );
        let today = today(&mut contest, &mut kv, "0", 9 * H);
        assert_eq!(
            today.evenings[0].state,
            EveningState::Done {
                points: 13,
                max: 13
            }
        );
        // A practice round has no evening recall.
        let practice = Play::new(Kind::Practice, Discipline::Cards, 1, 3);
        contest
            .finish(&mut kv, practice.hand_in(&id(4), practice.blank(), H))
            .unwrap();
        let request = EveningRequest {
            epoch: "0".into(),
            round_id: id(4),
            now_ms: 9 * H,
        };
        assert!(contest.evening(&mut kv, request).is_err());
    }

    #[test]
    fn trophies_come_once_and_untimed_practice_earns_none() {
        let mut contest = Contest::default();
        let mut kv = Memory::new(5000);
        let deck = |seed| Play::new(Kind::Practice, Discipline::Cards, 3, seed);
        let task = deck(99)
            .begin(&mut contest, &mut kv, &id(1), H)
            .unwrap()
            .task;
        let mut untimed = deck(99).hand_in(&id(1), perfect(&task, Lang::De), 2 * H);
        untimed.untimed = true;
        untimed.memorize_ms = 9_000_000;
        let (finished, _) = contest.finish(&mut kv, untimed).unwrap();
        assert!(finished.score.perfect && finished.trophies.is_empty());
        assert!(!finished.record.new && finished.record.best.is_none());
        assert_eq!(
            stored(&kv, "0", &id(1)).memorize_ms,
            9_000_000,
            "untimed keeps its time"
        );
        let task = deck(100)
            .begin(&mut contest, &mut kv, &id(2), H)
            .unwrap()
            .task;
        let first = deck(100).hand_in(&id(2), perfect(&task, Lang::De), 3 * H);
        let (finished, _) = contest.finish(&mut kv, first).unwrap();
        assert_eq!(finished.trophies, ["compete-first", "compete-deck"]);
        let task = deck(101)
            .begin(&mut contest, &mut kv, &id(3), H)
            .unwrap()
            .task;
        let second = deck(101).hand_in(&id(3), perfect(&task, Lang::De), 4 * H);
        let (finished, _) = contest.finish(&mut kv, second).unwrap();
        assert!(finished.trophies.is_empty(), "earned already");
        assert!(!finished.record.new, "as good, but later");
        let unlocked: Vec<String> = records(&mut contest, &mut kv)
            .trophies
            .into_iter()
            .filter(|t| t.unlocked == Some(wednesday()))
            .map(|t| t.id)
            .collect();
        assert_eq!(unlocked, ["compete-first", "compete-deck"]);
        // A round with the time limit never stores more than its limits.
        let small = Play::new(Kind::Practice, Discipline::Cards, 1, 101);
        let mut slow = small.hand_in(&id(9), small.blank(), 5 * H);
        slow.memorize_ms = 9_000_000;
        slow.recall_ms = 9_000_000;
        contest.finish(&mut kv, slow).unwrap();
        let fact = stored(&kv, "0", &id(9));
        assert_eq!((fact.memorize_ms, fact.recall_ms), (180_000, 310_000));
    }

    /// A practice round handed in by another device (its own plugin, the same storage).
    fn played_elsewhere(kv: &mut Memory, epoch: &str, round: &str) {
        let play = Play::new(Kind::Practice, Discipline::Cards, 1, 5);
        let mut request = play.hand_in(round, play.blank(), H);
        request.epoch = epoch.into();
        Contest::default().finish(kv, request).unwrap();
    }

    #[test]
    fn a_reset_empties_older_epochs_and_late_old_facts_stay_ignored() {
        let mut contest = Contest::default();
        let mut kv = Memory::new(5000);
        for n in 1..=2 {
            played_elsewhere(&mut kv, "0", &id(n));
        }
        assert_eq!(history(&mut contest, &mut kv, "0").len(), 2);
        assert_eq!(contest.reset(&mut kv, "1-abc").unwrap(), 2);
        assert!(kv.keys.values().all(|(_, value)| value.is_empty()));
        assert!(history(&mut contest, &mut kv, "1-abc").is_empty());
        // A device that was offline sends a round of the old epoch later: it stays out.
        let mut offline = Memory::new(5000);
        played_elsewhere(&mut offline, "0", &id(3));
        kv.merge(&offline);
        assert!(history(&mut contest, &mut kv, "1-abc").is_empty());
        // Rounds of the new epoch count.
        played_elsewhere(&mut kv, "1-abc", &id(4));
        assert_eq!(history(&mut contest, &mut kv, "1-abc").len(), 1);
        // The offline device hears of the new epoch from a request and empties its old rounds.
        let mut there = Contest::default();
        assert!(history(&mut there, &mut offline, "1-abc").is_empty());
        assert_eq!(offline.value(&facts::round_key("0", &id(3))), Some(&[][..]));
        // An epoch comes only from a request, and only a well-formed one.
        assert!(contest.reset(&mut kv, "bad/epoch").is_err());
        assert!(
            contest
                .records(
                    &mut kv,
                    RecordsRequest {
                        epoch: String::new()
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn the_mirror_follows_every_page_of_a_long_list() {
        let mut contest = Contest::default();
        let mut kv = Memory::new(3);
        for n in 1..=7 {
            played_elsewhere(&mut kv, "0", &id(n));
        }
        let before = kv.lists;
        assert_eq!(history(&mut contest, &mut kv, "0").len(), 7);
        assert_eq!(kv.lists - before, 3, "pages of 3, 3 and 1");
        // Nothing new: one look, and the facts come from memory.
        let before = kv.lists;
        records(&mut contest, &mut kv);
        assert_eq!(kv.lists - before, 1);
    }

    #[test]
    fn requests_are_checked_before_anything_is_written() {
        let mut contest = Contest::default();
        let mut kv = Memory::new(5000);
        let play = Play::new(Kind::Practice, Discipline::Cards, 1, 5);
        assert!(play.begin(&mut contest, &mut kv, "short", H).is_err());
        let mut request = play.hand_in(&id(1), Recall::Cards(vec![Some(52); 13]), H);
        assert!(
            contest.finish(&mut kv, request.clone()).is_err(),
            "no card 52"
        );
        request.recall = Recall::List(vec![String::new(); 13]);
        assert!(
            contest.finish(&mut kv, request.clone()).is_err(),
            "another discipline"
        );
        request.recall = Recall::Cards(vec![None; 12]);
        assert!(contest.finish(&mut kv, request).is_err(), "one place short");
        assert!(kv.keys.is_empty());
    }
}
