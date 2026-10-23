//! A journey in the app: the same plan, questions and grading as
//! `locora practice`/`review`, one question at a time over IPC.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use locora_api::core::{Grade, JourneyCompleted, JourneyStarted};
use locora_api::journey::{Answer, AnswerKind as Kind, Submission};
use locora_app_api::{
    ActionDone, AnswerKind, Feedback, GameView, GamificationStatus, InputField, JourneyStart,
    Media, NearbyLocus, Question, Reply, Revealed, Step, Summary, VideoPreviewItem,
};
use locora_formula::workbook::{ActionOutcome, Run};
use locora_runtime::game::{app_medal, unlock_app_medal};
use locora_runtime::journey::{PlanOptions, Stop, plan, resolve_text};
use locora_runtime::settings::DAY;
use locora_runtime::{AnswerRequest, Profile, now};
use locora_store::JourneyAnswer;
use locora_store::{Locus, PalaceTree};

use crate::views::{marker, media};

/// Storage plugin name for achievements (kept apart from palaces and their removal).
pub const ACHIEVEMENTS: &str = "achievements";

/// The built-in achievements: finishing the tutorial or the formula guide unlocks one.
const ACHIEVEMENT_IDS: [&str; 2] = ["tutorial-done", "formulas-done"];

/// The achievement a finished palace unlocks, if it is one of the built-in palaces (in any language).
pub fn built_in_achievement(palace_id: &str) -> Option<&'static str> {
    match locora_runtime::built_in_kind(palace_id)? {
        "tutorial" => Some("tutorial-done"),
        "formulas" => Some("formulas-done"),
        _ => None,
    }
}

/// Finishing the app tour, a lesson of „Mnemotechnik lernen“ or all of them, and walking the
/// route of a first own palace unlock these.
const MORE_ACHIEVEMENT_IDS: [&str; 3] = ["tour-done", "learn-all", "first-palace"];

/// The journey trophies of the gamification plugin, in the order they are shown.
const GAME_TROPHIES: [&str; 6] = [
    "first-journey",
    "hundred-xp",
    "seven-day-streak",
    "due-cleared",
    "rhythm-4",
    "perfect-journey",
];

/// The built-in achievements and whether each is unlocked: the built-in palaces, the tour, the
/// lessons (`learn-<lesson>`, in order), all lessons and the first own palace. `learn-all`
/// follows from the lessons, so it is unlocked on every device as soon as they are. With the
/// gamification plugin, the journey trophies (`game-<id>`) and the palace medals
/// (`palace-<level>`, one per palace and level) follow. Only medals of the current game epoch
/// count: deleting the game progress locks them all.
pub fn achievements(profile: &mut Profile) -> Result<Vec<locora_app_api::AchievementView>> {
    let epoch = profile.game_epoch()?.id;
    let lessons: Vec<String> = locora_app_api::LEARN_LESSONS
        .iter()
        .map(|lesson| format!("learn-{lesson}"))
        .collect();
    let mut views = Vec::new();
    for id in ACHIEVEMENT_IDS
        .iter()
        .chain(&MORE_ACHIEVEMENT_IDS)
        .map(|id| id.to_string())
        .chain(lessons.iter().cloned())
    {
        let earned = app_medal(&profile.store, &id, &epoch)?;
        views.push(locora_app_api::AchievementView {
            id,
            unlocked: earned.is_some(),
            day: earned.flatten(),
            palace: None,
            progress: None,
        });
    }
    let all = views
        .iter()
        .filter(|view| lessons.contains(&view.id))
        .all(|view| view.unlocked);
    if let Some(view) = views.iter_mut().find(|view| view.id == "learn-all") {
        view.unlocked |= all;
    }
    if let Some(status) = profile.gamification_status(true)? {
        views.extend(game_achievements(profile, &status)?);
    }
    Ok(views)
}

/// The journey trophies and palace medals of a status as achievements.
fn game_achievements(
    profile: &Profile,
    status: &GamificationStatus,
) -> Result<Vec<locora_app_api::AchievementView>> {
    let mut views = Vec::new();
    for id in GAME_TROPHIES {
        let earned = status.trophies.iter().find(|t| t.id == id);
        // A flawless journey can no longer be earned: it is shown only where it was.
        if earned.is_none() && !status.locked.iter().any(|locked| locked == id) {
            continue;
        }
        views.push(locora_app_api::AchievementView {
            id: format!("game-{id}"),
            unlocked: earned.is_some(),
            day: earned.and_then(|t| t.day.clone()),
            palace: None,
            progress: None,
        });
    }
    let names: HashMap<String, String> = profile
        .store
        .palaces()?
        .into_iter()
        .map(|p| (p.id, p.name))
        .collect();
    for medal in &status.palace_medals {
        let Some(name) = names.get(&medal.palace_id) else {
            continue;
        };
        views.push(locora_app_api::AchievementView {
            id: format!("palace-{}", medal.level),
            unlocked: medal.unlocked.is_some(),
            day: medal.unlocked.clone(),
            palace: Some(name.clone()),
            progress: Some((medal.hold, medal.loci)),
        });
    }
    Ok(views)
}

/// What a status counts as earned: trophies (`game-<id>`) and palace medals
/// (`palace-<level>@<palace id>`), to see what a journey added.
fn earned(status: &GamificationStatus) -> BTreeSet<String> {
    status
        .trophies
        .iter()
        .map(|t| format!("game-{}", t.id))
        .chain(
            status
                .palace_medals
                .iter()
                .filter(|m| m.unlocked.is_some())
                .map(|m| format!("palace-{}@{}", m.level, m.palace_id)),
        )
        .collect()
}

