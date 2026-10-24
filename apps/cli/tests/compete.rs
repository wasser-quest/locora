//! `locora compete`, scripted (`LOCORA_COMPETE_SCRIPTED=1`): rounds from challenge codes with the
//! golden tasks, the daily challenge once per level, records, history, and deleting the game
//! progress. The fixed time is a Friday, so today's challenge is numbers.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use locora_competition::{
    Date, Discipline, Lang, Level, Seed, Spec, Task, cards, code, daily_seed, generate, words,
};

const T0: &str = "1800000000";

fn run(data: &Path, args: &[&str], input: &str) -> Output {
    run_at(data, T0, args, input)
}

/// `locora` at the time `now` (seconds since 1970).
fn run_at(data: &Path, now: &str, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_locora"))
        .arg("--data")
        .arg(data)
        .args(args)
        .env("LOCORA_NOW", now)
        .env("LOCORA_COMPETE_SCRIPTED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Err(err) = child.stdin.take().unwrap().write_all(input.as_bytes()) {
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe, "{err}");
    }
    child.wait_with_output().unwrap()
}

fn ok(data: &Path, args: &[&str], input: &str) -> String {
    ok_at(data, T0, args, input)
}

fn ok_at(data: &Path, now: &str, args: &[&str], input: &str) -> String {
    let out = run_at(data, now, args, input);
    assert!(
        out.status.success() && out.stderr.is_empty(),
        "{args:?}\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn fails(data: &Path, args: &[&str], input: &str) -> String {
    let out = run(data, args, input);
    assert!(!out.status.success(), "{args:?} should fail");
    String::from_utf8(out.stderr).unwrap()
}

/// The seed of the golden vectors (spec, appendix A).
fn seed() -> Seed {
    Seed::new(0x123_4567_89ab_cdef).unwrap()
}

fn task(discipline: Discipline, level: u8) -> Task {
    let spec = Spec::standard(discipline, Level::new(level).unwrap());
    generate(&spec, seed()).unwrap()
}

/// Enter ends memorizing, then one line per place (or per row of digits).
fn answers(task: &Task) -> String {
    format!("\n{}", recall(task, Lang::De))
}

/// One line per place (or per row of digits); the shopping list in `lang`.
fn recall(task: &Task, lang: Lang) -> String {
    let lines: Vec<String> = match task {
        Task::List(items) => items
            .iter()
            .rev()
            .map(|id| words::display(*id, lang).unwrap().to_owned())
            .collect(),
        Task::Numbers(digits) => digits
            .chunks(10)
            .map(|row| row.iter().map(u8::to_string).collect())
            .collect(),
        Task::Cards(deck) => deck.iter().map(|card| typed(*card)).collect(),
    };
    format!("{}\n", lines.join("\n"))
}

/// A card as typed in the terminal: `jd`, `10s`, `ah`.
fn typed(card: u8) -> String {
    let rank = match cards::rank(card) {
        9 => "10".to_owned(),
        rank => cards::rank_key(rank, Lang::En)
            .to_ascii_lowercase()
            .to_string(),
    };
    let suit = ['s', 'h', 'd', 'c'][usize::from(cards::suit(card))];
    format!("{rank}{suit}")
}

#[test]
fn a_code_round_of_each_discipline_scores_full_points_with_the_right_answers() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    // The code of the spec: cards at level 2, the golden deck.
    let deck = task(Discipline::Cards, 2);
    let Task::Cards(golden) = &deck else {
        unreachable!()
    };
    assert_eq!(golden[..3], [36, 18, 7]);
    let out = ok(
        d,
        &["compete", "play", "--code", "C2-28T5-CY4T-QKFF-K"],
        &answers(&deck),
    );
    assert!(out.contains("26 von 26 Punkten"), "{out}");
    assert!(out.contains("B♦ Karo Bube"), "the task in German: {out}");
    assert!(out.contains("Challenge-Code: C2-28T5-CY4T-QKFF-K"), "{out}");
    assert!(out.contains("Neue Trophäe: Erste Runde"), "{out}");
    // The same code again with two neighbours swapped: 24 of 26, and no record.
    let mut swapped = golden.clone();
    swapped.swap(1, 2);
    let out = ok(
        d,
        &["compete", "play", "cards", "--code", "c2 28t5 cy4t qkff k"],
        &answers(&Task::Cards(swapped)),
    );
    assert!(out.contains("24 von 26 Punkten"), "{out}");
    assert!(out.contains("Richtig bis zum ersten Fehler: 1"), "{out}");
    assert!(out.contains("Diesen Code hast du schon gespielt"), "{out}");
    assert!(out.contains("Dein Rekord: 26/26"), "{out}");
    // Numbers and the shopping list from the same seed; the list in any order.
    for (discipline, level, max) in [(Discipline::Numbers, 1, 20), (Discipline::List, 1, 10)] {
        let code = code::encode(discipline, Level::new(level).unwrap(), seed());
        let out = ok(
            d,
            &["compete", "play", "--code", &code],
            &answers(&task(discipline, level)),
        );
        assert!(out.contains(&format!("{max} von {max} Punkten")), "{out}");
    }
    // A code of another discipline or level than asked for is refused, as is a typo.
    let err = fails(
        d,
        &["compete", "play", "list", "--code", "C2-28T5-CY4T-QKFF-K"],
        "",
    );
    assert!(err.contains("gehört zur Disziplin „Karten“"), "{err}");
    let err = fails(d, &["compete", "play", "--code", "C2-28T5-CY4T-QKFF-X"], "");
    assert!(err.contains("Dieser Code ist ungültig"), "{err}");
}

