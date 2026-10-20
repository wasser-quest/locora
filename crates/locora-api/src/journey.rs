//! Core journey contracts. Evaluation is provided by the Core WASM plugin.
use super::*;
use crate::core::Grade;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnswerKind {
    #[default]
    Recall,
    Quiz,
    Guess,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuizOption {
    pub text: String,
    pub correct: bool,
}

/// Guess compares against the resolved locus item (and `accept`). Quiz uses
/// explicit options: every correct one plus up to three randomly chosen wrong
/// ones are shown per question.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    #[serde(rename = "type")]
    pub kind: AnswerKind,
    #[serde(default)]
    pub options: Vec<QuizOption>,
    /// Guess: further answers that also count as right. Always serialized:
    /// postcard (plugin ABI) is not self-describing.
    #[serde(default)]
    pub accept: Vec<String>,
}

/// Most correct options in one quiz.
pub const MAX_QUIZ_CORRECT: usize = 9;
/// Most options (correct plus wrong) in one quiz's pool.
pub const MAX_QUIZ_OPTIONS: usize = 30;
/// Wrong options shown per quiz question.
pub const QUIZ_WRONG_SHOWN: usize = 3;

/// Shuffle `items` deterministically from `seed` (Fisher–Yates with a small
/// LCG; the same seed gives the same order on every platform).
pub fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
    for i in (1..items.len()).rev() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        items.swap(i, ((state >> 33) % (i as u64 + 1)) as usize);
    }
}

/// How typed answers are compared: case, punctuation and repeated spaces do
/// not matter (`„Die  Erde!“` = `die erde`).
pub fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl Answer {
    pub fn is_recall(&self) -> bool {
        *self == Self::default()
    }

    /// The options to show for one quiz question: every correct option plus
    /// up to three wrong ones, chosen and ordered by `seed` (pass something
    /// that changes per question, e.g. the time). Returns indices into `options`.
    pub fn quiz_selection(&self, seed: u64) -> Vec<u32> {
        self.quiz_selection_with(seed, QUIZ_WRONG_SHOWN)
    }

    /// Like `quiz_selection`, but with up to `wrong_shown` of the wrong options.
    pub fn quiz_selection_with(&self, seed: u64, wrong_shown: usize) -> Vec<u32> {
        let indices = 0..self.options.len() as u32;
        let mut wrong: Vec<u32> = indices
            .clone()
            .filter(|&i| !self.options[i as usize].correct)
            .collect();
        shuffle(&mut wrong, seed);
        wrong.truncate(wrong_shown);
        let mut shown: Vec<u32> = indices
            .filter(|&i| self.options[i as usize].correct)
            .collect();
        shown.extend(wrong);
        shuffle(&mut shown, seed.rotate_left(17));
        shown
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.accept.is_empty() {
            if self.kind != AnswerKind::Guess {
                return Err("Weitere gültige Antworten gibt es nur bei Guess");
            }
            if self.accept.len() > 20 {
                return Err("Höchstens 20 weitere gültige Antworten");
            }
            let mut seen = Vec::new();
            for text in &self.accept {
                let key = normalize(text);
                if key.is_empty() || seen.contains(&key) {
                    return Err("Weitere gültige Antworten müssen nichtleer und eindeutig sein");
                }
                seen.push(key);
            }
        }
        if self.kind != AnswerKind::Quiz {
            return if self.options.is_empty() {
                Ok(())
            } else {
                Err("Nur Quiz darf Antwortoptionen enthalten")
            };
        }
        if !(2..=MAX_QUIZ_OPTIONS).contains(&self.options.len()) {
            return Err("Quiz braucht 2 bis 30 Optionen");
        }
        if self.options.iter().filter(|o| o.correct).count() > MAX_QUIZ_CORRECT {
            return Err("Höchstens 9 richtige Quizoptionen");
        }
        let mut seen = Vec::new();
        for option in &self.options {
            let text = option.text.trim().to_lowercase();
            if text.is_empty() || seen.contains(&text) {
                return Err("Quizoptionen müssen nichtleer und eindeutig sein");
            }
            seen.push(text);
        }
        if !self.options.iter().any(|o| o.correct) || self.options.iter().all(|o| o.correct) {
            return Err("Quiz braucht richtige und falsche Optionen");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Submission {
    Recall(Grade),
    /// Zero-based indices; duplicates and out-of-range indices are invalid.
    Quiz(Vec<u32>),
    Guess(String),
    /// Revealing or consciously skipping an automatic question forfeits it.
    Forfeit,
}

pub struct Evaluate;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluateRequest {
    pub answer: Answer,
    pub expected: String,
    pub submission: Submission,
    pub elapsed_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evaluation {
    pub correct: bool,
    pub grade: Grade,
    pub points: u64,
}
impl Service for Evaluate {
    const NAME: &'static str = "core.journey.evaluate@1";
    type Request = EvaluateRequest;
    type Response = Evaluation;
}

/// Final points after host-driven gamification filters; does not affect review.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scored {
    pub palace_id: String,
    pub locus_id: String,
    pub points: u64,
}
impl Event for Scored {
    const TOPIC: &'static str = "core.journey.scored";
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn postcard_contract() {
        let request = EvaluateRequest {
            answer: Answer::default(),
            expected: "x".into(),
            submission: Submission::Recall(Grade::Good),
            elapsed_ms: 1,
        };
        assert_eq!(
            decode::<EvaluateRequest>(&encode(&request).unwrap()).unwrap(),
            request
        );
    }

    fn quiz(correct: usize, wrong: usize) -> Answer {
        Answer {
            kind: AnswerKind::Quiz,
            options: (0..correct)
                .map(|i| QuizOption {
                    text: format!("r{i}"),
                    correct: true,
                })
                .chain((0..wrong).map(|i| QuizOption {
                    text: format!("f{i}"),
                    correct: false,
                }))
                .collect(),
            ..Answer::default()
        }
    }

    #[test]
    fn quiz_shows_all_correct_and_three_wrong_shuffled() {
        let answer = quiz(2, 10);
        answer.validate().unwrap();
        let mut orders = std::collections::HashSet::new();
        for seed in 0..50 {
            let shown = answer.quiz_selection(seed);
            assert_eq!(shown.len(), 5);
            assert!(shown.contains(&0) && shown.contains(&1));
            let unique: std::collections::HashSet<_> = shown.iter().collect();
            assert_eq!(unique.len(), 5);
            assert_eq!(shown, answer.quiz_selection(seed), "stable per seed");
            orders.insert(shown);
        }
        assert!(
            orders.len() > 20,
            "different seeds give different questions"
        );
        assert_eq!(quiz(1, 1).quiz_selection(7).len(), 2);
        assert!(quiz(2, 29).validate().is_err());
        assert!(quiz(10, 2).validate().is_err());
    }

    #[test]
    fn typed_answers_ignore_case_punctuation_and_spaces() {
        assert_eq!(normalize("  „Die  Erde!“ "), "die erde");
        assert_eq!(normalize("Ä-Ö ü"), "ä ö ü");
        let mut guess = Answer {
            kind: AnswerKind::Guess,
            accept: vec!["Hermes".into()],
            ..Answer::default()
        };
        guess.validate().unwrap();
        guess.accept.push("hermes!".into());
        assert!(guess.validate().is_err(), "duplicates after normalizing");
        guess.accept = vec!["?".into()];
        assert!(guess.validate().is_err(), "empty after normalizing");
        let quiz = Answer {
            accept: vec!["x".into()],
            ..quiz(1, 1)
        };
        assert!(quiz.validate().is_err());
    }
}