/// Unlock an achievement the UI earned (a lesson or the finished tour); other ids are refused,
/// so the UI cannot hand out the rest. Returns whether it was new.
pub fn unlock(profile: &Profile, id: &str) -> Result<bool> {
    anyhow::ensure!(
        locora_app_api::unlockable_by_ui(id),
        "Unbekannter Erfolg: {id}"
    );
    unlock_medal(profile, id)
}

/// Unlocks an app medal in the current game epoch, on today's learning day.
fn unlock_medal(profile: &Profile, id: &str) -> Result<bool> {
    let epoch = profile.game_epoch()?.id;
    let (_, day) = profile.learning_day()?;
    unlock_app_medal(&profile.store, id, &epoch, &day)
}

/// The achievement a finished journey through `palace_id` unlocks: the built-in palaces their
/// own, a palace of the person's own (not Simo's example from the tour) `first-palace`.
fn finished_palace_achievement(palace_id: &str) -> Option<&'static str> {
    built_in_achievement(palace_id).or_else(|| {
        let demo = locora_source::bundled_palace_id(locora_runtime::tour_demo::NAMESPACE);
        (palace_id != demo).then_some("first-palace")
    })
}

pub struct Journey {
    trees: Vec<PalaceTree>,
    runs: Vec<Run>,
    stops: Vec<Stop>,
    options: JourneyStart,
    seed: u64,
    at: usize,
    /// Quiz options shown for the current question (indices into `options`).
    shown: Vec<u32>,
    /// The same for the question answered last (actions after grading).
    answered_shown: Vec<u32>,
    /// `finish` ran: completed palaces are reported only once.
    finished: Option<Summary>,
    asked: Instant,
    results: Vec<(String, bool)>,
    /// Start of the journey; answers are timed from here for rankings.
    started: Instant,
    /// Graded answers per palace, stored when the palace is completed.
    transcripts: HashMap<String, Vec<JourneyAnswer>>,
    points: u64,
    /// Time per palace, only at its own loci.
    spent: HashMap<String, Duration>,
    notes: Vec<String>,
    /// XP and earned trophies when the journey began (with the gamification plugin).
    game_start: Option<(u64, BTreeSet<String>)>,
}

