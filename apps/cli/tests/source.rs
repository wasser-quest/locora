//! `locora build` and `locora dump` with real profiles.

use std::io::Write;
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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn ok(data: &Path, args: &[&str]) -> String {
    let out = run(data, args, "");
    assert!(
        out.status.success(),
        "{args:?}\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn fail(data: &Path, args: &[&str]) -> String {
    let out = run(data, args, "");
    assert!(!out.status.success(), "{args:?} unexpectedly succeeded");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

const SOURCE: &str = r#"
name = "Planeten"

[[room]]
name = "Wohnung"
image = "wohnung.png"
items = ["Merkur", "Venus"]

[[room]]
name = "Garten"

[[room.locus]]
place = "Baum"
item = "Mars"
answer = { type = "quiz", correct = ["Mars"], wrong = ["Venus"] }
"#;

#[test]
fn build_check_replace_and_dump() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let profile = d.join("profil");
    std::fs::write(d.join("wohnung.png"), b"\x89PNG fake").unwrap();
    let file = d.join("planeten.locora.toml");
    std::fs::write(&file, SOURCE).unwrap();
    let f = file.to_str().unwrap();

    let out = ok(&profile, &["build", f, "--check"]);
    assert!(
        out.contains("2 Räume, 3 Loci") && out.contains("nur geprüft"),
        "{out}"
    );
    assert!(!profile.exists(), "--check never touches the profile");

    assert!(ok(&profile, &["build", f]).contains("gebaut"));
    let shown = ok(&profile, &["show", "Planeten"]);
    assert!(
        shown.contains("Merkur") && shown.contains("Baum") && shown.contains("Mars"),
        "{shown}"
    );
    assert!(fail(&profile, &["build", f]).contains("--replace"));

    // Replacing swaps the content and keeps exactly one palace of that name.
    std::fs::write(
        &file,
        SOURCE.replace("\"Venus\"]\n", "\"Venus\", \"Erde\"]\n"),
    )
    .unwrap();
    ok(&profile, &["build", f, "--replace", "--yes"]);
    let list = ok(&profile, &["ls"]);
    assert_eq!(list.matches("Planeten").count(), 1, "{list}");
    assert!(ok(&profile, &["show", "Planeten"]).contains("Erde"));

    // Dump and rebuild in another profile.
    let out_dir = d.join("quelle");
    let out = ok(
        &profile,
        &["dump", "Planeten", "-o", out_dir.to_str().unwrap()],
    );
    assert!(out.contains("planeten.locora.toml"), "{out}");
    let other = d.join("anderes");
    ok(
        &other,
        &[
            "build",
            out_dir.join("planeten.locora.toml").to_str().unwrap(),
        ],
    );
    let shown = ok(&other, &["show", "Planeten"]);
    assert!(shown.contains("Erde") && shown.contains("🖼"), "{shown}");

    let err = fail(&profile, &["build", d.join("fehlt.toml").to_str().unwrap()]);
    assert!(err.contains("fehlt.toml"), "{err}");
}
