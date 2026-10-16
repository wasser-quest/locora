//! End-to-end: the `locora` binary with a temporary profile and the built-in
//! review plugin. `LOCORA_NOW` pins the clock.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const T0: u64 = 1_800_000_000;

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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
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
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains('⚠'), "{args:?}: {stderr}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn create_practise_and_review_with_spaced_repetition() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let out = ok(
        d,
        T0,
        &["new", "Planeten", "--", "Merkur", "Venus", "Erde"],
        "",
    );
    assert!(out.contains("angelegt: 3 Loci"), "{out}");
    assert!(ok(d, T0, &["due"], "").contains("Planeten: 3 neu"));

    // Good, Again, Easy
    let out = ok(d, T0, &["practice", "Planeten"], "\n3\n\n1\n\n4\n");
    assert!(out.contains("3 von 3 geübt, 2 gewusst"), "{out}");
    let shown = ok(d, T0, &["show", "Planeten"], "");
    assert!(
        shown.contains("Merkur") && shown.contains("in 3 Tagen"),
        "{shown}"
    );
    assert!(shown.contains("in 10 Min."), "{shown}");

    // Ten minutes later only the forgotten locus is due.
    let later = T0 + 700;
    assert!(ok(d, later, &["due"], "").contains("Planeten: 1 fällig"));
    let out = ok(d, later, &["review"], "\n3\n");
    assert!(out.contains("1 Locus") && out.contains("Venus"), "{out}");
    assert!(ok(d, later, &["due"], "").contains("Nichts fällig. Nächste Wiederholung in"));

    // Deleting a locus also forgets its schedule.
    ok(d, later, &["rm", "Planeten", "1", "--yes"], "");
    let shown = ok(d, later, &["show", "Planeten"], "");
    assert!(!shown.contains("Merkur"), "{shown}");
}

#[test]
fn palaces_from_files_with_labels_positions_and_video_times() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let list = d.join("vokabeln.txt");
    std::fs::write(
        &list,
        "# Küche\nHerd | der Herd | 0.2,0.3\nSpüle | die Spüle\nTisch\n",
    )
    .unwrap();
    ok(
        d,
        T0,
        &["new", "Vokabeln", "--from", list.to_str().unwrap()],
        "",
    );
    let shown = ok(d, T0, &["show", "vokabeln"], "");
    assert!(
        shown.contains("Herd") && shown.contains("der Herd"),
        "{shown}"
    );
    assert!(
        shown.contains("Ort 3") && shown.contains("Tisch"),
        "{shown}"
    );

    let video = d.join("vortrag.mp4");
    std::fs::write(&video, b"not really a video").unwrap();
    let out = run(
        d,
        T0,
        &[
            "new",
            "Vortrag",
            "--video",
            video.to_str().unwrap(),
            "--",
            "Einleitung",
        ],
        "",
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("braucht im Video-Palast eine Zeit"));
    ok(
        d,
        T0,
        &[
            "new",
            "Vortrag",
            "--video",
            video.to_str().unwrap(),
            "--",
            "0:12=Einleitung",
            "1:23=These",
        ],
        "",
    );
    let shown = ok(d, T0, &["show", "Vortrag"], "");
    assert!(shown.contains("🎞") && shown.contains(" 1:23 "), "{shown}");

    let out = run(d, T0, &["show", "Gibtsnicht"], "");
    assert!(String::from_utf8_lossy(&out.stderr).contains("nicht gefunden"));
}
