//! Plugin management against real WASM fixtures; no management command activates guests.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;

use locora_store::Store;

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

fn wasm(name: &str) -> String {
    wasm_dir()
        .join(format!("{name}.wasm"))
        .to_str()
        .unwrap()
        .into()
}

fn run(data: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_locora"))
        .arg("--data")
        .arg(data)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    feed(child.stdin.take().unwrap(), input.as_bytes());
    child.wait_with_output().unwrap()
}

fn ok(data: &Path, args: &[&str]) -> String {
    let out = run(data, args, "");
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn refused(data: &Path, args: &[&str]) -> String {
    let out = run(data, args, "ja\n");
    assert!(!out.status.success(), "{args:?} unexpectedly succeeded");
    String::from_utf8(out.stderr).unwrap()
}

// Same-length replacements keep the component encoding valid and avoid extra
// fixture crates for every untrusted manifest variant.
fn patched(dir: &Path, old: &[u8], new: &[u8]) -> String {
    assert_eq!(old.len(), new.len());
    let mut bytes = fs::read(wasm("install_probe")).unwrap();
    let offsets: Vec<_> = bytes
        .windows(old.len())
        .enumerate()
        .filter_map(|(i, part)| (part == old).then_some(i))
        .collect();
    assert!(!offsets.is_empty());
    for i in offsets {
        bytes[i..i + old.len()].copy_from_slice(new);
    }
    let path = dir.join("patched.wasm");
    fs::write(&path, bytes).unwrap();
    path.to_str().unwrap().into()
}

#[cfg(unix)]
#[test]
fn nonregular_components_and_approvals_are_rejected_without_following_links() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("profile");
    let fifo = dir.path().join("source.wasm");
    let make_fifo = |path: &Path| {
        assert!(Command::new("mkfifo").arg(path).status().unwrap().success());
    };
    make_fifo(&fifo);
    assert!(
        refused(&d, &["plugin", "install", fifo.to_str().unwrap(), "--yes"])
            .contains("reguläre Plugin-Datei")
    );
    assert!(!d.exists());
    let probe = wasm("install_probe");
    ok(&d, &["plugin", "install", &probe, "--yes"]);
    let receipt = d.join("plugins/install-probe.wasm.json");
    fs::remove_file(&receipt).unwrap();
    make_fifo(&receipt);
    assert!(ok(&d, &["plugin", "ls"]).contains("Keine passende Installationszustimmung"));
    let out = run(&d, &["ls"], "");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("reguläre Plugin-Datei"));
    ok(&d, &["plugin", "remove", "install-probe"]);
    ok(&d, &["plugin", "install", &probe, "--yes"]);
    let component = d.join("plugins/install-probe.wasm");
    fs::remove_file(&component).unwrap();
    make_fifo(&component);
    assert!(ok(&d, &["plugin", "ls"]).contains("defekt/nicht lesbar"));
    let out = run(&d, &["ls"], "");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("reguläre Plugin-Datei"));
    ok(&d, &["plugin", "remove", "install-probe"]);
    let outside = dir.path().join("outside.wasm");
    fs::write(&outside, b"untouched").unwrap();
    symlink(&outside, d.join("plugins/link.wasm")).unwrap();
    ok(&d, &["plugin", "remove", "link.wasm"]);
    assert_eq!(fs::read(outside).unwrap(), b"untouched");
    let store = Store::open(&d).unwrap();
    assert!(
        store
            .kv_get("install-probe", "activated")
            .unwrap()
            .is_none()
    );
}

#[test]
fn refusal_check_and_listing_do_not_create_profile_or_activate() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("untouched");
    let probe = wasm("install_probe");
    let err = refused(&data, &["plugin", "install", &probe]);
    assert!(
        err.contains("--yes")
            && err.contains("palaces:write")
            && err.contains("eigene Verantwortung")
    );
    assert!(!data.exists());
    assert!(ok(&data, &["plugin", "check", &probe]).contains("install-probe"));
    assert!(ok(&data, &["plugin", "ls"]).contains("core [eingebaut"));
    assert!(!data.exists());
    // Activation always traps, yet inspection and even approved installation succeed.
    ok(&data, &["plugin", "install", &probe, "--yes"]);
    assert!(ok(&data, &["plugin", "ls"]).contains("install-probe"));
    assert_eq!(fs::read_dir(&data).unwrap().count(), 1); // plugins only; no database
    assert_eq!(fs::read_dir(data.join("plugins")).unwrap().count(), 2);
    ok(&data, &["plugin", "remove", "install-probe"]);
    assert_eq!(fs::read_dir(data.join("plugins")).unwrap().count(), 0);
}

