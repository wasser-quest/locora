//! Signed plugins: unknown and trusted publishers, updates, and refusals.
//! Uses the install probe, which must never be activated, so no test command
//! here opens the profile after an install.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

fn wasm_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let plugins = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        assert!(
            Command::new(env!("CARGO"))
                .args(["build", "--release", "--target", "wasm32-wasip2"])
                .current_dir(&plugins)
                .status()
                .unwrap()
                .success()
        );
        plugins.join("target/wasm32-wasip2/release")
    })
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_locora"))
        .args(args)
        .output()
        .unwrap()
}

fn ok(args: &[&str]) -> String {
    let out = run(args);
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn refused(args: &[&str]) -> String {
    let out = run(args);
    assert!(!out.status.success(), "{args:?} must be refused");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// Writes a copy of the probe with a different version and returns its path.
fn version(dir: &Path, name: &str, version: &str) -> PathBuf {
    let mut bytes = fs::read(wasm_dir().join("install_probe.wasm")).unwrap();
    let old = b"0.1.0";
    let new = version.as_bytes();
    assert_eq!(new.len(), old.len());
    let offsets: Vec<_> = bytes
        .windows(old.len())
        .enumerate()
        .filter_map(|(i, part)| (part == old).then_some(i))
        .collect();
    assert!(!offsets.is_empty());
    for i in offsets {
        bytes[i..i + old.len()].copy_from_slice(new);
    }
    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn publisher_key(dir: &Path, name: &str) -> (PathBuf, String) {
    let key = dir.join(name);
    let text = ok(&["plugin", "keygen", s(&key)]);
    let public = text
        .lines()
        .find_map(|line| line.strip_prefix("  Öffentlicher Schlüssel: "))
        .unwrap()
        .trim()
        .to_owned();
    (key, public)
}

#[test]
fn signed_plugins_install_trust_and_update_only_from_their_publisher() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let data = d.join("profile");
    let data_s = s(&data);
    let (key, public) = publisher_key(d, "publisher.key");
    let (other_key, _) = publisher_key(d, "other.key");
    let v1 = version(d, "probe.wasm", "0.1.0");
    ok(&["plugin", "sign", s(&v1), "--key", s(&key)]);
    assert!(d.join("probe.wasm.sig").exists());

    // Unknown publisher: possible only with explicit consent.
    let text = refused(&["--data", data_s, "plugin", "install", s(&v1)]);
    assert!(text.contains("Keine interaktive Zustimmung"), "{text}");
    let text = ok(&["--data", data_s, "plugin", "install", s(&v1), "--yes"]);
    assert!(text.contains("unbekannten Herausgeber"), "{text}");
    let receipt = fs::read_to_string(data.join("plugins/install-probe.wasm.json")).unwrap();
    assert!(receipt.contains(&public), "approval records the signer");

    // Trusting the publisher is shown in later installs of the same signer.
    ok(&[
        "--data",
        data_s,
        "plugin",
        "trust",
        &public,
        "--name",
        "Probe-Verlag",
    ]);
    assert!(ok(&["--data", data_s, "plugin", "publishers"]).contains("Probe-Verlag"));
    ok(&["--data", data_s, "plugin", "remove", "install-probe.wasm"]);
    let text = ok(&["--data", data_s, "plugin", "install", s(&v1), "--yes"]);
    assert!(text.contains("vertrauenswürdig"), "{text}");

    // A newer version from the same publisher updates the installed plugin.
    let v2 = version(d, "probe-2.wasm", "0.2.0");
    ok(&["plugin", "sign", s(&v2), "--key", s(&key)]);
    assert!(ok(&["--data", data_s, "plugin", "update", s(&v2), "--yes"]).contains("aktualisiert"));
    let receipt = fs::read_to_string(data.join("plugins/install-probe.wasm.json")).unwrap();
    assert!(receipt.contains("0.2.0"));

    // Same version again, a foreign signature, and no signature are all refused.
    let same = version(d, "probe-same.wasm", "0.2.0");
    ok(&["plugin", "sign", s(&same), "--key", s(&key)]);
    let text = refused(&["--data", data_s, "plugin", "update", s(&same), "--yes"]);
    assert!(text.contains("nicht neuer"), "{text}");
    let foreign = version(d, "probe-foreign.wasm", "0.3.0");
    ok(&["plugin", "sign", s(&foreign), "--key", s(&other_key)]);
    let text = refused(&["--data", data_s, "plugin", "update", s(&foreign), "--yes"]);
    assert!(text.contains("anderen Herausgeber"), "{text}");
    let unsigned = version(d, "probe-unsigned.wasm", "0.3.0");
    let text = refused(&["--data", data_s, "plugin", "update", s(&unsigned), "--yes"]);
    assert!(text.contains("braucht eine Signatur"), "{text}");

    // A signature that does not match its file is refused at install time too.
    let tampered = version(d, "probe-tampered.wasm", "0.4.0");
    fs::copy(
        d.join("probe-2.wasm.sig"),
        d.join("probe-tampered.wasm.sig"),
    )
    .unwrap();
    let text = refused(&["--data", data_s, "plugin", "update", s(&tampered), "--yes"]);
    assert!(text.contains("Signatur passt nicht zur Datei"), "{text}");
}

#[test]
fn unsigned_installs_cannot_be_updated_and_keys_stay_private() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let data = d.join("profile");
    let data_s = s(&data);
    let (key, _) = publisher_key(d, "publisher.key");
    let v1 = version(d, "probe.wasm", "0.1.0");
    ok(&["--data", data_s, "plugin", "install", s(&v1), "--yes"]);
    let v2 = version(d, "probe-2.wasm", "0.2.0");
    ok(&["plugin", "sign", s(&v2), "--key", s(&key)]);
    let text = refused(&["--data", data_s, "plugin", "update", s(&v2), "--yes"]);
    assert!(text.contains("nicht signiert installiert"), "{text}");

    // Key files readable by others are not used for signing on Unix.
    #[cfg(unix)]
    {
        fs::set_permissions(&key, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
        let text = refused(&["plugin", "sign", s(&v2), "--key", s(&key)]);
        assert!(text.contains("chmod 600"), "{text}");
    }
}

#[test]
fn update_refuses_a_changed_missing_or_nonregular_installed_component() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let data = d.join("profile");
    let (key, _) = publisher_key(d, "publisher.key");
    let v1 = version(d, "probe.wasm", "0.1.0");
    ok(&["plugin", "sign", s(&v1), "--key", s(&key)]);
    ok(&["--data", s(&data), "plugin", "install", s(&v1), "--yes"]);
    let v2 = version(d, "probe-2.wasm", "0.2.0");
    ok(&["plugin", "sign", s(&v2), "--key", s(&key)]);
    let destination = data.join("plugins/install-probe.wasm");
    let receipt = destination.with_extension("wasm.json");
    let approved = fs::read(&receipt).unwrap();
    fs::write(&destination, b"changed").unwrap();
    let text = refused(&["--data", s(&data), "plugin", "update", s(&v2), "--yes"]);
    assert!(text.contains("passt nicht zur Freigabe"), "{text}");
    assert_eq!(fs::read(&destination).unwrap(), b"changed");
    fs::remove_file(&destination).unwrap();
    refused(&["--data", s(&data), "plugin", "update", s(&v2), "--yes"]);
    fs::create_dir(&destination).unwrap();
    let text = refused(&["--data", s(&data), "plugin", "update", s(&v2), "--yes"]);
    assert!(text.contains("Keine reguläre Plugin-Datei"), "{text}");
    fs::remove_dir(&destination).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&v1, &destination).unwrap();
        let text = refused(&["--data", s(&data), "plugin", "update", s(&v2), "--yes"]);
        assert!(text.contains("Keine reguläre Plugin-Datei"), "{text}");
        assert_eq!(fs::read(&destination).unwrap(), fs::read(&v1).unwrap());
    }
    assert_eq!(fs::read(&receipt).unwrap(), approved);
}