#[test]
fn today_records_and_history_tell_what_was_played() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let today = ok(d, &["compete"], "");
    assert!(
        today.contains("Heute, Freitag 15.01.2027: Zahlen"),
        "{today}"
    );
    assert!(
        today.contains("Stufe 2 · 40 Ziffern · noch nicht gespielt"),
        "{today}"
    );
    assert!(today.contains("Morgen: Karten"), "{today}");
    let history: serde_json::Value =
        serde_json::from_str(&ok(d, &["compete", "history", "--json"], "")).unwrap();
    assert_eq!(history, serde_json::json!([]));
    assert!(ok(d, &["compete", "history"], "").contains("Noch keine Runde"));
    // A practice round of cards with nothing recalled, then the code of the spec.
    let out = ok(d, &["compete", "play", "cards"], "\n");
    assert!(out.contains("0 von 13 Punkten"), "{out}");
    assert!(out.contains("Challenge-Code: C1-"), "{out}");
    ok(
        d,
        &["compete", "play", "--code", "C2-28T5-CY4T-QKFF-K"],
        &answers(&task(Discipline::Cards, 2)),
    );
    let records: serde_json::Value =
        serde_json::from_str(&ok(d, &["compete", "records", "--json"], "")).unwrap();
    let levels = records["levels"].as_array().unwrap();
    assert_eq!(levels.len(), 9);
    let cards_2 = levels
        .iter()
        .find(|l| l["discipline"] == "cards" && l["level"] == 2)
        .unwrap();
    assert_eq!(cards_2["best"]["points"], 26);
    assert_eq!(cards_2["rounds"], 1);
    assert_eq!(records["trophies"][0]["id"], "compete-first");
    assert_eq!(records["trophies"][0]["unlocked"], "2027-01-15");
    assert_eq!(records["daily_days"], 0);
    let history: serde_json::Value =
        serde_json::from_str(&ok(d, &["compete", "history", "--json"], "")).unwrap();
    let kinds: Vec<&str> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["code", "practice"], "newest first");
    let text = ok(d, &["compete", "records"], "");
    assert!(
        text.contains("Trophäen: Erste Runde (15.01.2027)"),
        "{text}"
    );
    let code = ok(d, &["compete", "code", "list", "--level", "3"], "");
    assert!(code.starts_with("S3-"), "{code}");
}

#[test]
fn the_daily_challenge_is_one_try_per_level() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let err = fails(d, &["compete", "play", "cards", "--daily"], "");
    assert!(
        err.contains("Heute ist die Tages-Challenge Zahlen, nicht Karten"),
        "{err}"
    );
    let out = ok(d, &["compete", "play", "--daily"], "\n0123456789\n");
    assert!(out.contains("Zahlen · Stufe 1 · 20 Ziffern"), "{out}");
    assert!(out.contains("Abendabfrage: ab"), "{out}");
    assert!(
        !out.contains("Challenge-Code"),
        "a daily offers no code: {out}"
    );
    let err = fails(d, &["compete", "play", "--daily"], "\n");
    assert!(
        err.contains("Diese Tages-Challenge hast du heute schon gespielt"),
        "{err}"
    );
    // Another level is a try of its own.
    ok(d, &["compete", "play", "--daily", "--level", "2"], "\n");
    let today = ok(d, &["compete", "today"], "");
    assert!(
        today.contains("Stufe 3 · 80 Ziffern · noch nicht gespielt"),
        "{today}"
    );
    assert!(
        !today.contains("Stufe 1 · 20 Ziffern · noch nicht gespielt"),
        "{today}"
    );
    let records: serde_json::Value =
        serde_json::from_str(&ok(d, &["compete", "records", "--json"], "")).unwrap();
    assert_eq!(records["daily_days"], 1);
    // The evening recall opens six hours later.
    let evening = ok(d, &["compete", "evening"], "");
    assert!(
        evening.contains("Die Abendabfrage öffnet um 14:00"),
        "{evening}"
    );
}