#[test]
fn rejects_unsafe_ids_unsupported_permissions_api_and_builtins_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("untouched");
    let bad = patched(dir.path(), b"install-probe", b"../evil-probe");
    assert!(refused(&data, &["plugin", "install", &bad, "--yes"]).contains("Unsichere Plugin-ID"));
    let bad = patched(dir.path(), b"storage", b"network");
    assert!(
        refused(&data, &["plugin", "install", &bad, "--yes"])
            .contains("Nicht unterstützte Berechtigung")
    );
    let bad = patched(dir.path(), b"0.4.123", b"0.4.bad");
    assert!(
        refused(&data, &["plugin", "install", &bad, "--yes"]).contains("diese App spricht API")
    );
    let bad = patched(dir.path(), b"0.4.123", b"0.3.123");
    assert!(
        refused(&data, &["plugin", "install", &bad, "--yes"]).contains("diese App spricht API")
    );
    for name in ["core", "review"] {
        assert!(refused(&data, &["plugin", "install", &wasm(name), "--yes"]).contains("geschützt"));
        assert!(refused(&data, &["plugin", "remove", name]).contains("geschützt"));
    }
    assert!(!data.exists());
}

#[test]
fn duplicates_aliases_and_existing_destinations_never_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let source = wasm("lernzeit");
    ok(d, &["plugin", "install", &source, "--yes"]);
    let installed = d.join("plugins/lernzeit.wasm");
    let bytes = fs::read(&installed).unwrap();
    refused(d, &["plugin", "install", &source, "--yes"]);
    assert_eq!(fs::read(&installed).unwrap(), bytes);
    fs::rename(&installed, d.join("plugins/alias.wasm")).unwrap();
    fs::remove_file(d.join("plugins/lernzeit.wasm.json")).unwrap();
    assert!(refused(d, &["plugin", "install", &source, "--yes"]).contains("bereits installiert"));
    fs::write(&installed, b"broken").unwrap();
    refused(d, &["plugin", "install", &source, "--yes"]);
    assert_eq!(fs::read(&installed).unwrap(), b"broken");
    let text = ok(d, &["plugin", "ls"]);
    assert!(text.contains("alias.wasm") && text.contains("defekt/nicht lesbar"));
    assert!(text.contains("Keine passende Installationszustimmung"));
    ok(d, &["plugin", "remove", "lernzeit.wasm"]);
    ok(d, &["plugin", "remove", "alias.wasm"]);
    assert_eq!(fs::read_dir(d.join("plugins")).unwrap().count(), 0);
}

#[test]
fn remove_handles_broken_files_without_activation_and_rejects_paths() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    fs::create_dir(d.join("plugins")).unwrap();
    fs::write(d.join("outside.wasm"), b"safe").unwrap();
    for path in [
        "../outside",
        "../outside.wasm",
        "/outside",
        "a/b",
        "a\\b",
        "..",
        ".",
    ] {
        refused(d, &["plugin", "remove", path]);
    }
    assert_eq!(fs::read(d.join("outside.wasm")).unwrap(), b"safe");
    // A broken profile copy under a built-in name isn't the embedded component.
    for name in ["core", "review", "broken"] {
        fs::write(d.join(format!("plugins/{name}.wasm")), b"bad").unwrap();
        ok(d, &["plugin", "remove", name]);
    }
    fs::write(d.join("plugins/orphan.wasm.json"), b"bad").unwrap();
    ok(d, &["plugin", "remove", "orphan"]);
    assert!(ok(d, &["plugin", "ls"]).contains("core [eingebaut, geschützt"));
    assert_eq!(fs::read_dir(d.join("plugins")).unwrap().count(), 0);
}

#[test]
fn approved_plugins_load_on_profile_commands_and_removal_retains_kv() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, &["plugin", "install", &wasm("lernzeit"), "--yes"]);
    ok(d, &["new", "P", "--", "A"]);
    let out = run(d, &["practice", "P"], "\n3\n");
    assert!(
        out.status.success() && out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let store = Store::open(d).unwrap();
    assert!(store.kv_get("lernzeit", "seconds").unwrap().is_some());
    let before = store.kv_get("lernzeit", "seconds").unwrap();
    ok(d, &["plugin", "remove", "lernzeit"]);
    assert_eq!(store.kv_get("lernzeit", "seconds").unwrap(), before);
    assert_eq!(store.palaces().unwrap().len(), 1);
}

