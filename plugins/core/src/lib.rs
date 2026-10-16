//! Core answer rules. Terminal I/O and hook orchestration stay in the native client.
use locora_sdk::prelude::*;

#[locora_plugin(name = "Kern")]
struct Core;

impl Plugin for Core {
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.provide::<Evaluate>(|_, request| evaluate(request))
    }
}

fn evaluate(request: EvaluateRequest) -> Result<Evaluation> {
    request.answer.validate().map_err(Error::msg)?;
    let correct = match (&request.answer.kind, &request.submission) {
        (_, Submission::Forfeit) => false,
        (AnswerKind::Recall, Submission::Recall(grade)) => *grade != Grade::Again,
        (AnswerKind::Guess, Submission::Guess(text)) => {
            if text.trim().is_empty() || request.expected.trim().is_empty() {
                return Err(Error::msg("Die Antwort darf nicht leer sein"));
            }
            let given = normalize(text);
            std::iter::once(&request.expected)
                .chain(&request.answer.accept)
                .any(|candidate| normalize(candidate) == given)
        }
        (AnswerKind::Quiz, Submission::Quiz(selected)) => {
            let mut seen = Vec::new();
            for index in selected {
                if *index as usize >= request.answer.options.len() || seen.contains(index) {
                    return Err(Error::msg("Ungültige oder doppelte Quiz-Auswahl"));
                }
                seen.push(*index);
            }
            request
                .answer
                .options
                .iter()
                .enumerate()
                .all(|(i, option)| option.correct == selected.contains(&(i as u32)))
        }
        _ => return Err(Error::msg("Antwort passt nicht zur Abfrageart")),
    };
    let grade = if !correct {
        Grade::Again
    } else if let Submission::Recall(grade) = request.submission {
        grade
    } else if request.elapsed_ms <= 5_000 {
        Grade::Easy
    } else {
        Grade::Good
    };
    Ok(Evaluation {
        correct,
        grade,
        points: u64::from(correct),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(kind: AnswerKind, submission: Submission) -> EvaluateRequest {
        EvaluateRequest {
            answer: Answer {
                kind,
                options: vec![],
                accept: vec![],
            },
            expected: " Merkur ".into(),
            submission,
            elapsed_ms: 5_000,
        }
    }
    #[test]
    fn recall_and_guess_rules() {
        for grade in [Grade::Again, Grade::Hard, Grade::Good, Grade::Easy] {
            let result = evaluate(request(AnswerKind::Recall, Submission::Recall(grade))).unwrap();
            assert_eq!(result.grade, grade);
            assert_eq!(result.points, u64::from(grade != Grade::Again));
        }
        let mut r = request(AnswerKind::Guess, Submission::Guess("mErKuR".into()));
        assert_eq!(evaluate(r.clone()).unwrap().grade, Grade::Easy);
        r.elapsed_ms += 1;
        assert_eq!(evaluate(r.clone()).unwrap().grade, Grade::Good);
        r.submission = Submission::Guess("Merk".into());
        assert_eq!(evaluate(r.clone()).unwrap().grade, Grade::Again);
        r.submission = Submission::Guess("  merkur!".into());
        assert!(evaluate(r.clone()).unwrap().correct);
        r.answer.accept = vec!["Hermes".into()];
        r.submission = Submission::Guess("hermes".into());
        assert!(evaluate(r.clone()).unwrap().correct);
        r.submission = Submission::Guess(" ".into());
        assert!(evaluate(r).is_err());
        assert!(
            !evaluate(request(AnswerKind::Guess, Submission::Forfeit))
                .unwrap()
                .correct
        );
        assert!(evaluate(request(AnswerKind::Guess, Submission::Recall(Grade::Good))).is_err());
    }
    #[test]
    fn quiz_requires_exact_set() {
        let mut r = request(AnswerKind::Quiz, Submission::Quiz(vec![2, 0]));
        r.answer.options = vec![
            QuizOption {
                text: "a".into(),
                correct: true,
            },
            QuizOption {
                text: "b".into(),
                correct: false,
            },
            QuizOption {
                text: "c".into(),
                correct: true,
            },
        ];
        assert!(evaluate(r.clone()).unwrap().correct);
        for selection in [vec![], vec![0], vec![0, 1, 2]] {
            r.submission = Submission::Quiz(selection);
            assert!(!evaluate(r.clone()).unwrap().correct);
        }
        for selection in [vec![0, 0], vec![3]] {
            r.submission = Submission::Quiz(selection);
            assert!(evaluate(r.clone()).is_err());
        }
        r.answer.options[1].text = " A ".into();
        assert!(evaluate(r).is_err());
    }
}
