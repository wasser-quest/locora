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

#[test]
fn export_and_import_between_profiles() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let image = a.path().join("kueche.png");
    std::fs::write(&image, b"\x89PNG not really").unwrap();
    ok(
        a.path(),
        T0,
        &[
            "new",
            "Küche",
            "--image",
            image.to_str().unwrap(),
            "--",
            "Herd",
            "Spüle",
        ],
        "",
    );
    let file = a.path().join("kueche.locora");
    let out = ok(
        a.path(),
        T0,
        &["export", "Küche", "-o", file.to_str().unwrap()],
        "",
    );
    assert!(out.contains("1 Palast, 2 Loci, 1 Medium"), "{out}");
    let again = run(
        a.path(),
        T0,
        &["export", "Küche", "-o", file.to_str().unwrap()],
        "",
    );
    assert!(String::from_utf8_lossy(&again.stderr).contains("gibt es schon"));

    let preview = ok(
        b.path(),
        T0,
        &["import", file.to_str().unwrap(), "--dry-run"],
        "",
    );
    assert!(
        preview.contains("Küche (2 Loci)") && preview.contains("nichts importiert"),
        "{preview}"
    );
    assert!(ok(b.path(), T0, &["ls"], "").contains("Noch keine Paläste"));
    ok(b.path(), T0, &["import", file.to_str().unwrap()], "");
    let out = ok(b.path(), T0, &["import", file.to_str().unwrap()], "");
    assert!(out.contains("als „Küche (2)“"), "{out}");
    let shown = ok(b.path(), T0, &["show", "Küche"], "");
    assert!(shown.contains("🖼") && shown.contains("Spüle"), "{shown}");
}

/// Write test input; a process that exits before reading it is fine.
fn feed(mut stdin: std::process::ChildStdin, input: &[u8]) {
    if let Err(err) = stdin.write_all(input) {
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe, "{err}");
    }
}

#[test]
fn locus_image_sizes_add_edit_and_invalid_values_are_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, T0, &["new", "Größen", "--", "Standard"], "");
    ok(d, T0, &["add", "Größen", "Bild", "--image-size", "0.4"], "");
    let sizes = || {
        locora_store::Store::open(d)
            .unwrap()
            .palace("Größen")
            .unwrap()
            .route()
            .map(|l| l.image_size)
            .collect::<Vec<_>>()
    };
    assert_eq!(sizes(), [0.25, 0.4]);
    ok(d, T0, &["edit", "Größen", "2", "--image-size", "0.05"], "");
    ok(d, T0, &["edit", "Größen", "1", "--note", "Merkhilfe"], "");
    assert_eq!(sizes(), [0.25, 0.05]);
    for value in ["0", "0.049", "1.01", "NaN", "inf"] {
        for args in [
            vec!["add", "Größen", "Ungültig", "--image-size", value],
            vec![
                "edit",
                "Größen",
                "2",
                "--item",
                "Nicht speichern",
                "--image-size",
                value,
            ],
        ] {
            let out = run(d, T0, &args, "");
            assert!(!out.status.success(), "{args:?}");
            assert!(String::from_utf8_lossy(&out.stderr).contains("Bildgröße"));
            assert_eq!(sizes(), [0.25, 0.05]);
        }
    }
    let store = locora_store::Store::open(d).unwrap();
    assert_eq!(
        store.palace("Größen").unwrap().route().nth(1).unwrap().item,
        "Bild"
    );
}

#[test]
fn zoom_on_add_and_edit() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, T0, &["new", "Zoom", "--", "Erster"], "");
    ok(d, T0, &["add", "Zoom", "Nah", "--zoom", "2,5"], "");
    ok(d, T0, &["edit", "Zoom", "1", "--zoom", "3"], "");
    let zooms = || {
        locora_store::Store::open(d)
            .unwrap()
            .palace("Zoom")
            .unwrap()
            .route()
            .map(|l| l.zoom)
            .collect::<Vec<_>>()
    };
    assert_eq!(zooms(), [3.0, 2.5]);
    let out = run(d, T0, &["edit", "Zoom", "2", "--zoom", "5"], "");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Zoom"));
    assert_eq!(zooms(), [3.0, 2.5]);
}
