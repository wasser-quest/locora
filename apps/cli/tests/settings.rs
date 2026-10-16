//! `locora settings`, the daily limit for new loci, `locora stats` and `locora optimize`.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const T0: u64 = 1_800_000_000;
const DAY: u64 = 86_400;

fn run(data: &Path, now: u64, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_locora"))
        .arg("--data")
        .arg(data)
        .args(args)
        .env("LOCORA_NOW", now.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    feed(child.stdin.take().unwrap(), stdin.as_bytes());
    child.wait_with_output().unwrap()
}

fn ok(data: &Path, now: u64, args: &[&str], stdin: &str) -> String {
    let out = run(data, now, args, stdin);
    assert!(
        out.status.success(),
        "{args:?}\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Write test input; a process that exits before reading it is fine.
fn feed(mut stdin: std::process::ChildStdin, input: &[u8]) {
    if let Err(err) = stdin.write_all(input) {
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe, "{err}");
    }
}

#[test]
fn daily_new_limit_settings_and_stats() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let shown = ok(d, T0, &["settings"], "");
    assert!(
        shown.contains("Neue Loci pro Tag:     10")
            && shown.contains("90 %")
            && shown.contains("Standard"),
        "{shown}"
    );

    ok(d, T0, &["new", "Liste", "--", "a", "b", "c", "d", "e"], "");
    ok(d, T0, &["settings", "new-per-day", "2"], "");
    let out = ok(d, T0, &["review"], "\n3\n\n3\n\n3\n");
    assert!(
        out.contains("2 Loci") && out.contains("2 von 2 geübt"),
        "{out}"
    );
    // Same day: the budget is used up, nothing new starts.
    let out = ok(d, T0 + 60, &["review"], "");
    assert!(out.contains("Nichts fällig"), "{out}");
    // An explicit --new still overrides the limit.
    assert!(ok(d, T0 + 60, &["review", "--new", "1"], "\n3\n").contains("1 von 1 geübt"));
    // Next day the budget is back.
    assert!(ok(d, T0 + DAY, &["review"], "\n3\n\n3\n").contains("2 von 2 geübt"));

    let out = ok(d, T0 + DAY, &["settings", "retention", "85 %"], "");
    assert!(out.contains("85 %"), "{out}");
    assert!(ok(d, T0, &["settings"], "").contains("Ziel-Behaltensrate:    85 %"));
    let bad = run(d, T0, &["settings", "retention", "0,5"], "");
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("0,70 und 0,99"));

    let stats = ok(d, T0 + DAY, &["stats"], "");
    assert!(stats.contains("Antworten insgesamt:   5"), "{stats}");
    assert!(
        stats.contains("Heute:                 2 Antworten, 2 neue Loci"),
        "{stats}"
    );
    assert!(stats.contains("Serie:                 2 Tage"), "{stats}");
    assert!(stats.contains("Trefferquote 30 Tage:  100 %"), "{stats}");

    assert!(ok(d, T0 + DAY, &["optimize"], "").contains("Noch zu wenig Daten"));
}

#[test]
fn optimize_learns_from_enough_history() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let items: Vec<String> = (1..=25).map(|i| format!("Ding {i}")).collect();
    let mut args = vec!["new", "Viel", "--"];
    args.extend(items.iter().map(String::as_str));
    ok(d, T0, &args, "");
    // Five rounds over all loci, two days apart; every other answer is forgotten.
    for round in 0..5u64 {
        let answers: String = (0..25)
            .map(|i| {
                if (i + round) % 2 == 0 {
                    "\n3\n"
                } else {
                    "\n1\n"
                }
            })
            .collect();
        let out = ok(d, T0 + round * 2 * DAY, &["practice", "Viel"], &answers);
        assert!(out.contains("25 von 25 geübt"), "{out}");
    }
    let out = ok(d, T0 + 10 * DAY, &["optimize", "--dry-run"], "");
    assert!(
        out.contains("Aus 100 Wiederholungen") && out.contains("nichts geändert"),
        "{out}"
    );
    assert!(ok(d, T0, &["settings"], "").contains("Standard"));
    let out = ok(d, T0 + 10 * DAY, &["optimize"], "");
    assert!(
        out.contains("gespeichert") || out.contains("passen schon"),
        "{out}"
    );
    if out.contains("gespeichert") {
        assert!(ok(d, T0, &["settings"], "").contains("persönlich"));
    }
    assert!(ok(d, T0, &["optimize", "--reset"], "").contains("zurückgesetzt"));
    assert!(ok(d, T0, &["settings"], "").contains("FSRS-Gewichte:         Standard"));
}
