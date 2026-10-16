//! Mixed answer journeys through the embedded Core and Review WASM plugins.
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use locora_api::review::CardState;
use locora_store::Store;

const QUIZ: &str = r#"{"type":"quiz","options":[{"text":"Merkur","correct":true},{"text":"Venus","correct":false},{"text":"Erde","correct":true}]}"#;

/// The quiz as shown at plan position `index`: (input choosing all correct
/// options, the „Richtige Auswahl“ line). Same seed as the CLI: LOCORA_NOW ^ index.
fn quiz_shown(index: u64) -> (String, String) {
    let answer: locora_api::journey::Answer = serde_json::from_str(QUIZ).unwrap();
    let shown = answer.quiz_selection(1_800_000_000 ^ index);
    let correct: Vec<_> = shown
        .iter()
        .enumerate()
        .filter(|(_, i)| answer.options[**i as usize].correct)
        .collect();
    let input = correct
        .iter()
        .map(|(n, _)| (n + 1).to_string())
        .collect::<Vec<_>>()
        .join(",");
    let line = correct
        .iter()
        .map(|(n, i)| format!("{} {}", n + 1, answer.options[**i as usize].text))
        .collect::<Vec<_>>()
        .join(", ");
    (input, format!("Richtige Auswahl: {line}"))
}

fn run(data: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_locora"))
        .arg("--data")
        .arg(data)
        .args(args)
        .env("LOCORA_NOW", "1800000000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    feed(child.stdin.take().unwrap(), input.as_bytes());
    child.wait_with_output().unwrap()
}

fn ok(data: &Path, args: &[&str], input: &str) -> String {
    let out = run(data, args, input);
    assert!(
        out.status.success() && out.stderr.is_empty(),
        "{args:?}: {}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn answer(data: &Path, n: &str, json: &str) {
    ok(data, &["edit", "P", n, "--answer", json], "");
}

fn cards(data: &Path) -> BTreeMap<String, CardState> {
    let store = Store::open(data).unwrap();
    let p = store.palace("P").unwrap();
    store
        .kv_get("review", &format!("cards/{}", p.palace.id))
        .unwrap()
        .map(|bytes| locora_api::decode(&bytes).unwrap())
        .unwrap_or_default()
}

#[test]
fn mixed_answers_review_and_archive_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, &["new", "P", "--", "Recall", "Merkur", "Erklärung"], "");
    answer(d, "2", r#"{"type":"guess"}"#);
    answer(d, "3", QUIZ);
    let (choice, solution) = quiz_shown(2);
    let out = ok(d, &["practice", "P"], &format!("\n2\n mErKuR \n{choice}\n"));
    assert!(out.contains("3 von 3 geübt, 3 gewusst. 3 Punkte."), "{out}");
    assert!(out.contains(&solution), "{out}");
    assert_eq!(cards(d).len(), 3);
    assert!(cards(d).values().all(|c| c.reps == 1 && c.lapses == 0));
    assert!(ok(d, &["review", "P", "--new", "0"], "").contains("Nichts fällig"));
    let out = ok(d, &["practice", "P"], "\n1\nNein\n1\n");
    assert!(out.contains("3 von 3 geübt, 0 gewusst. 0 Punkte."));
    assert!(cards(d).values().all(|c| c.reps == 2 && c.lapses == 1));
    let file = d.join("P.locora");
    ok(d, &["export", "P", "-o", file.to_str().unwrap()], "");
    let target = tempfile::tempdir().unwrap();
    ok(target.path(), &["import", file.to_str().unwrap()], "");
    let out = ok(
        target.path(),
        &["review", "P", "--new", "3"],
        &format!("\n3\nMerkur\n{choice}\n"),
    );
    assert!(out.contains("3 von 3 geübt, 3 gewusst."));
    assert_eq!(cards(target.path()).len(), 3);
}

#[test]
fn reveal_forfeits_quit_does_not_grade_and_invalid_choices_retry() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, &["new", "P", "--", "Merkur", "Erklärung", "Ende"], "");
    answer(d, "1", r#"{"type":"guess"}"#);
    answer(d, "2", QUIZ);
    ok(
        d,
        &[
            "table",
            "set",
            "--palace",
            "P",
            "Actions.loesung.kind",
            "reveal",
        ],
        "",
    );
    let out = ok(d, &["review", "P"], "!loesung\n\n1,1\n0\n\n\nq\n");
    assert!(out.contains("Bitte gültige Optionsnummern"), "{out}");
    assert!(out.contains("2 von 3 geübt, 0 gewusst. 0 Punkte."));
    assert!(out.contains(&quiz_shown(1).1), "{out}");
    assert_eq!(cards(d).len(), 2);
    assert!(cards(d).values().all(|c| c.reps == 1));
    let before = cards(d);
    ok(d, &["practice", "P"], "q\n");
    ok(d, &["practice", "P"], "");
    assert_eq!(cards(d), before);
}

