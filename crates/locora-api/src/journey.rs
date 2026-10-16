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

/// Guess compares against the resolved locus item. Quiz uses explicit options.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    #[serde(rename = "type")]
    pub kind: AnswerKind,
    #[serde(default)]
    pub options: Vec<QuizOption>,
}

impl Answer {
    pub fn is_recall(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.kind != AnswerKind::Quiz {
            return if self.options.is_empty() {
                Ok(())
            } else {
                Err("Nur Quiz darf Antwortoptionen enthalten")
            };
        }
        if !(2..=9).contains(&self.options.len()) {
            return Err("Quiz braucht 2 bis 9 Optionen");
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
}
