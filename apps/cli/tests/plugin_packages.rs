//! Package round trips and hostile archive structure checks.
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn probe() -> &'static PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let plugins = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        assert!(
            Command::new(env!("CARGO"))
                .args(["build", "--release", "--target", "wasm32-wasip2"])
                .current_dir(&plugins)
                .status()
                .unwrap()
                .success()
        );
        std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| plugins.join("target"))
            .join("wasm32-wasip2/release/install_probe.wasm")
    })
}

fn run(data: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_locora"))
        .arg("--data")
        .arg(data)
        .args(args)
        .output()
        .unwrap()
}

fn ok(data: &Path, args: &[&str]) {
    let output = run(data, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fail(data: &Path, args: &[&str]) -> String {
    let output = run(data, args);
    assert!(!output.status.success());
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn rewrite(path: &Path, change: impl Fn(&str, &mut Vec<u8>) -> bool) {
    let bytes = fs::read(path).unwrap();
    let mut input = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if change(&name, &mut bytes) {
            out.start_file(name, SimpleFileOptions::default()).unwrap();
            out.write_all(&bytes).unwrap();
        }
    }
    fs::write(path, out.finish().unwrap().into_inner()).unwrap();
}

#[test]
fn signed_package_preserves_exact_component_and_publisher() {
    let temp = tempfile::tempdir().unwrap();
    let d = temp.path();
    let data = d.join("profile");
    let source = d.join("source.wasm");
    fs::copy(probe(), &source).unwrap();
    let key = d.join("key");
    ok(&data, &["plugin", "keygen", s(&key)]);
    ok(&data, &["plugin", "sign", s(&source), "--key", s(&key)]);
    let package = d.join("probe.locora-plugin");
    ok(
        &data,
        &["plugin", "pack", s(&source), "--output", s(&package)],
    );
    ok(&data, &["plugin", "check", s(&package)]);
    ok(&data, &["plugin", "install", s(&package), "--yes"]);
    assert_eq!(
        fs::read(data.join("plugins/install-probe.wasm")).unwrap(),
        fs::read(&source).unwrap()
    );
    let receipt = fs::read_to_string(data.join("plugins/install-probe.wasm.json")).unwrap();
    assert!(receipt.contains("signer"));
    assert!(
        fail(
            &data,
            &["plugin", "pack", s(&source), "--output", s(&package)]
        )
        .contains("existiert bereits")
    );

    let mut newer = fs::read(&source).unwrap();
    let offsets: Vec<_> = newer
        .windows(5)
        .enumerate()
        .filter_map(|(i, bytes)| (bytes == b"0.1.0").then_some(i))
        .collect();
    assert!(!offsets.is_empty());
    for i in offsets {
        newer[i..i + 5].copy_from_slice(b"0.2.0");
    }
    let v2 = d.join("v2.wasm");
    fs::write(&v2, &newer).unwrap();
    ok(&data, &["plugin", "sign", s(&v2), "--key", s(&key)]);
    let update = d.join("update.locora-plugin");
    ok(&data, &["plugin", "pack", s(&v2), "--output", s(&update)]);
    ok(&data, &["plugin", "update", s(&update), "--yes"]);
    assert_eq!(
        fs::read(data.join("plugins/install-probe.wasm")).unwrap(),
        newer
    );
}

#[test]
fn package_rejects_mismatch_bad_signature_and_unexpected_entries_before_install() {
    let temp = tempfile::tempdir().unwrap();
    let d = temp.path();
    let data = d.join("profile");
    let source = d.join("source.wasm");
    fs::copy(probe(), &source).unwrap();
    let key = d.join("key");
    ok(&data, &["plugin", "keygen", s(&key)]);
    ok(&data, &["plugin", "sign", s(&source), "--key", s(&key)]);
    let package = d.join("probe.locora-plugin");
    ok(
        &data,
        &["plugin", "pack", s(&source), "--output", s(&package)],
    );
    let good = fs::read(&package).unwrap();

    rewrite(&package, |name, bytes| {
        if name == "manifest.toml" {
            *bytes = String::from_utf8(bytes.clone())
                .unwrap()
                .replace("install-probe", "forged-probe")
                .into_bytes();
        }
        true
    });
    assert!(fail(&data, &["plugin", "check", s(&package)]).contains("stimmt nicht"));
    fs::write(&package, &good).unwrap();
    rewrite(&package, |name, bytes| {
        if name == "plugin.wasm.sig" {
            bytes[0] = b'!';
        }
        true
    });
    assert!(
        !run(&data, &["plugin", "install", s(&package), "--yes"])
            .status
            .success()
    );
    assert!(!data.exists());

    fs::write(&package, &good).unwrap();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("../plugin.wasm", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"x").unwrap();
    fs::write(&package, writer.finish().unwrap().into_inner()).unwrap();
    assert!(fail(&data, &["plugin", "check", s(&package)]).contains("unsicherer Paketeintrag"));

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for name in ["plugin.wasm", "plugin1wasm"] {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"x").unwrap();
    }
    let bytes = writer.finish().unwrap().into_inner();
    let mut duplicated = bytes.clone();
    for (i, window) in bytes.windows(11).enumerate() {
        if window == b"plugin1wasm" {
            duplicated[i..i + 11].copy_from_slice(b"plugin.wasm");
        }
    }
    fs::write(&package, duplicated).unwrap();
    let error = fail(&data, &["plugin", "check", s(&package)]);
    assert!(error.contains("Doppelte Paketeinträge"), "{error}");
}