/// The palaces of a journey and one workbook run per palace.
fn load(profile: &Profile, options: &JourneyStart) -> Result<(Vec<PalaceTree>, Vec<Run>)> {
    let trees: Vec<PalaceTree> = match &options.palace_id {
        Some(id) => vec![profile.store.palace(id)?],
        None => profile
            .store
            .palaces()?
            .into_iter()
            .map(|p| profile.store.palace(&p.id))
            .collect::<Result<_, _>>()?,
    };
    let runs = trees
        .iter()
        .map(|t| {
            Run::new(profile.store.effective_rows(&t.palace.id)?)
                .with_context(|| format!("Tabellen von „{}“", t.palace.name))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((trees, runs))
}

/// Workbook inputs the journey asks for before it starts.
pub fn inputs(profile: &Profile, options: &JourneyStart) -> Result<Vec<InputField>> {
    let (trees, runs) = load(profile, options)?;
    let mut fields = Vec::new();
    for (tree, run) in trees.iter().zip(&runs) {
        let specs = run
            .inputs()
            .with_context(|| format!("Eingaben von „{}“", tree.palace.name))?;
        fields.extend(specs.into_iter().map(|spec| InputField {
            palace_id: tree.palace.id.clone(),
            palace: tree.palace.name.clone(),
            key: spec.key,
            kind: format!("{:?}", spec.kind).to_lowercase(),
            default: spec.default.to_string(),
        }));
    }
    Ok(fields)
}

impl Journey {
    /// Plan a journey; `None` when there is nothing to ask.
    pub fn start(profile: &mut Profile, options: JourneyStart) -> Result<Option<Self>> {
        if !profile
            .host
            .provides(<locora_api::journey::Evaluate as locora_api::Service>::NAME)
        {
            bail!("Das Core-Plugin fehlt – ohne es kann Locora keine Antworten prüfen.");
        }
        let (trees, mut runs) = load(profile, &options)?;
        // Inputs come first: `active` formulas may depend on them.
        for input in &options.inputs {
            let index = trees
                .iter()
                .position(|t| t.palace.id == input.palace_id)
                .context("Eingabe für einen Palast außerhalb dieser Journey")?;
            runs[index]
                .set_input(&input.key, &input.value)
                .with_context(|| format!("Eingabe {}", input.key))?;
        }
        let cards = profile.schedule(options.palace_id.as_deref())?;
        let new = if options.due_only {
            profile.new_budget()?
        } else {
            0
        };
        let seed = now();
        let planned = plan(
            &trees,
            &runs,
            &cards,
            seed,
            &PlanOptions {
                due_only: options.due_only,
                new,
                shuffle: options.shuffle,
                seed,
            },
        );
        if planned.stops.is_empty() {
            return Ok(None);
        }
        let mut palace_ids = Vec::new();
        for stop in &planned.stops {
            let id = &trees[stop.palace].palace.id;
            if !palace_ids.contains(id) {
                palace_ids.push(id.clone());
            }
        }
        let start_notes = profile.publish(&JourneyStarted { palace_ids })?;
        // The game must not stop a journey: without its numbers the summary is as before.
        let game_start = profile
            .gamification_status(true)
            .ok()
            .flatten()
            .map(|status| (status.xp, earned(&status)));
        let mut journey = Self {
            game_start,
            trees,
            runs,
            stops: planned.stops,
            options,
            seed,
            at: 0,
            shown: Vec::new(),
            answered_shown: Vec::new(),
            finished: None,
            asked: Instant::now(),
            results: Vec::new(),
            started: Instant::now(),
            transcripts: HashMap::new(),
            points: 0,
            spent: HashMap::new(),
            notes: planned.warnings.into_iter().chain(start_notes).collect(),
        };
        journey.enter();
        Ok(Some(journey))
    }

    /// Every question is answered.
    pub fn done(&self) -> bool {
        self.at >= self.stops.len()
    }

    fn stop(&self) -> Stop {
        self.stops[self.at]
    }

    fn locus(&self) -> &Locus {
        self.stop().locus(&self.trees)
    }

    fn run(&self) -> &Run {
        &self.runs[self.stop().palace]
    }

    fn kind(&self) -> Kind {
        if self.options.reverse {
            Kind::Recall
        } else {
            self.locus().answer.kind
        }
    }

    fn enter(&mut self) {
        let answer = &self.locus().answer;
        // Every correct answer is shown; the rest of the chosen total are wrong answers.
        let total = self.options.quiz_answers.map_or(4, usize::from);
        let correct = answer
            .options
            .iter()
            .filter(|option| option.correct)
            .count();
        self.shown =
            answer.quiz_selection_with(self.seed ^ self.at as u64, total.saturating_sub(correct));
        self.asked = Instant::now();
    }

    fn text(&self, reveal: bool) -> String {
        resolve_text(self.run(), self.locus(), reveal).unwrap_or_else(|e| e)
    }

    fn label(&self) -> Option<String> {
        let stop = self.stop();
        self.options.numbers.then(|| {
            let room = stop.room_name(&self.trees);
            if self.trees.len() > 1 {
                let palace = &stop.tree(&self.trees).palace.name;
                format!("{palace} · Ort {} · {room}", stop.number)
            } else {
                format!("Ort {} · {room}", stop.number)
            }
        })
    }

    fn audio(&self, profile: &Profile) -> Option<Media> {
        self.locus()
            .audio
            .as_deref()
            .and_then(|h| media(&profile.store, h))
    }

    fn picture(&self, profile: &Profile) -> Option<Media> {
        self.locus()
            .image
            .as_deref()
            .and_then(|h| media(&profile.store, h))
    }

    pub fn question(&self, profile: &Profile, default_marker: &str) -> Question {
        let stop = self.stop();
        let tree = stop.tree(&self.trees);
        let room = &tree.rooms[stop.room].room;
        let locus = self.locus();
        let prompt = match (self.label(), self.options.reverse) {
            (_, true) => self.text(true),
            (Some(label), false) => label,
            (None, false) => self.text(false),
        };
        let kind = match self.kind() {
            Kind::Recall => AnswerKind::Recall,
            Kind::Guess => AnswerKind::Guess,
            Kind::Quiz => AnswerKind::Quiz,
        };
        let first_at_start = locus.time == Some(0.0)
            && !self.stops[..self.at].iter().any(|earlier| {
                earlier.palace == stop.palace
                    && earlier.room == stop.room
                    && earlier.locus(&self.trees).time == Some(0.0)
            });
        let video_preview = if room.kind == locora_store::RoomKind::Video && first_at_start {
            tree.rooms[stop.room]
                .loci
                .iter()
                .filter(|locus| locus.time == Some(0.0))
                .map(|locus| VideoPreviewItem {
                    place: locus.place.clone(),
                    item: locus.item.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        Question {
            n: self.at as u32 + 1,
            total: self.stops.len() as u32,
            palace: tree.palace.name.clone(),
            room: room.name.clone(),
            time: locus.time,
            video_preview,
            backdrop: room.media.as_deref().and_then(|h| media(&profile.store, h)),
            marker: marker(&profile.store, tree, default_marker),
            nearby: self
                .stops
                .iter()
                .enumerate()
                .filter_map(|(index, nearby)| {
                    if nearby.palace != stop.palace || nearby.room != stop.room {
                        return None;
                    }
                    let locus = nearby.locus(&self.trees);
                    Some(NearbyLocus {
                        id: locus.id.clone(),
                        offset: index as i32 - self.at as i32,
                        number: nearby.number as u32,
                        x: locus.x,
                        y: locus.y,
                        time: locus.time,
                        picture: locus
                            .image
                            .as_deref()
                            .and_then(|h| media(&profile.store, h)),
                        image_size: locus.image_size,
                    })
                })
                .collect(),
            x: locus.x,
            y: locus.y,
            zoom: locus.zoom,
            prompt,
            kind,
            options: if kind == AnswerKind::Quiz {
                self.shown
                    .iter()
                    .map(|i| locus.answer.options[*i as usize].text.clone())
                    .collect()
            } else {
                Vec::new()
            },
            picture: if self.options.reverse {
                self.picture(profile)
            } else {
                None
            },
            actions: self.run().actions(),
        }
    }

    /// Run a workbook action on the current question. `revealed` says
    /// whether the answer is already visible (`View.reveal` for formulas).
    pub fn action(
        &mut self,
        profile: &Profile,
        key: &str,
        revealed: bool,
        default_marker: &str,
    ) -> Result<ActionDone> {
        let palace = self.stop().palace;
        let outcome = self.runs[palace]
            .execute(key, revealed)
            .map_err(|e| anyhow::anyhow!("Aktion fehlgeschlagen: {e}"))?;
        let reveal = matches!(outcome, ActionOutcome::Reveal);
        Ok(ActionDone {
            reveal,
            question: self.question(profile, default_marker),
            revealed: (reveal || revealed).then(|| self.reveal(profile)),
            next: None,
        })
    }

    /// Run an action on the question answered last (its answer is visible),
    /// like `locora practice` after automatic feedback. Grades stay as they are.
    pub fn action_after(
        &mut self,
        profile: &Profile,
        key: &str,
        default_marker: &str,
    ) -> Result<ActionDone> {
        let previous = self
            .at
            .checked_sub(1)
            .context("Noch keine Frage beantwortet")?;
        let (at, shown) = (self.at, std::mem::take(&mut self.shown));
        self.at = previous;
        self.shown = self.answered_shown.clone();
        let result = self.action(profile, key, true, default_marker);
        self.at = at;
        self.shown = shown;
        // Actions write run values that later stops read: the next question
        // is shown – and graded – with them.
        let mut done = result?;
        done.next = (!self.done()).then(|| self.question(profile, default_marker));
        Ok(done)
    }

    /// The answer side of the current question.
    pub fn reveal(&self, profile: &Profile) -> Revealed {
        let solution = match (self.label(), self.options.reverse) {
            (Some(label), true) => format!("{label}: {}", self.text(false)),
            (None, true) => self.text(false),
            (Some(_), false) => format!("{} → {}", self.text(false), self.text(true)),
            (None, false) => self.text(true),
        };
        let locus = self.locus();
        Revealed {
            image_size: locus.image_size,
            solution,
            note: locus.note.clone(),
            audio: self.audio(profile),
            picture: if self.options.reverse {
                None
            } else {
                self.picture(profile)
            },
            right: if self.kind() == Kind::Quiz {
                self.shown
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| locus.answer.options[**i as usize].correct)
                    .map(|(n, _)| n as u32)
                    .collect()
            } else {
                Vec::new()
            },
        }
    }

    fn submission(&self, reply: Reply) -> Result<Submission> {
        let kind = self.kind();
        Ok(match reply {
            Reply::Forfeit => Submission::Forfeit,
            Reply::Recall(grade) if kind == Kind::Recall => Submission::Recall(match grade {
                1 => Grade::Again,
                2 => Grade::Hard,
                3 => Grade::Good,
                4 => Grade::Easy,
                _ => bail!("Bewertung 1 bis 4"),
            }),
            Reply::Guess(text) if kind == Kind::Guess => {
                if text.trim().is_empty() {
                    bail!("Bitte eine Antwort eingeben");
                }
                Submission::Guess(text)
            }
            Reply::Quiz(positions) if kind == Kind::Quiz => {
                let mut picked = Vec::new();
                for p in positions {
                    let index = *self
                        .shown
                        .get(p as usize)
                        .context("Ungültige Quiz-Auswahl")?;
                    if picked.contains(&index) {
                        bail!("Doppelte Quiz-Auswahl");
                    }
                    picked.push(index);
                }
                Submission::Quiz(picked)
            }
            _ => bail!("Antwort passt nicht zur Frage"),
        })
    }

    /// Grade the current question and move on.
    pub fn answer(
        &mut self,
        profile: &mut Profile,
        reply: Reply,
        default_marker: &str,
    ) -> Result<Feedback> {
        let submission = self.submission(reply)?;
        // Recall grades (and reverse journeys, which recall) neither show nor break a run.
        let recalled = self.kind() == Kind::Recall;
        let elapsed = self.asked.elapsed();
        let stop = self.stop();
        let palace_id = stop.tree(&self.trees).palace.id.clone();
        let locus = self.locus().clone();
        let revealed = self.reveal(profile);
        let expected = if self.kind() == Kind::Guess && submission != Submission::Forfeit {
            match resolve_text(self.run(), &locus, true) {
                Ok(text) if !text.trim().is_empty() => Some(text),
                Ok(_) => {
                    self.notes.push(format!(
                        "⚠ {}: leere erwartete Antwort – nicht bewertet",
                        locus.place
                    ));
                    None
                }
                Err(error) => {
                    self.notes.push(format!("{error} – nicht bewertet"));
                    None
                }
            }
        } else {
            Some(String::new())
        };
        *self.spent.entry(palace_id.clone()).or_default() += elapsed;
        let recall = Answer::default();
        let (correct, points) = match expected {
            Some(expected) => {
                let answered = profile.answer(AnswerRequest {
                    palace_id: &palace_id,
                    locus_id: &locus.id,
                    answer: if self.options.reverse {
                        &recall
                    } else {
                        &locus.answer
                    },
                    expected: &expected,
                    submission,
                    elapsed_ms: elapsed.as_millis().min(u64::MAX as u128) as u64,
                })?;
                self.notes.extend(answered.notes);
                self.transcripts
                    .entry(palace_id.clone())
                    .or_default()
                    .push(JourneyAnswer {
                        correct: answered.correct,
                        at_ms: self.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                    });
                self.results.push((palace_id, answered.correct));
                self.points = self.points.saturating_add(answered.points);
                (answered.correct, answered.points)
            }
            None => (false, 0),
        };
        self.answered_shown = std::mem::take(&mut self.shown);
        self.at += 1;
        // Sudden death ends the journey at the first wrong answer.
        let ended = self.options.sudden_death && !correct;
        if ended {
            self.notes
                .push("Sudden Death: die Runde endete bei der ersten falschen Antwort.".into());
        }
        let next = if !ended && self.at < self.stops.len() {
            self.enter();
            Step::Question(self.question(profile, default_marker))
        } else {
            Step::Done(self.finish(profile)?)
        };
        // The run shows after typed and chosen answers only; a recalled grade is the learner's own.
        let game = if self.game_start.is_none() {
            None
        } else if recalled {
            Some(GameView { combo: None })
        } else {
            profile
                .gamification_status(false)
                .ok()
                .flatten()
                .map(|status| GameView {
                    combo: status.session.map(|session| session.combo),
                })
        };
        Ok(Feedback {
            game,
            correct,
            points,
            revealed,
            next,
        })
    }

    /// End early or at the end: report completed palaces, sum up.
    pub fn finish(&mut self, profile: &mut Profile) -> Result<Summary> {
        if let Some(summary) = &self.finished {
            return Ok(Summary {
                notes: Vec::new(),
                ..summary.clone()
            });
        }
        for tree in &self.trees {
            let mine: Vec<bool> = self
                .results
                .iter()
                .filter(|(id, _)| *id == tree.palace.id)
                .map(|(_, ok)| *ok)
                .collect();
            let planned = self
                .stops
                .iter()
                .filter(|s| s.tree(&self.trees).palace.id == tree.palace.id)
                .count();
            if mine.is_empty() || mine.len() != planned {
                continue;
            }
            let notes = profile.publish(&JourneyCompleted {
                palace_id: tree.palace.id.clone(),
                correct: mine.iter().filter(|ok| **ok).count() as u32,
                total: mine.len() as u32,
                duration_secs: self
                    .spent
                    .get(&tree.palace.id)
                    .map_or(0, |d| d.as_secs_f64().round() as u64),
            })?;
            self.notes.extend(notes);
            if let Some(answers) = self.transcripts.get(&tree.palace.id) {
                profile.store.record_journey(&tree.palace.id, answers)?;
            }
            if let Some(achievement) = finished_palace_achievement(&tree.palace.id) {
                unlock_medal(profile, achievement)?;
            }
        }
        let mut summary = Summary {
            asked: self.results.len() as u32,
            correct: self.results.iter().filter(|(_, ok)| *ok).count() as u32,
            points: self.points,
            notes: std::mem::take(&mut self.notes),
            ..Summary::default()
        };
        if let Some(start) = &self.game_start {
            // If the plugin fails, the summary still shows.
            if let Ok(Some(status)) = profile.gamification_status(true) {
                self.game_summary(profile, &mut summary, start, &status)?;
            }
        }
        self.finished = Some(summary.clone());
        Ok(summary)
    }

    /// What the journey earned and what comes next, for the end card.
    fn game_summary(
        &self,
        profile: &mut Profile,
        summary: &mut Summary,
        (xp_before, earned_before): &(u64, BTreeSet<String>),
        status: &GamificationStatus,
    ) -> Result<()> {
        match &status.session {
            Some(session) => {
                summary.xp_gained = Some(session.xp);
                summary.due_done = session.due;
                summary.new_done = session.new;
                summary.early = session.early;
            }
            None => summary.xp_gained = Some(status.xp.saturating_sub(*xp_before)),
        }
        let names: HashMap<String, String> = profile
            .store
            .palaces()?
            .into_iter()
            .map(|p| (p.id, p.name))
            .collect();
        summary.new_trophies = earned(status)
            .difference(earned_before)
            .map(|id| match id.split_once('@') {
                Some((medal, palace)) => format!(
                    "{medal}@{}",
                    names.get(palace).map_or(palace, String::as_str)
                ),
                None => id.clone(),
            })
            .collect();
        summary.due_left_today = Some(status.today.due_open);
        // The next learning day with reviews, and how many are due on it.
        let now = now();
        let clock = profile.day_clock()?;
        let cards = profile.schedule(None)?;
        if let Some(next) = cards.values().map(|c| c.due).filter(|due| *due > now).min() {
            let day = clock.day_start(next);
            let count = cards
                .values()
                .filter(|c| c.due > now && clock.day_start(c.due) == day)
                .count() as u32;
            summary.next_due = Some((clock.date(day), count));
            let today = clock.day_start(now);
            let days = (day.saturating_sub(today) + DAY / 2) / DAY;
            summary.next_due_in = Some(u32::try_from(days).unwrap_or(u32::MAX));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_store::{NewLocus, RoomKind};

    #[test]
    fn video_question_preserves_locus_time() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let palace = profile.store.create_palace("Video").unwrap();
        let video = profile.store.put_media(b"video", "video/mp4").unwrap();
        let room = profile
            .store
            .add_room(&palace.id, "Film", RoomKind::Video, Some(&video.hash))
            .unwrap();
        profile
            .store
            .add_locus(
                &room.id,
                NewLocus {
                    audio: None,
                    place: "Szene".into(),
                    item: "Antwort".into(),
                    x: 0.5,
                    y: 0.5,
                    time: Some(6.25),
                    answer: Answer::default(),
                    image: None,
                    note: String::new(),
                    image_size: locora_store::default_image_size(),
                    zoom: locora_store::default_zoom(),
                },
            )
            .unwrap();
        let journey = Journey::start(
            &mut profile,
            JourneyStart {
                palace_id: Some(palace.id),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        let question = journey.question(&profile, "marker");
        assert_eq!(question.time, Some(6.25));
        assert_eq!(
            question.backdrop.unwrap().kind,
            locora_app_api::MediaKind::Video
        );
    }

    #[test]
    fn video_start_question_previews_all_zero_second_places() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let palace = profile.store.create_palace("Video").unwrap();
        let video = profile.store.put_media(b"video", "video/mp4").unwrap();
        let room = profile
            .store
            .add_room(&palace.id, "Film", RoomKind::Video, Some(&video.hash))
            .unwrap();
        for (place, item, time) in [
            ("Tor", "Erster", 0.0),
            ("Fenster", "Zweiter", 0.0),
            ("Garten", "Später", 2.0),
        ] {
            profile
                .store
                .add_locus(
                    &room.id,
                    NewLocus {
                        audio: None,
                        place: place.into(),
                        item: item.into(),
                        time: Some(time),
                        x: 0.5,
                        y: 0.5,
                        answer: Answer::default(),
                        image: None,
                        note: String::new(),
                        image_size: locora_store::default_image_size(),
                        zoom: locora_store::default_zoom(),
                    },
                )
                .unwrap();
        }
        let journey = Journey::start(
            &mut profile,
            JourneyStart {
                palace_id: Some(palace.id),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        let question = journey.question(&profile, "marker");
        assert_eq!(question.time, Some(0.0));
        assert_eq!(question.nearby.len(), 3);
        assert_eq!(question.nearby[0].offset, 0);
        assert_eq!(question.nearby[1].offset, 1);
        assert_eq!(
            question
                .video_preview
                .iter()
                .map(|item| item.place.as_str())
                .collect::<Vec<_>>(),
            vec!["Tor", "Fenster"]
        );
    }

    #[test]
    fn pictures_and_widths_travel_only_on_reveal_except_reverse_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let p = profile.store.create_palace("P").unwrap();
        let r = profile
            .store
            .add_room(&p.id, "R", RoomKind::Blank, None)
            .unwrap();
        let image = profile.store.put_media(b"png", "image/png").unwrap();
        profile
            .store
            .add_locus(
                &r.id,
                NewLocus {
                    audio: None,
                    place: "Tor".into(),
                    item: "Sonne".into(),
                    x: 0.4,
                    y: 0.6,
                    time: None,
                    answer: Answer::default(),
                    image: Some(image.hash.clone()),
                    note: String::new(),
                    image_size: 0.4,
                    zoom: locora_store::default_zoom(),
                },
            )
            .unwrap();
        for reverse in [false, true] {
            let journey = Journey::start(
                &mut profile,
                JourneyStart {
                    palace_id: Some(p.id.clone()),
                    reverse,
                    ..Default::default()
                },
            )
            .unwrap()
            .unwrap();
            let q = journey.question(&profile, "marker");
            assert_eq!(q.picture.is_some(), reverse);
            let answer = journey.reveal(&profile);
            assert_eq!(answer.image_size, 0.4);
            assert_eq!(answer.picture.is_some(), !reverse);
            if !reverse {
                assert_eq!(answer.picture.unwrap().hash, image.hash);
            }
        }
        let view = crate::views::palace(&mut profile, &p.id, "marker").unwrap();
        assert_eq!(view.rooms[0].loci[0].image_size, 0.4);
        assert_eq!(
            view.rooms[0].loci[0].picture.as_ref().unwrap().hash,
            image.hash
        );
    }

    #[test]
    fn inputs_are_asked_and_set_and_actions_reveal() {
        use locora_formula::workbook::{Binding, BindingField};
        use locora_formula::{Cell, CellType, Row};
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let p = profile.store.create_palace("P").unwrap();
        let r = profile
            .store
            .add_room(&p.id, "R", RoomKind::Blank, None)
            .unwrap();
        let locus = profile
            .store
            .add_locus(
                &r.id,
                NewLocus {
                    audio: None,
                    place: "Tor".into(),
                    item: "?".into(),
                    x: 0.5,
                    y: 0.5,
                    time: None,
                    answer: Answer::default(),
                    image: None,
                    note: String::new(),
                    image_size: 0.25,
                    zoom: locora_store::default_zoom(),
                },
            )
            .unwrap();
        let binding = Binding {
            field: BindingField::Item,
            expression: "$n".into(),
        };
        profile.store.set_binding(&locus.id, &binding).unwrap();
        let rows = [
            Row::typed("Inputs", "n", CellType::Text, "3"),
            Row {
                table: "Actions".into(),
                key: "zeigen".into(),
                cells: vec![Cell {
                    column: "kind".into(),
                    kind: CellType::Text,
                    value: "reveal".into(),
                }],
            },
        ];
        for row in &rows {
            profile.store.set_row(Some(&p.id), row).unwrap();
        }
        let mut start = JourneyStart {
            palace_id: Some(p.id.clone()),
            ..Default::default()
        };
        let fields = inputs(&profile, &start).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(
            (fields[0].key.as_str(), fields[0].default.as_str()),
            ("n", "3")
        );

        start.inputs = vec![locora_app_api::InputValue {
            palace_id: p.id.clone(),
            key: "n".into(),
            value: "7".into(),
        }];
        let mut journey = Journey::start(&mut profile, start.clone())
            .unwrap()
            .unwrap();
        let q = journey.question(&profile, "marker");
        assert_eq!(q.actions, ["zeigen"]);
        assert_eq!(journey.reveal(&profile).solution, "7");

        let done = journey.action(&profile, "zeigen", false, "marker").unwrap();
        assert!(done.reveal);
        assert_eq!(done.revealed.unwrap().solution, "7");
        assert!(journey.action(&profile, "fehlt", false, "marker").is_err());

        // Actions on the answered question, also after the last one.
        let feedback = journey
            .answer(&mut profile, Reply::Recall(3), "marker")
            .unwrap();
        assert!(matches!(feedback.next, Step::Done(_)));
        assert!(journey.done());
        let after = journey.action_after(&profile, "zeigen", "marker").unwrap();
        assert_eq!(after.revealed.unwrap().solution, "7");
        let again = journey.finish(&mut profile).unwrap();
        assert_eq!((again.asked, again.notes.len()), (1, 0), "reported once");

        start.inputs[0].palace_id = "anderer".into();
        assert!(Journey::start(&mut profile, start).is_err());
    }

    #[test]
    fn actions_after_grading_update_the_next_question() {
        use locora_formula::workbook::{Binding, BindingField};
        use locora_formula::{Cell, CellType, Row};
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let p = profile.store.create_palace("P").unwrap();
        let r = profile
            .store
            .add_room(&p.id, "R", RoomKind::Blank, None)
            .unwrap();
        for item in ["a", "b"] {
            let locus = profile
                .store
                .add_locus(
                    &r.id,
                    NewLocus {
                        audio: None,
                        place: "?".into(),
                        item: item.into(),
                        x: 0.5,
                        y: 0.5,
                        time: None,
                        answer: Answer::default(),
                        image: None,
                        note: String::new(),
                        image_size: 0.25,
                        zoom: locora_store::default_zoom(),
                    },
                )
                .unwrap();
            let binding = Binding {
                field: BindingField::Place,
                expression: "RAM.x".into(),
            };
            profile.store.set_binding(&locus.id, &binding).unwrap();
        }
        let cell = |column: &str, value: &str| Cell {
            column: column.into(),
            kind: CellType::Text,
            value: value.into(),
        };
        let rows = [
            Row::typed("RAM", "x", CellType::Number, "1"),
            Row {
                table: "Actions".into(),
                key: "plus".into(),
                cells: vec![
                    cell("kind", "set"),
                    cell("target", "RAM.x"),
                    cell("expression", "RAM.x + 1"),
                ],
            },
        ];
        for row in &rows {
            profile.store.set_row(Some(&p.id), row).unwrap();
        }
        let start = JourneyStart {
            palace_id: Some(p.id.clone()),
            ..Default::default()
        };
        let mut journey = Journey::start(&mut profile, start).unwrap().unwrap();
        let feedback = journey
            .answer(&mut profile, Reply::Recall(3), "marker")
            .unwrap();
        let Step::Question(before) = feedback.next else {
            panic!("a second question")
        };
        assert_eq!(before.prompt, "1");
        let done = journey.action_after(&profile, "plus", "marker").unwrap();
        assert_eq!(
            done.next.unwrap().prompt,
            "2",
            "the next stop sees the new value"
        );
        assert_eq!(journey.question(&profile, "marker").prompt, "2");
    }

    #[test]
    fn sudden_death_ends_at_the_first_wrong_answer() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let p = profile.store.create_palace("P").unwrap();
        let r = profile
            .store
            .add_room(&p.id, "R", RoomKind::Blank, None)
            .unwrap();
        for item in ["a", "b"] {
            profile
                .store
                .add_locus(
                    &r.id,
                    NewLocus {
                        audio: None,
                        place: "?".into(),
                        item: item.into(),
                        x: 0.5,
                        y: 0.5,
                        time: None,
                        answer: Answer::default(),
                        image: None,
                        note: String::new(),
                        image_size: 0.25,
                        zoom: locora_store::default_zoom(),
                    },
                )
                .unwrap();
        }
        let start = JourneyStart {
            palace_id: Some(p.id.clone()),
            sudden_death: true,
            ..Default::default()
        };
        let mut journey = Journey::start(&mut profile, start).unwrap().unwrap();
        let feedback = journey
            .answer(&mut profile, Reply::Forfeit, "marker")
            .unwrap();
        assert!(!feedback.correct);
        let Step::Done(summary) = feedback.next else {
            panic!("sudden death ends the journey")
        };
        assert_eq!(summary.asked, 1);
        assert!(
            summary
                .notes
                .iter()
                .any(|note| note.starts_with("Sudden Death"))
        );
    }

    fn unlocked(profile: &mut Profile, id: &str) -> bool {
        super::achievements(profile)
            .unwrap()
            .into_iter()
            .any(|view| view.id == id && view.unlocked)
    }

    #[test]
    fn lessons_and_the_tour_unlock_their_achievements_and_all_lessons_unlock_learn_all() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        assert!(super::unlock(&profile, "tour-done").unwrap());
        assert!(
            !super::unlock(&profile, "tour-done").unwrap(),
            "only once new"
        );
        assert!(unlocked(&mut profile, "tour-done"));
        assert!(
            super::unlock(&profile, "tutorial-done").is_err(),
            "earned by the journey"
        );
        assert!(
            super::unlock(&profile, "learn-all").is_err(),
            "follows from the lessons"
        );
        assert!(super::unlock(&profile, "learn-nothing").is_err());
        let (last, rest) = locora_app_api::LEARN_LESSONS.split_last().unwrap();
        for lesson in rest {
            super::unlock(&profile, &format!("learn-{lesson}")).unwrap();
        }
        assert!(unlocked(&mut profile, "learn-basics"));
        assert!(!unlocked(&mut profile, "learn-all"));
        super::unlock(&profile, &format!("learn-{last}")).unwrap();
        assert!(unlocked(&mut profile, "learn-all"));
    }

    #[test]
    fn walking_an_own_palace_unlocks_the_first_palace_achievement() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let p = profile.store.create_palace("Küche").unwrap();
        let r = profile
            .store
            .add_room(&p.id, "R", RoomKind::Blank, None)
            .unwrap();
        profile
            .store
            .add_locus(
                &r.id,
                NewLocus {
                    audio: None,
                    place: "Tür".into(),
                    item: "Milch".into(),
                    x: 0.5,
                    y: 0.5,
                    time: None,
                    answer: Answer::default(),
                    image: None,
                    note: String::new(),
                    image_size: 0.25,
                    zoom: locora_store::default_zoom(),
                },
            )
            .unwrap();
        assert!(!unlocked(&mut profile, "first-palace"));
        let start = JourneyStart {
            palace_id: Some(p.id.clone()),
            ..Default::default()
        };
        let mut journey = Journey::start(&mut profile, start).unwrap().unwrap();
        let feedback = journey
            .answer(&mut profile, Reply::Forfeit, "marker")
            .unwrap();
        assert!(matches!(feedback.next, Step::Done(_)));
        assert!(unlocked(&mut profile, "first-palace"));
    }

    /// A palace „Küche“ with `count` recall loci.
    fn kitchen(profile: &mut Profile, count: usize) -> String {
        let p = profile.store.create_palace("Küche").unwrap();
        let r = profile
            .store
            .add_room(&p.id, "R", RoomKind::Blank, None)
            .unwrap();
        for n in 0..count {
            profile
                .store
                .add_locus(
                    &r.id,
                    NewLocus {
                        audio: None,
                        place: format!("Ort {n}"),
                        item: "Milch".into(),
                        x: 0.5,
                        y: 0.5,
                        time: None,
                        answer: Answer::default(),
                        image: None,
                        note: String::new(),
                        image_size: 0.25,
                        zoom: locora_store::default_zoom(),
                    },
                )
                .unwrap();
        }
        p.id
    }

    /// Walks the whole palace, grading every recall with `grade` (1–4).
    fn walk(profile: &mut Profile, palace: &str, grade: u8) -> Summary {
        let start = JourneyStart {
            palace_id: Some(palace.to_owned()),
            ..Default::default()
        };
        let mut journey = Journey::start(profile, start).unwrap().unwrap();
        loop {
            let feedback = journey
                .answer(profile, Reply::Recall(grade), "marker")
                .unwrap();
            assert_eq!(
                feedback.game,
                Some(GameView { combo: None }),
                "no run for recall"
            );
            if let Step::Done(summary) = feedback.next {
                return summary;
            }
        }
    }

    #[test]
    fn the_end_card_counts_what_the_journey_earned_and_what_comes_next() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let palace = kitchen(&mut profile, 3);
        let summary = walk(&mut profile, &palace, 1);
        assert_eq!(
            summary.xp_gained,
            Some(18),
            "three new loci, whatever the grade"
        );
        assert_eq!(
            (summary.due_done, summary.new_done, summary.early),
            (0, 3, 0)
        );
        assert_eq!(summary.new_trophies, ["game-first-journey"]);
        assert_eq!(summary.due_left_today, Some(0));
        let (date, count) = summary.next_due.clone().expect("the loci come back");
        assert_eq!(count, 3, "all three are due on the same learning day");
        assert!(date.len() == 10, "{date}");
        assert!(
            summary.next_due_in.is_some_and(|days| days <= 1),
            "{summary:?}"
        );
        // Walking the same palace again the same day: practice, no XP, no new trophy.
        let again = walk(&mut profile, &palace, 3);
        assert_eq!(again.xp_gained, Some(0));
        assert_eq!(again.early, 3);
        assert!(again.new_trophies.is_empty());
    }

    #[test]
    fn without_the_game_the_end_card_is_as_before() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        assert!(profile.host.unload("gamification"));
        let palace = kitchen(&mut profile, 2);
        let start = JourneyStart {
            palace_id: Some(palace),
            ..Default::default()
        };
        let mut journey = Journey::start(&mut profile, start).unwrap().unwrap();
        let summary = loop {
            let feedback = journey
                .answer(&mut profile, Reply::Recall(3), "marker")
                .unwrap();
            assert_eq!(feedback.game, None);
            if let Step::Done(summary) = feedback.next {
                break summary;
            }
        };
        assert_eq!(summary.xp_gained, None);
        assert_eq!(summary.next_due, None);
        assert!(summary.new_trophies.is_empty());
        assert!(
            achievements(&mut profile)
                .unwrap()
                .iter()
                .all(|view| !view.id.starts_with("game-"))
        );
    }

    #[test]
    fn deleting_the_game_progress_locks_every_medal() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let palace = kitchen(&mut profile, 1);
        walk(&mut profile, &palace, 3);
        super::unlock(&profile, "tour-done").unwrap();
        let before = achievements(&mut profile).unwrap();
        let first = before
            .iter()
            .find(|view| view.id == "game-first-journey")
            .unwrap();
        assert!(first.unlocked && first.day.is_some());
        assert!(
            before
                .iter()
                .any(|view| view.id == "tour-done" && view.day.is_some())
        );
        assert!(
            !before.iter().any(|view| view.id == "game-perfect-journey"),
            "no placeholder for a trophy that cannot be earned"
        );
        profile.reset_game().unwrap();
        let after = achievements(&mut profile).unwrap();
        assert!(after.iter().all(|view| !view.unlocked), "{after:?}");
        // An older version on another device marks a medal the old way: it does not count.
        profile
            .store
            .kv_set(ACHIEVEMENTS, "first-palace", b"1")
            .unwrap();
        assert!(!unlocked(&mut profile, "first-palace"));
        // Earned again in the new epoch.
        walk(&mut profile, &palace, 3);
        assert!(unlocked(&mut profile, "first-palace"));
    }
}