#[test]
fn guesses_use_resolved_items_without_leaking_them_and_escape_controls() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, &["new", "P", "--", "Basis", "?", "!important"], "");
    for n in ["1", "2", "3"] {
        answer(d, n, r#"{"type":"guess"}"#);
    }
    ok(
        d,
        &[
            "bind",
            "P",
            "1",
            "item",
            "IF(View.reveal, \"Q\", \"falsch\")",
        ],
        "",
    );
    let out = ok(d, &["practice", "P"], "q\n");
    assert!(!out.contains("→ Q"));
    assert!(cards(d).is_empty());
    let out = ok(d, &["practice", "P"], "=q\n=?\n=!important\n");
    assert!(out.contains("3 von 3 geübt, 3 gewusst."), "{out}");
    let invalid = run(
        d,
        &[
            "edit",
            "P",
            "2",
            "--answer",
            r#"{"type":"quiz","options":[]}"#,
            "--item",
            "bad",
        ],
        "",
    );
    assert!(!invalid.status.success());
    let tree = Store::open(d).unwrap().palace("P").unwrap();
    assert_eq!(tree.rooms[0].loci[1].item, "?");
    assert_eq!(
        tree.rooms[0].loci[1].answer.kind,
        locora_api::journey::AnswerKind::Guess
    );
}

#[test]
fn automatic_feedback_keeps_actions_on_the_current_locus_without_regrading() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, &["new", "P", "--", "Basis", "Basis"], "");
    for n in ["1", "2"] {
        answer(d, n, r#"{"type":"guess"}"#);
        ok(d, &["bind", "P", n, "item", "RAM.n"], "");
    }
    for (address, value, kind) in [
        ("RAM.n", "0", "number"),
        ("Actions.Show.kind", "reveal", "text"),
        ("Actions.Next.kind", "set", "text"),
        ("Actions.Next.target", "RAM.n", "text"),
        ("Actions.Next.expression", "RAM.n + 1", "text"),
    ] {
        ok(
            d,
            &["table", "set", "--palace", "P", address, value, "-t", kind],
            "",
        );
    }
    let out = ok(d, &["practice", "P"], "!Show\n!Next\n\n1\n!Next\nq\n");
    assert!(out.contains("→ 1") && out.contains("→ 2"), "{out}");
    assert!(out.contains("2 von 2 geübt, 1 gewusst. 1 Punkte."), "{out}");
    let cards = cards(d);
    assert_eq!(cards.len(), 2);
    assert!(cards.values().all(|card| card.reps == 1));
    assert_eq!(ok(d, &["calc", "P", "RAM.n"], ""), "0\n");
}

#[test]
fn broken_or_empty_guess_expected_values_do_not_abort_or_schedule() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, &["new", "P", "--", "Leer", "Fehler", "Ende"], "");
    answer(d, "1", r#"{"type":"guess"}"#);
    answer(d, "2", r#"{"type":"guess"}"#);
    ok(d, &["bind", "P", "1", "item", "\"\""], "");
    ok(d, &["bind", "P", "2", "item", "Missing.value"], "");
    let out = ok(d, &["practice", "P"], "a\nb\n\n3\n");
    assert!(out.contains("Leere erwartete Antwort – nicht bewertet."));
    assert!(out.contains("Formel für item fehlerhaft:"));
    assert!(out.contains("1 von 3 geübt, 1 gewusst."));
    assert_eq!(cards(d).len(), 1);
    // Deliberate reveal/forgot can still record failure for a broken question.
    let out = ok(d, &["practice", "P"], "\n\nq\n");
    assert!(out.contains("2 von 3 geübt, 0 gewusst."));
    assert_eq!(cards(d).len(), 3);
}

/// Write test input; a process that exits before reading it is fine.
fn feed(mut stdin: std::process::ChildStdin, input: &[u8]) {
    if let Err(err) = stdin.write_all(input) {
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe, "{err}");
    }
}

#[test]
fn notes_accepted_answers_and_practice_modes() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, &["new", "P", "--", "Merkur", "Venus"], "");
    ok(
        d,
        &[
            "edit", "P", "1", "--accept", "Hermes", "--accept", "der Bote",
        ],
        "",
    );
    ok(
        d,
        &["edit", "P", "2", "--note", "Eine Venusfalle schnappt zu"],
        "",
    );
    let show = ok(d, &["show", "P"], "");
    assert!(show.contains("auch richtig: Hermes · der Bote"), "{show}");
    assert!(show.contains("✎ Eine Venusfalle"), "{show}");
    // Guess: punctuation, case and extra answers count; the note follows the answer.
    let out = ok(d, &["practice", "P"], "  DER bote! \n\n3\n");
    assert!(out.contains("Richtig → Merkur"), "{out}");
    assert!(
        out.contains("→ Venus\n        ✎ Eine Venusfalle schnappt zu"),
        "{out}"
    );
    assert!(out.contains("2 von 2 geübt, 2 gewusst."), "{out}");
    // Reverse: the item is the question, every locus is recalled.
    let out = ok(d, &["practice", "P", "--reverse"], "\n1\n\n3\n");
    assert!(
        out.contains("[1/2] Merkur") && out.contains("→ Ort 1"),
        "{out}"
    );
    assert!(out.contains("2 von 2 geübt, 1 gewusst."), "{out}");
    // Numbers: only the route number and room are shown.
    let out = ok(
        d,
        &["practice", "P", "--numbers", "--shuffle"],
        "=Merkur\n\n3\n",
    );
    assert!(
        out.contains("Ort 1 · Raum 1") && out.contains("Ort 2 · Raum 1"),
        "{out}"
    );
    assert!(out.contains("→ Ort 2 → Venus"), "{out}");
    assert!(ok(d, &["edit", "P", "2", "--note", ""], "").contains("Venus"));
    assert!(!ok(d, &["show", "P"], "").contains('✎'));
    let bad = run(
        d,
        &["edit", "P", "1", "--accept", "x", "--answer", "{}"],
        "",
    );
    assert!(!bad.status.success());
}
