//! A journey in the app: the same plan, questions and grading as
//! `locora practice`/`review`, one question at a time over IPC.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use locora_api::core::{Grade, JourneyCompleted};
use locora_api::journey::{Answer, AnswerKind as Kind, Submission};
use locora_app_api::{
    ActionDone, AnswerKind, Feedback, InputField, JourneyStart, Media, Question, Reply, Revealed,
    Step, Summary,
};
use locora_formula::workbook::{ActionOutcome, Run};
use locora_runtime::journey::{PlanOptions, Stop, plan, resolve_text};
use locora_runtime::{AnswerRequest, Profile, now};
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
    asked: Instant,
    results: Vec<(String, bool)>,
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
        let mut journey = Self {
            trees,
            runs,
            stops: planned.stops,
            options,
            seed,
            at: 0,
            shown: Vec::new(),
            asked: Instant::now(),
            results: Vec::new(),
            points: 0,
            spent: HashMap::new(),
            notes: planned.warnings,
        };
        journey.enter();
        Ok(Some(journey))
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
        Question {
            n: self.at as u32 + 1,
            total: self.stops.len() as u32,
            palace: tree.palace.name.clone(),
            room: room.name.clone(),
            backdrop: room.media.as_deref().and_then(|h| media(&profile.store, h)),
            marker: marker(&profile.store, tree, default_marker),
            x: locus.x,
            y: locus.y,
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
        })
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
                self.results.push((palace_id, answered.correct));
                self.points = self.points.saturating_add(answered.points);
                (answered.correct, answered.points)
            }
            None => (false, 0),
        };
        self.at += 1;
        let next = if self.at < self.stops.len() {
            self.enter();
            Step::Question(self.question(profile, default_marker))
        } else {
            Step::Done(self.finish(profile)?)
        };
        Ok(Feedback {
            correct,
            points,
            revealed,
            next,
        })
    }

    /// End early or at the end: report completed palaces, sum up.
    pub fn finish(&mut self, profile: &mut Profile) -> Result<Summary> {
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
        }
        Ok(Summary {
            asked: self.results.len() as u32,
            correct: self.results.iter().filter(|(_, ok)| *ok).count() as u32,
            points: self.points,
            notes: std::mem::take(&mut self.notes),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_store::{NewLocus, RoomKind};

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

        start.inputs[0].palace_id = "anderer".into();
        assert!(Journey::start(&mut profile, start).is_err());
    }
}
