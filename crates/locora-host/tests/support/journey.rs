//! Core evaluation and scoring across the component boundary, independent of FSRS.
use super::{load, tiles, wasm_dir};
use locora_api::Service;
use locora_api::core::{Grade, JourneyScore, LocusAnswered, Score};
use locora_api::journey::{
    Answer, AnswerKind, Evaluate, EvaluateRequest, QuizOption, Scored, Submission,
};
use locora_api::review::{Schedule, ScheduleRequest};
use locora_host::PluginHost;

#[test]
fn core_bound_calls_and_profile_loads_cannot_be_hijacked() {
    let mut host = PluginHost::new(None).unwrap();
    load(&mut host, "core");
    load(&mut host, "fixture");
    let request = EvaluateRequest {
        answer: Answer {
            kind: AnswerKind::Guess,
            accept: vec![],
            options: vec![],
        },
        expected: "Geheim".into(),
        submission: Submission::Guess("falsch".into()),
        elapsed_ms: 1,
    };
    host.drain_events();
    let result = host.call_plugin::<Evaluate>("core", &request).unwrap();
    assert!(!result.correct);
    assert!(tiles(&host.drain_events()).is_empty());
    assert!(
        host.load_additional_file(&wasm_dir().join("core.wasm"))
            .is_err()
    );
    assert!(host.plugin_provides("core", Evaluate::NAME));
    assert!(
        !host
            .call_plugin::<Evaluate>("core", &request)
            .unwrap()
            .correct
    );
    assert!(host.unload("core"));
    assert!(host.provides(Evaluate::NAME)); // the untrusted competitor remains
    assert!(!host.plugin_provides("core", Evaluate::NAME));
    assert!(host.call_plugin::<Evaluate>("core", &request).is_err());
    assert!(tiles(&host.drain_events()).is_empty());
    // Prove that the competing service really exists and would leak the answer.
    assert_eq!(host.call::<Evaluate>(&request).unwrap().points, 999);
    assert!(
        tiles(&host.drain_events())
            .iter()
            .any(|value| value == "Geheim")
    );
}

#[test]
fn core_wasm_evaluation_hook_and_review_remain_separate() {
    let mut host = PluginHost::new(None).unwrap();
    load(&mut host, "core");
    load(&mut host, "review");
    load(&mut host, "palace_writer"); // Adds 3 points, even for incorrect answers.
    let mut request = EvaluateRequest {
        answer: Answer {
            kind: AnswerKind::Guess,
            accept: vec![],
            options: vec![],
        },
        expected: " Merkur ".into(),
        submission: Submission::Guess("MERKUR".into()),
        elapsed_ms: 5_000,
    };
    let good = host.call::<Evaluate>(&request).unwrap();
    assert!(good.correct);
    assert_eq!(good.grade, Grade::Easy);
    assert_eq!(good.points, 1);
    request.elapsed_ms = 5_001;
    assert_eq!(host.call::<Evaluate>(&request).unwrap().grade, Grade::Good);
    request.submission = Submission::Guess("Merk".into());
    let bad = host.call::<Evaluate>(&request).unwrap();
    assert_eq!(bad.grade, Grade::Again);
    assert!(!bad.correct);
    let score = host
        .filter::<JourneyScore>(Score {
            palace_id: "p".into(),
            locus_id: "l".into(),
            points: bad.points,
        })
        .unwrap();
    assert_eq!(score.points, 3);
    host.publish(&LocusAnswered {
        palace_id: "p".into(),
        locus_id: "l".into(),
        correct: bad.correct,
        grade: bad.grade,
        elapsed_ms: request.elapsed_ms,
        answered_at: 1_800_000_000,
    })
    .unwrap();
    host.publish(&Scored {
        palace_id: "p".into(),
        locus_id: "l".into(),
        points: score.points,
    })
    .unwrap();
    let cards = host
        .call::<Schedule>(&ScheduleRequest {
            palace_id: Some("p".into()),
        })
        .unwrap();
    assert_eq!(cards.len(), 1);
    // Initial Again is due in ten minutes, regardless of points.
    assert_eq!(cards[0].card.due, 1_800_000_600);
    assert!(tiles(&host.drain_events()).iter().any(|text| text == "3"));

    request.answer = Answer {
        kind: AnswerKind::Quiz,
        accept: vec![],
        options: vec![
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
        ],
    };
    for (selected, correct) in [(vec![2, 0], true), (vec![0], false), (vec![0, 1, 2], false)] {
        request.submission = Submission::Quiz(selected);
        assert_eq!(host.call::<Evaluate>(&request).unwrap().correct, correct);
    }
    request.submission = Submission::Quiz(vec![0, 0]);
    assert!(host.call::<Evaluate>(&request).is_err());
    assert!(host.call_raw(Evaluate::NAME, &[255]).is_err());
    assert!(host.plugins().any(|p| p.id == "core"));
    request.submission = Submission::Forfeit;
    assert_eq!(host.call::<Evaluate>(&request).unwrap().grade, Grade::Again);
    host.load_file(&wasm_dir().join("core.wasm")).unwrap();
    assert!(host.provides(Evaluate::NAME));
    assert!(host.unload("core"));
    assert!(!host.provides(Evaluate::NAME));
    assert!(host.call::<Evaluate>(&request).is_err());
}