#[test]
fn changed_approval_and_unapproved_manual_copies_are_never_activated() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, &["plugin", "install", &wasm("install_probe"), "--yes"]);
    let path = d.join("plugins/install-probe.wasm.json");
    let mut approval: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    approval["permissions"] = serde_json::json!([]);
    fs::write(&path, serde_json::to_vec(&approval).unwrap()).unwrap();
    let out = run(d, &["ls"], "");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("bestätigten Installation"));
    let store = Store::open(d).unwrap();
    assert!(
        store
            .kv_get("install-probe", "activated")
            .unwrap()
            .is_none()
    );
    assert!(store.palaces().unwrap().is_empty());
    fs::remove_file(&path).unwrap();
    let out = run(d, &["ls"], "");
    assert!(String::from_utf8_lossy(&out.stderr).contains("Keine Installationszustimmung"));
    assert!(
        store
            .kv_get("install-probe", "activated")
            .unwrap()
            .is_none()
    );
    // Neither check nor metadata listing should touch the profile either.
    ok(d, &["plugin", "check", &wasm("install_probe")]);
    ok(d, &["plugin", "ls"]);
    ok(d, &["plugin", "remove", "install-probe"]);
    assert!(
        store
            .kv_get("install-probe", "activated")
            .unwrap()
            .is_none()
    );
}

#[test]
fn remove_resolves_manifest_ids_instead_of_unrelated_file_stems() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let plugins = d.join("plugins");
    fs::create_dir(&plugins).unwrap();
    fs::copy(wasm("lernzeit"), plugins.join("renamed.wasm")).unwrap();
    fs::copy(wasm("fixture"), plugins.join("lernzeit.wasm")).unwrap();
    ok(d, &["plugin", "remove", "lernzeit"]);
    assert!(!plugins.join("renamed.wasm").exists());
    assert!(plugins.join("lernzeit.wasm").exists());
    assert!(refused(d, &["plugin", "remove", "lernzeit"]).contains("Dateiname"));
    assert!(plugins.join("lernzeit.wasm").exists());
    ok(d, &["plugin", "remove", "lernzeit.wasm"]);
    for name in ["first.wasm", "second.wasm"] {
        fs::copy(wasm("lernzeit"), plugins.join(name)).unwrap();
    }
    assert!(refused(d, &["plugin", "remove", "lernzeit"]).contains("Mehrere"));
    assert!(plugins.join("first.wasm").exists());
    assert!(plugins.join("second.wasm").exists());
    ok(d, &["plugin", "remove", "first.wasm"]);
    ok(d, &["plugin", "remove", "lernzeit"]);
    assert_eq!(fs::read_dir(plugins).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn symlinked_plugin_directory_never_writes_or_removes_outside_profile() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("profile");
    let outside = dir.path().join("outside");
    fs::create_dir(&data).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel.wasm"), b"untouched").unwrap();
    fs::write(outside.join("sentinel.wasm.json"), b"approval").unwrap();
    symlink(&outside, data.join("plugins")).unwrap();
    let source = wasm("lernzeit");
    for args in [
        vec!["plugin", "install", &source, "--yes"],
        vec!["plugin", "remove", "sentinel.wasm"],
        vec!["plugin", "ls"],
        vec!["ls"],
    ] {
        assert!(refused(&data, &args).contains("Plugin-Ordner"));
    }
    assert_eq!(
        fs::read(outside.join("sentinel.wasm")).unwrap(),
        b"untouched"
    );
    assert_eq!(
        fs::read(outside.join("sentinel.wasm.json")).unwrap(),
        b"approval"
    );
    assert_eq!(fs::read_dir(outside).unwrap().count(), 2);
    assert!(!data.join("locora.db").exists());
}

#[cfg(unix)]
#[test]
fn runtime_never_activates_a_component_rejected_as_a_symlink() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("profile");
    ok(
        &data,
        &["plugin", "install", &wasm("install_probe"), "--yes"],
    );
    let component = data.join("plugins/install-probe.wasm");
    let outside = dir.path().join("outside.wasm");
    fs::rename(&component, &outside).unwrap();
    let bytes = fs::read(&outside).unwrap();
    symlink(&outside, &component).unwrap();
    assert!(ok(&data, &["plugin", "ls"]).contains("defekt/nicht lesbar"));
    let out = run(&data, &["ls"], "");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("reguläre Plugin-Datei"));
    let store = Store::open(&data).unwrap();
    assert!(
        store
            .kv_get("install-probe", "activated")
            .unwrap()
            .is_none()
    );
    ok(&data, &["plugin", "remove", "install-probe.wasm"]);
    assert_eq!(fs::read(outside).unwrap(), bytes);
}

/// Write test input; a process that exits before reading it is fine.
fn feed(mut stdin: std::process::ChildStdin, input: &[u8]) {
    if let Err(err) = stdin.write_all(input) {
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe, "{err}");
    }
}
