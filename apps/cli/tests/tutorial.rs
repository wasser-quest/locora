//! `locora tutorial` and pictures on loci (`--image`).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(data: &Path, args: &[&str], stdin: &str) -> Output {
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
    feed(child.stdin.take().unwrap(), stdin.as_bytes());
    child.wait_with_output().unwrap()
}

fn ok(data: &Path, args: &[&str], stdin: &str) -> String {
    let out = run(data, args, stdin);
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
fn tutorial_is_a_palace_with_pictures_at_every_place() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let out = ok(d, &["tutorial"], "");
    assert!(out.contains("3 Räume, 9 Orte"), "{out}");
    let shown = ok(d, &["show", "Locora lernen"], "");
    assert_eq!(
        shown.matches('▣').count(),
        9,
        "every place has a picture:\n{shown}"
    );
    assert_eq!(
        shown.matches('🖼').count(),
        3,
        "every room has a backdrop:\n{shown}"
    );
    assert!(ok(d, &["tutorial"], "").contains("schon da"));

    // Walk the whole route: six recalls, the quiz (answer 2) and the typed answer.
    let answers = format!("{}2\nlocora new\n\n3\n", "\n3\n".repeat(6));
    let out = ok(d, &["practice", "Locora lernen"], &answers);
    assert!(out.contains("9 von 9 geübt, 9 gewusst"), "{out}");

    // --reset builds it again from scratch.
    assert!(ok(d, &["tutorial", "--reset"], "").contains("angelegt"));
    assert!(ok(d, &["show", "Locora lernen"], "").contains("neu"));
    assert_eq!(ok(d, &["ls"], "").matches("Locora lernen").count(), 1);

    // The tutorial travels like any palace, pictures included.
    let file = d.join("tutorial.locora");
    let out = ok(
        d,
        &["export", "Locora lernen", "-o", file.to_str().unwrap()],
        "",
    );
    assert!(out.contains("9 Loci, 12 Medien"), "{out}");
}

#[test]
fn pictures_on_loci_from_the_command_line() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    std::fs::write(d.join("bild.sply"), b"\x89SPLY\r\n\x1a fake").unwrap();
    std::fs::write(d.join("falsch.sply"), b"<svg/>").unwrap();
    std::fs::write(d.join("film.mp4"), b"fake").unwrap();
    ok(d, &["new", "P", "--", "a"], "");
    ok(
        d,
        &[
            "add",
            "P",
            "b",
            "--image",
            d.join("bild.sply").to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(ok(d, &["show", "P"], "").matches('▣').count(), 1);
    ok(
        d,
        &[
            "edit",
            "P",
            "1",
            "--image",
            d.join("bild.sply").to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(ok(d, &["show", "P"], "").matches('▣').count(), 2);
    ok(d, &["edit", "P", "2", "--no-image"], "");
    assert_eq!(ok(d, &["show", "P"], "").matches('▣').count(), 1);
    for (file, expected) in [
        ("falsch.sply", "keine binäre .sply-Datei"),
        ("film.mp4", "kein Video"),
    ] {
        let out = run(
            d,
            &["edit", "P", "1", "--image", d.join(file).to_str().unwrap()],
            "",
        );
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(expected),
            "{file}"
        );
    }
}

#[test]
fn palaces_can_use_a_custom_marker() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    std::fs::write(d.join("stern.sply"), b"\x89SPLY\r\n\x1a star").unwrap();
    std::fs::write(d.join("stern.png"), b"\x89PNG").unwrap();
    ok(d, &["new", "P", "--", "a"], "");
    assert!(ok(d, &["marker", "P"], "").contains("Standard-Marker"));
    ok(
        d,
        &["marker", "P", d.join("stern.sply").to_str().unwrap()],
        "",
    );
    assert!(ok(d, &["marker", "P"], "").contains("eigenen Marker"));
    let out = run(
        d,
        &["marker", "P", d.join("stern.png").to_str().unwrap()],
        "",
    );
    assert!(!out.status.success());
    ok(d, &["marker", "P", "--reset"], "");
    assert!(ok(d, &["marker", "P"], "").contains("Standard-Marker"));
}