#[test]
fn the_evening_recall_is_scored_in_the_language_of_the_morning_round() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    // A Monday: the daily is a shopping list, played in English.
    let monday = 1_800_259_200;
    let date = Date::parse("2027-01-18").unwrap();
    let level = Level::new(1).unwrap();
    let spec = Spec::standard(Discipline::List, level);
    let list = generate(&spec, daily_seed(date, Discipline::List, level)).unwrap();
    let args = ["compete", "play", "--daily", "--lang", "en"];
    let morning = format!("\n{}", recall(&list, Lang::En));
    let out = ok_at(d, &monday.to_string(), &args, &morning);
    assert!(out.contains("Einkaufsliste · Stufe 1 · 10 Dinge"), "{out}");
    assert!(out.contains("10 von 10 Punkten"), "{out}");
    // Seven hours later the same English answers keep all ten, without `--lang`.
    let later = (monday + 7 * 3_600).to_string();
    let out = ok_at(d, &later, &["compete", "evening"], &recall(&list, Lang::En));
    assert!(out.contains("Behalten: 10 von 10 (vorhin: 10)"), "{out}");
}

#[test]
fn deleting_the_game_progress_empties_the_contest_history() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(
        d,
        &["compete", "play", "--code", "C2-28T5-CY4T-QKFF-K"],
        &answers(&task(Discipline::Cards, 2)),
    );
    let before: serde_json::Value =
        serde_json::from_str(&ok(d, &["compete", "history", "--json"], "")).unwrap();
    assert_eq!(before.as_array().unwrap().len(), 1);
    ok(d, &["game", "reset", "--yes"], "");
    let after: serde_json::Value =
        serde_json::from_str(&ok(d, &["compete", "history", "--json"], "")).unwrap();
    assert_eq!(after, serde_json::json!([]));
    let records: serde_json::Value =
        serde_json::from_str(&ok(d, &["compete", "records", "--json"], "")).unwrap();
    let trophies = records["trophies"].as_array().unwrap();
    assert!(
        trophies.iter().all(|t| t["unlocked"].is_null()),
        "{records}"
    );
    // Played again after the deletion, the code is a first play and a record again.
    let out = ok(
        d,
        &["compete", "play", "--code", "C2-28T5-CY4T-QKFF-K"],
        &answers(&task(Discipline::Cards, 2)),
    );
    assert!(out.contains("Neuer persönlicher Rekord!"), "{out}");
}

#[test]
fn a_round_needs_a_terminal_unless_it_is_scripted() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_locora"))
        .arg("--data")
        .arg(dir.path())
        .args(["compete", "play", "cards"])
        .env("LOCORA_NOW", T0)
        .env_remove("LOCORA_COMPETE_SCRIPTED")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("braucht ein Terminal"), "{err}");
}

/// Ctrl-C while the task is shown leaves the alternate screen before the signal ends the
/// process. The terminal comes from util-linux `script`, so this runs on Linux only.
#[cfg(target_os = "linux")]
#[test]
fn ctrl_c_during_a_round_leaves_the_alternate_screen() {
    use std::io::Read;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().unwrap();
    let command = format!(
        "'{}' --data '{}' compete play cards",
        env!("CARGO_BIN_EXE_locora"),
        dir.path().display()
    );
    let mut child = Command::new("script")
        .args(["-qefc", &command, "/dev/null"])
        .env("LOCORA_NOW", T0)
        .env_remove("LOCORA_COMPETE_SCRIPTED")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("util-linux script");
    let mut stdout = child.stdout.take().unwrap();
    let (sender, chunks) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        while let Ok(n @ 1..) = stdout.read(&mut buffer) {
            if sender.send(buffer[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut out = Vec::new();
    let wait_for = |marker: &[u8], out: &mut Vec<u8>| {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !out.windows(marker.len()).any(|w| w == marker) {
            let left = deadline.saturating_duration_since(Instant::now());
            match chunks.recv_timeout(left) {
                Ok(chunk) => out.extend(chunk),
                Err(_) => panic!("no {marker:?} in {}", String::from_utf8_lossy(out)),
            }
        }
    };
    wait_for("Enter: fertig".as_bytes(), &mut out);
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"\x03").unwrap();
    wait_for(b"\x1b[?1049l", &mut out);
    let status = child.wait().unwrap();
    assert!(!status.success(), "the signal ends the round");
    let text = String::from_utf8_lossy(&out);
    let entered = text.find("\x1b[?1049h").unwrap();
    assert!(text.rfind("\x1b[?1049l").unwrap() > entered, "{text}");
}

/// A round whose output goes to a file is refused before the daily's try is used up (a
/// terminal for the input from util-linux `script`, so Linux only).
#[cfg(target_os = "linux")]
#[test]
fn a_round_needs_a_terminal_for_its_output_too() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let command = format!(
        "'{}' --data '{}' compete play --daily > '{}'",
        env!("CARGO_BIN_EXE_locora"),
        d.display(),
        d.join("task.txt").display()
    );
    let out = Command::new("script")
        .args(["-qefc", &command, "/dev/null"])
        .env("LOCORA_NOW", T0)
        .env_remove("LOCORA_COMPETE_SCRIPTED")
        .stdin(Stdio::null())
        .output()
        .expect("util-linux script");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("braucht ein Terminal"), "{text}");
    let today = ok(d, &["compete", "today"], "");
    assert!(
        today.contains("Stufe 1 · 20 Ziffern · noch nicht gespielt"),
        "{today}"
    );
}
