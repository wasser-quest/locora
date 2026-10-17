//! A journey in the app: the same plan, questions and grading as
//! `locora practice`/`review`, one question at a time over IPC.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use locora_api::core::{Grade, JourneyCompleted, JourneyStarted};
use locora_api::journey::{Answer, AnswerKind as Kind, Submission};
use locora_app_api::{
    ActionDone, AnswerKind, Feedback, GameView, InputField, JourneyStart, Media, Question, Reply,
    Revealed, Step, Summary, VideoPreviewItem,
};
use locora_formula::workbook::{ActionOutcome, Run};
use locora_runtime::journey::{PlanOptions, Stop, plan, resolve_text};
use locora_runtime::{AnswerRequest, Profile, now};
use locora_store::JourneyAnswer;
use locora_store::{Locus, PalaceTree};

use crate::views::{marker, media};

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
        let mut journey = Self {
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
        self.shown = self
            .locus()
            .answer
            .quiz_selection(self.seed ^ self.at as u64);
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
        let next = if self.at < self.stops.len() {
            self.enter();
            Step::Question(self.question(profile, default_marker))
        } else {
            Step::Done(self.finish(profile)?)
        };
        let game = profile.gamification_status()?.map(|status| GameView {
            combo: status.combo,
            hearts: status.hearts,
        });
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
        }
        let summary = Summary {
            asked: self.results.len() as u32,
            correct: self.results.iter().filter(|(_, ok)| *ok).count() as u32,
            points: self.points,
            notes: std::mem::take(&mut self.notes),
        };
        self.finished = Some(summary.clone());
        Ok(summary)
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
        let room = profile.store.add_room(&palace.id, "Film", RoomKind::Video, Some(&video.hash)).unwrap();
        for (place, item, time) in [("Tor", "Erster", 0.0), ("Fenster", "Zweiter", 0.0), ("Garten", "Später", 2.0)] {
            profile.store.add_locus(&room.id, NewLocus {
                place: place.into(), item: item.into(), time: Some(time),
                x: 0.5, y: 0.5, answer: Answer::default(), image: None,
                note: String::new(), image_size: locora_store::default_image_size(),
                zoom: locora_store::default_zoom(),
            }).unwrap();
        }
        let journey = Journey::start(&mut profile, JourneyStart {
            palace_id: Some(palace.id), ..Default::default()
        }).unwrap().unwrap();
        let question = journey.question(&profile, "marker");
        assert_eq!(question.time, Some(0.0));
        assert_eq!(question.video_preview.iter().map(|item| item.place.as_str()).collect::<Vec<_>>(), vec!["Tor", "Fenster"]);
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
}
