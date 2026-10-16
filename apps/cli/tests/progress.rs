//! `locora export --with-progress` / `locora import`: the review schedule travels with the palace.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};

const T0: u64 = 1_800_000_000;

fn run(data: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_locora"))
        .arg("--data")
        .arg(data)
        .args(args)
        .env("LOCORA_NOW", T0.to_string())
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

/// Replace one entry of a .locora archive (keeping order and the stored mimetype).
fn patch(file: &Path, name: &str, edit: impl FnOnce(&str) -> String) {
    let mut edit = Some(edit);
    let bytes = std::fs::read(file).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let entry_name = entry.name().unwrap().into_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        if entry_name == name {
            let edit = edit.take().expect("one matching entry");
            data = edit(std::str::from_utf8(&data).unwrap()).into_bytes();
        }
        let method = if entry_name == "mimetype" {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        };
        out.start_file(
            entry_name.as_str(),
            zip::write::SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        out.write_all(&data).unwrap();
    }
    std::fs::write(file, out.finish().unwrap().into_inner()).unwrap();
}

#[test]
fn progress_travels_and_can_be_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b, c) = (
        dir.path().join("a"),
        dir.path().join("b"),
        dir.path().join("c"),
    );
    ok(
        &a,
        &["new", "Planeten", "--", "Merkur", "Venus", "Erde"],
        "",
    );
    // Good, Again; Erde stays new.
    ok(&a, &["practice", "Planeten"], "\n3\n\n1\nq\n");
    let before = ok(&a, &["show", "Planeten"], "");
    assert!(
        before.contains("in 3 Tagen") && before.contains("in 10 Min."),
        "{before}"
    );

    let file = dir.path().join("planeten.locora");
    let f = file.to_str().unwrap();
    let out = ok(&a, &["export", "Planeten", "-o", f, "--with-progress"], "");
    assert!(out.contains("mit Wiederholungsstand für 2 Loci"), "{out}");

    let out = ok(&b, &["import", f], "");
    assert!(
        out.contains("Wiederholungsstand für 2 Loci übernommen"),
        "{out}"
    );
    let after = ok(&b, &["show", "Planeten"], "");
    let status = |text: &str| {
        text.lines()
            .filter(|l| l.contains('→'))
            .map(|l| l.split_whitespace().last().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(status(&after), status(&before), "{after}");

    let out = ok(&c, &["import", f, "--without-progress"], "");
    assert!(!out.contains("übernommen"), "{out}");
    assert_eq!(
        status(&ok(&c, &["show", "Planeten"], "")),
        ["neu", "neu", "neu"]
    );

    // Without the flag nothing about progress is written.
    let plain = dir.path().join("ohne.locora");
    let out = ok(
        &a,
        &["export", "Planeten", "-o", plain.to_str().unwrap()],
        "",
    );
    assert!(!out.contains("Wiederholungsstand"), "{out}");
}

#[test]
fn broken_progress_is_rejected_before_importing() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    ok(&a, &["new", "Planeten", "--", "Merkur"], "");
    ok(&a, &["practice", "Planeten"], "\n3\n");
    let file = dir.path().join("p.locora");
    ok(
        &a,
        &[
            "export",
            "Planeten",
            "-o",
            file.to_str().unwrap(),
            "--with-progress",
        ],
        "",
    );

    for (edit, expected) in [
        ("\"difficulty\": 99", "ungültige Werte"),
        ("\"locus\": \"gibts-nicht\"", "gibt es in der Datei nicht"),
        ("\"surprise\": 1, \"version\": 1", "ungültig"),
    ] {
        let broken = dir.path().join(format!("kaputt-{}.locora", expected.len()));
        std::fs::copy(&file, &broken).unwrap();
        patch(&broken, "plugins/review/progress.json", |text| {
            let key = edit.split(':').next().unwrap();
            if key == "\"surprise\"" {
                text.replacen("\"version\": 1", edit, 1)
            } else {
                let start = text.find(key).unwrap();
                let end = start + text[start..].find([',', '\n']).unwrap();
                format!("{}{}{}", &text[..start], edit, &text[end..])
            }
        });
        let out = run(&b, &["import", broken.to_str().unwrap()], "");
        assert!(!out.status.success());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(expected), "{edit}: {err}");
        assert!(
            ok(&b, &["ls"], "").contains("Noch keine Paläste"),
            "nothing imported"
        );
    }
}

/// Write test input; a process that exits before reading it is fine.
fn feed(mut stdin: std::process::ChildStdin, input: &[u8]) {
    if let Err(err) = stdin.write_all(input) {
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe, "{err}");
    }
}
